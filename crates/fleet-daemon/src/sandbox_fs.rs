use fleet_proto::{resolve_project_dir_in_root, ProjectPathError};
use serde::{Deserialize, Serialize};
use std::{
    ffi::{OsStr, OsString},
    fmt,
    fs::{self, File},
    os::unix::{fs::MetadataExt, io::AsRawFd, process::CommandExt},
    path::{Component, Path, PathBuf},
    process::Command,
};

pub const MAX_SANDBOX_READ_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_SANDBOX_WRITE_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug)]
pub enum SandboxFsError {
    ProjectDirBoundary(ProjectPathError),
    Io(std::io::Error),
    PayloadTooLarge { actual: usize, limit: usize },
    DirectoryPinnedIdentityMismatch(PathBuf),
    InvalidProgram(String),
}

impl fmt::Display for SandboxFsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ProjectDirBoundary(err) => write!(f, "project_dir_boundary_error: {err}"),
            Self::Io(err) => write!(f, "sandbox fs io error: {err}"),
            Self::PayloadTooLarge { actual, limit } => {
                write!(f, "sandbox file payload {actual} bytes exceeds {limit} byte limit")
            }
            Self::DirectoryPinnedIdentityMismatch(path) => {
                write!(
                    f,
                    "pinned directory descriptor identity changed or unlinked: {}",
                    path.display()
                )
            }
            Self::InvalidProgram(prog) => {
                write!(f, "executable path must be absolute and non-empty: {prog}")
            }
        }
    }
}

impl std::error::Error for SandboxFsError {}

impl From<ProjectPathError> for SandboxFsError {
    fn from(value: ProjectPathError) -> Self {
        Self::ProjectDirBoundary(value)
    }
}

impl From<std::io::Error> for SandboxFsError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SandboxDirEntry {
    pub name: String,
    pub is_dir: bool,
    pub size_bytes: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SandboxExecOutput {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// Pinned project directory handle verified by `(dev, ino)` and `nlink > 0`.
struct PinnedProjectDir {
    canonical_dir: PathBuf,
    dir_file: File,
    identity: (u64, u64),
}

impl PinnedProjectDir {
    fn open(projects_root: &Path, project_slug: &str) -> Result<Self, SandboxFsError> {
        let resolved_dir = resolve_project_dir_in_root(projects_root, project_slug)?;
        if !resolved_dir.exists() {
            return Err(SandboxFsError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("project directory does not exist: {}", resolved_dir.display()),
            )));
        }
        let canonical_root = fs::canonicalize(projects_root)
            .map_err(|_| ProjectPathError::InvalidRoot(projects_root.to_path_buf()))?;
        let canonical_dir = fs::canonicalize(&resolved_dir).map_err(|_| {
            ProjectPathError::OutsideProjectsRoot {
                root: canonical_root.clone(),
                candidate: resolved_dir.clone(),
            }
        })?;
        if !canonical_dir.starts_with(&canonical_root) || canonical_dir == canonical_root {
            return Err(ProjectPathError::OutsideProjectsRoot {
                root: canonical_root,
                candidate: canonical_dir,
            }
            .into());
        }
        let dir_file = File::open(&canonical_dir)?;
        let fd_meta = dir_file.metadata()?;
        let path_meta = fs::metadata(&canonical_dir)?;
        let id = (fd_meta.dev(), fd_meta.ino());
        if !fd_meta.is_dir()
            || fd_meta.nlink() == 0
            || id != (path_meta.dev(), path_meta.ino())
        {
            return Err(SandboxFsError::DirectoryPinnedIdentityMismatch(canonical_dir));
        }
        Ok(Self {
            canonical_dir,
            dir_file,
            identity: id,
        })
    }

    fn revalidate(&self) -> Result<(), SandboxFsError> {
        let fd_meta = self.dir_file.metadata()?;
        let path_meta = fs::metadata(&self.canonical_dir)?;
        if !fd_meta.is_dir()
            || fd_meta.nlink() == 0
            || self.identity != (fd_meta.dev(), fd_meta.ino())
            || self.identity != (path_meta.dev(), path_meta.ino())
        {
            return Err(SandboxFsError::DirectoryPinnedIdentityMismatch(
                self.canonical_dir.clone(),
            ));
        }
        Ok(())
    }
}

/// Validates a relative path within a project directory, rejecting `..`, absolute paths,
/// and sensitive hidden segments (`.anet`, `.ssh`, `.private_keys`, `.cache`, `Library/Caches`).
pub fn validate_relative_workspace_path(
    project_dir: &Path,
    relative_path: &str,
) -> Result<PathBuf, ProjectPathError> {
    let trimmed = relative_path.trim();
    if trimmed.is_empty() || trimmed == "." {
        return Ok(project_dir.to_path_buf());
    }
    if trimmed.contains('\0') {
        return Err(ProjectPathError::PathTraversal(relative_path.to_string()));
    }
    let rel = Path::new(trimmed);
    if rel.is_absolute() {
        return Err(ProjectPathError::OutsideProjectsRoot {
            root: project_dir.to_path_buf(),
            candidate: rel.to_path_buf(),
        });
    }

    let mut clean = PathBuf::from(project_dir);
    let mut prev_segment: Option<&OsStr> = None;
    for comp in rel.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(ProjectPathError::PathTraversal(relative_path.to_string()));
            }
            Component::Normal(seg) => {
                let s = seg.to_string_lossy();
                if s == ".anet"
                    || s == ".ssh"
                    || s == ".private_keys"
                    || s == ".cache"
                    || s == ".gnupg"
                    || s == ".aws"
                    || (prev_segment == Some(OsStr::new("Library")) && s == "Caches")
                {
                    return Err(ProjectPathError::CacheOrHiddenSegment(s.into_owned()));
                }
                prev_segment = Some(seg);
                clean.push(seg);
            }
        }
    }
    Ok(clean)
}

/// Sandboxed filesystem and command handler for remote workbench RPCs.
/// Strictly binds all operations to `<projects_root>/<slug>` via `resolve_project_dir_in_root`
/// and pins child command execution via `libc::fchdir` on the verified directory descriptor.
#[derive(Clone, Debug)]
pub struct SandboxFsHandler {
    projects_root: PathBuf,
}

impl SandboxFsHandler {
    pub fn new(projects_root: PathBuf) -> Result<Self, SandboxFsError> {
        if !projects_root.is_absolute() {
            return Err(ProjectPathError::InvalidRoot(projects_root).into());
        }
        Ok(Self { projects_root })
    }

    pub fn projects_root(&self) -> &Path {
        &self.projects_root
    }

    fn resolve_verified_target(
        &self,
        pinned: &PinnedProjectDir,
        relative_path: &str,
        allow_nonexistent_leaf: bool,
    ) -> Result<PathBuf, SandboxFsError> {
        pinned.revalidate()?;
        let candidate = validate_relative_workspace_path(&pinned.canonical_dir, relative_path)?;
        // Use lstat (`symlink_metadata`) first so dangling symlinks pointing outside the
        // project root are caught before `exists()` (which uses `stat`) can fall through
        // to the nonexistent-leaf branch on `write_file`.
        if let Ok(sym_meta) = fs::symlink_metadata(&candidate) {
            let resolved = fs::canonicalize(&candidate).map_err(|_| {
                ProjectPathError::OutsideProjectsRoot {
                    root: pinned.canonical_dir.clone(),
                    candidate: candidate.clone(),
                }
            })?;
            if !resolved.starts_with(&pinned.canonical_dir) {
                return Err(ProjectPathError::OutsideProjectsRoot {
                    root: pinned.canonical_dir.clone(),
                    candidate: resolved,
                }
                .into());
            }
            let _ = sym_meta;
            return Ok(resolved);
        } else if allow_nonexistent_leaf {
            let parent = candidate.parent().ok_or_else(|| {
                ProjectPathError::OutsideProjectsRoot {
                    root: pinned.canonical_dir.clone(),
                    candidate: candidate.clone(),
                }
            })?;
            let canonical_parent = fs::canonicalize(parent).map_err(|_| {
                ProjectPathError::OutsideProjectsRoot {
                    root: pinned.canonical_dir.clone(),
                    candidate: candidate.clone(),
                }
            })?;
            if !canonical_parent.starts_with(&pinned.canonical_dir) {
                return Err(ProjectPathError::OutsideProjectsRoot {
                    root: pinned.canonical_dir.clone(),
                    candidate: canonical_parent,
                }
                .into());
            }
            let file_name = candidate.file_name().ok_or_else(|| {
                ProjectPathError::PathTraversal(relative_path.to_string())
            })?;
            Ok(canonical_parent.join(file_name))
        } else {
            Err(SandboxFsError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("workspace path not found: {relative_path}"),
            )))
        }
    }

    pub fn list_dir(
        &self,
        project_slug: &str,
        relative_path: &str,
    ) -> Result<Vec<SandboxDirEntry>, SandboxFsError> {
        // Validate relative path syntax first so traversal/secret paths fail closed immediately.
        let preview_dir = resolve_project_dir_in_root(&self.projects_root, project_slug)?;
        let _ = validate_relative_workspace_path(&preview_dir, relative_path)?;

        let pinned = PinnedProjectDir::open(&self.projects_root, project_slug)?;
        let target_dir = self.resolve_verified_target(&pinned, relative_path, false)?;
        let mut entries = Vec::new();
        for entry in fs::read_dir(&target_dir)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_string();
            if name == ".anet" || name == ".ssh" || name == ".private_keys" {
                continue;
            }
            let meta = entry.metadata()?;
            entries.push(SandboxDirEntry {
                name,
                is_dir: meta.is_dir(),
                size_bytes: meta.len(),
            });
        }
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(entries)
    }

    pub fn read_file(
        &self,
        project_slug: &str,
        relative_path: &str,
    ) -> Result<String, SandboxFsError> {
        let preview_dir = resolve_project_dir_in_root(&self.projects_root, project_slug)?;
        let _ = validate_relative_workspace_path(&preview_dir, relative_path)?;

        let pinned = PinnedProjectDir::open(&self.projects_root, project_slug)?;
        let target = self.resolve_verified_target(&pinned, relative_path, false)?;
        let meta = fs::metadata(&target)?;
        if meta.len() as usize > MAX_SANDBOX_READ_BYTES {
            return Err(SandboxFsError::PayloadTooLarge {
                actual: meta.len() as usize,
                limit: MAX_SANDBOX_READ_BYTES,
            });
        }
        pinned.revalidate()?;
        let bytes = fs::read(&target)?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    pub fn write_file(
        &self,
        project_slug: &str,
        relative_path: &str,
        content: &str,
    ) -> Result<usize, SandboxFsError> {
        if content.len() > MAX_SANDBOX_WRITE_BYTES {
            return Err(SandboxFsError::PayloadTooLarge {
                actual: content.len(),
                limit: MAX_SANDBOX_WRITE_BYTES,
            });
        }
        let preview_dir = resolve_project_dir_in_root(&self.projects_root, project_slug)?;
        let _ = validate_relative_workspace_path(&preview_dir, relative_path)?;

        let pinned = PinnedProjectDir::open(&self.projects_root, project_slug)?;
        let target = self.resolve_verified_target(&pinned, relative_path, true)?;
        pinned.revalidate()?;
        fs::write(&target, content.as_bytes())?;
        Ok(content.len())
    }

    /// Executes an absolute program inside the verified project directory using `libc::fchdir`
    /// on the open directory descriptor between `fork` and `exec`.
    pub fn execute_in_project(
        &self,
        project_slug: &str,
        program: &OsStr,
        args: &[OsString],
    ) -> Result<SandboxExecOutput, SandboxFsError> {
        let prog_path = Path::new(program);
        if program.is_empty() || !prog_path.is_absolute() {
            return Err(SandboxFsError::InvalidProgram(
                program.to_string_lossy().into_owned(),
            ));
        }
        let pinned = PinnedProjectDir::open(&self.projects_root, project_slug)?;
        pinned.revalidate()?;
        let fd = pinned.dir_file.as_raw_fd();
        let mut command = Command::new(program);
        command
            .args(args)
            .env_clear()
            .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin");
        unsafe {
            command.pre_exec(move || {
                if libc::fchdir(fd) == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let output = command.output()?;
        Ok(SandboxExecOutput {
            exit_code: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}
