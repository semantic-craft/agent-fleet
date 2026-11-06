use fleet_proto::{
    resolve_project_dir_in_root, validate_brief, validate_nonempty_id, validate_session_id,
    BriefPolicyError, CreateSessionRequest, DeliverTurnRequest, DeliveryTier, DesktopHostKind,
    ExternalAgentManifest, HostSessionReceipt, InboxMessage, PeerAddress, ProjectDirRecord,
    ProjectPathError, SidebarProjectBinding,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt, fs,
    path::{Path, PathBuf},
    process::Command,
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
    RuntimeNotConfigured(DesktopHostKind),
    AntigravityProtocol(String),
    OpenAiDotsProtocol(String),
    DshProtocol(String),
    CindyProtocol(String),
    ZcodeProtocol(String),
    ExternalAgentProtocol(String),
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
            Self::RuntimeNotConfigured(host) => write!(
                f,
                "no active local runtime or socket detected for {host}; refusing silent accepted fallback"
            ),
            Self::AntigravityProtocol(reason) => {
                write!(f, "antigravity desktop adapter error: {reason}")
            }
            Self::OpenAiDotsProtocol(reason) => {
                write!(f, "openai-dots desktop adapter error: {reason}")
            }
            Self::DshProtocol(reason) => {
                write!(f, "deepseek-harness desktop adapter error: {reason}")
            }
            Self::CindyProtocol(reason) => {
                write!(f, "cindy desktop adapter error: {reason}")
            }
            Self::ZcodeProtocol(reason) => {
                write!(f, "zcode desktop adapter error: {reason}")
            }
            Self::ExternalAgentProtocol(reason) => {
                write!(f, "external agent adapter error: {reason}")
            }
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

    fn ensure_sidebar_project_with_override(
        &mut self,
        slug: &str,
        title: Option<&str>,
        project_id_override: Option<String>,
    ) -> Result<SidebarProjectBinding, HostAdapterError> {
        let dir = self.ensure_project_dir(slug)?;
        let resolved_title = match title {
            Some(t) => {
                validate_nonempty_id("project title", t)?;
                t.to_string()
            }
            None => slug.to_string(),
        };
        let project_id = match project_id_override {
            Some(id) => id,
            None => self
                .state
                .slug_to_project_id
                .get(slug)
                .cloned()
                .unwrap_or_else(|| format!("proj-{}-{slug}", self.host.as_str())),
        };
        validate_nonempty_id("project_id", &project_id)?;
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

    fn ensure_sidebar_project(
        &mut self,
        slug: &str,
        title: Option<&str>,
    ) -> Result<SidebarProjectBinding, HostAdapterError> {
        self.ensure_sidebar_project_with_override(slug, title, None)
    }

    fn validate_create_session(
        &self,
        request: &CreateSessionRequest,
    ) -> Result<SidebarProjectBinding, HostAdapterError> {
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
        if self.state.claimed_deliveries.contains(&request.delivery_id) {
            return Err(HostAdapterError::DuplicateDelivery(
                request.delivery_id.clone(),
            ));
        }
        Ok(project)
    }

    fn commit_created_session(
        &mut self,
        request: &CreateSessionRequest,
        project: SidebarProjectBinding,
        session_id_override: Option<String>,
        turn_id_override: Option<String>,
        workspace_uris_override: Option<Vec<String>>,
        tier: DeliveryTier,
    ) -> Result<HostSessionReceipt, HostAdapterError> {
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
        let session_id = session_id_override.unwrap_or_else(|| {
            format!(
                "{}-session-{}",
                self.host.as_str(),
                self.state.next_session_seq
            )
        });
        let turn_id = turn_id_override
            .unwrap_or_else(|| format!("{}-turn-{}", self.host.as_str(), self.state.next_turn_seq));
        let workspace_uris = workspace_uris_override.unwrap_or(project.workspace_uris);

        let session_record = PersistedSessionRecord {
            host: self.host,
            project_id: project.project_id.clone(),
            session_id: session_id.clone(),
            title: request.title.clone(),
            project_dir: project.project_dir.clone(),
            workspace_uris: workspace_uris.clone(),
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
            workspace_uris,
            tier,
            model: request.model.clone(),
        })
    }

    fn validate_deliver_turn(
        &self,
        request: &DeliverTurnRequest,
    ) -> Result<SidebarProjectBinding, HostAdapterError> {
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
        if self.state.claimed_deliveries.contains(&request.delivery_id) {
            return Err(HostAdapterError::DuplicateDelivery(
                request.delivery_id.clone(),
            ));
        }
        Ok(project)
    }

    fn commit_delivered_turn(
        &mut self,
        request: &DeliverTurnRequest,
        project: SidebarProjectBinding,
        turn_id_override: Option<String>,
        workspace_uris_override: Option<Vec<String>>,
        tier: DeliveryTier,
    ) -> Result<HostSessionReceipt, HostAdapterError> {
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
        let turn_id = turn_id_override
            .unwrap_or_else(|| format!("{}-turn-{}", self.host.as_str(), self.state.next_turn_seq));
        let workspace_uris = workspace_uris_override.unwrap_or(project.workspace_uris);
        if let Some(session_mut) = self.state.sessions.get_mut(&request.session_id) {
            session_mut.workspace_uris = workspace_uris.clone();
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
            workspace_uris,
            tier,
            model: request.model.clone(),
        })
    }

    fn session(&self, session_id: &str) -> Option<&PersistedSessionRecord> {
        self.state.sessions.get(session_id)
    }

    fn project(&self, project_id: &str) -> Option<&SidebarProjectBinding> {
        self.state.projects.get(project_id)
    }

    fn adopt_runtime_session(
        &mut self,
        project_id: &str,
        session_id: &str,
        title: &str,
        project_dir: &Path,
        workspace_uris: Vec<String>,
    ) -> Result<(), HostAdapterError> {
        if self.state.sessions.contains_key(session_id) {
            return Ok(());
        }
        let resolved_title = if title.trim().is_empty() {
            session_id.to_string()
        } else {
            title.trim().to_string()
        };
        let record = PersistedSessionRecord {
            host: self.host,
            project_id: project_id.to_string(),
            session_id: session_id.to_string(),
            title: resolved_title,
            project_dir: project_dir.to_path_buf(),
            workspace_uris,
            turns: Vec::new(),
        };
        self.state.sessions.insert(session_id.to_string(), record);
        self.persist()
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

#[derive(Debug)]
pub struct OpenAiDotsAdapter {
    core: HostLifecycleCore,
    pub(crate) runtime: Option<crate::tri_hosts::OpenAiDotsRuntimeConfig>,
}

impl OpenAiDotsAdapter {
    pub fn new(
        projects_root: PathBuf,
        state_root: PathBuf,
    ) -> Result<Self, HostAdapterError> {
        Ok(Self {
            core: HostLifecycleCore::new(
                DesktopHostKind::OpenaiDots,
                projects_root,
                state_root,
            )?,
            runtime: None,
        })
    }

    pub fn with_runtime(
        projects_root: PathBuf,
        state_root: PathBuf,
        runtime: crate::tri_hosts::OpenAiDotsRuntimeConfig,
    ) -> Result<Self, HostAdapterError> {
        if !runtime.codex_root.is_absolute() {
            return Err(HostAdapterError::OpenAiDotsProtocol(format!(
                "codex_root must be absolute: {}",
                runtime.codex_root.display()
            )));
        }
        if !runtime.app_server_socket.is_absolute() {
            return Err(HostAdapterError::OpenAiDotsProtocol(format!(
                "app_server_socket must be absolute: {}",
                runtime.app_server_socket.display()
            )));
        }
        let mut adapter = Self::new(projects_root, state_root)?;
        adapter.runtime = Some(runtime);
        Ok(adapter)
    }

    pub fn session(&self, session_id: &str) -> Option<&PersistedSessionRecord> {
        self.core.session(session_id)
    }

    pub fn project(&self, project_id: &str) -> Option<&SidebarProjectBinding> {
        self.core.project(project_id)
    }

    pub fn read_project_row(
        &self,
        project_id: &str,
    ) -> Result<Option<crate::tri_hosts::CodexProjectRow>, HostAdapterError> {
        let Some(runtime) = &self.runtime else {
            return Err(HostAdapterError::OpenAiDotsProtocol(
                "OpenAiDotsRuntimeConfig is not configured on this adapter".into(),
            ));
        };
        crate::tri_hosts::read_codex_project(
            &runtime.sqlite3_bin,
            &runtime.state_db_path(),
            project_id,
        )
    }

    pub fn read_thread_row(
        &self,
        thread_id: &str,
    ) -> Result<Option<crate::tri_hosts::CodexThreadRow>, HostAdapterError> {
        let Some(runtime) = &self.runtime else {
            return Err(HostAdapterError::OpenAiDotsProtocol(
                "OpenAiDotsRuntimeConfig is not configured on this adapter".into(),
            ));
        };
        crate::tri_hosts::read_codex_thread(
            &runtime.sqlite3_bin,
            &runtime.state_db_path(),
            thread_id,
        )
    }

    pub fn create_dots_session(
        &mut self,
        request: &CreateSessionRequest,
    ) -> Result<crate::tri_hosts::OpenAiDotsReceipt, HostAdapterError> {
        let receipt = self.create_session(request)?;
        let Some(runtime) = &self.runtime else {
            return Ok(crate::tri_hosts::OpenAiDotsReceipt {
                host: receipt.host,
                delivery_id: receipt.delivery_id,
                project_id: receipt.project_id,
                thread_id: receipt.session_id.clone(),
                session_id: receipt.session_id.clone(),
                turn_id: receipt.turn_id,
                project_dir: receipt.project_dir,
                workspace_uris: receipt.workspace_uris,
                preview: request.prompt.clone(),
                deep_link: format!("codex://threads/{}", receipt.session_id),
                tier: receipt.tier,
                model: request.model.clone(),
            });
        };
        let row = crate::tri_hosts::read_codex_thread(
            &runtime.sqlite3_bin,
            &runtime.state_db_path(),
            &receipt.session_id,
        )?
        .ok_or_else(|| {
            HostAdapterError::OpenAiDotsProtocol(format!(
                "thread {} missing from state_5.sqlite after create_session",
                receipt.session_id
            ))
        })?;
        crate::tri_hosts::verify_codex_thread_binding(
            &row,
            &receipt.project_id,
            &receipt.project_dir,
        )?;
        Ok(crate::tri_hosts::OpenAiDotsReceipt {
            host: receipt.host,
            delivery_id: receipt.delivery_id,
            project_id: row.project_id,
            thread_id: row.thread_id.clone(),
            session_id: row.thread_id.clone(),
            turn_id: receipt.turn_id,
            project_dir: receipt.project_dir,
            workspace_uris: receipt.workspace_uris,
            preview: row.preview,
            deep_link: format!("codex://threads/{}", row.thread_id),
            tier: receipt.tier,
            model: request.model.clone(),
        })
    }

    pub fn deliver_dots_turn(
        &mut self,
        request: &DeliverTurnRequest,
    ) -> Result<crate::tri_hosts::OpenAiDotsReceipt, HostAdapterError> {
        let receipt = self.deliver_turn(request)?;
        let Some(runtime) = &self.runtime else {
            return Ok(crate::tri_hosts::OpenAiDotsReceipt {
                host: receipt.host,
                delivery_id: receipt.delivery_id,
                project_id: receipt.project_id,
                thread_id: receipt.session_id.clone(),
                session_id: receipt.session_id.clone(),
                turn_id: receipt.turn_id,
                project_dir: receipt.project_dir,
                workspace_uris: receipt.workspace_uris,
                preview: request.prompt.clone(),
                deep_link: format!("codex://threads/{}", receipt.session_id),
                tier: receipt.tier,
                model: request.model.clone(),
            });
        };
        let row = crate::tri_hosts::read_codex_thread(
            &runtime.sqlite3_bin,
            &runtime.state_db_path(),
            &receipt.session_id,
        )?
        .ok_or_else(|| {
            HostAdapterError::OpenAiDotsProtocol(format!(
                "thread {} missing from state_5.sqlite after deliver_turn",
                receipt.session_id
            ))
        })?;
        crate::tri_hosts::verify_codex_thread_binding(
            &row,
            &receipt.project_id,
            &receipt.project_dir,
        )?;
        Ok(crate::tri_hosts::OpenAiDotsReceipt {
            host: receipt.host,
            delivery_id: receipt.delivery_id,
            project_id: row.project_id,
            thread_id: row.thread_id.clone(),
            session_id: row.thread_id.clone(),
            turn_id: receipt.turn_id,
            project_dir: receipt.project_dir,
            workspace_uris: receipt.workspace_uris,
            preview: row.preview,
            deep_link: format!("codex://threads/{}", row.thread_id),
            tier: receipt.tier,
            model: request.model.clone(),
        })
    }
}

impl DesktopHostAdapter for OpenAiDotsAdapter {
    fn host_kind(&self) -> DesktopHostKind {
        self.core.host
    }

    fn projects_root(&self) -> &Path {
        &self.core.projects_root
    }

    fn ensure_project_dir(&mut self, slug: &str) -> Result<ProjectDirRecord, HostAdapterError> {
        self.core.ensure_project_dir(slug)
    }

    fn ensure_sidebar_project(
        &mut self,
        slug: &str,
        title: Option<&str>,
    ) -> Result<SidebarProjectBinding, HostAdapterError> {
        let dir = self.core.ensure_project_dir(slug)?;
        let Some(runtime) = self.runtime.clone() else {
            return self.core.ensure_sidebar_project(slug, title);
        };
        if let Some(existing_id) = self.core.state.slug_to_project_id.get(slug).cloned() {
            if self.core.state.projects.contains_key(&existing_id) {
                return self.core.ensure_sidebar_project_with_override(
                    slug,
                    title,
                    Some(existing_id),
                );
            }
        }
        let resolved_title = match title {
            Some(t) => {
                validate_nonempty_id("project title", t)?;
                t.to_string()
            }
            None => slug.to_string(),
        };
        let mut client = crate::tri_hosts::AppServerClient::connect(&runtime)?;
        let result = client.request(
            "project/create",
            serde_json::json!({
                "name": resolved_title,
                "rootPath": dir.path,
                "roots": [dir.path],
            }),
        )?;
        let project_id = result
            .pointer("/project/id")
            .or_else(|| result.get("projectId"))
            .or_else(|| result.get("id"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| {
                HostAdapterError::OpenAiDotsProtocol(
                    "codex app-server project/create response omitted project id".into(),
                )
            })?;
        let row = crate::tri_hosts::read_codex_project(
            &runtime.sqlite3_bin,
            &runtime.state_db_path(),
            &project_id,
        )?
        .ok_or_else(|| {
            HostAdapterError::OpenAiDotsProtocol(format!(
                "project {project_id} missing from state_5.sqlite projects/project_roots after project/create"
            ))
        })?;
        crate::tri_hosts::verify_codex_project_binding(&row, &project_id, &dir.path)?;
        self.core
            .ensure_sidebar_project_with_override(slug, title, Some(project_id))
    }

    fn create_session(
        &mut self,
        request: &CreateSessionRequest,
    ) -> Result<HostSessionReceipt, HostAdapterError> {
        let project = self.core.validate_create_session(request)?;
        let Some(runtime) = self.runtime.clone() else {
            return self.core.commit_created_session(
                request,
                project,
                None,
                None,
                None,
                DeliveryTier::Accepted,
            );
        };
        let magpie_cfg = crate::tri_hosts::read_codex_magpie_config(&runtime.codex_root);
        let resolved_model = request
            .model
            .clone()
            .unwrap_or_else(|| magpie_cfg.model.clone());
        let mut client = crate::tri_hosts::AppServerClient::connect(&runtime)?;
        let start_params = serde_json::json!({
            "projectId": project.project_id,
            "cwd": project.project_dir,
            "title": request.title,
            "model": resolved_model,
            "effort": magpie_cfg.model_reasoning_effort,
        });
        let started = client.request("thread/start", start_params)?;
        let thread_id = started
            .pointer("/thread/id")
            .or_else(|| started.get("threadId"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| {
                HostAdapterError::OpenAiDotsProtocol(
                    "codex app-server thread/start omitted thread.id".into(),
                )
            })?;
        let turn_params = serde_json::json!({
            "threadId": thread_id,
            "projectId": project.project_id,
            "cwd": project.project_dir,
            "clientUserMessageId": request.delivery_id,
            "model": resolved_model,
            "effort": magpie_cfg.model_reasoning_effort,
            "input": [{"type": "text", "text": request.prompt, "text_elements": []}],
        });
        let turn_resp = client.request("turn/start", turn_params)?;
        let turn_id = turn_resp
            .pointer("/turn/id")
            .or_else(|| turn_resp.get("turnId"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| {
                HostAdapterError::OpenAiDotsProtocol(
                    "codex app-server turn/start omitted turn.id".into(),
                )
            })?;
        let row = crate::tri_hosts::read_codex_thread(
            &runtime.sqlite3_bin,
            &runtime.state_db_path(),
            &thread_id,
        )?
        .ok_or_else(|| {
            HostAdapterError::OpenAiDotsProtocol(format!(
                "thread {thread_id} missing from state_5.sqlite after thread/start + turn/start"
            ))
        })?;
        crate::tri_hosts::verify_codex_thread_binding(
            &row,
            &project.project_id,
            &project.project_dir,
        )?;
        self.core.commit_created_session(
            request,
            project,
            Some(thread_id),
            Some(turn_id),
            None,
            DeliveryTier::ThreadVisible,
        )
    }

    fn deliver_turn(
        &mut self,
        request: &DeliverTurnRequest,
    ) -> Result<HostSessionReceipt, HostAdapterError> {
        let project = self.core.validate_deliver_turn(request)?;
        let Some(runtime) = self.runtime.clone() else {
            return self.core.commit_delivered_turn(
                request,
                project,
                None,
                None,
                DeliveryTier::Accepted,
            );
        };
        let magpie_cfg = crate::tri_hosts::read_codex_magpie_config(&runtime.codex_root);
        let resolved_model = request
            .model
            .clone()
            .unwrap_or_else(|| magpie_cfg.model.clone());
        let mut client = crate::tri_hosts::AppServerClient::connect(&runtime)?;
        let resumed = client.request(
            "thread/resume",
            serde_json::json!({
                "threadId": request.session_id,
                "projectId": project.project_id,
                "cwd": project.project_dir,
            }),
        )?;
        if let Some(resumed_id) = resumed
            .pointer("/thread/id")
            .or_else(|| resumed.get("threadId"))
            .and_then(serde_json::Value::as_str)
        {
            if resumed_id != request.session_id {
                return Err(HostAdapterError::OpenAiDotsProtocol(format!(
                    "thread/resume returned mismatched threadId {resumed_id}, expected {}",
                    request.session_id
                )));
            }
        }
        let turn_params = serde_json::json!({
            "threadId": request.session_id,
            "projectId": project.project_id,
            "cwd": project.project_dir,
            "clientUserMessageId": request.delivery_id,
            "model": resolved_model,
            "effort": magpie_cfg.model_reasoning_effort,
            "input": [{"type": "text", "text": request.prompt, "text_elements": []}],
        });
        let turn_resp = client.request("turn/start", turn_params)?;
        let turn_id = turn_resp
            .pointer("/turn/id")
            .or_else(|| turn_resp.get("turnId"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| {
                HostAdapterError::OpenAiDotsProtocol(
                    "codex app-server turn/start omitted turn.id on follow-up".into(),
                )
            })?;
        let row = crate::tri_hosts::read_codex_thread(
            &runtime.sqlite3_bin,
            &runtime.state_db_path(),
            &request.session_id,
        )?
        .ok_or_else(|| {
            HostAdapterError::OpenAiDotsProtocol(format!(
                "thread {} missing from state_5.sqlite after deliver_turn",
                request.session_id
            ))
        })?;
        crate::tri_hosts::verify_codex_thread_binding(
            &row,
            &project.project_id,
            &project.project_dir,
        )?;
        self.core.commit_delivered_turn(
            request,
            project,
            Some(turn_id),
            None,
            DeliveryTier::ThreadVisible,
        )
    }
}

#[derive(Debug)]
pub struct DeepSeekHarnessAdapter {
    core: HostLifecycleCore,
    pub(crate) runtime: Option<crate::tri_hosts::DshRuntimeConfig>,
}

impl DeepSeekHarnessAdapter {
    pub fn new(
        projects_root: PathBuf,
        state_root: PathBuf,
    ) -> Result<Self, HostAdapterError> {
        Ok(Self {
            core: HostLifecycleCore::new(
                DesktopHostKind::DeepseekHarness,
                projects_root,
                state_root,
            )?,
            runtime: None,
        })
    }

    pub fn with_runtime(
        projects_root: PathBuf,
        state_root: PathBuf,
        runtime: crate::tri_hosts::DshRuntimeConfig,
    ) -> Result<Self, HostAdapterError> {
        if let Some(base_url) = &runtime.base_url {
            let _ = crate::tri_hosts::parse_loopback_http_endpoint(base_url)?;
        } else if let (Some(dsh_root), Some(dsh_bin)) = (&runtime.dsh_root, &runtime.dsh_bin) {
            if !dsh_root.is_absolute() {
                return Err(HostAdapterError::DshProtocol(format!(
                    "dsh_root must be absolute: {}",
                    dsh_root.display()
                )));
            }
            if !dsh_bin.is_absolute() {
                return Err(HostAdapterError::DshProtocol(format!(
                    "dsh_bin must be absolute: {}",
                    dsh_bin.display()
                )));
            }
        } else {
            return Err(HostAdapterError::DshProtocol(
                "DshRuntimeConfig requires either base_url or (dsh_root, dsh_bin)".into(),
            ));
        }
        let mut adapter = Self::new(projects_root, state_root)?;
        adapter.runtime = Some(runtime);
        Ok(adapter)
    }

    pub fn session(&self, session_id: &str) -> Option<&PersistedSessionRecord> {
        self.core.session(session_id)
    }

    pub fn project(&self, project_id: &str) -> Option<&SidebarProjectBinding> {
        self.core.project(project_id)
    }

    pub fn read_dsh_session(
        &self,
        session_id: &str,
    ) -> Result<crate::tri_hosts::DshSessionSnapshot, HostAdapterError> {
        let Some(runtime) = &self.runtime else {
            return Err(HostAdapterError::DshProtocol(
                "DshRuntimeConfig is not configured on this adapter".into(),
            ));
        };
        let use_gateway = runtime.base_url.is_some()
            && (runtime.dsh_root.is_none() || runtime.is_gateway_active());
        if !use_gateway {
            if let Some(dsh_root) = &runtime.dsh_root {
                return crate::tri_hosts::read_dsh_native_session(dsh_root, session_id);
            }
        }
        let val = crate::tri_hosts::dsh_http_request(
            runtime,
            "GET",
            &format!("/v1/sessions/{session_id}"),
            None,
        )?;
        let session_obj = val.get("session").unwrap_or(&val);
        let returned_id = session_obj
            .get("id")
            .or_else(|| session_obj.get("session_id"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string();
        let project_id = session_obj
            .get("project_id")
            .or_else(|| session_obj.get("projectId"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string();
        let title = session_obj
            .get("title")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string();
        let cwd = PathBuf::from(
            session_obj
                .get("cwd")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default(),
        );
        let turns = session_obj
            .get("turns")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        let last_turn_id = turns
            .last()
            .and_then(|t| t.get("id").or_else(|| t.get("turn_id")))
            .and_then(serde_json::Value::as_str)
            .map(str::to_string);
        let status = session_obj
            .get("status")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("active")
            .to_string();
        Ok(crate::tri_hosts::DshSessionSnapshot {
            session_id: returned_id,
            project_id,
            title,
            cwd,
            turn_count: turns.len(),
            last_turn_id,
            status,
        })
    }

    pub fn create_dsh_session(
        &mut self,
        request: &CreateSessionRequest,
    ) -> Result<crate::tri_hosts::DshSessionReceipt, HostAdapterError> {
        let receipt = self.create_session(request)?;
        let Some(_runtime) = &self.runtime else {
            return Ok(crate::tri_hosts::DshSessionReceipt {
                host: receipt.host,
                delivery_id: receipt.delivery_id,
                project_id: receipt.project_id,
                session_id: receipt.session_id,
                turn_id: receipt.turn_id,
                project_dir: receipt.project_dir,
                workspace_uris: receipt.workspace_uris,
                turn_count: 1,
                status: "active".into(),
                tier: receipt.tier,
                model: request.model.clone(),
            });
        };
        let snap = self.read_dsh_session(&receipt.session_id)?;
        if snap.cwd != receipt.project_dir {
            return Err(HostAdapterError::ProjectDirMismatch {
                expected: receipt.project_dir,
                supplied: snap.cwd,
            });
        }
        if !snap.project_id.is_empty() && snap.project_id != receipt.project_id {
            return Err(HostAdapterError::SessionProjectMismatch {
                session_id: receipt.session_id,
                expected_project_id: receipt.project_id,
                supplied_project_id: snap.project_id,
            });
        }
        Ok(crate::tri_hosts::DshSessionReceipt {
            host: receipt.host,
            delivery_id: receipt.delivery_id,
            project_id: receipt.project_id,
            session_id: receipt.session_id,
            turn_id: receipt.turn_id,
            project_dir: receipt.project_dir,
            workspace_uris: receipt.workspace_uris,
            turn_count: snap.turn_count,
            status: snap.status,
            tier: receipt.tier,
            model: request.model.clone(),
        })
    }

    pub fn deliver_dsh_turn(
        &mut self,
        request: &DeliverTurnRequest,
    ) -> Result<crate::tri_hosts::DshSessionReceipt, HostAdapterError> {
        let receipt = self.deliver_turn(request)?;
        let Some(_runtime) = &self.runtime else {
            return Ok(crate::tri_hosts::DshSessionReceipt {
                host: receipt.host,
                delivery_id: receipt.delivery_id,
                project_id: receipt.project_id,
                session_id: receipt.session_id,
                turn_id: receipt.turn_id,
                project_dir: receipt.project_dir,
                workspace_uris: receipt.workspace_uris,
                turn_count: 2,
                status: "active".into(),
                tier: receipt.tier,
                model: request.model.clone(),
            });
        };
        let snap = self.read_dsh_session(&receipt.session_id)?;
        if snap.cwd != receipt.project_dir {
            return Err(HostAdapterError::ProjectDirMismatch {
                expected: receipt.project_dir,
                supplied: snap.cwd,
            });
        }
        if !snap.project_id.is_empty() && snap.project_id != receipt.project_id {
            return Err(HostAdapterError::SessionProjectMismatch {
                session_id: receipt.session_id,
                expected_project_id: receipt.project_id,
                supplied_project_id: snap.project_id,
            });
        }
        Ok(crate::tri_hosts::DshSessionReceipt {
            host: receipt.host,
            delivery_id: receipt.delivery_id,
            project_id: receipt.project_id,
            session_id: receipt.session_id,
            turn_id: receipt.turn_id,
            project_dir: receipt.project_dir,
            workspace_uris: receipt.workspace_uris,
            turn_count: snap.turn_count,
            status: snap.status,
            tier: receipt.tier,
            model: request.model.clone(),
        })
    }
}

impl DesktopHostAdapter for DeepSeekHarnessAdapter {
    fn host_kind(&self) -> DesktopHostKind {
        self.core.host
    }

    fn projects_root(&self) -> &Path {
        &self.core.projects_root
    }

    fn ensure_project_dir(&mut self, slug: &str) -> Result<ProjectDirRecord, HostAdapterError> {
        self.core.ensure_project_dir(slug)
    }

    fn ensure_sidebar_project(
        &mut self,
        slug: &str,
        title: Option<&str>,
    ) -> Result<SidebarProjectBinding, HostAdapterError> {
        let dir = self.core.ensure_project_dir(slug)?;
        let Some(runtime) = self.runtime.clone() else {
            return self.core.ensure_sidebar_project(slug, title);
        };
        let Some(dsh_root) = &runtime.dsh_root else {
            return self.core.ensure_sidebar_project(slug, title);
        };
        let resolved_title = match title {
            Some(t) => {
                validate_nonempty_id("project title", t)?;
                t.to_string()
            }
            None => slug.to_string(),
        };
        let ws_id = crate::tri_hosts::ensure_dsh_native_workspace(
            dsh_root,
            &dir.path,
            &resolved_title,
        )?;
        self.core
            .ensure_sidebar_project_with_override(slug, title, Some(ws_id))
    }

    fn create_session(
        &mut self,
        request: &CreateSessionRequest,
    ) -> Result<HostSessionReceipt, HostAdapterError> {
        let project = self.core.validate_create_session(request)?;
        let Some(runtime) = self.runtime.clone() else {
            return self.core.commit_created_session(
                request,
                project,
                None,
                None,
                None,
                DeliveryTier::Accepted,
            );
        };
        let use_gateway = runtime.base_url.is_some()
            && (runtime.dsh_bin.is_none() || runtime.is_gateway_active());
        if !use_gateway {
            if let (Some(dsh_root), Some(dsh_bin)) = (&runtime.dsh_root, &runtime.dsh_bin) {
                let (session_id, turn_id) = crate::tri_hosts::run_dsh_headless_command(
                    dsh_root,
                    dsh_bin,
                    &project.project_dir,
                    None,
                    &request.prompt,
                    request.model.as_deref(),
                )?;
                crate::tri_hosts::sync_dsh_session_title_in_projcache(
                    dsh_root,
                    &session_id,
                    &request.title,
                )?;
                crate::tri_hosts::attach_dsh_session_to_workspace(
                    dsh_root,
                    &project.project_id,
                    &session_id,
                )?;
                let snap = self.read_dsh_session(&session_id)?;
                if snap.cwd != project.project_dir {
                    return Err(HostAdapterError::ProjectDirMismatch {
                        expected: project.project_dir,
                        supplied: snap.cwd,
                    });
                }
                if !snap.project_id.is_empty() && snap.project_id != project.project_id {
                    return Err(HostAdapterError::SessionProjectMismatch {
                        session_id,
                        expected_project_id: project.project_id,
                        supplied_project_id: snap.project_id,
                    });
                }
                return self.core.commit_created_session(
                    request,
                    project,
                    Some(session_id),
                    Some(turn_id),
                    None,
                    DeliveryTier::ThreadVisible,
                );
            }
        }
        let mut create_body = serde_json::json!({
            "project_id": project.project_id,
            "cwd": project.project_dir,
            "title": request.title,
            "delivery_id": request.delivery_id,
        });
        if let Some(model) = &request.model {
            create_body["model"] = serde_json::json!(model);
        }
        let create_resp = crate::tri_hosts::dsh_http_request(
            &runtime,
            "POST",
            "/v1/sessions",
            Some(&create_body),
        )?;
        let session_obj = create_resp.get("session").unwrap_or(&create_resp);
        let session_id = session_obj
            .get("id")
            .or_else(|| session_obj.get("session_id"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| {
                HostAdapterError::DshProtocol(
                    "DSH POST /v1/sessions response omitted session id".into(),
                )
            })?;
        if let Some(returned_cwd) = session_obj.get("cwd").and_then(serde_json::Value::as_str) {
            if Path::new(returned_cwd) != project.project_dir {
                return Err(HostAdapterError::ProjectDirMismatch {
                    expected: project.project_dir,
                    supplied: PathBuf::from(returned_cwd),
                });
            }
        }
        let mut turn_body = serde_json::json!({
            "delivery_id": request.delivery_id,
            "project_id": project.project_id,
            "cwd": project.project_dir,
            "prompt": request.prompt,
        });
        if let Some(model) = &request.model {
            turn_body["model"] = serde_json::json!(model);
        }
        let turn_resp = crate::tri_hosts::dsh_http_request(
            &runtime,
            "POST",
            &format!("/v1/sessions/{session_id}/turns"),
            Some(&turn_body),
        )?;
        let turn_obj = turn_resp.get("turn").unwrap_or(&turn_resp);
        let turn_id = turn_obj
            .get("id")
            .or_else(|| turn_obj.get("turn_id"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| {
                HostAdapterError::DshProtocol(
                    "DSH POST /v1/sessions/<id>/turns response omitted turn id".into(),
                )
            })?;
        if let Some(dsh_root) = &runtime.dsh_root {
            let _ = crate::tri_hosts::attach_dsh_session_to_workspace(
                dsh_root,
                &project.project_id,
                &session_id,
            );
            let _ = crate::tri_hosts::sync_dsh_session_title_in_projcache(
                dsh_root,
                &session_id,
                &request.title,
            );
        }
        let snap = self.read_dsh_session(&session_id)?;
        if snap.cwd != project.project_dir {
            return Err(HostAdapterError::ProjectDirMismatch {
                expected: project.project_dir,
                supplied: snap.cwd,
            });
        }
        if !snap.project_id.is_empty() && snap.project_id != project.project_id {
            return Err(HostAdapterError::SessionProjectMismatch {
                session_id,
                expected_project_id: project.project_id,
                supplied_project_id: snap.project_id,
            });
        }
        self.core.commit_created_session(
            request,
            project,
            Some(session_id),
            Some(turn_id),
            None,
            DeliveryTier::ThreadVisible,
        )
    }

    fn deliver_turn(
        &mut self,
        request: &DeliverTurnRequest,
    ) -> Result<HostSessionReceipt, HostAdapterError> {
        if !self.core.state.sessions.contains_key(&request.session_id) {
            if self.runtime.is_some() {
                if let Ok(canonical_dir) =
                    self.core.verify_existing_dir_inside_root(&request.project_dir)
                {
                    if let Ok(snap) = self.read_dsh_session(&request.session_id) {
                        if !snap.session_id.is_empty() && snap.cwd == canonical_dir {
                            let ws_uri = format!("file://{}", canonical_dir.display());
                            let _ = self.core.adopt_runtime_session(
                                &request.project_id,
                                &request.session_id,
                                &snap.title,
                                &canonical_dir,
                                vec![ws_uri],
                            );
                        }
                    }
                }
            }
        }
        let project = self.core.validate_deliver_turn(request)?;
        let Some(runtime) = self.runtime.clone() else {
            return self.core.commit_delivered_turn(
                request,
                project,
                None,
                None,
                DeliveryTier::Accepted,
            );
        };
        let use_gateway = runtime.base_url.is_some()
            && (runtime.dsh_bin.is_none() || runtime.is_gateway_active());
        if !use_gateway {
            if let (Some(dsh_root), Some(dsh_bin)) = (&runtime.dsh_root, &runtime.dsh_bin) {
                let existing_title = self
                    .core
                    .session(&request.session_id)
                    .map(|s| s.title.clone())
                    .unwrap_or_default();
                let (returned_sid, turn_id) = crate::tri_hosts::run_dsh_headless_command(
                    dsh_root,
                    dsh_bin,
                    &project.project_dir,
                    Some(&request.session_id),
                    &request.prompt,
                    request.model.as_deref(),
                )?;
                if returned_sid != request.session_id {
                    return Err(HostAdapterError::DshProtocol(format!(
                        "dsh --session-id {} returned mismatched session_id {returned_sid}",
                        request.session_id
                    )));
                }
                crate::tri_hosts::sync_dsh_session_title_in_projcache(
                    dsh_root,
                    &request.session_id,
                    &existing_title,
                )?;
                crate::tri_hosts::attach_dsh_session_to_workspace(
                    dsh_root,
                    &project.project_id,
                    &request.session_id,
                )?;
                let snap = self.read_dsh_session(&request.session_id)?;
                if snap.cwd != project.project_dir {
                    return Err(HostAdapterError::ProjectDirMismatch {
                        expected: project.project_dir,
                        supplied: snap.cwd,
                    });
                }
                if !snap.project_id.is_empty() && snap.project_id != project.project_id {
                    return Err(HostAdapterError::SessionProjectMismatch {
                        session_id: request.session_id.clone(),
                        expected_project_id: project.project_id,
                        supplied_project_id: snap.project_id,
                    });
                }
                return self.core.commit_delivered_turn(
                    request,
                    project,
                    Some(turn_id),
                    None,
                    DeliveryTier::ThreadVisible,
                );
            }
        }
        let mut turn_body = serde_json::json!({
            "delivery_id": request.delivery_id,
            "project_id": project.project_id,
            "cwd": project.project_dir,
            "prompt": request.prompt,
        });
        if let Some(model) = &request.model {
            turn_body["model"] = serde_json::json!(model);
        }
        let turn_resp = crate::tri_hosts::dsh_http_request(
            &runtime,
            "POST",
            &format!("/v1/sessions/{}/turns", request.session_id),
            Some(&turn_body),
        )?;
        let turn_obj = turn_resp.get("turn").unwrap_or(&turn_resp);
        let turn_id = turn_obj
            .get("id")
            .or_else(|| turn_obj.get("turn_id"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| {
                HostAdapterError::DshProtocol(
                    "DSH POST /v1/sessions/<id>/turns follow-up omitted turn id".into(),
                )
            })?;
        if let Some(dsh_root) = &runtime.dsh_root {
            let _ = crate::tri_hosts::attach_dsh_session_to_workspace(
                dsh_root,
                &project.project_id,
                &request.session_id,
            );
        }
        let snap = self.read_dsh_session(&request.session_id)?;
        if snap.cwd != project.project_dir {
            return Err(HostAdapterError::ProjectDirMismatch {
                expected: project.project_dir,
                supplied: snap.cwd,
            });
        }
        if !snap.project_id.is_empty() && snap.project_id != project.project_id {
            return Err(HostAdapterError::SessionProjectMismatch {
                session_id: request.session_id.clone(),
                expected_project_id: project.project_id,
                supplied_project_id: snap.project_id,
            });
        }
        self.core.commit_delivered_turn(
            request,
            project,
            Some(turn_id),
            None,
            DeliveryTier::ThreadVisible,
        )
    }
}

#[derive(Debug)]
pub struct CindyAdapter {
    core: HostLifecycleCore,
    pub(crate) runtime: Option<crate::tri_hosts::CindyRuntimeConfig>,
}

impl CindyAdapter {
    pub fn new(
        projects_root: PathBuf,
        state_root: PathBuf,
    ) -> Result<Self, HostAdapterError> {
        Ok(Self {
            core: HostLifecycleCore::new(
                DesktopHostKind::Cindy,
                projects_root,
                state_root,
            )?,
            runtime: None,
        })
    }

    pub fn with_runtime(
        projects_root: PathBuf,
        state_root: PathBuf,
        runtime: crate::tri_hosts::CindyRuntimeConfig,
    ) -> Result<Self, HostAdapterError> {
        if !runtime.cindy_root.is_absolute() {
            return Err(HostAdapterError::CindyProtocol(format!(
                "cindy_root must be absolute: {}",
                runtime.cindy_root.display()
            )));
        }
        if !runtime.gateway_socket.is_absolute() {
            return Err(HostAdapterError::CindyProtocol(format!(
                "gateway_socket must be absolute: {}",
                runtime.gateway_socket.display()
            )));
        }
        let mut adapter = Self::new(projects_root, state_root)?;
        adapter.runtime = Some(runtime);
        Ok(adapter)
    }

    pub fn session(&self, session_id: &str) -> Option<&PersistedSessionRecord> {
        self.core.session(session_id)
    }

    pub fn project(&self, project_id: &str) -> Option<&SidebarProjectBinding> {
        self.core.project(project_id)
    }

    pub fn read_cindy_session(
        &self,
        session_key: &str,
    ) -> Result<crate::tri_hosts::CindySessionSnapshot, HostAdapterError> {
        let Some(runtime) = &self.runtime else {
            return Err(HostAdapterError::CindyProtocol(
                "CindyRuntimeConfig is not configured on this adapter".into(),
            ));
        };
        let mut client = crate::tri_hosts::CindyGatewayClient::connect(runtime)?;
        let resp = client.request(
            "sessions.get",
            serde_json::json!({
                "sessionKey": session_key,
            }),
        )?;
        let sess = resp.get("session").unwrap_or(&resp);
        let key = sess
            .get("sessionKey")
            .or_else(|| sess.get("session_id"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string();
        let project_id = sess
            .get("projectId")
            .or_else(|| sess.get("project_id"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string();
        let title = sess
            .get("title")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string();
        let cwd = PathBuf::from(
            sess.get("cwd")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default(),
        );
        let turns = sess
            .get("turns")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        let last_turn_id = turns
            .last()
            .and_then(|t| t.get("turnId").or_else(|| t.get("id")))
            .and_then(serde_json::Value::as_str)
            .map(str::to_string);
        let transcript_path = sess
            .get("transcriptPath")
            .and_then(serde_json::Value::as_str)
            .map(PathBuf::from)
            .unwrap_or_else(|| runtime.transcripts_dir().join(format!("{key}.jsonl")));
        Ok(crate::tri_hosts::CindySessionSnapshot {
            session_key: key,
            project_id,
            title,
            cwd,
            turn_count: turns.len(),
            last_turn_id,
            transcript_path,
        })
    }

    pub fn create_cindy_session(
        &mut self,
        request: &CreateSessionRequest,
    ) -> Result<crate::tri_hosts::CindySessionReceipt, HostAdapterError> {
        let receipt = self.create_session(request)?;
        let Some(_runtime) = &self.runtime else {
            return Ok(crate::tri_hosts::CindySessionReceipt {
                host: receipt.host,
                delivery_id: receipt.delivery_id,
                project_id: receipt.project_id,
                session_key: receipt.session_id.clone(),
                session_id: receipt.session_id.clone(),
                turn_id: receipt.turn_id,
                project_dir: receipt.project_dir.clone(),
                workspace_uris: receipt.workspace_uris,
                transcript_path: receipt.project_dir.join(format!("{}.jsonl", receipt.session_id)),
                turn_count: 1,
                tier: receipt.tier,
                model: request.model.clone(),
            });
        };
        let snap = self.read_cindy_session(&receipt.session_id)?;
        let entries = crate::tri_hosts::read_cindy_transcript(&snap.transcript_path)?;
        if entries.is_empty() {
            return Err(HostAdapterError::CindyProtocol(format!(
                "Cindy transcript {} is empty after create_session",
                snap.transcript_path.display()
            )));
        }
        Ok(crate::tri_hosts::CindySessionReceipt {
            host: receipt.host,
            delivery_id: receipt.delivery_id,
            project_id: receipt.project_id,
            session_key: snap.session_key.clone(),
            session_id: snap.session_key,
            turn_id: receipt.turn_id,
            project_dir: receipt.project_dir,
            workspace_uris: receipt.workspace_uris,
            transcript_path: snap.transcript_path,
            turn_count: snap.turn_count,
            tier: receipt.tier,
            model: request.model.clone(),
        })
    }

    pub fn deliver_cindy_turn(
        &mut self,
        request: &DeliverTurnRequest,
    ) -> Result<crate::tri_hosts::CindySessionReceipt, HostAdapterError> {
        let receipt = self.deliver_turn(request)?;
        let Some(_runtime) = &self.runtime else {
            return Ok(crate::tri_hosts::CindySessionReceipt {
                host: receipt.host,
                delivery_id: receipt.delivery_id,
                project_id: receipt.project_id,
                session_key: receipt.session_id.clone(),
                session_id: receipt.session_id.clone(),
                turn_id: receipt.turn_id,
                project_dir: receipt.project_dir.clone(),
                workspace_uris: receipt.workspace_uris,
                transcript_path: receipt.project_dir.join(format!("{}.jsonl", receipt.session_id)),
                turn_count: 2,
                tier: receipt.tier,
                model: request.model.clone(),
            });
        };
        let snap = self.read_cindy_session(&receipt.session_id)?;
        let entries = crate::tri_hosts::read_cindy_transcript(&snap.transcript_path)?;
        if entries.len() < 2 {
            return Err(HostAdapterError::CindyProtocol(format!(
                "Cindy transcript {} has fewer than 2 entries after deliver_turn",
                snap.transcript_path.display()
            )));
        }
        Ok(crate::tri_hosts::CindySessionReceipt {
            host: receipt.host,
            delivery_id: receipt.delivery_id,
            project_id: receipt.project_id,
            session_key: snap.session_key.clone(),
            session_id: snap.session_key,
            turn_id: receipt.turn_id,
            project_dir: receipt.project_dir,
            workspace_uris: receipt.workspace_uris,
            transcript_path: snap.transcript_path,
            turn_count: snap.turn_count,
            tier: receipt.tier,
            model: request.model.clone(),
        })
    }
}

impl DesktopHostAdapter for CindyAdapter {
    fn host_kind(&self) -> DesktopHostKind {
        self.core.host
    }

    fn projects_root(&self) -> &Path {
        &self.core.projects_root
    }

    fn ensure_project_dir(&mut self, slug: &str) -> Result<ProjectDirRecord, HostAdapterError> {
        self.core.ensure_project_dir(slug)
    }

    fn ensure_sidebar_project(
        &mut self,
        slug: &str,
        title: Option<&str>,
    ) -> Result<SidebarProjectBinding, HostAdapterError> {
        let dir = self.core.ensure_project_dir(slug)?;
        let Some(runtime) = self.runtime.clone() else {
            return self.core.ensure_sidebar_project(slug, title);
        };
        if let Some(existing_id) = self.core.state.slug_to_project_id.get(slug).cloned() {
            if self.core.state.projects.contains_key(&existing_id) {
                return self.core.ensure_sidebar_project_with_override(
                    slug,
                    title,
                    Some(existing_id),
                );
            }
        }
        let resolved_title = match title {
            Some(t) => {
                validate_nonempty_id("project title", t)?;
                t.to_string()
            }
            None => slug.to_string(),
        };
        let mut client = crate::tri_hosts::CindyGatewayClient::connect(&runtime)?;
        let patched = client.request(
            "sessions.patch",
            serde_json::json!({
                "slug": slug,
                "title": resolved_title,
                "cwd": dir.path,
                "workspaceUris": [dir.workspace_uri],
            }),
        )?;
        let project_id = patched
            .get("projectId")
            .or_else(|| patched.get("project_id"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| format!("proj-cindy-{slug}"));
        self.core
            .ensure_sidebar_project_with_override(slug, title, Some(project_id))
    }

    fn create_session(
        &mut self,
        request: &CreateSessionRequest,
    ) -> Result<HostSessionReceipt, HostAdapterError> {
        let project = self.core.validate_create_session(request)?;
        let Some(runtime) = self.runtime.clone() else {
            return self.core.commit_created_session(
                request,
                project,
                None,
                None,
                None,
                DeliveryTier::Accepted,
            );
        };
        let mut client = crate::tri_hosts::CindyGatewayClient::connect(&runtime)?;
        let created = client.request(
            "sessions.create",
            serde_json::json!({
                "projectId": project.project_id,
                "cwd": project.project_dir,
                "title": request.title,
                "requestKey": format!("{}:create", request.delivery_id),
            }),
        )?;
        let session_key = created
            .get("sessionKey")
            .or_else(|| created.get("session_id"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| {
                HostAdapterError::CindyProtocol(
                    "Cindy gateway sessions.create omitted sessionKey".into(),
                )
            })?;
        let mut send_params = serde_json::json!({
            "sessionKey": session_key,
            "projectId": project.project_id,
            "cwd": project.project_dir,
            "deliveryId": request.delivery_id,
            "text": request.prompt,
        });
        if let Some(model) = &request.model {
            send_params["model"] = serde_json::json!(model);
        }
        let sent = client.request("cindy_session_send", send_params)?;
        let turn_id = sent
            .get("turnId")
            .or_else(|| sent.get("runId"))
            .or_else(|| sent.get("turn_id"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| {
                HostAdapterError::CindyProtocol(
                    "Cindy gateway cindy_session_send omitted turnId/runId".into(),
                )
            })?;
        let snap = self.read_cindy_session(&session_key)?;
        if snap.cwd != project.project_dir {
            return Err(HostAdapterError::ProjectDirMismatch {
                expected: project.project_dir,
                supplied: snap.cwd,
            });
        }
        if snap.project_id != project.project_id {
            return Err(HostAdapterError::SessionProjectMismatch {
                session_id: session_key,
                expected_project_id: project.project_id,
                supplied_project_id: snap.project_id,
            });
        }
        self.core.commit_created_session(
            request,
            project,
            Some(session_key),
            Some(turn_id),
            None,
            DeliveryTier::ThreadVisible,
        )
    }

    fn deliver_turn(
        &mut self,
        request: &DeliverTurnRequest,
    ) -> Result<HostSessionReceipt, HostAdapterError> {
        let project = self.core.validate_deliver_turn(request)?;
        let Some(runtime) = self.runtime.clone() else {
            return self.core.commit_delivered_turn(
                request,
                project,
                None,
                None,
                DeliveryTier::Accepted,
            );
        };
        let mut client = crate::tri_hosts::CindyGatewayClient::connect(&runtime)?;
        let mut send_params = serde_json::json!({
            "sessionKey": request.session_id,
            "projectId": project.project_id,
            "cwd": project.project_dir,
            "deliveryId": request.delivery_id,
            "text": request.prompt,
        });
        if let Some(model) = &request.model {
            send_params["model"] = serde_json::json!(model);
        }
        let sent = client.request("cindy_session_send", send_params)?;
        let turn_id = sent
            .get("turnId")
            .or_else(|| sent.get("runId"))
            .or_else(|| sent.get("turn_id"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| {
                HostAdapterError::CindyProtocol(
                    "Cindy gateway cindy_session_send follow-up omitted turnId/runId".into(),
                )
            })?;
        let snap = self.read_cindy_session(&request.session_id)?;
        if snap.cwd != project.project_dir {
            return Err(HostAdapterError::ProjectDirMismatch {
                expected: project.project_dir,
                supplied: snap.cwd,
            });
        }
        if snap.project_id != project.project_id {
            return Err(HostAdapterError::SessionProjectMismatch {
                session_id: request.session_id.clone(),
                expected_project_id: project.project_id,
                supplied_project_id: snap.project_id,
            });
        }
        self.core.commit_delivered_turn(
            request,
            project,
            Some(turn_id),
            None,
            DeliveryTier::ThreadVisible,
        )
    }
}

#[derive(Debug)]
pub struct AntigravityDesktopAdapter {
    core: HostLifecycleCore,
    pub(crate) runtime: Option<crate::antigravity::AntigravityRuntimeConfig>,
}

impl AntigravityDesktopAdapter {
    pub fn new(
        projects_root: PathBuf,
        state_root: PathBuf,
    ) -> Result<Self, HostAdapterError> {
        Ok(Self {
            core: HostLifecycleCore::new(
                DesktopHostKind::AntigravityDesktop,
                projects_root,
                state_root,
            )?,
            runtime: None,
        })
    }

    pub fn session(&self, session_id: &str) -> Option<&PersistedSessionRecord> {
        self.core.session(session_id)
    }

    pub fn project(&self, project_id: &str) -> Option<&SidebarProjectBinding> {
        self.core.project(project_id)
    }
}

impl DesktopHostAdapter for AntigravityDesktopAdapter {
    fn host_kind(&self) -> DesktopHostKind {
        self.core.host
    }

    fn projects_root(&self) -> &Path {
        &self.core.projects_root
    }

    fn ensure_project_dir(&mut self, slug: &str) -> Result<ProjectDirRecord, HostAdapterError> {
        self.core.ensure_project_dir(slug)
    }

    fn ensure_sidebar_project(
        &mut self,
        slug: &str,
        title: Option<&str>,
    ) -> Result<SidebarProjectBinding, HostAdapterError> {
        let dir = self.core.ensure_project_dir(slug)?;
        let Some(runtime) = self.runtime.clone() else {
            return self.core.ensure_sidebar_project(slug, title);
        };
        if let Some(existing_id) = self.core.state.slug_to_project_id.get(slug).cloned() {
            if self.core.state.projects.contains_key(&existing_id) {
                return self.core.ensure_sidebar_project_with_override(
                    slug,
                    title,
                    Some(existing_id),
                );
            }
        }
        let project_name = match title {
            Some(t) => {
                validate_nonempty_id("project title", t)?;
                t.to_string()
            }
            None => slug.to_string(),
        };
        let hubs = crate::antigravity::discover_antigravity_hubs(&runtime.daemon_dir())?;
        if let Some(hub) = hubs
            .first()
            .filter(|h| crate::antigravity::is_desktop_hub_agentapi_available(&runtime, Some(h)))
        {
            let project_id = crate::antigravity::ensure_antigravity_desktop_project(
                hub,
                &project_name,
                &dir.path,
            )?;
            return self
                .core
                .ensure_sidebar_project_with_override(slug, title, Some(project_id));
        }
        let args = vec![
            "--new-project".into(),
            "--project".into(),
            project_name.into(),
            "--add-dir".into(),
            dir.path.as_os_str().to_os_string(),
            "--output-format".into(),
            "json".into(),
        ];
        let payload = crate::antigravity::run_agy_command(
            &runtime,
            &dir.path,
            &args,
            hubs.first(),
        )?;
        let project_id = payload
            .get("project_id")
            .or_else(|| payload.get("projectId"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| format!("proj-{}-{slug}", self.core.host.as_str()));
        self.core
            .ensure_sidebar_project_with_override(slug, title, Some(project_id))
    }

    fn create_session(
        &mut self,
        request: &CreateSessionRequest,
    ) -> Result<HostSessionReceipt, HostAdapterError> {
        let project = self.core.validate_create_session(request)?;
        let Some(runtime) = self.runtime.clone() else {
            return self.core.commit_created_session(
                request,
                project,
                None,
                None,
                None,
                DeliveryTier::Accepted,
            );
        };
        let hubs = crate::antigravity::discover_antigravity_hubs(&runtime.daemon_dir())?;
        let (conversation_id, turn_id) = if let Some(hub) = hubs
            .first()
            .filter(|h| crate::antigravity::is_desktop_hub_agentapi_available(&runtime, Some(h)))
        {
            let (cid, tid) = crate::antigravity::run_agentapi_new_conversation(
                &runtime,
                hub,
                &request.project_id,
                &project.project_dir,
                &request.title,
                request.model.as_deref(),
                &request.prompt,
            )?;
            (cid, Some(tid))
        } else {
            let mut args: Vec<std::ffi::OsString> = vec![
                "--project".into(),
                request.project_id.clone().into(),
                "--add-dir".into(),
                project.project_dir.as_os_str().to_os_string(),
                "--output-format".into(),
                "json".into(),
            ];
            if let Some(model) = &request.model {
                args.push("--model".into());
                args.push(model.into());
            }
            args.push("-p".into());
            args.push(request.prompt.clone().into());

            let payload = crate::antigravity::run_agy_command(
                &runtime,
                &project.project_dir,
                &args,
                hubs.first(),
            )?;
            let cid = payload
                .get("conversation_id")
                .or_else(|| payload.get("conversationId"))
                .or_else(|| payload.get("session_id"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
                .ok_or_else(|| {
                    HostAdapterError::AntigravityProtocol(
                        "agy create_session output did not include conversation_id".into(),
                    )
                })?;
            let tid = payload
                .get("turn_id")
                .or_else(|| payload.get("turnId"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_string);
            (cid, tid)
        };

        let summary = crate::antigravity::read_conversation_summary(
            &runtime.sqlite3_bin,
            &runtime.summaries_db_path(),
            &conversation_id,
        )?
        .ok_or_else(|| {
            HostAdapterError::AntigravityProtocol(format!(
                "conversation {conversation_id} missing from conversation_summaries.db"
            ))
        })?;
        let expected_uri = format!("file://{}", project.project_dir.display());
        crate::antigravity::verify_summary_binding(
            &summary,
            &project.project_id,
            &expected_uri,
        )?;

        self.core.commit_created_session(
            request,
            project,
            Some(conversation_id),
            turn_id,
            Some(summary.workspace_uris),
            DeliveryTier::ThreadVisible,
        )
    }

    fn deliver_turn(
        &mut self,
        request: &DeliverTurnRequest,
    ) -> Result<HostSessionReceipt, HostAdapterError> {
        if !self.core.state.sessions.contains_key(&request.session_id) {
            if let Some(runtime) = &self.runtime {
                if let Ok(canonical_dir) =
                    self.core.verify_existing_dir_inside_root(&request.project_dir)
                {
                    if let Ok(Some(summary)) = crate::antigravity::read_conversation_summary(
                        &runtime.sqlite3_bin,
                        &runtime.summaries_db_path(),
                        &request.session_id,
                    ) {
                        let expected_uri = format!("file://{}", canonical_dir.display());
                        let uri_matches = summary
                            .workspace_uris
                            .iter()
                            .any(|u| u.trim_end_matches('/') == expected_uri.trim_end_matches('/'));
                        if uri_matches || summary.project_id == request.project_id {
                            let uris = if summary.workspace_uris.is_empty() {
                                vec![expected_uri]
                            } else {
                                summary.workspace_uris.clone()
                            };
                            self.core.adopt_runtime_session(
                                &request.project_id,
                                &request.session_id,
                                &summary.title,
                                &canonical_dir,
                                uris,
                            )?;
                        }
                    }
                }
            }
        }
        let project = self.core.validate_deliver_turn(request)?;
        let Some(runtime) = self.runtime.clone() else {
            return self.core.commit_delivered_turn(
                request,
                project,
                None,
                None,
                DeliveryTier::Accepted,
            );
        };
        let hubs = crate::antigravity::discover_antigravity_hubs(&runtime.daemon_dir())?;
        let turn_id = if let Some(hub) = hubs
            .first()
            .filter(|h| crate::antigravity::is_desktop_hub_agentapi_available(&runtime, Some(h)))
        {
            let tid = crate::antigravity::run_agentapi_send_message(
                &runtime,
                hub,
                &request.project_id,
                &project.project_dir,
                &request.session_id,
                &request.prompt,
                request.model.as_deref(),
            )?;
            Some(tid)
        } else {
            let mut args: Vec<std::ffi::OsString> = vec![
                "--conversation".into(),
                request.session_id.clone().into(),
                "--project".into(),
                request.project_id.clone().into(),
                "--add-dir".into(),
                project.project_dir.as_os_str().to_os_string(),
                "--output-format".into(),
                "json".into(),
            ];
            if let Some(model) = &request.model {
                args.push("--model".into());
                args.push(model.into());
            }
            args.push("-p".into());
            args.push(request.prompt.clone().into());

            let payload = crate::antigravity::run_agy_command(
                &runtime,
                &project.project_dir,
                &args,
                hubs.first(),
            )?;
            if let Some(returned_conv) = payload
                .get("conversation_id")
                .or_else(|| payload.get("conversationId"))
                .and_then(serde_json::Value::as_str)
            {
                if returned_conv != request.session_id {
                    return Err(HostAdapterError::AntigravityProtocol(format!(
                        "agy --conversation {} returned mismatched conversation_id {returned_conv}",
                        request.session_id
                    )));
                }
            }
            payload
                .get("turn_id")
                .or_else(|| payload.get("turnId"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        };

        let summary = crate::antigravity::read_conversation_summary(
            &runtime.sqlite3_bin,
            &runtime.summaries_db_path(),
            &request.session_id,
        )?
        .ok_or_else(|| {
            HostAdapterError::AntigravityProtocol(format!(
                "conversation {} missing from conversation_summaries.db after deliver_turn",
                request.session_id
            ))
        })?;
        let expected_uri = format!("file://{}", project.project_dir.display());
        crate::antigravity::verify_summary_binding(
            &summary,
            &project.project_id,
            &expected_uri,
        )?;

        self.core.commit_delivered_turn(
            request,
            project,
            turn_id,
            Some(summary.workspace_uris),
            DeliveryTier::ThreadVisible,
        )
    }
}

/// Auto-detects the local OpenAI Dots (`openai-dots` / Codex Desktop) runtime from `home` and environment variables.
pub fn detect_openai_dots_runtime(
    home: &Path,
) -> Option<crate::tri_hosts::OpenAiDotsRuntimeConfig> {
    let default_root = std::env::var("CODEX_HOME")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".codex"));
    if !default_root.is_dir() {
        return None;
    }
    let mut candidate_sockets = Vec::new();
    if let Ok(sock) = std::env::var("CODEX_APP_SERVER_SOCKET") {
        if !sock.trim().is_empty() {
            candidate_sockets.push(PathBuf::from(sock));
        }
    }
    candidate_sockets.push(default_root.join("app-server.sock"));
    candidate_sockets.push(
        default_root
            .join("app-server-control")
            .join("app-server-control.sock"),
    );
    candidate_sockets.push(default_root.join("ipc").join("ipc.sock"));
    candidate_sockets.push(
        default_root
            .join("app-server-daemon")
            .join("daemon-updater.sock"),
    );
    let sock = candidate_sockets
        .into_iter()
        .find(|p| fs::metadata(p).is_ok())?;
    let resolved_sock = fs::canonicalize(&sock).unwrap_or(sock);
    Some(crate::tri_hosts::OpenAiDotsRuntimeConfig::new(
        default_root,
        resolved_sock,
    ))
}

/// Auto-detects the local Cindy (`cindy`) runtime from `home` and environment variables.
pub fn detect_cindy_runtime(home: &Path) -> Option<crate::tri_hosts::CindyRuntimeConfig> {
    let default_root = std::env::var("CINDY_HOME")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".cindy"));
    if !default_root.is_dir() {
        return None;
    }
    let mut candidate_sockets = Vec::new();
    if let Ok(sock) = std::env::var("CINDY_GATEWAY_SOCKET") {
        if !sock.trim().is_empty() {
            candidate_sockets.push(PathBuf::from(sock));
        }
    }
    candidate_sockets.push(default_root.join("cindy-gateway.sock"));
    candidate_sockets.push(default_root.join("gateway.sock"));
    candidate_sockets.push(home.join("Library/Application Support/Cindy/gateway.sock"));
    candidate_sockets.push(home.join("Library/Application Support/CindyGlobal/gateway.sock"));
    let sock = candidate_sockets
        .into_iter()
        .find(|p| fs::metadata(p).is_ok())?;
    let resolved_sock = fs::canonicalize(&sock).unwrap_or(sock);
    Some(crate::tri_hosts::CindyRuntimeConfig::new(
        default_root,
        resolved_sock,
    ))
}

/// Auto-detects the local DeepSeek Harness (`deepseek-harness`) runtime from `home` and environment variables.
pub fn detect_dsh_runtime(home: &Path) -> Option<crate::tri_hosts::DshRuntimeConfig> {
    let base_url = std::env::var("DSH_BASE_URL")
        .ok()
        .filter(|s| !s.trim().is_empty());
    let default_root = std::env::var("DSH_HOME")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".dsh"));
    let mut candidate_bins = Vec::new();
    if let Ok(bin) = std::env::var("DSH_BIN") {
        if !bin.trim().is_empty() {
            candidate_bins.push(PathBuf::from(bin));
        }
    }
    candidate_bins.push(home.join(".local").join("bin").join("dsh"));
    candidate_bins.push(home.join("bin").join("dsh"));
    candidate_bins.push(PathBuf::from(
        "/Applications/DeepSeek Harness.app/Contents/Resources/runtime/cli/bin/dsh",
    ));
    let bin_opt = candidate_bins.into_iter().find(|p| p.is_file());

    if default_root.is_dir() && bin_opt.is_some() {
        let mut cfg = crate::tri_hosts::DshRuntimeConfig::new_native(
            default_root,
            bin_opt.unwrap(),
        );
        if let Some(url) = base_url {
            cfg.base_url = Some(url);
        }
        Some(cfg)
    } else if let Some(url) = base_url {
        Some(crate::tri_hosts::DshRuntimeConfig::new(url))
    } else {
        None
    }
}

/// Auto-detects the local Antigravity Desktop (`antigravity-desktop`) runtime from `home` and environment variables.
pub fn detect_antigravity_runtime(
    home: &Path,
) -> Option<crate::antigravity::AntigravityRuntimeConfig> {
    let default_root = std::env::var("ANTIGRAVITY_DATA_DIR")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".gemini").join("antigravity"));
    if !default_root.join("conversation_summaries.db").is_file() {
        return None;
    }
    let mut candidate_bins = Vec::new();
    if let Ok(bin) = std::env::var("AGY_BIN") {
        if !bin.trim().is_empty() {
            candidate_bins.push(PathBuf::from(bin));
        }
    }
    candidate_bins.push(home.join(".local").join("bin").join("agy"));
    candidate_bins.push(PathBuf::from(
        "/Applications/Antigravity.app/Contents/Resources/app/bin/agy",
    ));
    candidate_bins.push(home.join(".local").join("libexec").join("agy").join("agy"));
    let bin = candidate_bins.into_iter().find(|p| p.is_file())?;
    Some(crate::antigravity::AntigravityRuntimeConfig::new(
        default_root,
        bin,
    ))
}

/// Auto-detects the local ZCode (`zcode`) runtime from `home` and environment variables.
pub fn detect_zcode_runtime(home: &Path) -> Option<crate::tri_hosts::ZcodeRuntimeConfig> {
    let default_root = std::env::var("ZCODE_HOME")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".zcode"));
    if !default_root.is_dir() {
        return None;
    }
    let mut candidate_bins = Vec::new();
    if let Ok(bin) = std::env::var("ZCODE_BIN") {
        if !bin.trim().is_empty() {
            candidate_bins.push(PathBuf::from(bin));
        }
    }
    candidate_bins.push(home.join("bin").join("zcode"));
    candidate_bins.push(home.join(".local").join("bin").join("zcode"));
    candidate_bins.push(PathBuf::from(
        "/Applications/ZCode.app/Contents/Resources/cli/zcode",
    ));
    let bin = candidate_bins.into_iter().find(|p| p.is_file())?;
    Some(crate::tri_hosts::ZcodeRuntimeConfig::new(default_root, bin))
}

/// Factory constructing a runtime-backed `DesktopHostAdapter` for production dispatch and MCP.
/// Strictly refuses to fall back to `<state_root>/<host>-state.json` `tier: accepted` when no runtime is configured.
pub fn build_configured_host_adapter(
    host: DesktopHostKind,
    projects_root: PathBuf,
    state_root: PathBuf,
    runtimes: &crate::relay_self_dev::HostRuntimeOptions,
) -> Result<Box<dyn DesktopHostAdapter>, HostAdapterError> {
    match host {
        DesktopHostKind::OpenaiDots => {
            let cfg = runtimes
                .openai_dots
                .clone()
                .ok_or(HostAdapterError::RuntimeNotConfigured(host))?;
            Ok(Box::new(OpenAiDotsAdapter::with_runtime(
                projects_root,
                state_root,
                cfg,
            )?))
        }
        DesktopHostKind::Cindy => {
            let cfg = runtimes
                .cindy
                .clone()
                .ok_or(HostAdapterError::RuntimeNotConfigured(host))?;
            Ok(Box::new(CindyAdapter::with_runtime(
                projects_root,
                state_root,
                cfg,
            )?))
        }
        DesktopHostKind::DeepseekHarness => {
            let cfg = runtimes
                .dsh
                .clone()
                .ok_or(HostAdapterError::RuntimeNotConfigured(host))?;
            Ok(Box::new(DeepSeekHarnessAdapter::with_runtime(
                projects_root,
                state_root,
                cfg,
            )?))
        }
        DesktopHostKind::AntigravityDesktop => {
            let cfg = runtimes
                .antigravity
                .clone()
                .ok_or(HostAdapterError::RuntimeNotConfigured(host))?;
            Ok(Box::new(AntigravityDesktopAdapter::with_runtime(
                projects_root,
                state_root,
                cfg,
            )?))
        }
        DesktopHostKind::Zcode => {
            let cfg = runtimes
                .zcode
                .clone()
                .ok_or(HostAdapterError::RuntimeNotConfigured(host))?;
            Ok(Box::new(ZcodeAdapter::with_runtime(
                projects_root,
                state_root,
                cfg,
            )?))
        }
    }
}

/// Factory constructing a boxed `DesktopHostAdapter` for any of the five canonical hosts.
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
        DesktopHostKind::Zcode => Ok(Box::new(ZcodeAdapter::new(projects_root, state_root)?)),
    }
}

/// Reads persisted projects and sessions for a canonical desktop host from `<state_root>/<host>-state.json`.
pub fn inspect_host_state(
    host: DesktopHostKind,
    state_root: &Path,
) -> Result<(Vec<SidebarProjectBinding>, Vec<PersistedSessionRecord>), HostAdapterError> {
    if !state_root.is_absolute() {
        return Err(ProjectPathError::InvalidRoot(state_root.to_path_buf()).into());
    }
    let state_file = state_root.join(format!("{}-state.json", host.as_str()));
    if !state_file.exists() {
        return Ok((Vec::new(), Vec::new()));
    }
    let state: PersistedHostState = serde_json::from_slice(&fs::read(&state_file)?).map_err(|e| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())
    })?;
    let projects = state.projects.into_values().collect();
    let sessions = state.sessions.into_values().collect();
    Ok((projects, sessions))
}

#[derive(Debug)]
pub struct ZcodeAdapter {
    core: HostLifecycleCore,
    pub(crate) runtime: Option<crate::tri_hosts::ZcodeRuntimeConfig>,
}

impl ZcodeAdapter {
    pub fn new(
        projects_root: PathBuf,
        state_root: PathBuf,
    ) -> Result<Self, HostAdapterError> {
        Ok(Self {
            core: HostLifecycleCore::new(DesktopHostKind::Zcode, projects_root, state_root)?,
            runtime: None,
        })
    }

    pub fn with_runtime(
        projects_root: PathBuf,
        state_root: PathBuf,
        runtime: crate::tri_hosts::ZcodeRuntimeConfig,
    ) -> Result<Self, HostAdapterError> {
        if !runtime.zcode_root.is_absolute() {
            return Err(HostAdapterError::ZcodeProtocol(format!(
                "zcode_root must be absolute: {}",
                runtime.zcode_root.display()
            )));
        }
        if !runtime.zcode_bin.is_absolute() {
            return Err(HostAdapterError::ZcodeProtocol(format!(
                "zcode_bin must be absolute: {}",
                runtime.zcode_bin.display()
            )));
        }
        let mut adapter = Self::new(projects_root, state_root)?;
        adapter.runtime = Some(runtime);
        Ok(adapter)
    }

    pub fn session(&self, session_id: &str) -> Option<&PersistedSessionRecord> {
        self.core.session(session_id)
    }

    pub fn project(&self, project_id: &str) -> Option<&SidebarProjectBinding> {
        self.core.project(project_id)
    }

    pub fn read_project_row(
        &self,
        project_id: &str,
    ) -> Result<Option<crate::tri_hosts::ZcodeProjectRow>, HostAdapterError> {
        let Some(runtime) = &self.runtime else {
            return Err(HostAdapterError::ZcodeProtocol(
                "ZcodeRuntimeConfig is not configured on this adapter".into(),
            ));
        };
        crate::tri_hosts::read_zcode_project(
            &runtime.sqlite3_bin,
            &runtime.tasks_index_db_path(),
            project_id,
        )
    }

    pub fn read_zcode_session(
        &self,
        session_id: &str,
    ) -> Result<Option<crate::tri_hosts::ZcodeSessionSnapshot>, HostAdapterError> {
        let Some(runtime) = &self.runtime else {
            return Err(HostAdapterError::ZcodeProtocol(
                "ZcodeRuntimeConfig is not configured on this adapter".into(),
            ));
        };
        crate::tri_hosts::read_zcode_session(
            &runtime.sqlite3_bin,
            &runtime.tasks_index_db_path(),
            &runtime.cli_db_path(),
            session_id,
        )
    }

    pub fn create_zcode_session(
        &mut self,
        request: &CreateSessionRequest,
    ) -> Result<crate::tri_hosts::ZcodeSessionReceipt, HostAdapterError> {
        let receipt = self.create_session(request)?;
        let Some(_runtime) = &self.runtime else {
            return Ok(crate::tri_hosts::ZcodeSessionReceipt {
                host: receipt.host,
                delivery_id: receipt.delivery_id,
                project_id: receipt.project_id,
                session_id: receipt.session_id.clone(),
                turn_id: receipt.turn_id,
                project_dir: receipt.project_dir.clone(),
                workspace_uris: receipt.workspace_uris,
                workspace_key: receipt.project_dir.display().to_string(),
                message_count: 1,
                tier: receipt.tier,
                model: request.model.clone(),
            });
        };
        let snap = self
            .read_zcode_session(&receipt.session_id)?
            .ok_or_else(|| {
                HostAdapterError::ZcodeProtocol(format!(
                    "session {} missing from ZCode SQLite stores after create_session",
                    receipt.session_id
                ))
            })?;
        crate::tri_hosts::verify_zcode_session_binding(
            &snap,
            &receipt.project_id,
            &receipt.project_dir,
        )?;
        Ok(crate::tri_hosts::ZcodeSessionReceipt {
            host: receipt.host,
            delivery_id: receipt.delivery_id,
            project_id: receipt.project_id,
            session_id: snap.session_id,
            turn_id: receipt.turn_id,
            project_dir: receipt.project_dir,
            workspace_uris: receipt.workspace_uris,
            workspace_key: snap.workspace_key,
            message_count: snap.message_count,
            tier: receipt.tier,
            model: request.model.clone(),
        })
    }

    pub fn deliver_zcode_turn(
        &mut self,
        request: &DeliverTurnRequest,
    ) -> Result<crate::tri_hosts::ZcodeSessionReceipt, HostAdapterError> {
        let receipt = self.deliver_turn(request)?;
        let Some(_runtime) = &self.runtime else {
            return Ok(crate::tri_hosts::ZcodeSessionReceipt {
                host: receipt.host,
                delivery_id: receipt.delivery_id,
                project_id: receipt.project_id,
                session_id: receipt.session_id.clone(),
                turn_id: receipt.turn_id,
                project_dir: receipt.project_dir.clone(),
                workspace_uris: receipt.workspace_uris,
                workspace_key: receipt.project_dir.display().to_string(),
                message_count: 2,
                tier: receipt.tier,
                model: request.model.clone(),
            });
        };
        let snap = self
            .read_zcode_session(&receipt.session_id)?
            .ok_or_else(|| {
                HostAdapterError::ZcodeProtocol(format!(
                    "session {} missing from ZCode SQLite stores after deliver_turn",
                    receipt.session_id
                ))
            })?;
        crate::tri_hosts::verify_zcode_session_binding(
            &snap,
            &receipt.project_id,
            &receipt.project_dir,
        )?;
        Ok(crate::tri_hosts::ZcodeSessionReceipt {
            host: receipt.host,
            delivery_id: receipt.delivery_id,
            project_id: receipt.project_id,
            session_id: snap.session_id,
            turn_id: receipt.turn_id,
            project_dir: receipt.project_dir,
            workspace_uris: receipt.workspace_uris,
            workspace_key: snap.workspace_key,
            message_count: snap.message_count,
            tier: receipt.tier,
            model: request.model.clone(),
        })
    }
}

impl DesktopHostAdapter for ZcodeAdapter {
    fn host_kind(&self) -> DesktopHostKind {
        self.core.host
    }

    fn projects_root(&self) -> &Path {
        &self.core.projects_root
    }

    fn ensure_project_dir(&mut self, slug: &str) -> Result<ProjectDirRecord, HostAdapterError> {
        self.core.ensure_project_dir(slug)
    }

    fn ensure_sidebar_project(
        &mut self,
        slug: &str,
        title: Option<&str>,
    ) -> Result<SidebarProjectBinding, HostAdapterError> {
        let dir = self.core.ensure_project_dir(slug)?;
        let Some(runtime) = self.runtime.clone() else {
            return self.core.ensure_sidebar_project(slug, title);
        };
        let resolved_title = match title {
            Some(t) => {
                validate_nonempty_id("project title", t)?;
                t.to_string()
            }
            None => slug.to_string(),
        };
        let row = crate::tri_hosts::ensure_zcode_sidebar_project(
            &runtime.sqlite3_bin,
            &runtime.tasks_index_db_path(),
            &dir.path,
            &resolved_title,
        )?;
        self.core
            .ensure_sidebar_project_with_override(slug, title, Some(row.project_id))
    }

    fn create_session(
        &mut self,
        request: &CreateSessionRequest,
    ) -> Result<HostSessionReceipt, HostAdapterError> {
        let project = self.core.validate_create_session(request)?;
        let Some(runtime) = self.runtime.clone() else {
            return self.core.commit_created_session(
                request,
                project,
                None,
                None,
                None,
                DeliveryTier::Accepted,
            );
        };
        let (session_id, turn_id) = crate::tri_hosts::run_zcode_headless_command(
            &runtime,
            &project.project_dir,
            None,
            &request.prompt,
            request.model.as_deref(),
        )?;
        crate::tri_hosts::attach_zcode_session_to_tasks_index(
            &runtime.sqlite3_bin,
            &runtime.tasks_index_db_path(),
            &runtime.cli_db_path(),
            &project.project_id,
            &project.project_dir,
            &session_id,
            &request.title,
            &request.prompt,
            &turn_id,
        )?;
        let snap = crate::tri_hosts::read_zcode_session(
            &runtime.sqlite3_bin,
            &runtime.tasks_index_db_path(),
            &runtime.cli_db_path(),
            &session_id,
        )?
        .ok_or_else(|| {
            HostAdapterError::ZcodeProtocol(format!(
                "session {session_id} missing from ZCode SQLite stores after create_session"
            ))
        })?;
        crate::tri_hosts::verify_zcode_session_binding(
            &snap,
            &project.project_id,
            &project.project_dir,
        )?;
        self.core.commit_created_session(
            request,
            project,
            Some(session_id),
            Some(turn_id),
            None,
            DeliveryTier::ThreadVisible,
        )
    }

    fn deliver_turn(
        &mut self,
        request: &DeliverTurnRequest,
    ) -> Result<HostSessionReceipt, HostAdapterError> {
        if !self.core.state.sessions.contains_key(&request.session_id) {
            if self.runtime.is_some() {
                if let Ok(canonical_dir) =
                    self.core.verify_existing_dir_inside_root(&request.project_dir)
                {
                    if let Ok(Some(snap)) = self.read_zcode_session(&request.session_id) {
                        if snap.cwd == canonical_dir {
                            let ws_uri = format!("file://{}", canonical_dir.display());
                            self.core.adopt_runtime_session(
                                &request.project_id,
                                &request.session_id,
                                &snap.title,
                                &canonical_dir,
                                vec![ws_uri],
                            )?;
                        }
                    }
                }
            }
        }
        let project = self.core.validate_deliver_turn(request)?;
        let Some(runtime) = self.runtime.clone() else {
            return self.core.commit_delivered_turn(
                request,
                project,
                None,
                None,
                DeliveryTier::Accepted,
            );
        };
        let existing_title = self
            .core
            .session(&request.session_id)
            .map(|s| s.title.clone())
            .unwrap_or_else(|| project.title.clone());
        let (returned_sid, turn_id) = crate::tri_hosts::run_zcode_headless_command(
            &runtime,
            &project.project_dir,
            Some(&request.session_id),
            &request.prompt,
            request.model.as_deref(),
        )?;
        if returned_sid != request.session_id {
            return Err(HostAdapterError::ZcodeProtocol(format!(
                "zcode --resume {} returned mismatched session_id {returned_sid}",
                request.session_id
            )));
        }
        crate::tri_hosts::attach_zcode_session_to_tasks_index(
            &runtime.sqlite3_bin,
            &runtime.tasks_index_db_path(),
            &runtime.cli_db_path(),
            &project.project_id,
            &project.project_dir,
            &request.session_id,
            &existing_title,
            &request.prompt,
            &turn_id,
        )?;
        let snap = crate::tri_hosts::read_zcode_session(
            &runtime.sqlite3_bin,
            &runtime.tasks_index_db_path(),
            &runtime.cli_db_path(),
            &request.session_id,
        )?
        .ok_or_else(|| {
            HostAdapterError::ZcodeProtocol(format!(
                "session {} missing from ZCode SQLite stores after deliver_turn",
                request.session_id
            ))
        })?;
        crate::tri_hosts::verify_zcode_session_binding(
            &snap,
            &project.project_id,
            &project.project_dir,
        )?;
        self.core.commit_delivered_turn(
            request,
            project,
            Some(turn_id),
            None,
            DeliveryTier::ThreadVisible,
        )
    }
}

/// Receipt proving task dispatch to an external open-source agent registered in `agents.d/<id>.json`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalAgentReceipt {
    pub agent_id: String,
    pub display_name: String,
    pub delivery_id: String,
    pub slug: String,
    pub project_dir: PathBuf,
    pub session_id: String,
    pub turn_id: String,
    pub tier: DeliveryTier,
    pub stdout_preview: String,
}

/// Registry managing pluggable third-party/open-source agent manifests in `<state_root>/agents.d/*.json`.
#[derive(Debug)]
pub struct ExternalAgentRegistry {
    state_root: PathBuf,
}

impl ExternalAgentRegistry {
    pub fn new(state_root: PathBuf) -> Result<Self, HostAdapterError> {
        if !state_root.is_absolute() {
            return Err(ProjectPathError::InvalidRoot(state_root).into());
        }
        fs::create_dir_all(state_root.join("agents.d"))?;
        Ok(Self { state_root })
    }

    pub fn agents_dir(&self) -> PathBuf {
        self.state_root.join("agents.d")
    }

    pub fn register(
        &self,
        manifest: &ExternalAgentManifest,
    ) -> Result<PathBuf, HostAdapterError> {
        manifest.validate()?;
        let path = self.agents_dir().join(format!("{}.json", manifest.id));
        let tmp = path.with_extension("json.tmp");
        let bytes = serde_json::to_vec_pretty(manifest)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
        fs::write(&tmp, bytes)?;
        fs::rename(&tmp, &path)?;
        Ok(path)
    }

    pub fn get(&self, id: &str) -> Result<Option<ExternalAgentManifest>, HostAdapterError> {
        fleet_proto::validate_project_slug(id)?;
        let path = self.agents_dir().join(format!("{id}.json"));
        if !path.exists() {
            return Ok(None);
        }
        let manifest: ExternalAgentManifest =
            serde_json::from_slice(&fs::read(&path)?).map_err(|e| {
                HostAdapterError::ExternalAgentProtocol(format!(
                    "invalid manifest {}: {e}",
                    path.display()
                ))
            })?;
        manifest.validate()?;
        Ok(Some(manifest))
    }

    pub fn list(&self) -> Result<Vec<ExternalAgentManifest>, HostAdapterError> {
        let dir = self.agents_dir();
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let mut files: Vec<PathBuf> = fs::read_dir(&dir)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|ext| ext == "json"))
            .collect();
        files.sort();
        let mut manifests = Vec::new();
        for file in files {
            let manifest: ExternalAgentManifest = serde_json::from_slice(&fs::read(&file)?)
                .map_err(|e| {
                    HostAdapterError::ExternalAgentProtocol(format!(
                        "invalid manifest {}: {e}",
                        file.display()
                    ))
                })?;
            manifest.validate()?;
            manifests.push(manifest);
        }
        Ok(manifests)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn dispatch(
        &self,
        agent_id: &str,
        projects_root: &Path,
        slug: &str,
        delivery_id: &str,
        session_id: Option<&str>,
        title: &str,
        prompt: &str,
        execute_command: bool,
    ) -> Result<ExternalAgentReceipt, HostAdapterError> {
        validate_nonempty_id("delivery_id", delivery_id)?;
        validate_nonempty_id("title", title)?;
        validate_brief(prompt)?;
        let manifest = self.get(agent_id)?.ok_or_else(|| {
            HostAdapterError::ExternalAgentProtocol(format!(
                "external agent '{agent_id}' is not registered in {}",
                self.agents_dir().display()
            ))
        })?;
        let candidate = resolve_project_dir_in_root(projects_root, slug)?;
        if !candidate.exists() {
            fs::create_dir_all(&candidate)?;
        }
        let meta = fs::symlink_metadata(&candidate)?;
        if meta.file_type().is_symlink() {
            return Err(HostAdapterError::SymlinkRejected(candidate));
        }
        let canonical_dir = fs::canonicalize(&candidate)?;

        let resolved_sid = session_id
            .map(str::to_string)
            .unwrap_or_else(|| format!("{agent_id}-{slug}-session-1"));
        let turn_id = format!("{agent_id}-{delivery_id}-turn");

        let mut stdout_preview = String::new();
        let tier = if execute_command {
            let template = if session_id.is_some() && !manifest.resume_args_template.is_empty() {
                &manifest.resume_args_template
            } else {
                &manifest.args_template
            };
            let rendered_args: Vec<String> = template
                .iter()
                .map(|arg| {
                    arg.replace("{cwd}", &canonical_dir.display().to_string())
                        .replace("{slug}", slug)
                        .replace("{session_id}", &resolved_sid)
                        .replace("{delivery_id}", delivery_id)
                        .replace("{title}", title)
                        .replace("{prompt}", prompt)
                })
                .collect();
            let output = Command::new(&manifest.command)
                .current_dir(&canonical_dir)
                .args(&rendered_args)
                .output()
                .map_err(|e| {
                    HostAdapterError::ExternalAgentProtocol(format!(
                        "failed to execute external agent '{}' ({}): {e}",
                        manifest.id,
                        manifest.command.display()
                    ))
                })?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(HostAdapterError::ExternalAgentProtocol(format!(
                    "external agent '{}' exited with {}: {}",
                    manifest.id,
                    output.status,
                    stderr.trim()
                )));
            }
            stdout_preview = String::from_utf8_lossy(&output.stdout).trim().to_string();
            DeliveryTier::Completed
        } else {
            DeliveryTier::Accepted
        };

        Ok(ExternalAgentReceipt {
            agent_id: manifest.id,
            display_name: manifest.display_name,
            delivery_id: delivery_id.to_string(),
            slug: slug.to_string(),
            project_dir: canonical_dir,
            session_id: resolved_sid,
            turn_id,
            tier,
            stdout_preview,
        })
    }
}

/// Persistent bidirectional peer mailbox store (`<state_root>/mailboxes.json`), enabling GrokBot
/// and the five canonical desktop hosts to exchange messages, replies, and `check_inbox` pulls.
#[derive(Debug)]
pub struct FleetMailboxStore {
    state_root: PathBuf,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct PersistedMailboxState {
    #[serde(default)]
    messages: Vec<InboxMessage>,
}

impl FleetMailboxStore {
    pub fn new(state_root: PathBuf) -> Result<Self, HostAdapterError> {
        if !state_root.is_absolute() {
            return Err(ProjectPathError::InvalidRoot(state_root).into());
        }
        fs::create_dir_all(&state_root)?;
        Ok(Self { state_root })
    }

    pub fn mailbox_file(&self) -> PathBuf {
        self.state_root.join("mailboxes.json")
    }

    fn load(&self) -> Result<PersistedMailboxState, HostAdapterError> {
        let path = self.mailbox_file();
        if !path.exists() {
            return Ok(PersistedMailboxState::default());
        }
        let bytes = fs::read(&path)?;
        serde_json::from_slice(&bytes).map_err(|e| {
            HostAdapterError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("corrupt {}: {e}", path.display()),
            ))
        })
    }

    fn save(&self, state: &PersistedMailboxState) -> Result<(), HostAdapterError> {
        let path = self.mailbox_file();
        let tmp = path.with_extension("json.tmp");
        let bytes = serde_json::to_vec_pretty(state)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
        fs::write(&tmp, bytes)?;
        fs::rename(&tmp, &path)?;
        Ok(())
    }

    /// Enqueues a peer message idempotently by `message.id`.
    pub fn enqueue(&self, mut message: InboxMessage) -> Result<InboxMessage, HostAdapterError> {
        validate_nonempty_id("message.id", &message.id)?;
        let from_addr = PeerAddress::parse(&message.from)?;
        let to_addr = PeerAddress::parse(&message.to)?;
        message.from = from_addr.to_string();
        message.to = to_addr.to_string();
        if message.project_slug.is_none() {
            message.project_slug = to_addr.slug.clone().or_else(|| from_addr.slug.clone());
        }
        if message.session_id.is_none() {
            message.session_id = to_addr.session_id.clone();
        }
        if let Some(ref sid) = message.session_id {
            validate_session_id(sid)?;
        }
        validate_brief(&message.body)?;
        let mut state = self.load()?;
        if let Some(existing) = state.messages.iter().find(|m| m.id == message.id) {
            if existing.from == message.from
                && existing.to == message.to
                && existing.body == message.body
            {
                return Ok(existing.clone());
            }
            return Err(HostAdapterError::DuplicateDelivery(message.id));
        }
        state.messages.push(message.clone());
        self.save(&state)?;
        Ok(message)
    }

    /// Reads unread messages for `recipient` (supporting `<agent>`, `<agent>/<slug>`, or `<agent>[/<slug>]#<session_id>`),
    /// isolating session-scoped messages from sibling sessions and unscoped pulls.
    pub fn check_inbox(
        &self,
        recipient: &str,
        mark_delivered: bool,
    ) -> Result<Vec<InboxMessage>, HostAdapterError> {
        let query = PeerAddress::parse(recipient)?;
        let mut state = self.load()?;
        let mut matched = Vec::new();
        let mut changed = false;
        for msg in &mut state.messages {
            if !msg.delivered && msg.matches_recipient(&query) {
                matched.push(msg.clone());
                if mark_delivered {
                    msg.delivered = true;
                    changed = true;
                }
            }
        }
        if changed {
            self.save(&state)?;
        }
        Ok(matched)
    }

    /// Reads outbound messages matching `sender` (`<agent>`, `<agent>/<slug>`, or `<agent>[/<slug>]#<session_id>`).
    pub fn check_outbox(&self, sender: &str) -> Result<Vec<InboxMessage>, HostAdapterError> {
        let query = PeerAddress::parse(sender)?;
        let state = self.load()?;
        Ok(state
            .messages
            .into_iter()
            .filter(|m| m.matches_sender(&query))
            .collect())
    }

    /// Lists all messages associated with `session_id` (either as sender `#session_id`, recipient `#session_id`, or `message.session_id`).
    pub fn list_by_session(&self, session_id: &str) -> Result<Vec<InboxMessage>, HostAdapterError> {
        let sid = validate_session_id(session_id.trim())?;
        let state = self.load()?;
        Ok(state
            .messages
            .into_iter()
            .filter(|m| {
                m.session_id.as_deref() == Some(sid)
                    || m.sender_address()
                        .ok()
                        .and_then(|a| a.session_id)
                        .as_deref()
                        == Some(sid)
                    || m.recipient_address()
                        .ok()
                        .and_then(|a| a.session_id)
                        .as_deref()
                        == Some(sid)
            })
            .collect())
    }

    /// Resolves the reply route (`PeerAddress` including original `session_id`) for a given `reply_to_id`.
    pub fn resolve_reply_route(
        &self,
        reply_to_id: &str,
        target_agent_hint: Option<&str>,
    ) -> Result<Option<PeerAddress>, HostAdapterError> {
        let Some(orig) = self.get_message(reply_to_id)? else {
            return Ok(None);
        };
        let orig_from = PeerAddress::parse(&orig.from)?;
        let orig_to = PeerAddress::parse(&orig.to)?;
        let hint_agent = target_agent_hint
            .map(|h| PeerAddress::parse(h).map(|p| p.agent))
            .transpose()?;
        if hint_agent.is_none() || hint_agent.as_deref() == Some(orig_from.agent.as_str()) {
            Ok(Some(PeerAddress {
                agent: orig_from.agent,
                slug: orig_from.slug.or(orig.project_slug),
                session_id: orig_from.session_id,
            }))
        } else if hint_agent.as_deref() == Some(orig_to.agent.as_str()) {
            Ok(Some(PeerAddress {
                agent: orig_to.agent,
                slug: orig_to.slug.or(orig.project_slug),
                session_id: orig_to.session_id.or(orig.session_id),
            }))
        } else {
            Ok(None)
        }
    }

    pub fn get_message(&self, id: &str) -> Result<Option<InboxMessage>, HostAdapterError> {
        let state = self.load()?;
        Ok(state.messages.into_iter().find(|m| m.id == id))
    }

    pub fn list_all(&self) -> Result<Vec<InboxMessage>, HostAdapterError> {
        Ok(self.load()?.messages)
    }
}

/// Summary of wiring `agent-fleet` MCP and lifecycle hooks into a local agent configuration directory.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentWiringReport {
    pub agent: String,
    pub config_files: Vec<PathBuf>,
    pub wired: bool,
}

fn write_atomic_json(path: &Path, val: &serde_json::Value) -> Result<(), HostAdapterError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp-fleet");
    let text = serde_json::to_string_pretty(val)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
    fs::write(&tmp, format!("{text}\n"))?;
    fs::rename(&tmp, path)?;
    Ok(())
}

/// Wires `fleet-cli mcp` and `fleet-cli hook` into GrokBot/Grok CLI and all five canonical desktop hosts
/// under `config_home` (modeled on `reason-machines/reason-network/src/setup.rs`).
pub fn wire_agent_configs(
    config_home: &Path,
    fleet_exe: &Path,
    projects_root: &Path,
    state_root: &Path,
) -> Result<Vec<AgentWiringReport>, HostAdapterError> {
    if !config_home.is_absolute() || !projects_root.is_absolute() || !state_root.is_absolute() {
        return Err(ProjectPathError::InvalidRoot(config_home.to_path_buf()).into());
    }
    let exe_str = fleet_exe.display().to_string();
    let proj_str = projects_root.display().to_string();
    let state_str = state_root.display().to_string();

    let mut reports = Vec::new();

    // 1. GrokBot / Grok CLI (.grok/config.toml + .grok/hooks/agent-fleet.json)
    let grok_dir = config_home.join(".grok");
    fs::create_dir_all(grok_dir.join("hooks"))?;
    let grok_toml_path = grok_dir.join("config.toml");
    let grok_toml = format!(
        "# Managed by agent-fleet setup\n[mcp_servers.agent-fleet]\ncommand = \"{exe_str}\"\nargs = [\"mcp\", \"--projects-root\", \"{proj_str}\", \"--state-root\", \"{state_str}\"]\nenabled = true\ntool_timeout_sec = 660\n\n[mcp_servers.agent-fleet.env]\nFLEET_AGENT = \"grokbot\"\n"
    );
    fs::write(&grok_toml_path, grok_toml)?;
    let grok_hooks_path = grok_dir.join("hooks/agent-fleet.json");
    let grok_hook_cmd = format!(
        "'{exe_str}' hook grokbot stop --state-root '{state_str}'"
    );
    write_atomic_json(
        &grok_hooks_path,
        &serde_json::json!({
            "hooks": {
                "SessionStart": [{ "matcher": "*", "hooks": [{ "type": "command", "command": grok_hook_cmd, "timeout": 10 }] }],
                "PostToolUse": [{ "matcher": "*", "hooks": [{ "type": "command", "command": grok_hook_cmd, "timeout": 10 }] }],
                "Stop": [{ "hooks": [{ "type": "command", "command": grok_hook_cmd, "timeout": 10 }] }]
            }
        }),
    )?;
    reports.push(AgentWiringReport {
        agent: "grokbot".into(),
        config_files: vec![grok_toml_path, grok_hooks_path],
        wired: true,
    });

    // 2. Antigravity Desktop (.gemini/config/mcp_config.json + .gemini/config/hooks.json)
    let agy_dir = config_home.join(".gemini/config");
    let agy_mcp_path = agy_dir.join("mcp_config.json");
    let agy_hooks_path = agy_dir.join("hooks.json");
    write_atomic_json(
        &agy_mcp_path,
        &serde_json::json!({
            "mcpServers": {
                "agent-fleet": {
                    "command": exe_str,
                    "args": ["mcp", "--projects-root", proj_str, "--state-root", state_str],
                    "env": { "FLEET_AGENT": "antigravity-desktop" }
                }
            }
        }),
    )?;
    write_atomic_json(
        &agy_hooks_path,
        &serde_json::json!({
            "agent-fleet": {
                "enabled": true,
                "SessionStart": [{ "type": "command", "command": format!("'{exe_str}' hook antigravity-desktop session-start --state-root '{state_str}'"), "timeout": 15 }],
                "Stop": [{ "type": "command", "command": format!("'{exe_str}' hook antigravity-desktop stop --state-root '{state_str}'"), "timeout": 15 }]
            }
        }),
    )?;
    reports.push(AgentWiringReport {
        agent: "antigravity-desktop".into(),
        config_files: vec![agy_mcp_path, agy_hooks_path],
        wired: true,
    });

    // 3. Codex Desktop (.codex/config.toml + .codex/hooks.json)
    let codex_dir = config_home.join(".codex");
    fs::create_dir_all(&codex_dir)?;
    let codex_toml_path = codex_dir.join("config.toml");
    let codex_toml = format!(
        "# Managed by agent-fleet setup\n[mcp_servers.agent-fleet]\ncommand = \"{exe_str}\"\nargs = [\"mcp\", \"--projects-root\", \"{proj_str}\", \"--state-root\", \"{state_str}\"]\ntool_timeout_sec = 660\ndefault_tools_approval_mode = \"approve\"\n\n[mcp_servers.agent-fleet.env]\nFLEET_AGENT = \"openai-dots\"\n"
    );
    fs::write(&codex_toml_path, codex_toml)?;
    let codex_hooks_path = codex_dir.join("hooks.json");
    write_atomic_json(
        &codex_hooks_path,
        &serde_json::json!({
            "hooks": {
                "SessionStart": [{ "matcher": "*", "hooks": [{ "type": "command", "command": format!("'{exe_str}' hook openai-dots session-start --state-root '{state_str}'"), "timeout": 10 }] }],
                "Stop": [{ "hooks": [{ "type": "command", "command": format!("'{exe_str}' hook openai-dots stop --state-root '{state_str}'"), "timeout": 10 }] }]
            }
        }),
    )?;
    reports.push(AgentWiringReport {
        agent: "openai-dots".into(),
        config_files: vec![codex_toml_path, codex_hooks_path],
        wired: true,
    });

    // 4. DeepSeek Harness (.dsh/cordis.patch.yml)
    let dsh_dir = config_home.join(".dsh");
    fs::create_dir_all(&dsh_dir)?;
    let dsh_patch_path = dsh_dir.join("cordis.patch.yml");
    let dsh_yaml = format!(
        "# === agent-fleet MCP BEGIN ===\n- insert:\n    - id: mcp-agent-fleet\n      name: '@deepseek-ai/dsh-mcp-client'\n      config:\n        serverName: agent-fleet\n        transport: stdio\n        command: '{exe_str}'\n        args:\n          - mcp\n          - --projects-root\n          - '{proj_str}'\n          - --state-root\n          - '{state_str}'\n        env:\n          FLEET_AGENT: deepseek-harness\n        toolCallTimeoutMs: 660000\n# === agent-fleet MCP END ===\n"
    );
    fs::write(&dsh_patch_path, dsh_yaml)?;
    reports.push(AgentWiringReport {
        agent: "deepseek-harness".into(),
        config_files: vec![dsh_patch_path],
        wired: true,
    });

    // 5. ZCode Desktop (.zcode/cli/config.json)
    let zcode_cfg_path = config_home.join(".zcode/cli/config.json");
    write_atomic_json(
        &zcode_cfg_path,
        &serde_json::json!({
            "mcp": {
                "servers": {
                    "agent-fleet": {
                        "type": "stdio",
                        "command": exe_str,
                        "args": ["mcp", "--projects-root", proj_str, "--state-root", state_str],
                        "env": { "FLEET_AGENT": "zcode" }
                    }
                }
            }
        }),
    )?;
    reports.push(AgentWiringReport {
        agent: "zcode".into(),
        config_files: vec![zcode_cfg_path],
        wired: true,
    });

    // 6. Cindy Desktop (.cindy/mcp.json)
    let cindy_cfg_path = config_home.join(".cindy/mcp.json");
    write_atomic_json(
        &cindy_cfg_path,
        &serde_json::json!({
            "mcpServers": {
                "agent-fleet": {
                    "command": exe_str,
                    "args": ["mcp", "--projects-root", proj_str, "--state-root", state_str],
                    "env": { "FLEET_AGENT": "cindy" }
                }
            }
        }),
    )?;
    reports.push(AgentWiringReport {
        agent: "cindy".into(),
        config_files: vec![cindy_cfg_path],
        wired: true,
    });

    Ok(reports)
}
