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
    AntigravityProtocol(String),
    OpenAiDotsProtocol(String),
    DshProtocol(String),
    CindyProtocol(String),
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
            tier: DeliveryTier::ThreadVisible,
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
            return self
                .core
                .commit_created_session(request, project, None, None, None);
        };
        let mut client = crate::tri_hosts::AppServerClient::connect(&runtime)?;
        let mut start_params = serde_json::json!({
            "projectId": project.project_id,
            "cwd": project.project_dir,
            "title": request.title,
        });
        if let Some(model) = &request.model {
            start_params["model"] = serde_json::json!(model);
        }
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
        let mut turn_params = serde_json::json!({
            "threadId": thread_id,
            "projectId": project.project_id,
            "cwd": project.project_dir,
            "clientUserMessageId": request.delivery_id,
            "input": [{"type": "text", "text": request.prompt, "text_elements": []}],
        });
        if let Some(model) = &request.model {
            turn_params["model"] = serde_json::json!(model);
        }
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
        )
    }

    fn deliver_turn(
        &mut self,
        request: &DeliverTurnRequest,
    ) -> Result<HostSessionReceipt, HostAdapterError> {
        let project = self.core.validate_deliver_turn(request)?;
        let Some(runtime) = self.runtime.clone() else {
            return self
                .core
                .commit_delivered_turn(request, project, None, None);
        };
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
        let mut turn_params = serde_json::json!({
            "threadId": request.session_id,
            "projectId": project.project_id,
            "cwd": project.project_dir,
            "clientUserMessageId": request.delivery_id,
            "input": [{"type": "text", "text": request.prompt, "text_elements": []}],
        });
        if let Some(model) = &request.model {
            turn_params["model"] = serde_json::json!(model);
        }
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
        self.core
            .commit_delivered_turn(request, project, Some(turn_id), None)
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
        let _ = crate::tri_hosts::parse_loopback_http_endpoint(&runtime.base_url)?;
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
        self.core.ensure_sidebar_project(slug, title)
    }

    fn create_session(
        &mut self,
        request: &CreateSessionRequest,
    ) -> Result<HostSessionReceipt, HostAdapterError> {
        let project = self.core.validate_create_session(request)?;
        let Some(runtime) = self.runtime.clone() else {
            return self
                .core
                .commit_created_session(request, project, None, None, None);
        };
        let create_body = serde_json::json!({
            "project_id": project.project_id,
            "cwd": project.project_dir,
            "title": request.title,
            "delivery_id": request.delivery_id,
        });
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
        )
    }

    fn deliver_turn(
        &mut self,
        request: &DeliverTurnRequest,
    ) -> Result<HostSessionReceipt, HostAdapterError> {
        let project = self.core.validate_deliver_turn(request)?;
        let Some(runtime) = self.runtime.clone() else {
            return self
                .core
                .commit_delivered_turn(request, project, None, None);
        };
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
        self.core
            .commit_delivered_turn(request, project, Some(turn_id), None)
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
            return self
                .core
                .commit_created_session(request, project, None, None, None);
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
        )
    }

    fn deliver_turn(
        &mut self,
        request: &DeliverTurnRequest,
    ) -> Result<HostSessionReceipt, HostAdapterError> {
        let project = self.core.validate_deliver_turn(request)?;
        let Some(runtime) = self.runtime.clone() else {
            return self
                .core
                .commit_delivered_turn(request, project, None, None);
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
        self.core
            .commit_delivered_turn(request, project, Some(turn_id), None)
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
            return self
                .core
                .commit_created_session(request, project, None, None, None);
        };
        let hubs = crate::antigravity::discover_antigravity_hubs(&runtime.daemon_dir())?;
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
        let conversation_id = payload
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
        let turn_id = payload
            .get("turn_id")
            .or_else(|| payload.get("turnId"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_string);

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
        )
    }

    fn deliver_turn(
        &mut self,
        request: &DeliverTurnRequest,
    ) -> Result<HostSessionReceipt, HostAdapterError> {
        let project = self.core.validate_deliver_turn(request)?;
        let Some(runtime) = self.runtime.clone() else {
            return self
                .core
                .commit_delivered_turn(request, project, None, None);
        };
        let hubs = crate::antigravity::discover_antigravity_hubs(&runtime.daemon_dir())?;
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
        let turn_id = payload
            .get("turn_id")
            .or_else(|| payload.get("turnId"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_string);

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
        )
    }
}

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
