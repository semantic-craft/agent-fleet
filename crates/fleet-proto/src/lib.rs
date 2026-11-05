mod delivery;
pub use delivery::DeliveryTier;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

mod brief;
pub use brief::{validate_brief, BriefPolicyError};

mod host;
pub use host::{
    resolve_project_dir_in_root, validate_nonempty_id, validate_project_slug, CreateSessionRequest,
    DeliverTurnRequest, DesktopHostKind, HostSessionReceipt, ProjectDirRecord, ProjectPathError,
    SidebarProjectBinding,
};

/// Native thread identity survives index file renames and daemon restarts.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SessionId {
    pub host: String,
    pub desktop: String,
    pub thread_id: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    Open,
    Closed,
}

/// Times are Unix seconds. Producers must replace records atomically.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SessionRecord {
    pub id: SessionId,
    pub project_cwd: PathBuf,
    pub state: SessionState,
    pub observed_at: u64,
    pub expires_at: u64,
}

#[derive(Clone, Debug, Default)]
pub struct SessionSelector {
    pub host: Option<String>,
    pub desktop: Option<String>,
    pub thread_id: Option<String>,
    pub project_cwd: Option<PathBuf>,
}

impl SessionSelector {
    pub fn matches(&self, record: &SessionRecord) -> bool {
        self.host.as_ref().is_none_or(|v| v == &record.id.host)
            && self
                .desktop
                .as_ref()
                .is_none_or(|v| v == &record.id.desktop)
            && self
                .thread_id
                .as_ref()
                .is_none_or(|v| v == &record.id.thread_id)
            && self
                .project_cwd
                .as_ref()
                .is_none_or(|v| v == &record.project_cwd)
    }
}
