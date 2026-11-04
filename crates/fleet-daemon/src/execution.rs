use crate::{DirectoryError, SessionDirectory};
use fleet_proto::{SessionRecord, SessionSelector};
use std::{
    ffi::{OsStr, OsString},
    fmt,
    fs::{self, File, Metadata},
    os::unix::{fs::MetadataExt, io::AsRawFd, process::CommandExt},
    path::{Component, Path, PathBuf},
    process::{Command, Output},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug)]
pub enum ExecutionError {
    Directory(DirectoryError),
    Io(std::io::Error),
    InvalidCwd(PathBuf),
    CacheCwd(PathBuf),
    SessionChanged,
    InvalidConfiguration(PathBuf),
    InvalidProgram,
    InvalidClock,
}

impl fmt::Display for ExecutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Directory(error) => error.fmt(f),
            Self::Io(error) => write!(f, "execution unavailable: {error}"),
            Self::InvalidCwd(path) => write!(f, "invalid project cwd: {}", path.display()),
            Self::CacheCwd(path) => write!(
                f,
                "cache directory is not a project cwd: {}",
                path.display()
            ),
            Self::SessionChanged => write!(f, "session changed; select it again before executing"),
            Self::InvalidConfiguration(path) => write!(
                f,
                "invalid or changed directory configuration: {}",
                path.display()
            ),
            Self::InvalidProgram => write!(f, "executable path must be absolute"),
            Self::InvalidClock => write!(f, "system clock is before the Unix epoch"),
        }
    }
}

impl std::error::Error for ExecutionError {}

impl From<DirectoryError> for ExecutionError {
    fn from(value: DirectoryError) -> Self {
        Self::Directory(value)
    }
}

impl From<std::io::Error> for ExecutionError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

#[derive(Debug)]
struct DirectoryRoot {
    supplied: PathBuf,
    canonical: PathBuf,
    identity: (u64, u64),
}

impl DirectoryRoot {
    fn new(supplied: PathBuf) -> Result<Self, ExecutionError> {
        if !supplied.is_absolute() {
            return Err(ExecutionError::InvalidConfiguration(supplied));
        }
        let canonical = fs::canonicalize(&supplied)
            .map_err(|_| ExecutionError::InvalidConfiguration(supplied.clone()))?;
        let metadata = fs::metadata(&canonical)?;
        if !metadata.is_dir() {
            return Err(ExecutionError::InvalidConfiguration(supplied));
        }
        Ok(Self {
            supplied,
            canonical,
            identity: identity(&metadata),
        })
    }

    fn validate(&self) -> Result<(), ExecutionError> {
        let current = fs::canonicalize(&self.supplied)
            .map_err(|_| ExecutionError::InvalidConfiguration(self.supplied.clone()))?;
        let metadata = fs::metadata(&current)?;
        if current != self.canonical || !metadata.is_dir() || identity(&metadata) != self.identity {
            return Err(ExecutionError::InvalidConfiguration(self.supplied.clone()));
        }
        Ok(())
    }
}

/// Execution core for one node. Selection changes require exclusive access.
#[derive(Debug)]
pub struct NodeDaemon {
    directory: DirectoryRoot,
    cache_roots: Vec<DirectoryRoot>,
    instance: Arc<()>,
    generation: u64,
}

/// A single-use selection tied to a daemon, its selection generation, and an open directory.
#[derive(Debug)]
pub struct SelectedSession {
    instance: Arc<()>,
    generation: u64,
    selector: SessionSelector,
    record: SessionRecord,
    cwd: PathBuf,
    directory: File,
}

impl NodeDaemon {
    pub fn new(directory: PathBuf, cache_roots: Vec<PathBuf>) -> Result<Self, ExecutionError> {
        Ok(Self {
            directory: DirectoryRoot::new(directory)?,
            cache_roots: cache_roots
                .into_iter()
                .map(DirectoryRoot::new)
                .collect::<Result<_, _>>()?,
            instance: Arc::new(()),
            generation: 0,
        })
    }

    pub fn select(
        &mut self,
        selector: &SessionSelector,
    ) -> Result<SelectedSession, ExecutionError> {
        // Replacing the instance also prevents generation wraparound from reviving old selections.
        self.generation = match self.generation.checked_add(1) {
            Some(next) => next,
            None => {
                self.instance = Arc::new(());
                0
            }
        };
        self.validate_configuration()?;
        let now = now()?;
        let directory = SessionDirectory::discover(&self.directory.canonical, now)?;
        let record = directory.resolve(selector, now)?.clone();
        let cwd = self.validate_cwd(&record.project_cwd)?;
        let file = File::open(&cwd)?;
        if !file.metadata()?.is_dir()
            || identity(&file.metadata()?) != identity(&fs::metadata(&cwd)?)
        {
            return Err(ExecutionError::InvalidCwd(cwd));
        }
        Ok(SelectedSession {
            instance: self.instance.clone(),
            generation: self.generation,
            selector: selector.clone(),
            record,
            cwd,
            directory: file,
        })
    }

    /// Revalidates immediately before spawn. External JSON writers do not share a transaction
    /// with process creation; changes after this boundary do not cancel an already accepted run.
    pub fn execute(
        &mut self,
        selected: SelectedSession,
        program: &OsStr,
        args: &[OsString],
    ) -> Result<Output, ExecutionError> {
        if !Arc::ptr_eq(&selected.instance, &self.instance)
            || selected.generation != self.generation
        {
            return Err(ExecutionError::SessionChanged);
        }
        if !Path::new(program).is_absolute() {
            return Err(ExecutionError::InvalidProgram);
        }
        self.validate_configuration()?;
        let cwd = self.validate_cwd(&selected.record.project_cwd)?;
        let pinned = selected.directory.metadata()?;
        if cwd != selected.cwd
            || pinned.nlink() == 0
            || identity(&fs::metadata(&cwd)?) != identity(&pinned)
        {
            return Err(ExecutionError::SessionChanged);
        }
        let now = now()?;
        let directory = SessionDirectory::discover(&self.directory.canonical, now)?;
        if directory.resolve(&selected.selector, now)? != &selected.record {
            return Err(ExecutionError::SessionChanged);
        }
        let fd = selected.directory.as_raw_fd();
        let mut command = Command::new(program);
        command.args(args).env_clear();
        // fchdir pins the child to the checked directory even if a pathname is replaced.
        // The descriptor stays open until output() returns. Only an async-signal-safe call
        // runs between fork and exec; the daemon's own cwd never changes.
        unsafe {
            command.pre_exec(move || {
                if libc::fchdir(fd) == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        Ok(command.output()?)
    }

    fn validate_configuration(&self) -> Result<(), ExecutionError> {
        self.directory.validate()?;
        for root in &self.cache_roots {
            root.validate()?;
        }
        Ok(())
    }

    fn validate_cwd(&self, path: &Path) -> Result<PathBuf, ExecutionError> {
        if !path.is_absolute() {
            return Err(ExecutionError::InvalidCwd(path.into()));
        }
        if known_cache(path) {
            return Err(ExecutionError::CacheCwd(path.into()));
        }
        let canonical =
            fs::canonicalize(path).map_err(|_| ExecutionError::InvalidCwd(path.into()))?;
        if !fs::metadata(&canonical)?.is_dir() {
            return Err(ExecutionError::InvalidCwd(path.into()));
        }
        if known_cache(&canonical)
            || canonical.starts_with(&self.directory.canonical)
            || self.cache_roots.iter().any(|root| {
                path.starts_with(&root.supplied) || canonical.starts_with(&root.canonical)
            })
        {
            return Err(ExecutionError::CacheCwd(path.into()));
        }
        Ok(canonical)
    }
}

fn known_cache(path: &Path) -> bool {
    let mut previous = None;
    for part in path.components() {
        if let Component::Normal(name) = part {
            if name == ".cache"
                || name == ".anet"
                || (previous == Some(OsStr::new("Library")) && name == "Caches")
            {
                return true;
            }
            previous = Some(name);
        }
    }
    false
}

fn identity(metadata: &Metadata) -> (u64, u64) {
    (metadata.dev(), metadata.ino())
}

fn now() -> Result<u64, ExecutionError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|time| time.as_secs())
        .map_err(|_| ExecutionError::InvalidClock)
}
