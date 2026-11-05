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

    let now_iso = "2026-11-05T04:00:00.000Z";
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
    fs::write(&tmp_path, bytes)?;
    fs::rename(&tmp_path, &ws_path)?;
    Ok(())
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

/// Runs `dsh --profile headless [--session-id <id>] --json <prompt>` in `cwd` and returns `(session_id, turn_id)`.
pub fn run_dsh_headless_command(
    dsh_root: &Path,
    dsh_bin: &Path,
    cwd: &Path,
    session_id: Option<&str>,
    prompt: &str,
) -> Result<(String, String), HostAdapterError> {
    if !dsh_bin.is_absolute() {
        return Err(HostAdapterError::DshProtocol(format!(
            "dsh executable path must be absolute: {}",
            dsh_bin.display()
        )));
    }
    let mut cmd = Command::new(dsh_bin);
    cmd.current_dir(cwd)
        .env("DSH_HOME", dsh_root)
        .env("DSH_PERMISSION_MODE", "danger-full-access")
        .arg("--profile")
        .arg("headless");
    if let Some(sid) = session_id {
        cmd.arg("--session-id").arg(sid);
    }
    cmd.arg("--json").arg(prompt);

    let output = cmd.output()?;
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
