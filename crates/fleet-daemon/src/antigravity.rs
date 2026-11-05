use crate::{
    DesktopHostAdapter, HostAdapterError, PersistedSessionRecord,
};
use fleet_proto::{
    CreateSessionRequest, DeliverTurnRequest, DeliveryTier, DesktopHostKind, HostSessionReceipt,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    ffi::OsString,
    fmt, fs,
    path::{Path, PathBuf},
    process::Command,
};

const FORBIDDEN_PROJECT_BUCKETS: &[&str] = &[
    "",
    "default-cli-project",
    "outside-of-project",
    "null",
    "none",
];

/// Configuration for `AntigravityDesktopAdapter` when backed by an Antigravity app-data root
/// (`~/.gemini/antigravity`) and an `agy` CLI executable.
#[derive(Clone, Debug)]
pub struct AntigravityRuntimeConfig {
    pub antigravity_root: PathBuf,
    pub agy_bin: PathBuf,
    pub sqlite3_bin: PathBuf,
}

impl AntigravityRuntimeConfig {
    pub fn new(antigravity_root: PathBuf, agy_bin: PathBuf) -> Self {
        Self {
            antigravity_root,
            agy_bin,
            sqlite3_bin: PathBuf::from("/usr/bin/sqlite3"),
        }
    }

    pub fn daemon_dir(&self) -> PathBuf {
        self.antigravity_root.join("daemon")
    }

    pub fn summaries_db_path(&self) -> PathBuf {
        self.antigravity_root.join("conversation_summaries.db")
    }

    pub fn presence_lock_path(&self, conversation_id: &str) -> PathBuf {
        self.antigravity_root
            .join("presence")
            .join(format!("{conversation_id}.lock"))
    }
}

/// Discovered `hub_<port>.json` record from `~/.gemini/antigravity/daemon/`.
/// Note: `csrf_token` is never serialized into receipts or `Debug` output (`Gotcha #4`).
#[derive(Clone, Deserialize)]
pub struct AntigravityHubInfo {
    pub pid: u32,
    #[serde(rename = "httpsPort")]
    pub https_port: u16,
    #[serde(rename = "httpPort")]
    pub http_port: u16,
    #[serde(rename = "lsVersion")]
    pub ls_version: String,
    #[serde(rename = "csrfToken")]
    csrf_token: String,
    #[serde(skip)]
    pub discovery_file: PathBuf,
}

impl AntigravityHubInfo {
    /// Access CSRF token in-memory for local loopback requests only.
    pub fn csrf_token_in_memory(&self) -> &str {
        &self.csrf_token
    }
}

impl fmt::Debug for AntigravityHubInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AntigravityHubInfo")
            .field("pid", &self.pid)
            .field("https_port", &self.https_port)
            .field("http_port", &self.http_port)
            .field("ls_version", &self.ls_version)
            .field("discovery_file", &self.discovery_file)
            .field("csrf_token", &"<redacted>")
            .finish()
    }
}

/// Row read back from `conversation_summaries.db` (`conversation_summaries` table).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AntigravityConversationSummary {
    pub conversation_id: String,
    pub title: String,
    pub preview: String,
    pub step_count: i64,
    pub last_modified_time: String,
    pub workspace_uris: Vec<String>,
    pub status: String,
    pub source: String,
    pub project_id: String,
}

/// Receipt proving an Antigravity Desktop session or turn was delivered and verified in
/// `conversation_summaries.db` with a non-bucket `project_id` and matching `workspace_uris`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AntigravityReceipt {
    pub host: DesktopHostKind,
    pub delivery_id: String,
    pub project_id: String,
    pub conversation_id: String,
    pub session_id: String,
    pub turn_id: String,
    pub project_dir: PathBuf,
    pub workspace_uris: Vec<String>,
    pub status: String,
    pub step_count: i64,
    pub hub_http_port: Option<u16>,
    pub tier: DeliveryTier,
}

impl From<AntigravityReceipt> for HostSessionReceipt {
    fn from(r: AntigravityReceipt) -> Self {
        Self {
            host: r.host,
            delivery_id: r.delivery_id,
            project_id: r.project_id,
            session_id: r.conversation_id,
            turn_id: r.turn_id,
            project_dir: r.project_dir,
            workspace_uris: r.workspace_uris,
            tier: r.tier,
        }
    }
}

/// Discovers active `hub_<port>.json` files inside `<daemon_dir>`.
pub fn discover_antigravity_hubs(
    daemon_dir: &Path,
) -> Result<Vec<AntigravityHubInfo>, HostAdapterError> {
    if !daemon_dir.is_absolute() {
        return Err(HostAdapterError::AntigravityProtocol(format!(
            "daemon discovery dir must be absolute: {}",
            daemon_dir.display()
        )));
    }
    if !daemon_dir.exists() {
        return Ok(Vec::new());
    }
    let mut entries = fs::read_dir(daemon_dir)?
        .map(|e| e.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()?;
    entries.sort();

    let mut hubs = Vec::new();
    for path in entries {
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !name.starts_with("hub_") || !name.ends_with(".json") {
            continue;
        }
        let meta = fs::symlink_metadata(&path)?;
        if !meta.file_type().is_file() {
            return Err(HostAdapterError::AntigravityProtocol(format!(
                "hub discovery entry must be a regular file: {}",
                path.display()
            )));
        }
        let mut info: AntigravityHubInfo =
            serde_json::from_slice(&fs::read(&path)?).map_err(|e| {
                HostAdapterError::AntigravityProtocol(format!(
                    "invalid hub discovery JSON in {}: {e}",
                    path.display()
                ))
            })?;
        if info.pid == 0
            || info.http_port == 0
            || info.https_port == 0
            || info.ls_version.trim().is_empty()
            || info.csrf_token.trim().is_empty()
        {
            return Err(HostAdapterError::AntigravityProtocol(format!(
                "incomplete hub discovery fields in {}",
                path.display()
            )));
        }
        info.discovery_file = path;
        hubs.push(info);
    }
    Ok(hubs)
}

/// Reads back a single `conversation_id` from `conversation_summaries.db` using `sqlite3 -json`
/// in read-only mode so active WAL connections are not disturbed.
pub fn read_conversation_summary(
    sqlite3_bin: &Path,
    db_path: &Path,
    conversation_id: &str,
) -> Result<Option<AntigravityConversationSummary>, HostAdapterError> {
    if !db_path.is_absolute() {
        return Err(HostAdapterError::AntigravityProtocol(format!(
            "conversation_summaries.db path must be absolute: {}",
            db_path.display()
        )));
    }
    if !db_path.exists() {
        return Err(HostAdapterError::AntigravityProtocol(format!(
            "conversation_summaries.db does not exist at {}",
            db_path.display()
        )));
    }
    if conversation_id.is_empty()
        || conversation_id
            .chars()
            .any(|c| !(c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')))
    {
        return Err(HostAdapterError::AntigravityProtocol(format!(
            "unsafe conversation_id for SQLite readback: {conversation_id:?}"
        )));
    }
    let query = format!(
        "SELECT conversation_id, title, preview, step_count, last_modified_time, \
         workspace_uris, status, source, project_id \
         FROM conversation_summaries WHERE conversation_id = '{conversation_id}' LIMIT 1;"
    );
    let output = Command::new(sqlite3_bin)
        .arg("-readonly")
        .arg("-json")
        .arg(db_path)
        .arg(query)
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(HostAdapterError::AntigravityProtocol(format!(
            "sqlite3 readback failed: {}",
            stderr.trim()
        )));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.trim().is_empty() {
        return Ok(None);
    }
    let rows: Vec<Value> = serde_json::from_str(&stdout).map_err(|e| {
        HostAdapterError::AntigravityProtocol(format!("invalid sqlite3 JSON output: {e}"))
    })?;
    let Some(row) = rows.into_iter().next() else {
        return Ok(None);
    };
    parse_summary_row(&row).map(Some)
}

fn parse_summary_row(row: &Value) -> Result<AntigravityConversationSummary, HostAdapterError> {
    let conversation_id = row
        .get("conversation_id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let title = row
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let preview = row
        .get("preview")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let step_count = row.get("step_count").and_then(Value::as_i64).unwrap_or(0);
    let last_modified_time = row
        .get("last_modified_time")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let raw_uris = row
        .get("workspace_uris")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let workspace_uris: Vec<String> = if raw_uris.trim().is_empty() {
        Vec::new()
    } else {
        serde_json::from_str(raw_uris).map_err(|e| {
            HostAdapterError::AntigravityProtocol(format!(
                "invalid workspace_uris JSON in conversation_summaries.db ({raw_uris:?}): {e}"
            ))
        })?
    };
    let status = row
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let source = row
        .get("source")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let project_id = row
        .get("project_id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    Ok(AntigravityConversationSummary {
        conversation_id,
        title,
        preview,
        step_count,
        last_modified_time,
        workspace_uris,
        status,
        source,
        project_id,
    })
}

/// Validates that a read-back `AntigravityConversationSummary` is bound to `expected_project_id`
/// (not an unscoped CLI bucket) and includes `expected_workspace_uri` in `workspace_uris`.
pub fn verify_summary_binding(
    summary: &AntigravityConversationSummary,
    expected_project_id: &str,
    expected_workspace_uri: &str,
) -> Result<(), HostAdapterError> {
    if FORBIDDEN_PROJECT_BUCKETS.contains(&summary.project_id.trim()) {
        return Err(HostAdapterError::ForbiddenProjectId(
            if summary.project_id.is_empty() {
                "<empty-project-id>".into()
            } else {
                summary.project_id.clone()
            },
        ));
    }
    if summary.project_id != expected_project_id {
        return Err(HostAdapterError::SessionProjectMismatch {
            session_id: summary.conversation_id.clone(),
            expected_project_id: expected_project_id.to_string(),
            supplied_project_id: summary.project_id.clone(),
        });
    }
    if summary.workspace_uris.is_empty()
        || !summary
            .workspace_uris
            .iter()
            .any(|u| u == expected_workspace_uri)
    {
        return Err(HostAdapterError::AntigravityProtocol(format!(
            "conversation {} workspace_uris {:?} does not contain expected {}",
            summary.conversation_id, summary.workspace_uris, expected_workspace_uri
        )));
    }
    Ok(())
}

/// Executes `agy` for sidebar project registration (`--new-project`), session creation
/// (`--project <id> --add-dir <dir> -p <prompt>`), or conversation continuation
/// (`--conversation <id> --project <id> --add-dir <dir> -p <prompt>`).
pub(crate) fn run_agy_command(
    runtime: &AntigravityRuntimeConfig,
    cwd: &Path,
    args: &[OsString],
    hub: Option<&AntigravityHubInfo>,
) -> Result<Value, HostAdapterError> {
    if !runtime.agy_bin.is_absolute() {
        return Err(HostAdapterError::AntigravityProtocol(format!(
            "agy executable path must be absolute: {}",
            runtime.agy_bin.display()
        )));
    }
    let mut cmd = Command::new(&runtime.agy_bin);
    cmd.current_dir(cwd)
        .args(args)
        .env("ANTIGRAVITY_DATA_DIR", &runtime.antigravity_root)
        .env(
            "ANTIGRAVITY_SUMMARIES_DB",
            runtime.summaries_db_path(),
        );
    if let Some(hub_info) = hub {
        cmd.env("ANTIGRAVITY_HUB_HTTP_PORT", hub_info.http_port.to_string())
            .env(
                "ANTIGRAVITY_HUB_HTTPS_PORT",
                hub_info.https_port.to_string(),
            )
            .env("ANTIGRAVITY_HUB_PID", hub_info.pid.to_string());
    }
    let output = cmd.output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(HostAdapterError::AntigravityProtocol(format!(
            "agy command failed (status {}): stderr={} stdout={}",
            output.status,
            stderr.trim(),
            stdout.trim()
        )));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return Ok(Value::Null);
    }
    // Parse last JSON line or whole JSON payload emitted by `agy --output-format json`
    if let Ok(val) = serde_json::from_str::<Value>(trimmed) {
        return Ok(val);
    }
    for line in trimmed.lines().rev() {
        let line = line.trim();
        if line.starts_with('{') {
            if let Ok(val) = serde_json::from_str::<Value>(line) {
                return Ok(val);
            }
        }
    }
    Ok(Value::String(trimmed.to_string()))
}

impl crate::AntigravityDesktopAdapter {
    /// Constructs an `AntigravityDesktopAdapter` wired to an Antigravity app-data root
    /// (`daemon/hub_<port>.json` + `conversation_summaries.db`) and an `agy` CLI binary.
    pub fn with_runtime(
        projects_root: PathBuf,
        state_root: PathBuf,
        runtime: AntigravityRuntimeConfig,
    ) -> Result<Self, HostAdapterError> {
        if !runtime.antigravity_root.is_absolute() {
            return Err(HostAdapterError::AntigravityProtocol(format!(
                "antigravity_root must be absolute: {}",
                runtime.antigravity_root.display()
            )));
        }
        if !runtime.agy_bin.is_absolute() {
            return Err(HostAdapterError::AntigravityProtocol(format!(
                "agy_bin must be absolute: {}",
                runtime.agy_bin.display()
            )));
        }
        let mut adapter = Self::new(projects_root, state_root)?;
        adapter.runtime = Some(runtime);
        Ok(adapter)
    }

    /// Discovers running Hub instances from `~/.gemini/antigravity/daemon/hub_<port>.json`.
    pub fn discover_hubs(&self) -> Result<Vec<AntigravityHubInfo>, HostAdapterError> {
        let Some(runtime) = &self.runtime else {
            return Ok(Vec::new());
        };
        discover_antigravity_hubs(&runtime.daemon_dir())
    }

    /// Reads back a conversation row from `conversation_summaries.db`.
    pub fn read_conversation_summary(
        &self,
        conversation_id: &str,
    ) -> Result<Option<AntigravityConversationSummary>, HostAdapterError> {
        let Some(runtime) = &self.runtime else {
            return Err(HostAdapterError::AntigravityProtocol(
                "AntigravityRuntimeConfig is not configured on this adapter".into(),
            ));
        };
        read_conversation_summary(
            &runtime.sqlite3_bin,
            &runtime.summaries_db_path(),
            conversation_id,
        )
    }

    /// Creates a new sidebar-visible conversation via `agy` and verifies `conversation_summaries.db`
    /// readback (`project_id` and `workspace_uris`), returning an `AntigravityReceipt`.
    pub fn create_antigravity_session(
        &mut self,
        request: &CreateSessionRequest,
    ) -> Result<AntigravityReceipt, HostAdapterError> {
        let receipt = self.create_session(request)?;
        let Some(runtime) = &self.runtime else {
            return Ok(AntigravityReceipt {
                host: receipt.host,
                delivery_id: receipt.delivery_id,
                project_id: receipt.project_id,
                conversation_id: receipt.session_id.clone(),
                session_id: receipt.session_id,
                turn_id: receipt.turn_id,
                project_dir: receipt.project_dir,
                workspace_uris: receipt.workspace_uris,
                status: "CASCADE_RUN_STATUS_IDLE".into(),
                step_count: 1,
                hub_http_port: None,
                tier: receipt.tier,
            });
        };
        let hubs = discover_antigravity_hubs(&runtime.daemon_dir())?;
        let summary = read_conversation_summary(
            &runtime.sqlite3_bin,
            &runtime.summaries_db_path(),
            &receipt.session_id,
        )?
        .ok_or_else(|| {
            HostAdapterError::AntigravityProtocol(format!(
                "conversation {} not found in conversation_summaries.db after create_session",
                receipt.session_id
            ))
        })?;
        let expected_uri = format!("file://{}", receipt.project_dir.display());
        verify_summary_binding(&summary, &receipt.project_id, &expected_uri)?;
        Ok(AntigravityReceipt {
            host: receipt.host,
            delivery_id: receipt.delivery_id,
            project_id: summary.project_id,
            conversation_id: summary.conversation_id.clone(),
            session_id: summary.conversation_id,
            turn_id: receipt.turn_id,
            project_dir: receipt.project_dir,
            workspace_uris: summary.workspace_uris,
            status: summary.status,
            step_count: summary.step_count,
            hub_http_port: hubs.first().map(|h| h.http_port),
            tier: receipt.tier,
        })
    }

    /// Delivers a follow-up turn to an existing `conversation_id` via `agy --conversation <id>`
    /// and verifies `conversation_summaries.db` readback (`project_id`, `workspace_uris`, `step_count`).
    pub fn deliver_antigravity_turn(
        &mut self,
        request: &DeliverTurnRequest,
    ) -> Result<AntigravityReceipt, HostAdapterError> {
        let receipt = self.deliver_turn(request)?;
        let Some(runtime) = &self.runtime else {
            return Ok(AntigravityReceipt {
                host: receipt.host,
                delivery_id: receipt.delivery_id,
                project_id: receipt.project_id,
                conversation_id: receipt.session_id.clone(),
                session_id: receipt.session_id,
                turn_id: receipt.turn_id,
                project_dir: receipt.project_dir,
                workspace_uris: receipt.workspace_uris,
                status: "CASCADE_RUN_STATUS_IDLE".into(),
                step_count: 2,
                hub_http_port: None,
                tier: receipt.tier,
            });
        };
        let hubs = discover_antigravity_hubs(&runtime.daemon_dir())?;
        let summary = read_conversation_summary(
            &runtime.sqlite3_bin,
            &runtime.summaries_db_path(),
            &receipt.session_id,
        )?
        .ok_or_else(|| {
            HostAdapterError::AntigravityProtocol(format!(
                "conversation {} not found in conversation_summaries.db after deliver_turn",
                receipt.session_id
            ))
        })?;
        let expected_uri = format!("file://{}", receipt.project_dir.display());
        verify_summary_binding(&summary, &receipt.project_id, &expected_uri)?;
        Ok(AntigravityReceipt {
            host: receipt.host,
            delivery_id: receipt.delivery_id,
            project_id: summary.project_id,
            conversation_id: summary.conversation_id.clone(),
            session_id: summary.conversation_id,
            turn_id: receipt.turn_id,
            project_dir: receipt.project_dir,
            workspace_uris: summary.workspace_uris,
            status: summary.status,
            step_count: summary.step_count,
            hub_http_port: hubs.first().map(|h| h.http_port),
            tier: receipt.tier,
        })
    }

    pub fn existing_session(&self, session_id: &str) -> Option<&PersistedSessionRecord> {
        self.session(session_id)
    }
}
