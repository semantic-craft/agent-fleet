use fleet_proto::{
    resolve_project_dir_in_root, validate_brief, validate_nonempty_id, BriefPolicyError,
    CreateSessionRequest, DeliverTurnRequest, DeliveryTier, DesktopHostKind, HostSessionReceipt,
    ProjectDirRecord, ProjectPathError, SidebarProjectBinding,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt, fs,
    path::{Path, PathBuf},
};

#[derive(Debug)]
pub enum HostAdapterError {
    Path(ProjectPathError),
    Brief(BriefPolicyError),
    Io(std::io::Error),
    SymlinkRejected(PathBuf),
    UnregisteredProject {
        host: DesktopHostKind,
        project_id: String,
    },
    ProjectDirMismatch {
        expected: PathBuf,
        supplied: PathBuf,
    },
    UnknownSession {
        host: DesktopHostKind,
        session_id: String,
    },
    SessionProjectMismatch {
        session_id: String,
        expected_project_id: String,
        supplied_project_id: String,
    },
    DuplicateDelivery(String),
    InvalidModel(String),
    ForbiddenProjectId(String),
}

impl fmt::Display for HostAdapterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Path(e) => e.fmt(f),
            Self::Brief(e) => e.fmt(f),
            Self::Io(e) => write!(f, "host adapter I/O error: {e}"),
            Self::SymlinkRejected(path) => {
                write!(f, "symlink project path rejected: {}", path.display())
            }
            Self::UnregisteredProject { host, project_id } => {
                write!(f, "sidebar project {project_id} is not registered on {host}")
            }
            Self::ProjectDirMismatch { expected, supplied } => write!(
                f,
                "project directory mismatch: expected {}, got {}",
                expected.display(),
                supplied.display()
            ),
            Self::UnknownSession { host, session_id } => {
                write!(f, "session {session_id} does not exist on {host}")
            }
            Self::SessionProjectMismatch {
                session_id,
                expected_project_id,
                supplied_project_id,
            } => write!(
                f,
                "session {session_id} belongs to project {expected_project_id}, not {supplied_project_id}"
            ),
            Self::DuplicateDelivery(id) => {
                write!(f, "delivery id {id} already claimed on this host adapter")
            }
            Self::InvalidModel(model) => write!(f, "invalid model configuration: {model}"),
            Self::ForbiddenProjectId(id) => write!(
                f,
                "unscoped CLI/default bucket project_id rejected for sidebar visibility: {id}"
            ),
        }
    }
}

impl std::error::Error for HostAdapterError {}

impl From<ProjectPathError> for HostAdapterError {
    fn from(value: ProjectPathError) -> Self {
        Self::Path(value)
    }
}

impl From<BriefPolicyError> for HostAdapterError {
    fn from(value: BriefPolicyError) -> Self {
        Self::Brief(value)
    }
}

impl From<std::io::Error> for HostAdapterError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

/// Unified four-host lifecycle contract (`openai-dots`, `cindy`, `deepseek-harness`, `antigravity-desktop`).
pub trait DesktopHostAdapter {
    fn host_kind(&self) -> DesktopHostKind;
    fn projects_root(&self) -> &Path;

    /// Controlled creation/verification of `<projects_root>/<slug>`, rejecting path traversal,
    /// symlinks, cache/hidden directories, and out-of-bounds paths.
    fn ensure_project_dir(&mut self, slug: &str) -> Result<ProjectDirRecord, HostAdapterError>;

    /// Registers or updates the project in the desktop host's sidebar state and binds `project_id`
    /// to the verified project directory.
    fn ensure_sidebar_project(
        &mut self,
        slug: &str,
        title: Option<&str>,
    ) -> Result<SidebarProjectBinding, HostAdapterError>;

    /// Creates a new sidebar-visible session under a registered sidebar project and submits its initial turn.
    fn create_session(
        &mut self,
        request: &CreateSessionRequest,
    ) -> Result<HostSessionReceipt, HostAdapterError>;

    /// Delivers a follow-up turn to an existing session in the same sidebar project.
    fn deliver_turn(
        &mut self,
        request: &DeliverTurnRequest,
    ) -> Result<HostSessionReceipt, HostAdapterError>;
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct PersistedHostState {
    projects: BTreeMap<String, SidebarProjectBinding>,
    slug_to_project_id: BTreeMap<String, String>,
    sessions: BTreeMap<String, PersistedSessionRecord>,
    claimed_deliveries: BTreeSet<String>,
    next_session_seq: u64,
    next_turn_seq: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PersistedSessionRecord {
    pub host: DesktopHostKind,
    pub project_id: String,
    pub session_id: String,
    pub title: String,
    pub project_dir: PathBuf,
    pub workspace_uris: Vec<String>,
    pub turns: Vec<PersistedTurnRecord>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PersistedTurnRecord {
    pub delivery_id: String,
    pub turn_id: String,
    pub prompt: String,
    pub model: Option<String>,
}

/// Shared verified project-root & state engine backing each of the four canonical desktop adapters.
#[derive(Debug)]
struct HostLifecycleCore {
    host: DesktopHostKind,
    projects_root: PathBuf,
    state_file: PathBuf,
    state: PersistedHostState,
}

impl HostLifecycleCore {
    fn new(
        host: DesktopHostKind,
        projects_root: PathBuf,
        state_root: PathBuf,
    ) -> Result<Self, HostAdapterError> {
        if !projects_root.is_absolute() {
            return Err(ProjectPathError::InvalidRoot(projects_root).into());
        }
        if !state_root.is_absolute() {
            return Err(ProjectPathError::InvalidRoot(state_root).into());
        }
        // Validate root has no traversal or cache segments before touching disk.
        let _ = resolve_project_dir_in_root(&projects_root, "probe-slug")?;
        if projects_root.exists() {
            let meta = fs::symlink_metadata(&projects_root)?;
            if meta.file_type().is_symlink() {
                return Err(HostAdapterError::SymlinkRejected(projects_root));
            }
        } else {
            fs::create_dir_all(&projects_root)?;
        }
        let canonical_projects_root = fs::canonicalize(&projects_root)?;
        fs::create_dir_all(&state_root)?;
        let state_file = state_root.join(format!("{}-state.json", host.as_str()));
        let state = if state_file.exists() {
            serde_json::from_slice(&fs::read(&state_file)?).map_err(|e| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())
            })?
        } else {
            PersistedHostState::default()
        };
        Ok(Self {
            host,
            projects_root: canonical_projects_root,
            state_file,
            state,
        })
    }

    fn persist(&self) -> Result<(), HostAdapterError> {
        let tmp = self.state_file.with_extension("json.tmp");
        let bytes = serde_json::to_vec_pretty(&self.state)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
        fs::write(&tmp, bytes)?;
        fs::rename(tmp, &self.state_file)?;
        Ok(())
    }

    fn verify_existing_dir_inside_root(&self, candidate: &Path) -> Result<PathBuf, HostAdapterError> {
        if !candidate.is_absolute() {
            return Err(ProjectPathError::OutsideProjectsRoot {
                root: self.projects_root.clone(),
                candidate: candidate.to_path_buf(),
            }
            .into());
        }
        let meta = fs::symlink_metadata(candidate)?;
        if meta.file_type().is_symlink() {
            return Err(HostAdapterError::SymlinkRejected(candidate.to_path_buf()));
        }
        let canonical = fs::canonicalize(candidate)?;
        if canonical.parent() != Some(self.projects_root.as_path())
            || !canonical.starts_with(&self.projects_root)
        {
            return Err(ProjectPathError::OutsideProjectsRoot {
                root: self.projects_root.clone(),
                candidate: canonical,
            }
            .into());
        }
        Ok(canonical)
    }

    fn ensure_project_dir(&mut self, slug: &str) -> Result<ProjectDirRecord, HostAdapterError> {
        let candidate = resolve_project_dir_in_root(&self.projects_root, slug)?;
        let created = if candidate.exists() {
            let meta = fs::symlink_metadata(&candidate)?;
            if meta.file_type().is_symlink() {
                return Err(HostAdapterError::SymlinkRejected(candidate));
            }
            if !meta.is_dir() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    "target project path exists and is not a directory",
                )
                .into());
            }
            false
        } else {
            fs::create_dir(&candidate)?;
            true
        };
        let canonical = self.verify_existing_dir_inside_root(&candidate)?;
        let workspace_uri = format!("file://{}", canonical.display());
        Ok(ProjectDirRecord {
            slug: slug.to_string(),
            path: canonical,
            workspace_uri,
            created,
        })
    }

    fn ensure_sidebar_project(
        &mut self,
        slug: &str,
        title: Option<&str>,
    ) -> Result<SidebarProjectBinding, HostAdapterError> {
        let dir = self.ensure_project_dir(slug)?;
        let resolved_title = match title {
            Some(t) => {
                validate_nonempty_id("project title", t)?;
                t.to_string()
            }
            None => slug.to_string(),
        };
        let project_id = self
            .state
            .slug_to_project_id
            .get(slug)
            .cloned()
            .unwrap_or_else(|| format!("proj-{}-{slug}", self.host.as_str()));
        validate_sidebar_project_id(&project_id)?;
        let binding = SidebarProjectBinding {
            host: self.host,
            project_id: project_id.clone(),
            title: resolved_title,
            project_dir: dir.path,
            workspace_uris: vec![dir.workspace_uri],
        };
        self.state
            .slug_to_project_id
            .insert(slug.to_string(), project_id.clone());
        self.state.projects.insert(project_id, binding.clone());
        self.persist()?;
        Ok(binding)
    }

    fn create_session(
        &mut self,
        request: &CreateSessionRequest,
    ) -> Result<HostSessionReceipt, HostAdapterError> {
        validate_nonempty_id("delivery_id", &request.delivery_id)?;
        validate_nonempty_id("project_id", &request.project_id)?;
        validate_sidebar_project_id(&request.project_id)?;
        validate_nonempty_id("title", &request.title)?;
        if request.prompt.trim().is_empty() || request.prompt.len() > 64 * 1024 {
            return Err(ProjectPathError::InvalidIdentifier(
                "prompt must be nonempty and at most 64 KiB".into(),
            )
            .into());
        }
        validate_model(request.model.as_deref())?;
        validate_brief(&request.prompt)?;

        let canonical_dir = self.verify_existing_dir_inside_root(&request.project_dir)?;
        let Some(project) = self.state.projects.get(&request.project_id).cloned() else {
            return Err(HostAdapterError::UnregisteredProject {
                host: self.host,
                project_id: request.project_id.clone(),
            });
        };
        if project.project_dir != canonical_dir {
            return Err(HostAdapterError::ProjectDirMismatch {
                expected: project.project_dir,
                supplied: canonical_dir,
            });
        }
        if !self
            .state
            .claimed_deliveries
            .insert(request.delivery_id.clone())
        {
            return Err(HostAdapterError::DuplicateDelivery(
                request.delivery_id.clone(),
            ));
        }
        self.state.next_session_seq += 1;
        self.state.next_turn_seq += 1;
        let session_id = format!(
            "{}-session-{}",
            self.host.as_str(),
            self.state.next_session_seq
        );
        let turn_id = format!("{}-turn-{}", self.host.as_str(), self.state.next_turn_seq);

        let session_record = PersistedSessionRecord {
            host: self.host,
            project_id: project.project_id.clone(),
            session_id: session_id.clone(),
            title: request.title.clone(),
            project_dir: project.project_dir.clone(),
            workspace_uris: project.workspace_uris.clone(),
            turns: vec![PersistedTurnRecord {
                delivery_id: request.delivery_id.clone(),
                turn_id: turn_id.clone(),
                prompt: request.prompt.clone(),
                model: request.model.clone(),
            }],
        };
        self.state
            .sessions
            .insert(session_id.clone(), session_record);
        self.persist()?;

        Ok(HostSessionReceipt {
            host: self.host,
            delivery_id: request.delivery_id.clone(),
            project_id: project.project_id,
            session_id,
            turn_id,
            project_dir: project.project_dir,
            workspace_uris: project.workspace_uris,
            tier: DeliveryTier::ThreadVisible,
        })
    }

    fn deliver_turn(
        &mut self,
        request: &DeliverTurnRequest,
    ) -> Result<HostSessionReceipt, HostAdapterError> {
        validate_nonempty_id("delivery_id", &request.delivery_id)?;
        validate_nonempty_id("project_id", &request.project_id)?;
        validate_sidebar_project_id(&request.project_id)?;
        validate_nonempty_id("session_id", &request.session_id)?;
        if request.prompt.trim().is_empty() || request.prompt.len() > 64 * 1024 {
            return Err(ProjectPathError::InvalidIdentifier(
                "prompt must be nonempty and at most 64 KiB".into(),
            )
            .into());
        }
        validate_model(request.model.as_deref())?;
        validate_brief(&request.prompt)?;

        let canonical_dir = self.verify_existing_dir_inside_root(&request.project_dir)?;
        let Some(project) = self.state.projects.get(&request.project_id).cloned() else {
            return Err(HostAdapterError::UnregisteredProject {
                host: self.host,
                project_id: request.project_id.clone(),
            });
        };
        if project.project_dir != canonical_dir {
            return Err(HostAdapterError::ProjectDirMismatch {
                expected: project.project_dir,
                supplied: canonical_dir,
            });
        }
        let Some(existing_session) = self.state.sessions.get(&request.session_id).cloned() else {
            return Err(HostAdapterError::UnknownSession {
                host: self.host,
                session_id: request.session_id.clone(),
            });
        };
        if existing_session.project_id != request.project_id {
            return Err(HostAdapterError::SessionProjectMismatch {
                session_id: request.session_id.clone(),
                expected_project_id: existing_session.project_id,
                supplied_project_id: request.project_id.clone(),
            });
        }
        if existing_session.project_dir != canonical_dir {
            return Err(HostAdapterError::ProjectDirMismatch {
                expected: existing_session.project_dir,
                supplied: canonical_dir,
            });
        }
        if !self
            .state
            .claimed_deliveries
            .insert(request.delivery_id.clone())
        {
            return Err(HostAdapterError::DuplicateDelivery(
                request.delivery_id.clone(),
            ));
        }
        self.state.next_turn_seq += 1;
        let turn_id = format!("{}-turn-{}", self.host.as_str(), self.state.next_turn_seq);
        if let Some(session_mut) = self.state.sessions.get_mut(&request.session_id) {
            session_mut.turns.push(PersistedTurnRecord {
                delivery_id: request.delivery_id.clone(),
                turn_id: turn_id.clone(),
                prompt: request.prompt.clone(),
                model: request.model.clone(),
            });
        }
        self.persist()?;

        Ok(HostSessionReceipt {
            host: self.host,
            delivery_id: request.delivery_id.clone(),
            project_id: project.project_id,
            session_id: request.session_id.clone(),
            turn_id,
            project_dir: project.project_dir,
            workspace_uris: project.workspace_uris,
            tier: DeliveryTier::ThreadVisible,
        })
    }

    fn session(&self, session_id: &str) -> Option<&PersistedSessionRecord> {
        self.state.sessions.get(session_id)
    }

    fn project(&self, project_id: &str) -> Option<&SidebarProjectBinding> {
        self.state.projects.get(project_id)
    }
}

fn validate_sidebar_project_id(project_id: &str) -> Result<(), HostAdapterError> {
    if matches!(
        project_id,
        "default-cli-project" | "outside-of-project" | "null" | "none"
    ) {
        return Err(HostAdapterError::ForbiddenProjectId(project_id.into()));
    }
    Ok(())
}

fn validate_model(model: Option<&str>) -> Result<(), HostAdapterError> {
    if let Some(model) = model {
        if model.trim().is_empty() || model.len() > 256 || model.chars().any(char::is_control) {
            return Err(HostAdapterError::InvalidModel(model.into()));
        }
    }
    Ok(())
}

macro_rules! define_host_adapter {
    ($name:ident, $variant:ident) => {
        #[derive(Debug)]
        pub struct $name {
            core: HostLifecycleCore,
        }

        impl $name {
            pub fn new(
                projects_root: PathBuf,
                state_root: PathBuf,
            ) -> Result<Self, HostAdapterError> {
                Ok(Self {
                    core: HostLifecycleCore::new(
                        DesktopHostKind::$variant,
                        projects_root,
                        state_root,
                    )?,
                })
            }

            pub fn session(&self, session_id: &str) -> Option<&PersistedSessionRecord> {
                self.core.session(session_id)
            }

            pub fn project(&self, project_id: &str) -> Option<&SidebarProjectBinding> {
                self.core.project(project_id)
            }
        }

        impl DesktopHostAdapter for $name {
            fn host_kind(&self) -> DesktopHostKind {
                self.core.host
            }

            fn projects_root(&self) -> &Path {
                &self.core.projects_root
            }

            fn ensure_project_dir(
                &mut self,
                slug: &str,
            ) -> Result<ProjectDirRecord, HostAdapterError> {
                self.core.ensure_project_dir(slug)
            }

            fn ensure_sidebar_project(
                &mut self,
                slug: &str,
                title: Option<&str>,
            ) -> Result<SidebarProjectBinding, HostAdapterError> {
                self.core.ensure_sidebar_project(slug, title)
            }

            fn create_session(
                &mut self,
                request: &CreateSessionRequest,
            ) -> Result<HostSessionReceipt, HostAdapterError> {
                self.core.create_session(request)
            }

            fn deliver_turn(
                &mut self,
                request: &DeliverTurnRequest,
            ) -> Result<HostSessionReceipt, HostAdapterError> {
                self.core.deliver_turn(request)
            }
        }
    };
}

define_host_adapter!(OpenAiDotsAdapter, OpenaiDots);
define_host_adapter!(CindyAdapter, Cindy);
define_host_adapter!(DeepSeekHarnessAdapter, DeepseekHarness);
define_host_adapter!(AntigravityDesktopAdapter, AntigravityDesktop);

/// Factory constructing a boxed `DesktopHostAdapter` for any of the four canonical hosts.
pub fn build_host_adapter(
    host: DesktopHostKind,
    projects_root: PathBuf,
    state_root: PathBuf,
) -> Result<Box<dyn DesktopHostAdapter>, HostAdapterError> {
    match host {
        DesktopHostKind::OpenaiDots => Ok(Box::new(OpenAiDotsAdapter::new(
            projects_root,
            state_root,
        )?)),
        DesktopHostKind::Cindy => Ok(Box::new(CindyAdapter::new(projects_root, state_root)?)),
        DesktopHostKind::DeepseekHarness => Ok(Box::new(DeepSeekHarnessAdapter::new(
            projects_root,
            state_root,
        )?)),
        DesktopHostKind::AntigravityDesktop => Ok(Box::new(AntigravityDesktopAdapter::new(
            projects_root,
            state_root,
        )?)),
    }
}
