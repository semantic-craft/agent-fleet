mod execution;
pub use execution::{ExecutionError, NodeDaemon, SelectedSession};
mod desktop;
pub use desktop::{DesktopDelivery, DesktopError, DesktopReceipt};
mod antigravity;
pub use antigravity::{
    discover_antigravity_hubs, read_conversation_summary, verify_summary_binding,
    AntigravityConversationSummary, AntigravityHubInfo, AntigravityReceipt,
    AntigravityRuntimeConfig,
};
mod tri_hosts;
pub use tri_hosts::{
    attach_dsh_session_to_workspace, deterministic_dsh_workspace_uuid, dsh_cwd_bucket_slug,
    ensure_dsh_native_workspace, read_cindy_transcript, read_codex_magpie_config,
    read_codex_project, read_codex_thread, read_dsh_native_session, run_dsh_headless_command,
    verify_codex_project_binding, verify_codex_thread_binding, CindyRuntimeConfig,
    CindySessionReceipt, CindySessionSnapshot, CodexMagpieModelConfig, CodexProjectRow,
    CodexThreadRow, DshRuntimeConfig, DshSessionReceipt, DshSessionSnapshot, OpenAiDotsReceipt,
    OpenAiDotsRuntimeConfig,
};
mod host_adapter;
pub use host_adapter::{
    build_host_adapter, AntigravityDesktopAdapter, CindyAdapter, DeepSeekHarnessAdapter,
    DesktopHostAdapter, HostAdapterError, OpenAiDotsAdapter, PersistedSessionRecord,
    PersistedTurnRecord,
};
mod relay_self_dev;
pub use relay_self_dev::{
    run_host_poteto_relay, HostRelayClosureReceipt, HostRuntimeOptions,
    HostStoreVerificationEvidence, PotetoRelayTask,
};

use fleet_proto::{SessionId, SessionRecord, SessionSelector, SessionState};
use std::{collections::BTreeMap, fmt, fs, path::Path};

#[derive(Debug)]
pub enum DirectoryError {
    Io(std::io::Error),
    InvalidRecord(String),
    DuplicateIdentity(SessionId),
    Missing,
    Ambiguous(usize),
    Expired(SessionId),
    Closed(SessionId),
}

impl fmt::Display for DirectoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "session directory unavailable: {error}"),
            Self::InvalidRecord(reason) => write!(f, "invalid session record: {reason}"),
            Self::DuplicateIdentity(id) => write!(f, "duplicate session identity: {id:?}"),
            Self::Missing => write!(f, "session missing; no thread created"),
            Self::Ambiguous(count) => write!(
                f,
                "session ambiguous ({count} matches); select a thread explicitly"
            ),
            Self::Expired(id) => write!(f, "session expired: {id:?}"),
            Self::Closed(id) => write!(f, "session closed: {id:?}"),
        }
    }
}

impl std::error::Error for DirectoryError {}

impl From<std::io::Error> for DirectoryError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

/// A read-only snapshot. This type has no transport or thread-creation capability.
/// Consumers must rediscover before acting; a snapshot does not track later record changes.
#[derive(Debug)]
pub struct SessionDirectory {
    sessions: BTreeMap<SessionId, SessionRecord>,
}

impl SessionDirectory {
    pub fn discover(path: &Path, now: u64) -> Result<Self, DirectoryError> {
        let mut sessions = BTreeMap::new();
        let mut files = fs::read_dir(path)?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<Result<Vec<_>, _>>()?;
        files.sort();
        for file in files {
            if file.extension().is_none_or(|ext| ext != "json") {
                continue;
            }
            if !fs::symlink_metadata(&file)?.file_type().is_file() {
                return Err(DirectoryError::InvalidRecord(
                    "JSON entries must be regular files".into(),
                ));
            }
            let record: SessionRecord =
                serde_json::from_slice(&fs::read(&file)?).map_err(|_| {
                    DirectoryError::InvalidRecord("expected session JSON schema".into())
                })?;
            validate(&record, now)?;
            let id = record.id.clone();
            if sessions.insert(id.clone(), record).is_some() {
                return Err(DirectoryError::DuplicateIdentity(id));
            }
        }
        Ok(Self { sessions })
    }

    /// Diagnostic inventory includes closed and expired records. Use resolve for selection.
    pub fn list(&self) -> impl Iterator<Item = &SessionRecord> {
        self.sessions.values()
    }

    pub fn resolve(
        &self,
        selector: &SessionSelector,
        now: u64,
    ) -> Result<&SessionRecord, DirectoryError> {
        let matches: Vec<_> = self
            .sessions
            .values()
            .filter(|s| selector.matches(s))
            .collect();
        let record = match matches.as_slice() {
            [] => return Err(DirectoryError::Missing),
            [record] => *record,
            _ => return Err(DirectoryError::Ambiguous(matches.len())),
        };
        validate(record, now)?;
        if now >= record.expires_at {
            return Err(DirectoryError::Expired(record.id.clone()));
        }
        if record.state == SessionState::Closed {
            return Err(DirectoryError::Closed(record.id.clone()));
        }
        Ok(record)
    }
}

fn validate(record: &SessionRecord, now: u64) -> Result<(), DirectoryError> {
    if [&record.id.host, &record.id.desktop, &record.id.thread_id]
        .iter()
        .any(|s| s.is_empty() || s.trim() != s.as_str() || s.chars().any(char::is_control))
    {
        return Err(DirectoryError::InvalidRecord(
            "identity components must be nonempty and unpadded".into(),
        ));
    }
    if !record.project_cwd.is_absolute() {
        return Err(DirectoryError::InvalidRecord(
            "project_cwd must be absolute".into(),
        ));
    }
    if record.observed_at > now || record.expires_at <= record.observed_at {
        return Err(DirectoryError::InvalidRecord(
            "invalid observation or expiration time".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn session_directory_rechecks_expiration_at_selection() {
        let id = SessionId {
            host: "fixture-host".into(),
            desktop: "codex".into(),
            thread_id: "thread-1".into(),
        };
        let record = SessionRecord {
            id: id.clone(),
            project_cwd: PathBuf::from("/fixture/project"),
            state: SessionState::Open,
            observed_at: 10,
            expires_at: 20,
        };
        let directory = SessionDirectory {
            sessions: BTreeMap::from([(id, record)]),
        };
        assert!(directory.resolve(&SessionSelector::default(), 19).is_ok());
        assert!(matches!(
            directory.resolve(&SessionSelector::default(), 20),
            Err(DirectoryError::Expired(_))
        ));
    }
}
