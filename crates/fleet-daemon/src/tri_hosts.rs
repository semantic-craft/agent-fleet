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
/// `expected_dir`, `archived == 0`, and has a non-empty `preview` (`Gotcha #2`).
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
    if row.preview.trim().is_empty() {
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
// ============================================================================

/// Configuration for `DeepSeekHarnessAdapter` talking to the local DSH session gateway
/// (`http://127.0.0.1:<port>/v1`).
#[derive(Clone, Debug)]
pub struct DshRuntimeConfig {
    pub base_url: String,
    pub timeout: Duration,
}

impl DshRuntimeConfig {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            timeout: Duration::from_secs(5),
        }
    }
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
    let (host, port, base_path) = parse_loopback_http_endpoint(&runtime.base_url)?;
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
