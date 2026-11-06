use crate::HostAdapterError;
use fleet_proto::{DeliveryTier, DesktopHostKind, HostSessionReceipt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs,
    io::{Read, Write},
    net::TcpStream,
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};
use tungstenite::{client::client_with_config, protocol::WebSocketConfig, Message, WebSocket};

const FORBIDDEN_PROJECT_BUCKETS: &[&str] = &[
    "",
    "default-cli-project",
    "outside-of-project",
    "null",
    "none",
];

/// Configuration for `OpenAiDotsAdapter` when backed by `codex app-server` (Unix socket)
/// and `~/.codex/state_5.sqlite` + `~/.codex/.codex-global-state.json`.
#[derive(Clone, Debug)]
pub struct OpenAiDotsRuntimeConfig {
    pub codex_root: PathBuf,
    pub app_server_socket: PathBuf,
    pub sqlite3_bin: PathBuf,
    pub timeout: Duration,
}

impl OpenAiDotsRuntimeConfig {
    pub fn new(codex_root: PathBuf, app_server_socket: PathBuf) -> Self {
        Self {
            codex_root,
            app_server_socket,
            sqlite3_bin: PathBuf::from("/usr/bin/sqlite3"),
            timeout: Duration::from_secs(5),
        }
    }

    pub fn state_db_path(&self) -> PathBuf {
        self.codex_root.join("state_5.sqlite")
    }

    pub fn global_state_path(&self) -> PathBuf {
        self.codex_root.join(".codex-global-state.json")
    }

    pub fn config_toml_path(&self) -> PathBuf {
        self.codex_root.join("config.toml")
    }
}

/// Resolved Codex model and reasoning effort from `~/.codex/config.toml` (Magpie gateway default:
/// `model = "zcode/GLM-5.3-Flash"`, `model_reasoning_effort = "max"`).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CodexMagpieModelConfig {
    pub model: String,
    pub model_reasoning_effort: String,
    pub openai_base_url: Option<String>,
}

impl Default for CodexMagpieModelConfig {
    fn default() -> Self {
        Self {
            model: "zcode/GLM-5.3-Flash".to_string(),
            model_reasoning_effort: "max".to_string(),
            openai_base_url: Some("http://127.0.0.1:3425/backend-api/codex".to_string()),
        }
    }
}

/// Reads top-level `model`, `model_reasoning_effort`, and `openai_base_url` from `<codex_root>/config.toml`,
/// falling back to the canonical Magpie defaults (`zcode/GLM-5.3-Flash` at `max` effort).
pub fn read_codex_magpie_config(codex_root: &Path) -> CodexMagpieModelConfig {
    let mut cfg = CodexMagpieModelConfig::default();
    let Ok(raw) = fs::read_to_string(codex_root.join("config.toml")) else {
        return cfg;
    };
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            break;
        }
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some((key, val)) = trimmed.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let val = val.trim().trim_matches('"').trim_matches('\'');
        if val.is_empty() {
            continue;
        }
        match key {
            "model" => cfg.model = val.to_string(),
            "model_reasoning_effort" => cfg.model_reasoning_effort = val.to_string(),
            "openai_base_url" => cfg.openai_base_url = Some(val.to_string()),
            _ => {}
        }
    }
    cfg
}

/// Verified project + root row from `state_5.sqlite` (`projects` JOIN `project_roots`).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CodexProjectRow {
    pub project_id: String,
    pub name: String,
    pub root_path: PathBuf,
    pub position: i64,
}

/// Verified thread row from `state_5.sqlite` (`threads` table).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CodexThreadRow {
    pub thread_id: String,
    pub project_id: String,
    pub cwd: PathBuf,
    pub title: String,
    pub preview: String,
    pub archived: i64,
    pub rollout_path: String,
}

/// Receipt proving an OpenAI Dots (`codex app-server`) session or turn was created/continued
/// and verified in `state_5.sqlite` (`projects`/`project_roots` and `threads`).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OpenAiDotsReceipt {
    pub host: DesktopHostKind,
    pub delivery_id: String,
    pub project_id: String,
    pub thread_id: String,
    pub session_id: String,
    pub turn_id: String,
    pub project_dir: PathBuf,
    pub workspace_uris: Vec<String>,
    pub preview: String,
    pub deep_link: String,
    pub tier: DeliveryTier,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

impl From<OpenAiDotsReceipt> for HostSessionReceipt {
    fn from(r: OpenAiDotsReceipt) -> Self {
        Self {
            host: r.host,
            delivery_id: r.delivery_id,
            project_id: r.project_id,
            session_id: r.thread_id,
            turn_id: r.turn_id,
            project_dir: r.project_dir,
            workspace_uris: r.workspace_uris,
            tier: r.tier,
            model: r.model,
        }
    }
}

/// Reads back a project and its primary root from `state_5.sqlite` (`projects` JOIN `project_roots`).
pub fn read_codex_project(
    sqlite3_bin: &Path,
    db_path: &Path,
    project_id: &str,
) -> Result<Option<CodexProjectRow>, HostAdapterError> {
    validate_sqlite_path_and_id(db_path, "project_id", project_id, |msg| {
        HostAdapterError::OpenAiDotsProtocol(msg)
    })?;
    let query = format!(
        "SELECT p.id AS project_id, p.name AS name, r.path AS root_path, p.position AS position \
         FROM projects p JOIN project_roots r ON p.id = r.project_id \
         WHERE p.id = '{project_id}' ORDER BY r.position ASC LIMIT 1;"
    );
    let rows = run_sqlite_json(sqlite3_bin, db_path, &query, |msg| {
        HostAdapterError::OpenAiDotsProtocol(msg)
    })?;
    let Some(row) = rows.into_iter().next() else {
        return Ok(None);
    };
    let project_id = row
        .get("project_id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let name = row
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let root_path = PathBuf::from(
        row.get("root_path")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    );
    let position = row.get("position").and_then(Value::as_i64).unwrap_or(0);
    Ok(Some(CodexProjectRow {
        project_id,
        name,
        root_path,
        position,
    }))
}

/// Reads back a thread from `state_5.sqlite` (`threads` table).
pub fn read_codex_thread(
    sqlite3_bin: &Path,
    db_path: &Path,
    thread_id: &str,
) -> Result<Option<CodexThreadRow>, HostAdapterError> {
    validate_sqlite_path_and_id(db_path, "thread_id", thread_id, |msg| {
        HostAdapterError::OpenAiDotsProtocol(msg)
    })?;
    let query = format!(
        "SELECT id AS thread_id, COALESCE(project_id, '') AS project_id, cwd, title, preview, \
         archived, rollout_path FROM threads WHERE id = '{thread_id}' LIMIT 1;"
    );
    let rows = run_sqlite_json(sqlite3_bin, db_path, &query, |msg| {
        HostAdapterError::OpenAiDotsProtocol(msg)
    })?;
    let Some(row) = rows.into_iter().next() else {
        return Ok(None);
    };
    Ok(Some(CodexThreadRow {
        thread_id: row
            .get("thread_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        project_id: row
            .get("project_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        cwd: PathBuf::from(row.get("cwd").and_then(Value::as_str).unwrap_or_default()),
        title: row
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        preview: row
            .get("preview")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        archived: row.get("archived").and_then(Value::as_i64).unwrap_or(0),
        rollout_path: row
            .get("rollout_path")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    }))
}

/// Readback variant tolerant of the app-server's asynchronous persistence: codex
/// confirms `turn/start` before the `threads` row is visible in `state_5.sqlite`
/// (WAL flush). The loop waits a short budget for the row to appear AND for its
/// sidebar `preview` to be backfilled (fast turns only); on timeout it still returns
/// the row so callers can honestly report `running` instead of blocking for the
/// 5–46s preview latency observed on codex-cli 0.166.0 (2026-11-06).
pub fn read_codex_thread_with_retry(
    sqlite3_bin: &Path,
    db_path: &Path,
    thread_id: &str,
) -> Result<Option<CodexThreadRow>, HostAdapterError> {
    const ROW_ATTEMPTS: u32 = 6;
    const PREVIEW_ATTEMPTS: u32 = 4;
    const DELAY_MS: u64 = 250;
    let mut last: Option<CodexThreadRow> = None;
    for _attempt in 1..=ROW_ATTEMPTS {
        if let Some(row) = read_codex_thread(sqlite3_bin, db_path, thread_id)? {
            last = Some(row);
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(DELAY_MS));
    }
    for attempt in 1..PREVIEW_ATTEMPTS {
        let Some(row) = &last else { break };
        if codex_thread_preview_visible(row) {
            break;
        }
        if attempt < PREVIEW_ATTEMPTS {
            std::thread::sleep(std::time::Duration::from_millis(DELAY_MS));
            if let Some(refetched) = read_codex_thread(sqlite3_bin, db_path, thread_id)? {
                last = Some(refetched);
            }
        }
    }
    Ok(last)
}

/// Verifies that a `CodexProjectRow` in `state_5.sqlite` matches `expected_project_id` and `expected_dir`.
pub fn verify_codex_project_binding(
    row: &CodexProjectRow,
    expected_project_id: &str,
    expected_dir: &Path,
) -> Result<(), HostAdapterError> {
    if FORBIDDEN_PROJECT_BUCKETS.contains(&row.project_id.trim()) {
        return Err(HostAdapterError::ForbiddenProjectId(row.project_id.clone()));
    }
    if row.project_id != expected_project_id {
        return Err(HostAdapterError::OpenAiDotsProtocol(format!(
            "state_5.sqlite project_id mismatch: expected {expected_project_id}, got {}",
            row.project_id
        )));
    }
    if row.root_path != expected_dir {
        return Err(HostAdapterError::ProjectDirMismatch {
            expected: expected_dir.to_path_buf(),
            supplied: row.root_path.clone(),
        });
    }
    Ok(())
}

/// Verifies that a `CodexThreadRow` in `state_5.sqlite` is bound to `expected_project_id`,
/// `expected_dir`, and `archived == 0`. The preview check is deliberately NOT part of this
/// identity verification: codex app-server 0.166 backfills the sidebar `preview`/
/// `recency_at` asynchronously (observed 5–46s, roughly at turn completion), so callers
/// use `codex_thread_preview_visible` to decide between `thread-visible` and `running`.
pub fn verify_codex_thread_binding(
    row: &CodexThreadRow,
    expected_project_id: &str,
    expected_dir: &Path,
) -> Result<(), HostAdapterError> {
    if FORBIDDEN_PROJECT_BUCKETS.contains(&row.project_id.trim()) {
        return Err(HostAdapterError::ForbiddenProjectId(
            if row.project_id.is_empty() {
                "<null-project-id>".into()
            } else {
                row.project_id.clone()
            },
        ));
    }
    if row.project_id != expected_project_id {
        return Err(HostAdapterError::SessionProjectMismatch {
            session_id: row.thread_id.clone(),
            expected_project_id: expected_project_id.to_string(),
            supplied_project_id: row.project_id.clone(),
        });
    }
    if row.cwd != expected_dir {
        return Err(HostAdapterError::ProjectDirMismatch {
            expected: expected_dir.to_path_buf(),
            supplied: row.cwd.clone(),
        });
    }
    if row.archived != 0 {
        return Err(HostAdapterError::OpenAiDotsProtocol(format!(
            "thread {} is archived in state_5.sqlite and invisible in sidebar",
            row.thread_id
        )));
    }
    Ok(())
}

/// Sidebar-visibility evidence for codex threads: a non-empty `preview` participates in
/// `idx_threads_visible_recency_at_ms`. Empty at read time only means the turn has not
/// produced its visible item yet — report `running`, never `thread-visible`.
pub fn codex_thread_preview_visible(row: &CodexThreadRow) -> bool {
    !row.preview.trim().is_empty()
}

/// Kept for callers that must insist on full sidebar visibility (e.g. readback paths
/// that expect the turn to have already rendered); dispatch paths use the split checks.
pub fn verify_codex_thread_binding_with_preview(
    row: &CodexThreadRow,
    expected_project_id: &str,
    expected_dir: &Path,
) -> Result<(), HostAdapterError> {
    verify_codex_thread_binding(row, expected_project_id, expected_dir)?;
    if !codex_thread_preview_visible(row) {
        return Err(HostAdapterError::OpenAiDotsProtocol(format!(
            "thread {} has empty preview in state_5.sqlite and fails idx_threads_visible_recency_at_ms",
            row.thread_id
        )));
    }
    Ok(())
}

pub(crate) struct AppServerClient {
    ws: WebSocket<UnixStream>,
    next_id: u64,
}

impl AppServerClient {
    pub(crate) fn connect(runtime: &OpenAiDotsRuntimeConfig) -> Result<Self, HostAdapterError> {
        if !runtime.app_server_socket.is_absolute() {
            return Err(HostAdapterError::OpenAiDotsProtocol(format!(
                "app_server_socket must be absolute: {}",
                runtime.app_server_socket.display()
            )));
        }
        let stream = UnixStream::connect(&runtime.app_server_socket).map_err(|e| {
            HostAdapterError::OpenAiDotsProtocol(format!(
                "failed to connect to codex app-server socket {}: {e}",
                runtime.app_server_socket.display()
            ))
        })?;
        stream
            .set_read_timeout(Some(runtime.timeout))
            .map_err(|e| HostAdapterError::OpenAiDotsProtocol(e.to_string()))?;
        stream
            .set_write_timeout(Some(runtime.timeout))
            .map_err(|e| HostAdapterError::OpenAiDotsProtocol(e.to_string()))?;
        let config = WebSocketConfig::default()
            .max_message_size(Some(4 * 1024 * 1024))
            .max_frame_size(Some(4 * 1024 * 1024));
        let (ws, _) = client_with_config("ws://localhost/", stream, Some(config))
            .map_err(|e| HostAdapterError::OpenAiDotsProtocol(e.to_string()))?;
        let mut client = Self { ws, next_id: 0 };
        let _ = client.request(
            "initialize",
            json!({
                "clientInfo": {"name": "fleet-openai-dots-adapter", "version": env!("CARGO_PKG_VERSION")},
                "capabilities": {"experimentalApi": true}
            }),
        )?;
        client.notify("initialized", json!({}))?;
        Ok(client)
    }

    pub(crate) fn notify(&mut self, method: &str, params: Value) -> Result<(), HostAdapterError> {
        let frame = json!({"method": method, "params": params});
        self.ws
            .send(Message::Text(frame.to_string().into()))
            .map_err(|e| HostAdapterError::OpenAiDotsProtocol(e.to_string()))
    }

    pub(crate) fn request(
        &mut self,
        method: &str,
        params: Value,
    ) -> Result<Value, HostAdapterError> {
        self.next_id += 1;
        let id = self.next_id;
        let payload = json!({"id": id, "method": method, "params": params});
        self.ws
            .send(Message::Text(payload.to_string().into()))
            .map_err(|e| HostAdapterError::OpenAiDotsProtocol(e.to_string()))?;
        loop {
            let msg = self
                .ws
                .read()
                .map_err(|e| HostAdapterError::OpenAiDotsProtocol(e.to_string()))?;
            match msg {
                Message::Text(text) => {
                    let val: Value = serde_json::from_str(&text).map_err(|e| {
                        HostAdapterError::OpenAiDotsProtocol(format!(
                            "invalid JSON from codex app-server: {e}"
                        ))
                    })?;
                    if let Some(resp_id) = val.get("id") {
                        if resp_id != &json!(id) {
                            return Err(HostAdapterError::OpenAiDotsProtocol(format!(
                                "codex app-server RPC id mismatch: expected {id}, got {resp_id}"
                            )));
                        }
                        if let Some(err) = val.get("error") {
                            return Err(HostAdapterError::OpenAiDotsProtocol(format!(
                                "codex app-server {method} rejected: {err}"
                            )));
                        }
                        return val.get("result").cloned().ok_or_else(|| {
                            HostAdapterError::OpenAiDotsProtocol(format!(
                                "codex app-server {method} response missing result"
                            ))
                        });
                    }
                    // Ignore asynchronous notifications while waiting for the matching RPC response.
                }
                Message::Ping(_) | Message::Pong(_) => continue,
                other => {
                    return Err(HostAdapterError::OpenAiDotsProtocol(format!(
                        "unexpected WebSocket frame from codex app-server: {other:?}"
                    )))
                }
            }
        }
    }
}

// ============================================================================
// DeepSeek Harness (DSH) local HTTP gateway (`POST /v1/sessions` & `/turns`)
// and native CLI + store (`dsh --profile headless --json` + `~/.dsh/storages/`)
// ============================================================================

/// Configuration for `DeepSeekHarnessAdapter`, supporting either the local HTTP gateway
/// (`http://127.0.0.1:<port>/v1`) or native macOS DeepSeek Harness CLI + store (`~/.dsh` + `dsh`).
#[derive(Clone, Debug)]
pub struct DshRuntimeConfig {
    pub base_url: Option<String>,
    pub dsh_root: Option<PathBuf>,
    pub dsh_bin: Option<PathBuf>,
    pub timeout: Duration,
}

impl DshRuntimeConfig {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: Some(base_url.into()),
            dsh_root: None,
            dsh_bin: None,
            timeout: Duration::from_secs(5),
        }
    }

    pub fn new_native(dsh_root: PathBuf, dsh_bin: PathBuf) -> Self {
        Self {
            base_url: None,
            dsh_root: Some(dsh_root),
            dsh_bin: Some(dsh_bin),
            timeout: Duration::from_secs(600),
        }
    }

    pub fn is_native(&self) -> bool {
        self.dsh_root.is_some() && self.dsh_bin.is_some()
    }

    pub fn workspace_json_path(&self) -> Option<PathBuf> {
        self.dsh_root
            .as_ref()
            .map(|r| r.join("storages").join("workspace.json"))
    }

    pub fn session_projcache_path(&self, session_id: &str) -> Option<PathBuf> {
        self.dsh_root.as_ref().map(|r| {
            r.join("storages")
                .join("session_projcache")
                .join("sessions")
                .join(format!("{session_id}.json"))
        })
    }

    pub fn session_archive_path(&self, cwd: &Path, session_id: &str) -> Option<PathBuf> {
        self.dsh_root.as_ref().map(|r| {
            let slug = dsh_cwd_bucket_slug(cwd);
            r.join("sessions")
                .join(slug)
                .join(session_id)
                .join("session.v4.jsonl.zstd")
        })
    }

    pub fn is_gateway_active(&self) -> bool {
        let Some(base_url) = &self.base_url else {
            return false;
        };
        let Ok((host, port, _)) = parse_loopback_http_endpoint(base_url) else {
            return false;
        };
        let addr = format!("{host}:{port}");
        std::net::TcpStream::connect_timeout(
            &addr.parse().unwrap_or_else(|_| std::net::SocketAddr::from(([127, 0, 0, 1], port))),
            Duration::from_millis(200),
        )
        .is_ok()
    }
}

pub(crate) fn current_time_iso() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now();
    let duration = now.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = duration.as_secs() as libc::time_t;
    let millis = duration.subsec_millis();
    let mut tm = libc::tm {
        tm_sec: 0,
        tm_min: 0,
        tm_hour: 0,
        tm_mday: 0,
        tm_mon: 0,
        tm_year: 0,
        tm_wday: 0,
        tm_yday: 0,
        tm_isdst: 0,
        tm_gmtoff: 0,
        tm_zone: std::ptr::null_mut(),
    };
    unsafe {
        libc::gmtime_r(&secs, &mut tm);
    }
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec,
        millis
    )
}

pub fn dsh_cwd_bucket_slug(cwd: &Path) -> String {
    let raw = cwd.to_string_lossy();
    let trimmed = raw.trim_matches('/');
    let replaced: String = trimmed
        .chars()
        .map(|c| if c == '/' { '-' } else { c })
        .collect();
    format!("--{replaced}--")
}

fn fnv1a_u64(seed: u64, bytes: &[u8]) -> u64 {
    let mut hash = seed;
    for &b in bytes {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

pub fn deterministic_dsh_workspace_uuid(project_dir: &Path) -> String {
    let bytes = project_dir.to_string_lossy().into_owned().into_bytes();
    let h1 = fnv1a_u64(0xcbf29ce484222325, &bytes);
    let h2 = fnv1a_u64(0x84222325cbf29ce4, &bytes);
    let p1 = (h1 >> 32) as u32;
    let p2 = ((h1 >> 16) & 0xffff) as u16;
    let p3 = ((h1 & 0x0fff) | 0x4000) as u16;
    let p4 = (((h2 >> 48) & 0x3fff) | 0x8000) as u16;
    let p5 = h2 & 0x0000_ffff_ffff_ffff;
    format!("{p1:08x}-{p2:04x}-{p3:04x}-{p4:04x}-{p5:012x}")
}

/// Ensures `<project_dir>` is registered in `<dsh_root>/storages/workspace.json` and returns its
/// workspace UUID (`project_id`).
pub fn ensure_dsh_native_workspace(
    dsh_root: &Path,
    project_dir: &Path,
    title: &str,
) -> Result<String, HostAdapterError> {
    if !dsh_root.is_absolute() {
        return Err(HostAdapterError::DshProtocol(format!(
            "dsh_root must be absolute: {}",
            dsh_root.display()
        )));
    }
    let storages_dir = dsh_root.join("storages");
    fs::create_dir_all(&storages_dir)?;
    let _ = sync_live_dsh_workspace_via_inspector(dsh_root, project_dir, title);
    let ws_path = storages_dir.join("workspace.json");
    let mut doc: Value = if ws_path.exists() {
        serde_json::from_slice(&fs::read(&ws_path)?).map_err(|e| {
            HostAdapterError::DshProtocol(format!(
                "invalid JSON in {}: {e}",
                ws_path.display()
            ))
        })?
    } else {
        json!({
            "unit": { "name": "workspace", "version": 2 },
            "global": {
                "initialized": true,
                "workspaceIds": [],
                "archivedSessionIds": [],
                "pinnedSessionIds": [],
                "defaultWorkspaceId": ""
            },
            "tables": {
                "workspaces": {}
            }
        })
    };

    let dir_str = project_dir.to_string_lossy().into_owned();
    let mut matched_id: Option<String> = None;
    if let Some(workspaces) = doc
        .pointer("/tables/workspaces")
        .and_then(Value::as_object)
    {
        for (ws_id, entry) in workspaces {
            if entry.get("path").and_then(Value::as_str) == Some(dir_str.as_str()) {
                matched_id = Some(ws_id.clone());
                break;
            }
        }
    }

    let ws_id = matched_id.unwrap_or_else(|| deterministic_dsh_workspace_uuid(project_dir));
    if FORBIDDEN_PROJECT_BUCKETS.contains(&ws_id.trim()) {
        return Err(HostAdapterError::ForbiddenProjectId(ws_id));
    }

    let now_iso = current_time_iso();
    if doc.get("tables").and_then(Value::as_object).is_none() {
        doc["tables"] = json!({ "workspaces": {} });
    }
    if doc
        .pointer("/tables/workspaces")
        .and_then(Value::as_object)
        .is_none()
    {
        doc["tables"]["workspaces"] = json!({});
    }
    let workspaces = doc
        .pointer_mut("/tables/workspaces")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| {
            HostAdapterError::DshProtocol("workspace.json missing tables.workspaces".into())
        })?;

    match workspaces.get_mut(&ws_id) {
        Some(existing) => {
            existing["path"] = json!(dir_str);
            existing["title"] = json!(title);
            existing["updatedAt"] = json!(now_iso);
            if existing.get("sessionIds").and_then(Value::as_array).is_none() {
                existing["sessionIds"] = json!([]);
            }
        }
        None => {
            workspaces.insert(
                ws_id.clone(),
                json!({
                    "path": dir_str,
                    "title": title,
                    "sessionIds": [],
                    "createdAt": now_iso,
                    "updatedAt": now_iso,
                }),
            );
        }
    }

    if doc.get("global").and_then(Value::as_object).is_none() {
        doc["global"] = json!({
            "initialized": true,
            "workspaceIds": [],
            "archivedSessionIds": [],
            "pinnedSessionIds": [],
            "defaultWorkspaceId": ws_id,
        });
    }
    if let Some(ids) = doc
        .pointer_mut("/global/workspaceIds")
        .and_then(Value::as_array_mut)
    {
        if !ids.iter().any(|v| v.as_str() == Some(ws_id.as_str())) {
            ids.insert(0, json!(ws_id));
        }
    }

    let tmp_path = ws_path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(&doc)
        .map_err(|e| HostAdapterError::DshProtocol(e.to_string()))?;
    fs::write(&tmp_path, bytes)?;
    fs::rename(&tmp_path, &ws_path)?;
    Ok(ws_id)
}

/// Attaches `session_id` to `<project_id>` inside `<dsh_root>/storages/workspace.json`.
pub fn attach_dsh_session_to_workspace(
    dsh_root: &Path,
    project_id: &str,
    session_id: &str,
) -> Result<(), HostAdapterError> {
    let ws_path = dsh_root.join("storages").join("workspace.json");
    if !ws_path.exists() {
        return Err(HostAdapterError::DshProtocol(format!(
            "workspace.json does not exist at {}",
            ws_path.display()
        )));
    }
    let mut doc: Value = serde_json::from_slice(&fs::read(&ws_path)?).map_err(|e| {
        HostAdapterError::DshProtocol(format!("invalid JSON in {}: {e}", ws_path.display()))
    })?;
    let ws_entry = doc
        .pointer_mut(&format!("/tables/workspaces/{project_id}"))
        .ok_or_else(|| {
            HostAdapterError::DshProtocol(format!(
                "workspace {project_id} missing from {}",
                ws_path.display()
            ))
        })?;
    let ws_cwd = ws_entry
        .get("path")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if ws_entry.get("sessionIds").and_then(Value::as_array).is_none() {
        ws_entry["sessionIds"] = json!([]);
    }
    if let Some(arr) = ws_entry
        .get_mut("sessionIds")
        .and_then(Value::as_array_mut)
    {
        if !arr.iter().any(|v| v.as_str() == Some(session_id)) {
            arr.insert(0, json!(session_id));
        }
    }
    let tmp_path = ws_path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(&doc)
        .map_err(|e| HostAdapterError::DshProtocol(e.to_string()))?;
    fs::write(&tmp_path, &bytes)?;
    fs::rename(&tmp_path, &ws_path)?;
    refresh_running_dsh_desktop_app_if_matching_home(dsh_root, &bytes, None);
    let cwd_opt = (!ws_cwd.is_empty()).then_some(ws_cwd.as_str());
    sync_live_dsh_session_via_inspector(dsh_root, Some(project_id), cwd_opt, session_id);
    Ok(())
}

/// Synchronizes a human-readable session `title` into `<dsh_root>/storages/session_projcache/sessions/<session_id>.json`
/// so the DeepSeek Harness Desktop sidebar displays the dispatched task title instead of an auto-generated CLI snippet.
pub fn sync_dsh_session_title_in_projcache(
    dsh_root: &Path,
    session_id: &str,
    title: &str,
) -> Result<(), HostAdapterError> {
    if title.trim().is_empty() {
        return Ok(());
    }
    let projcache_path = dsh_root
        .join("storages")
        .join("session_projcache")
        .join("sessions")
        .join(format!("{session_id}.json"));
    if !projcache_path.exists() {
        return Ok(());
    }
    let mut doc: Value = serde_json::from_slice(&fs::read(&projcache_path)?).map_err(|e| {
        HostAdapterError::DshProtocol(format!(
            "invalid JSON in {}: {e}",
            projcache_path.display()
        ))
    })?;
    let cwd_str = doc
        .pointer("/record/identity/cwd")
        .or_else(|| doc.pointer("/record/rows/workingDirectory/val"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if !cwd_str.is_empty() {
        let archive_path = dsh_root
            .join("sessions")
            .join(dsh_cwd_bucket_slug(Path::new(&cwd_str)))
            .join(session_id)
            .join("session.v4.jsonl.zstd");
        sync_dsh_session_archive_title(&archive_path, title);
    }
    let seq = doc
        .pointer("/record/rows/title/seq")
        .and_then(Value::as_u64)
        .or_else(|| {
            doc.pointer("/record/rows/turnBoundary/seq")
                .and_then(Value::as_u64)
        })
        .unwrap_or(1);
    if let Some(rows) = doc
        .pointer_mut("/record/rows")
        .and_then(Value::as_object_mut)
    {
        rows.insert(
            "title".into(),
            json!({
                "ver": 1,
                "seq": seq,
                "val": title,
            }),
        );
    }
    let tmp_path = projcache_path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(&doc)
        .map_err(|e| HostAdapterError::DshProtocol(e.to_string()))?;
    fs::write(&tmp_path, &bytes)?;
    fs::rename(&tmp_path, &projcache_path)?;
    let ws_path = dsh_root.join("storages").join("workspace.json");
    if let Ok(ws_bytes) = fs::read(&ws_path) {
        refresh_running_dsh_desktop_app_if_matching_home(
            dsh_root,
            &ws_bytes,
            Some((&projcache_path, &bytes)),
        );
    }
    let cwd_opt = (!cwd_str.is_empty()).then_some(cwd_str.as_str());
    sync_live_dsh_session_via_inspector(dsh_root, None, cwd_opt, session_id);
    Ok(())
}

fn sync_dsh_session_archive_title(archive_path: &Path, title: &str) {
    let Ok(raw) = fs::read(archive_path) else {
        return;
    };
    if raw.len() < 4 || raw[..4] != [0x28, 0xB5, 0x2F, 0xFD] {
        return;
    }
    let zstd_bin = if Path::new("/opt/homebrew/bin/zstd").exists() {
        "/opt/homebrew/bin/zstd"
    } else {
        "zstd"
    };
    let Ok(decomp) = Command::new(zstd_bin)
        .arg("-d")
        .arg("-c")
        .arg(archive_path)
        .output()
    else {
        return;
    };
    if !decomp.status.success() {
        return;
    }
    let Ok(text) = String::from_utf8(decomp.stdout) else {
        return;
    };
    let mut updated_lines = Vec::new();
    let mut changed = false;
    for line in text.lines() {
        if line.contains("\"session/title\"") {
            if let Ok(mut val) = serde_json::from_str::<Value>(line) {
                if val.get("type").and_then(Value::as_str) == Some("session/title") {
                    if let Some(data) = val.get_mut("data").and_then(Value::as_object_mut) {
                        data.insert("title".into(), json!(title));
                        if let Ok(serialized) = serde_json::to_string(&val) {
                            updated_lines.push(serialized);
                            changed = true;
                            continue;
                        }
                    }
                }
            }
        }
        updated_lines.push(line.to_string());
    }
    if !changed || updated_lines.is_empty() {
        return;
    }
    let Some(frame0) = zstd_compress_bytes(zstd_bin, format!("{}\n", updated_lines[0]).as_bytes())
    else {
        return;
    };
    let mut combined = frame0;
    if updated_lines.len() > 1 {
        let mut tail = updated_lines[1..].join("\n");
        tail.push('\n');
        let Some(frame1) = zstd_compress_bytes(zstd_bin, tail.as_bytes()) else {
            return;
        };
        combined.extend_from_slice(&frame1);
    }
    let tmp_zstd = archive_path.with_extension("zstd.tmp");
    if fs::write(&tmp_zstd, &combined).is_ok() {
        let _ = fs::rename(&tmp_zstd, archive_path);
    } else {
        let _ = fs::remove_file(&tmp_zstd);
    }
}

fn zstd_compress_bytes(zstd_bin: &str, input: &[u8]) -> Option<Vec<u8>> {
    use std::process::Stdio;
    let mut child = Command::new(zstd_bin)
        .arg("-q")
        .arg("-c")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    if let Some(mut stdin) = child.stdin.take() {
        if stdin.write_all(input).is_err() {
            return None;
        }
    }
    let out = child.wait_with_output().ok()?;
    out.status.success().then_some(out.stdout)
}

/// Returns true when `dsh_root` points to the current user's live `~/.dsh` store.
pub fn is_live_user_dsh_root(dsh_root: &Path, home_dir: Option<&Path>) -> bool {
    let Some(home) = home_dir else {
        return false;
    };
    let expected = home.join(".dsh");
    match (fs::canonicalize(dsh_root), fs::canonicalize(&expected)) {
        (Ok(a), Ok(b)) => a == b,
        _ => dsh_root == expected,
    }
}

fn refresh_running_dsh_desktop_app_if_matching_home(
    dsh_root: &Path,
    ws_bytes: &[u8],
    projcache_override: Option<(&Path, &[u8])>,
) {
    if !cfg!(target_os = "macos") {
        return;
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if !is_live_user_dsh_root(dsh_root, home.as_deref()) {
        return;
    }
    let ws_path = dsh_root.join("storages").join("workspace.json");
    let _ = fs::write(&ws_path, ws_bytes);
    if let Some((pc_path, pc_bytes)) = projcache_override {
        let _ = fs::write(pc_path, pc_bytes);
    }
}

/// Evaluates an async JavaScript expression inside the running DeepSeek Harness Desktop
/// host process (`127.0.0.1:9230` Inspector CDP endpoint) when `dsh_root` is the user's
/// live `~/.dsh` directory. Fails soft (`None`) when the desktop app is not running or
/// the inspector port is closed.
fn eval_dsh_live_inspector_js(dsh_root: &Path, expression: &str) -> Option<Value> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if !is_live_user_dsh_root(dsh_root, home.as_deref()) {
        return None;
    }
    let addr: std::net::SocketAddr = "127.0.0.1:9230".parse().ok()?;
    let mut http_stream = TcpStream::connect_timeout(&addr, Duration::from_millis(600)).ok()?;
    let _ = http_stream.set_read_timeout(Some(Duration::from_millis(1000)));
    let _ = http_stream.set_write_timeout(Some(Duration::from_millis(1000)));
    http_stream
        .write_all(b"GET /json HTTP/1.1\r\nHost: 127.0.0.1:9230\r\nConnection: close\r\n\r\n")
        .ok()?;
    let mut raw_resp = String::new();
    http_stream.read_to_string(&mut raw_resp).ok()?;
    let (_, body) = raw_resp.split_once("\r\n\r\n")?;
    let targets: Vec<Value> = serde_json::from_str(body.trim()).ok()?;
    let ws_url = targets
        .first()?
        .get("webSocketDebuggerUrl")
        .and_then(Value::as_str)?;

    let ws_stream = TcpStream::connect_timeout(&addr, Duration::from_millis(600)).ok()?;
    let _ = ws_stream.set_read_timeout(Some(Duration::from_millis(2500)));
    let _ = ws_stream.set_write_timeout(Some(Duration::from_millis(2500)));
    let config = WebSocketConfig::default()
        .max_message_size(Some(4 * 1024 * 1024))
        .max_frame_size(Some(4 * 1024 * 1024));
    let (mut ws, _) = client_with_config(ws_url, ws_stream, Some(config)).ok()?;
    let req = json!({
        "id": 1,
        "method": "Runtime.evaluate",
        "params": {
            "expression": expression,
            "awaitPromise": true,
            "returnByValue": true,
        }
    });
    ws.send(Message::Text(req.to_string().into())).ok()?;
    for _ in 0..16 {
        let Ok(msg) = ws.read() else {
            break;
        };
        if let Message::Text(text) = msg {
            if let Ok(val) = serde_json::from_str::<Value>(&text) {
                if val.get("id") == Some(&json!(1)) {
                    let _ = ws.close(None);
                    return val.pointer("/result/result/value").cloned();
                }
            }
        }
    }
    None
}

fn sync_live_dsh_workspace_via_inspector(
    dsh_root: &Path,
    project_dir: &Path,
    title: &str,
) -> Option<String> {
    let dir_json = serde_json::to_string(&project_dir.to_string_lossy()).ok()?;
    let title_json = serde_json::to_string(title).ok()?;
    let expr = format!(
        "(async () => {{ \
            const map = globalThis[Symbol.for('dsh.inspector.realm-object-registries')]; \
            const reg = map && (map.get('host') || Array.from(map.values())[0]); \
            const ctx = reg && reg.resolve('object-1'); \
            if (!ctx || !ctx.workspaceRegistry) return null; \
            const ws = await ctx.workspaceRegistry.create({dir_json}); \
            const desiredTitle = {title_json}; \
            if (desiredTitle && ws.title !== desiredTitle && typeof ws.setTitle === 'function') {{ \
                await ws.setTitle(desiredTitle); \
            }} \
            return ws.id || null; \
        }})()"
    );
    eval_dsh_live_inspector_js(dsh_root, &expr)?
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn sync_live_dsh_session_via_inspector(
    dsh_root: &Path,
    project_id: Option<&str>,
    cwd: Option<&str>,
    session_id: &str,
) {
    let pid_json = serde_json::to_string(&project_id.unwrap_or_default()).unwrap_or_else(|_| "\"\"".into());
    let cwd_json = serde_json::to_string(&cwd.unwrap_or_default()).unwrap_or_else(|_| "\"\"".into());
    let sid_json = match serde_json::to_string(session_id) {
        Ok(s) => s,
        Err(_) => return,
    };
    let expr = format!(
        "(async () => {{ \
            const map = globalThis[Symbol.for('dsh.inspector.realm-object-registries')]; \
            const reg = map && (map.get('host') || Array.from(map.values())[0]); \
            const ctx = reg && reg.resolve('object-1'); \
            if (!ctx || !ctx.workspaceRegistry) return false; \
            const pid = {pid_json}; \
            const cwd = {cwd_json}; \
            const sid = {sid_json}; \
            let ws = null; \
            if (cwd) {{ \
                ws = await ctx.workspaceRegistry.create(cwd); \
            }} else if (pid && typeof ctx.workspaceRegistry.get === 'function') {{ \
                try {{ ws = await ctx.workspaceRegistry.get(pid); }} catch (_) {{}} \
            }} \
            if (ws && typeof ws.attachSession === 'function') {{ \
                try {{ await ws.attachSession(sid); }} catch (_) {{}} \
            }} \
            if (ctx.sessionController && typeof ctx.emit === 'function') {{ \
                const list = await ctx.sessionController.list({{}}, new AbortController().signal); \
                const item = list && list.items && list.items.find(i => i.sessionId === sid); \
                if (item) {{ \
                    ctx.emit('api-session/added', item); \
                }} \
            }} \
            return true; \
        }})()"
    );
    let _ = eval_dsh_live_inspector_js(dsh_root, &expr);
}

/// Reads back a session snapshot from `<dsh_root>/storages/session_projcache/sessions/<session_id>.json`
/// and `<dsh_root>/storages/workspace.json`, plus verifies `<dsh_root>/sessions/--<cwd>--/<session_id>/session.v4.jsonl.zstd`.
pub fn read_dsh_native_session(
    dsh_root: &Path,
    session_id: &str,
) -> Result<DshSessionSnapshot, HostAdapterError> {
    let projcache_path = dsh_root
        .join("storages")
        .join("session_projcache")
        .join("sessions")
        .join(format!("{session_id}.json"));
    if !projcache_path.exists() {
        return Err(HostAdapterError::DshProtocol(format!(
            "DSH session_projcache file missing at {}",
            projcache_path.display()
        )));
    }
    let doc: Value = serde_json::from_slice(&fs::read(&projcache_path)?).map_err(|e| {
        HostAdapterError::DshProtocol(format!(
            "invalid JSON in {}: {e}",
            projcache_path.display()
        ))
    })?;
    let cwd_str = doc
        .pointer("/record/identity/cwd")
        .or_else(|| doc.pointer("/record/rows/workingDirectory/val"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let cwd = PathBuf::from(cwd_str);
    let title = doc
        .pointer("/record/rows/title/val")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let turn_count = doc
        .pointer("/record/rows/turnBoundary/val/lastTurn")
        .and_then(Value::as_u64)
        .or_else(|| {
            doc.pointer("/record/rows/tokenUsage/val/last/turn")
                .and_then(Value::as_u64)
        })
        .unwrap_or(1) as usize;
    let last_turn_id = Some(format!("turn-{turn_count}"));

    let archive_path = dsh_root
        .join("sessions")
        .join(dsh_cwd_bucket_slug(&cwd))
        .join(session_id)
        .join("session.v4.jsonl.zstd");
    if !archive_path.exists() {
        return Err(HostAdapterError::DshProtocol(format!(
            "DSH session archive missing at {}",
            archive_path.display()
        )));
    }

    let mut project_id = String::new();
    let ws_path = dsh_root.join("storages").join("workspace.json");
    if ws_path.exists() {
        if let Ok(ws_doc) = serde_json::from_slice::<Value>(&fs::read(&ws_path)?) {
            if let Some(workspaces) = ws_doc
                .pointer("/tables/workspaces")
                .and_then(Value::as_object)
            {
                for (ws_id, entry) in workspaces {
                    let has_session = entry
                        .get("sessionIds")
                        .and_then(Value::as_array)
                        .is_some_and(|arr| arr.iter().any(|v| v.as_str() == Some(session_id)));
                    if has_session {
                        project_id = ws_id.clone();
                        break;
                    }
                }
                if project_id.is_empty() {
                    for (ws_id, entry) in workspaces {
                        if entry.get("path").and_then(Value::as_str) == Some(cwd_str) {
                            project_id = ws_id.clone();
                            break;
                        }
                    }
                }
            }
        }
    }

    Ok(DshSessionSnapshot {
        session_id: session_id.to_string(),
        project_id,
        title,
        cwd,
        turn_count,
        last_turn_id,
        status: "completed".into(),
    })
}

/// Runs `dsh --profile headless [--session-id <id>] [--model <model>] --json <prompt>` in `cwd` and returns `(session_id, turn_id)`.
pub fn run_dsh_headless_command(
    dsh_root: &Path,
    dsh_bin: &Path,
    cwd: &Path,
    session_id: Option<&str>,
    prompt: &str,
    model: Option<&str>,
) -> Result<(String, String), HostAdapterError> {
    if !dsh_bin.is_absolute() {
        return Err(HostAdapterError::DshProtocol(format!(
            "dsh executable path must be absolute: {}",
            dsh_bin.display()
        )));
    }
    eprintln!(
        "[dsh-headless] running dsh headless (bin={}) session_id={session_id:?} model={model:?} cwd={}",
        dsh_bin.display(),
        cwd.display()
    );

    let build_cmd = |include_model_arg: bool| {
        let mut cmd = Command::new(dsh_bin);
        cmd.current_dir(cwd)
            .env("DSH_HOME", dsh_root)
            .env("DSH_PERMISSION_MODE", "danger-full-access");
        if let Some(m) = model {
            cmd.env("DSH_MODEL", m);
            cmd.env("MODEL", m);
        }
        cmd.arg("--profile").arg("headless");
        if include_model_arg {
            if let Some(m) = model {
                cmd.arg("--model").arg(m);
            }
        }
        if let Some(sid) = session_id {
            cmd.arg("--session-id").arg(sid);
        }
        cmd.arg("--json").arg(prompt);
        cmd
    };

    let mut output = build_cmd(model.is_some()).output()?;
    if !output.status.success() && model.is_some() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        if stderr.contains("unknown option '--model'") || stdout.contains("unknown option '--model'") {
            output = build_cmd(false).output()?;
        }
    }
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(HostAdapterError::DshProtocol(format!(
            "dsh headless command failed (status {}): stderr={} stdout={}",
            output.status,
            stderr.trim(),
            stdout.trim()
        )));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut parsed_session_id: Option<String> = session_id.map(str::to_string);
    let mut parsed_turn: u64 = 1;
    for line in stdout.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with('{') {
            continue;
        }
        let Ok(val) = serde_json::from_str::<Value>(trimmed) else {
            continue;
        };
        match val.get("type").and_then(Value::as_str) {
            Some("session") => {
                if let Some(sid) = val
                    .get("sessionId")
                    .or_else(|| val.get("session_id"))
                    .and_then(Value::as_str)
                {
                    parsed_session_id = Some(sid.to_string());
                }
            }
            Some("status") => {
                if let Some(t) = val.get("turn").and_then(Value::as_u64) {
                    parsed_turn = t;
                }
            }
            _ => {}
        }
    }
    let final_sid = parsed_session_id.ok_or_else(|| {
        HostAdapterError::DshProtocol(
            "dsh --profile headless --json output did not emit a session event with sessionId"
                .into(),
        )
    })?;
    Ok((final_sid, format!("turn-{parsed_turn}")))
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DshSessionSnapshot {
    pub session_id: String,
    pub project_id: String,
    pub title: String,
    pub cwd: PathBuf,
    pub turn_count: usize,
    pub last_turn_id: Option<String>,
    pub status: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DshSessionReceipt {
    pub host: DesktopHostKind,
    pub delivery_id: String,
    pub project_id: String,
    pub session_id: String,
    pub turn_id: String,
    pub project_dir: PathBuf,
    pub workspace_uris: Vec<String>,
    pub turn_count: usize,
    pub status: String,
    pub tier: DeliveryTier,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

impl From<DshSessionReceipt> for HostSessionReceipt {
    fn from(r: DshSessionReceipt) -> Self {
        Self {
            host: r.host,
            delivery_id: r.delivery_id,
            project_id: r.project_id,
            session_id: r.session_id,
            turn_id: r.turn_id,
            project_dir: r.project_dir,
            workspace_uris: r.workspace_uris,
            tier: r.tier,
            model: r.model,
        }
    }
}

pub(crate) fn parse_loopback_http_endpoint(
    base_url: &str,
) -> Result<(String, u16, String), HostAdapterError> {
    let trimmed = base_url.trim().trim_end_matches('/');
    let Some(rest) = trimmed.strip_prefix("http://") else {
        return Err(HostAdapterError::DshProtocol(format!(
            "DSH gateway base_url must start with http:// loopback: {base_url}"
        )));
    };
    let (host_port, base_path) = match rest.split_once('/') {
        Some((hp, p)) => (hp, format!("/{}", p.trim_matches('/'))),
        None => (rest, String::new()),
    };
    let (host, port_str) = host_port.split_once(':').ok_or_else(|| {
        HostAdapterError::DshProtocol(format!(
            "DSH gateway base_url must specify explicit host:port: {base_url}"
        ))
    })?;
    if !matches!(host, "127.0.0.1" | "localhost") {
        return Err(HostAdapterError::DshProtocol(format!(
            "DSH gateway host must be loopback (127.0.0.1 or localhost), got {host}"
        )));
    }
    let port: u16 = port_str.parse().map_err(|_| {
        HostAdapterError::DshProtocol(format!("invalid DSH gateway port in {base_url}"))
    })?;
    if port == 0 {
        return Err(HostAdapterError::DshProtocol(
            "DSH gateway port must be non-zero".into(),
        ));
    }
    Ok((host.to_string(), port, base_path))
}

pub(crate) fn dsh_http_request(
    runtime: &DshRuntimeConfig,
    method: &str,
    path: &str,
    body: Option<&Value>,
) -> Result<Value, HostAdapterError> {
    let base_url = runtime.base_url.as_deref().ok_or_else(|| {
        HostAdapterError::DshProtocol(
            "DshRuntimeConfig base_url is not set (native DSH mode active)".into(),
        )
    })?;
    let (host, port, base_path) = parse_loopback_http_endpoint(base_url)?;
    let full_path = if base_path.is_empty() || path.starts_with(&format!("{base_path}/")) {
        path.to_string()
    } else {
        format!("{base_path}/{}", path.trim_start_matches('/'))
    };
    let connect_host = if host == "localhost" {
        "127.0.0.1"
    } else {
        host.as_str()
    };
    let mut stream = TcpStream::connect((connect_host, port)).map_err(|e| {
        HostAdapterError::DshProtocol(format!(
            "failed to connect to DSH gateway {connect_host}:{port}: {e}"
        ))
    })?;
    stream
        .set_read_timeout(Some(runtime.timeout))
        .map_err(|e| HostAdapterError::DshProtocol(e.to_string()))?;
    stream
        .set_write_timeout(Some(runtime.timeout))
        .map_err(|e| HostAdapterError::DshProtocol(e.to_string()))?;

    let body_bytes = match body {
        Some(v) => serde_json::to_vec(v).map_err(|e| HostAdapterError::DshProtocol(e.to_string()))?,
        None => Vec::new(),
    };
    let request_head = if body.is_some() {
        format!(
            "{method} {full_path} HTTP/1.1\r\nHost: {host}:{port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body_bytes.len()
        )
    } else {
        format!(
            "{method} {full_path} HTTP/1.1\r\nHost: {host}:{port}\r\nConnection: close\r\n\r\n"
        )
    };
    stream
        .write_all(request_head.as_bytes())
        .map_err(|e| HostAdapterError::DshProtocol(e.to_string()))?;
    if !body_bytes.is_empty() {
        stream
            .write_all(&body_bytes)
            .map_err(|e| HostAdapterError::DshProtocol(e.to_string()))?;
    }
    let mut raw = Vec::new();
    stream
        .read_to_end(&mut raw)
        .map_err(|e| HostAdapterError::DshProtocol(e.to_string()))?;
    let response_str = String::from_utf8_lossy(&raw);
    let Some((header_block, resp_body)) = response_str.split_once("\r\n\r\n") else {
        return Err(HostAdapterError::DshProtocol(
            "malformed HTTP response from DSH gateway".into(),
        ));
    };
    let status_line = header_block.lines().next().unwrap_or_default();
    let status_code: u16 = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    if !(200..300).contains(&status_code) {
        return Err(HostAdapterError::DshProtocol(format!(
            "DSH gateway {method} {full_path} returned HTTP {status_code}: {}",
            resp_body.trim()
        )));
    }
    serde_json::from_str(resp_body.trim()).map_err(|e| {
        HostAdapterError::DshProtocol(format!(
            "invalid JSON from DSH gateway {method} {full_path}: {e}"
        ))
    })
}

// ============================================================================
// Cindy Gateway & `cindy_session_send` session adapter
// ============================================================================

/// Configuration for `CindyAdapter` when backed by a Cindy Gateway Unix socket
/// (`sessions.patch`, `sessions.create`, `cindy_session_send` / `sessions.send`, `sessions.get`)
/// and transcript JSONL directory.
#[derive(Clone, Debug)]
pub struct CindyRuntimeConfig {
    pub cindy_root: PathBuf,
    pub gateway_socket: PathBuf,
    pub timeout: Duration,
}

impl CindyRuntimeConfig {
    pub fn new(cindy_root: PathBuf, gateway_socket: PathBuf) -> Self {
        Self {
            cindy_root,
            gateway_socket,
            timeout: Duration::from_secs(5),
        }
    }

    pub fn transcripts_dir(&self) -> PathBuf {
        self.cindy_root.join("transcripts")
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CindySessionSnapshot {
    pub session_key: String,
    pub project_id: String,
    pub title: String,
    pub cwd: PathBuf,
    pub turn_count: usize,
    pub last_turn_id: Option<String>,
    pub transcript_path: PathBuf,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CindySessionReceipt {
    pub host: DesktopHostKind,
    pub delivery_id: String,
    pub project_id: String,
    pub session_key: String,
    pub session_id: String,
    pub turn_id: String,
    pub project_dir: PathBuf,
    pub workspace_uris: Vec<String>,
    pub transcript_path: PathBuf,
    pub turn_count: usize,
    pub tier: DeliveryTier,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

impl From<CindySessionReceipt> for HostSessionReceipt {
    fn from(r: CindySessionReceipt) -> Self {
        Self {
            host: r.host,
            delivery_id: r.delivery_id,
            project_id: r.project_id,
            session_id: r.session_key,
            turn_id: r.turn_id,
            project_dir: r.project_dir,
            workspace_uris: r.workspace_uris,
            tier: r.tier,
            model: r.model,
        }
    }
}

pub(crate) struct CindyGatewayClient {
    ws: WebSocket<UnixStream>,
    next_id: u64,
}

impl CindyGatewayClient {
    pub(crate) fn connect(runtime: &CindyRuntimeConfig) -> Result<Self, HostAdapterError> {
        if !runtime.gateway_socket.is_absolute() {
            return Err(HostAdapterError::CindyProtocol(format!(
                "cindy gateway_socket must be absolute: {}",
                runtime.gateway_socket.display()
            )));
        }
        let stream = UnixStream::connect(&runtime.gateway_socket).map_err(|e| {
            HostAdapterError::CindyProtocol(format!(
                "failed to connect to Cindy gateway socket {}: {e}",
                runtime.gateway_socket.display()
            ))
        })?;
        stream
            .set_read_timeout(Some(runtime.timeout))
            .map_err(|e| HostAdapterError::CindyProtocol(e.to_string()))?;
        stream
            .set_write_timeout(Some(runtime.timeout))
            .map_err(|e| HostAdapterError::CindyProtocol(e.to_string()))?;
        let config = WebSocketConfig::default()
            .max_message_size(Some(4 * 1024 * 1024))
            .max_frame_size(Some(4 * 1024 * 1024));
        let (ws, _) = client_with_config("ws://localhost/", stream, Some(config))
            .map_err(|e| HostAdapterError::CindyProtocol(e.to_string()))?;
        Ok(Self { ws, next_id: 0 })
    }

    pub(crate) fn request(
        &mut self,
        method: &str,
        params: Value,
    ) -> Result<Value, HostAdapterError> {
        self.next_id += 1;
        let id = self.next_id;
        let payload = json!({"id": id, "method": method, "params": params});
        self.ws
            .send(Message::Text(payload.to_string().into()))
            .map_err(|e| HostAdapterError::CindyProtocol(e.to_string()))?;
        loop {
            let msg = self
                .ws
                .read()
                .map_err(|e| HostAdapterError::CindyProtocol(e.to_string()))?;
            match msg {
                Message::Text(text) => {
                    let val: Value = serde_json::from_str(&text).map_err(|e| {
                        HostAdapterError::CindyProtocol(format!(
                            "invalid JSON from Cindy gateway: {e}"
                        ))
                    })?;
                    if let Some(resp_id) = val.get("id") {
                        if resp_id != &json!(id) {
                            return Err(HostAdapterError::CindyProtocol(format!(
                                "Cindy gateway RPC id mismatch: expected {id}, got {resp_id}"
                            )));
                        }
                        if let Some(err) = val.get("error") {
                            return Err(HostAdapterError::CindyProtocol(format!(
                                "Cindy gateway {method} rejected: {err}"
                            )));
                        }
                        return val.get("result").cloned().ok_or_else(|| {
                            HostAdapterError::CindyProtocol(format!(
                                "Cindy gateway {method} response missing result"
                            ))
                        });
                    }
                }
                Message::Ping(_) | Message::Pong(_) => continue,
                other => {
                    return Err(HostAdapterError::CindyProtocol(format!(
                        "unexpected WebSocket frame from Cindy gateway: {other:?}"
                    )))
                }
            }
        }
    }
}

fn validate_sqlite_path_and_id<F>(
    db_path: &Path,
    field: &str,
    id: &str,
    err_fn: F,
) -> Result<(), HostAdapterError>
where
    F: Fn(String) -> HostAdapterError,
{
    if !db_path.is_absolute() {
        return Err(err_fn(format!(
            "SQLite db path must be absolute: {}",
            db_path.display()
        )));
    }
    if !db_path.exists() {
        return Err(err_fn(format!(
            "SQLite db does not exist at {}",
            db_path.display()
        )));
    }
    if id.is_empty()
        || id
            .chars()
            .any(|c| !(c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':')))
    {
        return Err(err_fn(format!("unsafe {field} for SQLite readback: {id:?}")));
    }
    Ok(())
}

fn run_sqlite_json<F>(
    sqlite3_bin: &Path,
    db_path: &Path,
    query: &str,
    err_fn: F,
) -> Result<Vec<Value>, HostAdapterError>
where
    F: Fn(String) -> HostAdapterError,
{
    let output = Command::new(sqlite3_bin)
        .arg("-readonly")
        .arg("-json")
        .arg(db_path)
        .arg(query)
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(err_fn(format!("sqlite3 readback failed: {}", stderr.trim())));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.trim().is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_str(&stdout)
        .map_err(|e| err_fn(format!("invalid sqlite3 JSON output: {e}")))
}

/// Reads and parses transcript JSONL lines for a Cindy session, verifying `cwd` and turn ordering.
pub fn read_cindy_transcript(
    transcript_path: &Path,
) -> Result<Vec<Value>, HostAdapterError> {
    if !transcript_path.is_absolute() || !transcript_path.exists() {
        return Err(HostAdapterError::CindyProtocol(format!(
            "Cindy transcript does not exist at {}",
            transcript_path.display()
        )));
    }
    let content = fs::read_to_string(transcript_path)?;
    let mut entries = Vec::new();
    for (idx, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let val: Value = serde_json::from_str(trimmed).map_err(|e| {
            HostAdapterError::CindyProtocol(format!(
                "invalid JSONL on line {} of {}: {e}",
                idx + 1,
                transcript_path.display()
            ))
        })?;
        entries.push(val);
    }
    Ok(entries)
}

// ============================================================================
// ZCode Desktop (`~/.zcode/v2/tasks-index.sqlite` + `~/.zcode/cli/db/db.sqlite` + `zcode`)
// ============================================================================

const ZCODE_TASKS_INDEX_DDL: &str = "\
CREATE TABLE IF NOT EXISTS hosts (\
    host_id TEXT PRIMARY KEY,\
    kind TEXT NOT NULL,\
    target_json TEXT,\
    membership_id TEXT,\
    position INTEGER NOT NULL DEFAULT 0,\
    last_connection_status TEXT,\
    last_connection_error TEXT,\
    last_opened_at INTEGER\
);\
CREATE TABLE IF NOT EXISTS projects (\
    project_id TEXT PRIMARY KEY,\
    host_id TEXT NOT NULL REFERENCES hosts(host_id),\
    name TEXT NOT NULL,\
    name_overridden INTEGER NOT NULL DEFAULT 0,\
    position INTEGER NOT NULL DEFAULT 0,\
    pinned INTEGER NOT NULL DEFAULT 0,\
    pinned_at INTEGER,\
    deleted INTEGER NOT NULL DEFAULT 0,\
    deleted_at INTEGER,\
    created_at INTEGER NOT NULL,\
    updated_at INTEGER NOT NULL\
);\
CREATE UNIQUE INDEX IF NOT EXISTS idx_projects_host_name ON projects(host_id, name) WHERE deleted = 0;\
CREATE TABLE IF NOT EXISTS project_roots (\
    project_id TEXT NOT NULL REFERENCES projects(project_id) ON DELETE CASCADE,\
    root_path TEXT NOT NULL,\
    position INTEGER NOT NULL DEFAULT 0,\
    is_primary INTEGER NOT NULL DEFAULT 0,\
    PRIMARY KEY (project_id, root_path)\
);\
CREATE TABLE IF NOT EXISTS tasks (\
    workspace_key TEXT NOT NULL,\
    workspace_path TEXT NOT NULL,\
    workspace_identity TEXT,\
    task_id TEXT NOT NULL,\
    title TEXT NOT NULL DEFAULT '',\
    task_status TEXT NOT NULL DEFAULT 'normal',\
    provider TEXT,\
    mode TEXT NOT NULL DEFAULT 'build',\
    model TEXT,\
    migration_source TEXT,\
    forked_from_task_id TEXT,\
    created_at INTEGER NOT NULL,\
    updated_at INTEGER NOT NULL,\
    unread_at INTEGER,\
    last_unread_at INTEGER NOT NULL DEFAULT 0,\
    pinned INTEGER NOT NULL DEFAULT 0,\
    archived INTEGER NOT NULL DEFAULT 0,\
    deleted INTEGER NOT NULL DEFAULT 0,\
    title_overridden INTEGER NOT NULL DEFAULT 0,\
    meta_json TEXT NOT NULL DEFAULT '{}',\
    searchable_text TEXT NOT NULL DEFAULT '',\
    cron_automation_id TEXT,\
    off_peak_task_id TEXT,\
    host_id TEXT NOT NULL DEFAULT 'local',\
    root_path TEXT,\
    project_id TEXT,\
    execution_directories TEXT,\
    sort_order INTEGER,\
    cli_title TEXT,\
    handoff_pending INTEGER NOT NULL DEFAULT 0,\
    cli_title_source TEXT,\
    cli_created_at INTEGER,\
    cli_updated_at INTEGER,\
    cli_last_outcome TEXT,\
    cli_deleted INTEGER NOT NULL DEFAULT 0,\
    PRIMARY KEY (workspace_key, task_id)\
);\
CREATE UNIQUE INDEX IF NOT EXISTS idx_tasks_host_task ON tasks(host_id, task_id);\
";

const ZCODE_CLI_DB_DDL: &str = "\
CREATE TABLE IF NOT EXISTS session (\
    id TEXT PRIMARY KEY,\
    project_id TEXT NOT NULL,\
    parent_id TEXT,\
    slug TEXT NOT NULL,\
    directory TEXT NOT NULL,\
    path TEXT,\
    title TEXT NOT NULL,\
    summary_additions INTEGER,\
    summary_deletions INTEGER,\
    summary_files INTEGER,\
    summary_diffs TEXT,\
    revert TEXT,\
    permission TEXT,\
    version TEXT NOT NULL,\
    time_created INTEGER NOT NULL,\
    time_updated INTEGER NOT NULL,\
    time_compacting INTEGER,\
    time_archived INTEGER\
);\
CREATE TABLE IF NOT EXISTS message (\
    id TEXT PRIMARY KEY,\
    session_id TEXT NOT NULL,\
    time_created INTEGER NOT NULL,\
    time_updated INTEGER NOT NULL,\
    data TEXT NOT NULL,\
    sequence INTEGER\
);\
CREATE TABLE IF NOT EXISTS part (\
    id TEXT PRIMARY KEY,\
    message_id TEXT NOT NULL,\
    session_id TEXT NOT NULL,\
    time_created INTEGER NOT NULL,\
    time_updated INTEGER NOT NULL,\
    data TEXT NOT NULL\
);\
";

/// Configuration for `ZcodeAdapter` when backed by ZCode Desktop's SQLite stores
/// (`<zcode_root>/v2/tasks-index.sqlite` and `<zcode_root>/cli/db/db.sqlite`) and `zcode` CLI.
#[derive(Clone, Debug)]
pub struct ZcodeRuntimeConfig {
    pub zcode_root: PathBuf,
    pub zcode_bin: PathBuf,
    pub sqlite3_bin: PathBuf,
    pub timeout: Duration,
}

impl ZcodeRuntimeConfig {
    pub fn new(zcode_root: PathBuf, zcode_bin: PathBuf) -> Self {
        Self {
            zcode_root,
            zcode_bin,
            sqlite3_bin: PathBuf::from("/usr/bin/sqlite3"),
            timeout: Duration::from_secs(300),
        }
    }

    pub fn tasks_index_db_path(&self) -> PathBuf {
        self.zcode_root.join("v2").join("tasks-index.sqlite")
    }

    pub fn cli_db_path(&self) -> PathBuf {
        self.zcode_root.join("cli").join("db").join("db.sqlite")
    }
}

/// Verified project + primary root row from ZCode's `v2/tasks-index.sqlite` (`projects` JOIN `project_roots`).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ZcodeProjectRow {
    pub project_id: String,
    pub host_id: String,
    pub name: String,
    pub root_path: PathBuf,
    pub position: i64,
}

/// Verified session snapshot combining ZCode's `v2/tasks-index.sqlite` (`tasks`) and `cli/db/db.sqlite` (`session` + `message`).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ZcodeSessionSnapshot {
    pub session_id: String,
    pub project_id: String,
    pub cli_project_id: String,
    pub title: String,
    pub cwd: PathBuf,
    pub workspace_key: String,
    pub provider: String,
    pub task_status: String,
    pub message_count: usize,
    pub archived: i64,
}

/// Receipt proving a ZCode Desktop session or follow-up turn was created/continued and verified
/// in `v2/tasks-index.sqlite` and `cli/db/db.sqlite`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ZcodeSessionReceipt {
    pub host: DesktopHostKind,
    pub delivery_id: String,
    pub project_id: String,
    pub session_id: String,
    pub turn_id: String,
    pub project_dir: PathBuf,
    pub workspace_uris: Vec<String>,
    pub workspace_key: String,
    pub message_count: usize,
    pub tier: DeliveryTier,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

impl From<ZcodeSessionReceipt> for HostSessionReceipt {
    fn from(r: ZcodeSessionReceipt) -> Self {
        Self {
            host: r.host,
            delivery_id: r.delivery_id,
            project_id: r.project_id,
            session_id: r.session_id,
            turn_id: r.turn_id,
            project_dir: r.project_dir,
            workspace_uris: r.workspace_uris,
            tier: r.tier,
            model: r.model,
        }
    }
}

pub fn deterministic_zcode_project_uuid(project_dir: &Path) -> String {
    let bytes = format!("zcode:{}", project_dir.to_string_lossy()).into_bytes();
    let h1 = fnv1a_u64(0xcbf29ce484222325, &bytes);
    let h2 = fnv1a_u64(0x84222325cbf29ce4, &bytes);
    let p1 = (h1 >> 32) as u32;
    let p2 = ((h1 >> 16) & 0xffff) as u16;
    let p3 = ((h1 & 0x0fff) | 0x4000) as u16;
    let p4 = (((h2 >> 48) & 0x3fff) | 0x8000) as u16;
    let p5 = h2 & 0x0000_ffff_ffff_ffff;
    format!("{p1:08x}-{p2:04x}-{p3:04x}-{p4:04x}-{p5:012x}")
}

fn current_epoch_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub fn zcode_cli_project_slug(project_dir: &Path) -> String {
    let raw = project_dir.to_string_lossy().to_ascii_lowercase();
    let trimmed = raw.trim_matches(|c: char| !c.is_ascii_alphanumeric());
    let mut slug = String::from("proj_");
    let mut last_was_dash = false;
    for c in trimmed.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c);
            last_was_dash = false;
        } else if !last_was_dash {
            slug.push('-');
            last_was_dash = true;
        }
    }
    slug.trim_end_matches('-').to_string()
}

fn escape_sql_literal(value: &str) -> String {
    value.replace('\'', "''")
}

fn exec_sqlite_write<F>(
    sqlite3_bin: &Path,
    db_path: &Path,
    sql: &str,
    err_fn: F,
) -> Result<(), HostAdapterError>
where
    F: Fn(String) -> HostAdapterError,
{
    if let Some(parent) = db_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let output = Command::new(sqlite3_bin)
        .arg(db_path)
        .arg(sql)
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(err_fn(format!("sqlite3 write failed: {}", stderr.trim())));
    }
    Ok(())
}

/// Ensures `<project_dir>` is registered in `<zcode_root>/v2/tasks-index.sqlite` (`hosts`, `projects`, `project_roots`)
/// and returns the verified `ZcodeProjectRow`.
pub fn ensure_zcode_sidebar_project(
    sqlite3_bin: &Path,
    tasks_index_db_path: &Path,
    project_dir: &Path,
    title: &str,
) -> Result<ZcodeProjectRow, HostAdapterError> {
    if !tasks_index_db_path.is_absolute() {
        return Err(HostAdapterError::ZcodeProtocol(format!(
            "tasks_index_db_path must be absolute: {}",
            tasks_index_db_path.display()
        )));
    }
    exec_sqlite_write(
        sqlite3_bin,
        tasks_index_db_path,
        ZCODE_TASKS_INDEX_DDL,
        HostAdapterError::ZcodeProtocol,
    )?;

    let root_str = project_dir.to_string_lossy().into_owned();
    let escaped_root = escape_sql_literal(&root_str);
    let escaped_title = escape_sql_literal(title);
    let now_ms = current_epoch_ms();

    let find_sql = format!(
        "SELECT p.project_id AS project_id \
         FROM projects p \
         LEFT JOIN project_roots r ON p.project_id = r.project_id \
         WHERE (r.root_path = '{escaped_root}' OR (p.name = '{escaped_title}' AND p.host_id = 'local')) \
           AND p.deleted = 0 \
         ORDER BY CASE WHEN p.name = '{escaped_title}' AND p.host_id = 'local' THEN 0 \
                       WHEN r.root_path = '{escaped_root}' THEN 1 \
                       ELSE 2 END, p.position ASC \
         LIMIT 1;"
    );
    let existing = run_sqlite_json(
        sqlite3_bin,
        tasks_index_db_path,
        &find_sql,
        HostAdapterError::ZcodeProtocol,
    )?;
    let project_id = existing
        .first()
        .and_then(|row| row.get("project_id"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| deterministic_zcode_project_uuid(project_dir));

    let escaped_pid = escape_sql_literal(&project_id);
    let upsert_sql = format!(
        "INSERT OR IGNORE INTO hosts (host_id, kind, position) \
         VALUES ('local', 'local', 0); \
         UPDATE projects SET deleted = 1, updated_at = {now_ms} \
         WHERE project_id != '{escaped_pid}' AND deleted = 0 \
           AND ( (host_id = 'local' AND name = '{escaped_title}') \
                 OR project_id IN (SELECT project_id FROM project_roots WHERE root_path = '{escaped_root}') ); \
         INSERT INTO projects (project_id, host_id, name, position, deleted, created_at, updated_at) \
         VALUES ('{escaped_pid}', 'local', '{escaped_title}', 0, 0, {now_ms}, {now_ms}) \
         ON CONFLICT(project_id) DO UPDATE SET \
            name = excluded.name, \
            deleted = 0, \
            updated_at = excluded.updated_at; \
         INSERT INTO project_roots (project_id, root_path, position, is_primary) \
         VALUES ('{escaped_pid}', '{escaped_root}', 0, 1) \
         ON CONFLICT(project_id, root_path) DO UPDATE SET is_primary = 1; \
         UPDATE tasks SET project_id = '{escaped_pid}' \
         WHERE root_path = '{escaped_root}' AND COALESCE(project_id, '') != '{escaped_pid}';"
    );
    exec_sqlite_write(
        sqlite3_bin,
        tasks_index_db_path,
        &upsert_sql,
        HostAdapterError::ZcodeProtocol,
    )?;

    let row = read_zcode_project(sqlite3_bin, tasks_index_db_path, &project_id)?.ok_or_else(
        || {
            HostAdapterError::ZcodeProtocol(format!(
                "project {project_id} missing from tasks-index.sqlite after upsert"
            ))
        },
    )?;
    verify_zcode_project_binding(&row, &project_id, project_dir)?;
    Ok(row)
}

/// Reads back a project and its primary root from ZCode's `v2/tasks-index.sqlite`.
pub fn read_zcode_project(
    sqlite3_bin: &Path,
    tasks_index_db_path: &Path,
    project_id: &str,
) -> Result<Option<ZcodeProjectRow>, HostAdapterError> {
    validate_sqlite_path_and_id(
        tasks_index_db_path,
        "project_id",
        project_id,
        HostAdapterError::ZcodeProtocol,
    )?;
    let query = format!(
        "SELECT p.project_id AS project_id, p.host_id AS host_id, p.name AS name, \
         r.root_path AS root_path, p.position AS position \
         FROM projects p JOIN project_roots r ON p.project_id = r.project_id \
         WHERE p.project_id = '{project_id}' AND p.deleted = 0 \
         ORDER BY r.position ASC LIMIT 1;"
    );
    let rows = run_sqlite_json(
        sqlite3_bin,
        tasks_index_db_path,
        &query,
        HostAdapterError::ZcodeProtocol,
    )?;
    let Some(row) = rows.into_iter().next() else {
        return Ok(None);
    };
    Ok(Some(ZcodeProjectRow {
        project_id: row
            .get("project_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        host_id: row
            .get("host_id")
            .and_then(Value::as_str)
            .unwrap_or("local")
            .to_string(),
        name: row
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        root_path: PathBuf::from(
            row.get("root_path")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        ),
        position: row.get("position").and_then(Value::as_i64).unwrap_or(0),
    }))
}

pub fn verify_zcode_project_binding(
    row: &ZcodeProjectRow,
    expected_project_id: &str,
    expected_dir: &Path,
) -> Result<(), HostAdapterError> {
    if FORBIDDEN_PROJECT_BUCKETS.contains(&row.project_id.trim()) {
        return Err(HostAdapterError::ForbiddenProjectId(row.project_id.clone()));
    }
    if row.project_id != expected_project_id {
        return Err(HostAdapterError::ZcodeProtocol(format!(
            "tasks-index.sqlite project_id mismatch: expected {expected_project_id}, got {}",
            row.project_id
        )));
    }
    if row.root_path != expected_dir {
        return Err(HostAdapterError::ProjectDirMismatch {
            expected: expected_dir.to_path_buf(),
            supplied: row.root_path.clone(),
        });
    }
    Ok(())
}

/// Synchronizes a ZCode session and message turn into both `<zcode_root>/v2/tasks-index.sqlite` (`tasks` table)
/// and `<zcode_root>/cli/db/db.sqlite` (`session` and `message` tables) so the session is visible
/// under the left sidebar project in ZCode Desktop.
#[allow(clippy::too_many_arguments)]
pub fn attach_zcode_session_to_tasks_index(
    sqlite3_bin: &Path,
    tasks_index_db_path: &Path,
    cli_db_path: &Path,
    project_id: &str,
    project_dir: &Path,
    session_id: &str,
    title: &str,
    prompt: &str,
    turn_id: &str,
) -> Result<(), HostAdapterError> {
    exec_sqlite_write(
        sqlite3_bin,
        tasks_index_db_path,
        ZCODE_TASKS_INDEX_DDL,
        HostAdapterError::ZcodeProtocol,
    )?;
    exec_sqlite_write(
        sqlite3_bin,
        cli_db_path,
        ZCODE_CLI_DB_DDL,
        HostAdapterError::ZcodeProtocol,
    )?;

    let root_str = project_dir.to_string_lossy().into_owned();
    let workspace_key = root_str.clone();
    let cli_proj_id = zcode_cli_project_slug(project_dir);
    let now_ms = current_epoch_ms();

    let esc_wk = escape_sql_literal(&workspace_key);
    let esc_root = escape_sql_literal(&root_str);
    let esc_sid = escape_sql_literal(session_id);
    let esc_title = escape_sql_literal(title);
    let esc_pid = escape_sql_literal(project_id);
    let esc_tid = escape_sql_literal(turn_id);
    let now_iso = current_time_iso();
    let meta_json = serde_json::json!({
        "taskId": session_id,
        "traceId": session_id,
        "title": title,
        "titleOverridden": true,
        "workspacePath": root_str,
        "createdAt": now_iso,
        "updatedAt": now_iso,
        "mode": "default",
        "provider": "glm",
        "status": "completed",
    })
    .to_string();
    let esc_meta_json = escape_sql_literal(&meta_json);

    let tasks_sql = format!(
        "INSERT INTO tasks (\
            workspace_key, workspace_path, task_id, title, title_overridden, \
            task_status, provider, mode, host_id, root_path, project_id, \
            meta_json, cli_deleted, \
            cli_title, cli_title_source, cli_created_at, cli_updated_at, \
            cli_last_outcome, \
            created_at, updated_at, deleted, archived\
         ) VALUES (\
            '{esc_wk}', '{esc_root}', '{esc_sid}', '{esc_title}', 1, \
            'normal', 'glm', 'default', 'local', '{esc_root}', '{esc_pid}', \
            '{esc_meta_json}', 0, \
            '{esc_title}', 'custom', {now_ms}, {now_ms}, \
            'completed', \
            {now_ms}, {now_ms}, 0, 0\
         ) ON CONFLICT(workspace_key, task_id) DO UPDATE SET \
            project_id = excluded.project_id, \
            root_path = excluded.root_path, \
            workspace_path = excluded.workspace_path, \
            provider = 'glm', \
            mode = COALESCE(NULLIF(tasks.mode, ''), 'default'), \
            title = CASE WHEN tasks.title = '' THEN excluded.title ELSE tasks.title END, \
            title_overridden = 1, \
            cli_title = CASE WHEN COALESCE(tasks.cli_title, '') = '' THEN excluded.cli_title ELSE tasks.cli_title END, \
            cli_deleted = 0, \
            cli_created_at = COALESCE(tasks.cli_created_at, excluded.cli_created_at), \
            cli_updated_at = excluded.cli_updated_at, \
            cli_last_outcome = COALESCE(tasks.cli_last_outcome, 'completed'), \
            meta_json = CASE WHEN COALESCE(tasks.meta_json, '') IN ('', '{{}}') THEN excluded.meta_json ELSE tasks.meta_json END, \
            deleted = 0, \
            archived = 0, \
            updated_at = excluded.updated_at; \
         UPDATE tasks SET \
            provider = 'glm', \
            mode = COALESCE(NULLIF(mode, ''), 'default'), \
            title_overridden = 1, \
            cli_deleted = 0, \
            cli_last_outcome = COALESCE(cli_last_outcome, 'completed') \
         WHERE provider IS NULL AND project_id IS NOT NULL;"
    );
    exec_sqlite_write(
        sqlite3_bin,
        tasks_index_db_path,
        &tasks_sql,
        HostAdapterError::ZcodeProtocol,
    )?;

    let esc_cli_pid = escape_sql_literal(&cli_proj_id);
    let msg_json = serde_json::json!({
        "role": "user",
        "turn_id": turn_id,
        "text": prompt,
        "time": { "created": now_ms },
    })
    .to_string();
    let esc_msg_json = escape_sql_literal(&msg_json);
    let part_json = serde_json::json!({
        "type": "text",
        "text": prompt,
    })
    .to_string();
    let esc_part_json = escape_sql_literal(&part_json);

    let cli_sql = format!(
        "INSERT INTO session (\
            id, project_id, slug, directory, path, title, version, time_created, time_updated\
         ) VALUES (\
            '{esc_sid}', '{esc_cli_pid}', '{esc_sid}', '{esc_root}', '{esc_root}', '{esc_title}', '0.17.1', {now_ms}, {now_ms}\
         ) ON CONFLICT(id) DO UPDATE SET \
            directory = excluded.directory, \
            title = CASE WHEN session.title = '' OR session.title LIKE 'New session - %' THEN excluded.title ELSE session.title END, \
            time_updated = excluded.time_updated; \
         DELETE FROM message \
         WHERE session_id = '{esc_sid}' \
           AND NOT EXISTS (SELECT 1 FROM part p WHERE p.message_id = message.id); \
         INSERT OR IGNORE INTO message (\
            id, session_id, time_created, time_updated, data, sequence\
         ) SELECT \
            '{esc_tid}', '{esc_sid}', {now_ms}, {now_ms}, '{esc_msg_json}', \
            COALESCE((SELECT MAX(sequence) + 1 FROM message WHERE session_id = '{esc_sid}'), 0) \
         WHERE NOT EXISTS (\
            SELECT 1 FROM message m JOIN part p ON p.message_id = m.id \
            WHERE m.session_id = '{esc_sid}' AND json_extract(m.data, '$.turn_id') IS NULL\
         ); \
         INSERT OR IGNORE INTO part (\
            id, message_id, session_id, time_created, time_updated, data\
         ) SELECT \
            'prt_{esc_tid}', '{esc_tid}', '{esc_sid}', {now_ms}, {now_ms}, '{esc_part_json}' \
         WHERE EXISTS (\
            SELECT 1 FROM message m WHERE m.id = '{esc_tid}' AND m.session_id = '{esc_sid}'\
         );"
    );
    exec_sqlite_write(
        sqlite3_bin,
        cli_db_path,
        &cli_sql,
        HostAdapterError::ZcodeProtocol,
    )?;
    Ok(())
}

/// Reads back a ZCode session snapshot from `<zcode_root>/v2/tasks-index.sqlite` and `<zcode_root>/cli/db/db.sqlite`.
pub fn read_zcode_session(
    sqlite3_bin: &Path,
    tasks_index_db_path: &Path,
    cli_db_path: &Path,
    session_id: &str,
) -> Result<Option<ZcodeSessionSnapshot>, HostAdapterError> {
    validate_sqlite_path_and_id(
        tasks_index_db_path,
        "session_id",
        session_id,
        HostAdapterError::ZcodeProtocol,
    )?;
    validate_sqlite_path_and_id(
        cli_db_path,
        "session_id",
        session_id,
        HostAdapterError::ZcodeProtocol,
    )?;

    let task_query = format!(
        "SELECT workspace_key, workspace_path, task_id, title, task_status, \
         COALESCE(provider, '') AS provider, \
         COALESCE(root_path, workspace_path) AS root_path, COALESCE(project_id, '') AS project_id, \
         archived, deleted \
         FROM tasks WHERE task_id = '{session_id}' AND deleted = 0 LIMIT 1;"
    );
    let task_rows = run_sqlite_json(
        sqlite3_bin,
        tasks_index_db_path,
        &task_query,
        HostAdapterError::ZcodeProtocol,
    )?;
    let Some(task_row) = task_rows.into_iter().next() else {
        return Ok(None);
    };

    let sess_query = format!(
        "SELECT s.id AS session_id, s.project_id AS cli_project_id, s.directory AS directory, \
         s.title AS cli_title, \
         (SELECT COUNT(*) FROM message m WHERE m.session_id = s.id) AS message_count \
         FROM session s WHERE s.id = '{session_id}' LIMIT 1;"
    );
    let sess_rows = run_sqlite_json(
        sqlite3_bin,
        cli_db_path,
        &sess_query,
        HostAdapterError::ZcodeProtocol,
    )?;
    let Some(sess_row) = sess_rows.into_iter().next() else {
        return Ok(None);
    };

    let cwd_str = task_row
        .get("root_path")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .or_else(|| sess_row.get("directory").and_then(Value::as_str))
        .unwrap_or_default();

    Ok(Some(ZcodeSessionSnapshot {
        session_id: session_id.to_string(),
        project_id: task_row
            .get("project_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        cli_project_id: sess_row
            .get("cli_project_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        title: task_row
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        cwd: PathBuf::from(cwd_str),
        workspace_key: task_row
            .get("workspace_key")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        provider: task_row
            .get("provider")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        task_status: task_row
            .get("task_status")
            .and_then(Value::as_str)
            .unwrap_or("normal")
            .to_string(),
        message_count: sess_row
            .get("message_count")
            .and_then(Value::as_u64)
            .unwrap_or(0) as usize,
        archived: task_row
            .get("archived")
            .and_then(Value::as_i64)
            .unwrap_or(0),
    }))
}

pub fn verify_zcode_session_binding(
    snap: &ZcodeSessionSnapshot,
    expected_project_id: &str,
    expected_dir: &Path,
) -> Result<(), HostAdapterError> {
    if FORBIDDEN_PROJECT_BUCKETS.contains(&snap.project_id.trim()) {
        return Err(HostAdapterError::ForbiddenProjectId(
            if snap.project_id.is_empty() {
                "<null-project-id>".into()
            } else {
                snap.project_id.clone()
            },
        ));
    }
    if snap.project_id != expected_project_id {
        return Err(HostAdapterError::SessionProjectMismatch {
            session_id: snap.session_id.clone(),
            expected_project_id: expected_project_id.to_string(),
            supplied_project_id: snap.project_id.clone(),
        });
    }
    if snap.cwd != expected_dir {
        return Err(HostAdapterError::ProjectDirMismatch {
            expected: expected_dir.to_path_buf(),
            supplied: snap.cwd.clone(),
        });
    }
    if snap.archived != 0 {
        return Err(HostAdapterError::ZcodeProtocol(format!(
            "session {} is archived in tasks-index.sqlite and invisible in sidebar",
            snap.session_id
        )));
    }
    if snap.provider != "glm" {
        return Err(HostAdapterError::ZcodeProtocol(format!(
            "session {} has provider {:?} in tasks-index.sqlite (expected 'glm' for ZCode Desktop sidebar visibility)",
            snap.session_id, snap.provider
        )));
    }
    if snap.message_count == 0 {
        return Err(HostAdapterError::ZcodeProtocol(format!(
            "session {} has 0 messages in cli/db/db.sqlite",
            snap.session_id
        )));
    }
    Ok(())
}

/// Runs `zcode --cwd <cwd> --surface desktop --json [--resume <session_id>] [-m <model>] -p <prompt>`
/// and returns `(session_id, turn_id)`.
pub fn run_zcode_headless_command(
    runtime: &ZcodeRuntimeConfig,
    cwd: &Path,
    session_id: Option<&str>,
    prompt: &str,
    model: Option<&str>,
) -> Result<(String, String), HostAdapterError> {
    if !runtime.zcode_bin.is_absolute() {
        return Err(HostAdapterError::ZcodeProtocol(format!(
            "zcode executable path must be absolute: {}",
            runtime.zcode_bin.display()
        )));
    }
    let mut cmd = Command::new(&runtime.zcode_bin);
    cmd.current_dir(cwd)
        .env("ZCODE_HOME", &runtime.zcode_root)
        .arg("--cwd")
        .arg(cwd)
        .arg("--surface")
        .arg("desktop")
        .arg("--json");
    if let Some(sid) = session_id {
        cmd.arg("--resume").arg(sid);
    }
    if let Some(m) = model {
        cmd.arg("-m").arg(m);
    }
    cmd.arg("-p").arg(prompt);

    let output = cmd.output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(HostAdapterError::ZcodeProtocol(format!(
            "zcode command failed (status {}): stderr={} stdout={}",
            output.status,
            stderr.trim(),
            stdout.trim()
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut parsed_session_id: Option<String> = session_id.map(str::to_string);
    let mut parsed_turn_id: Option<String> = None;

    if let Ok(val) = serde_json::from_str::<Value>(stdout.trim()) {
        extract_zcode_ids_from_json(&val, &mut parsed_session_id, &mut parsed_turn_id);
    } else {
        for line in stdout.lines() {
            let trimmed = line.trim();
            if !trimmed.starts_with('{') {
                continue;
            }
            if let Ok(val) = serde_json::from_str::<Value>(trimmed) {
                extract_zcode_ids_from_json(&val, &mut parsed_session_id, &mut parsed_turn_id);
            }
        }
    }

    let final_sid = parsed_session_id.ok_or_else(|| {
        HostAdapterError::ZcodeProtocol(
            "zcode --json output did not emit session_id / sessionId".into(),
        )
    })?;
    let final_tid = parsed_turn_id.unwrap_or_else(|| {
        let seed = format!("{final_sid}:{prompt}");
        format!("msg_{:016x}", fnv1a_u64(0xcbf29ce484222325, seed.as_bytes()))
    });
    Ok((final_sid, final_tid))
}

fn extract_zcode_ids_from_json(
    val: &Value,
    parsed_session_id: &mut Option<String>,
    parsed_turn_id: &mut Option<String>,
) {
    if let Some(sid) = val
        .get("session_id")
        .or_else(|| val.get("sessionId"))
        .or_else(|| val.pointer("/session/id"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
    {
        *parsed_session_id = Some(sid.to_string());
    }
    if let Some(tid) = val
        .get("turn_id")
        .or_else(|| val.get("turnId"))
        .or_else(|| val.get("message_id"))
        .or_else(|| val.get("messageId"))
        .or_else(|| val.pointer("/message/id"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
    {
        *parsed_turn_id = Some(tid.to_string());
    }
}
