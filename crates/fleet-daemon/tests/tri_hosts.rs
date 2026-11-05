#![cfg(unix)]

use fleet_daemon::{
    read_cindy_transcript, read_codex_project, read_codex_thread, CindyAdapter, CindyRuntimeConfig,
    DeepSeekHarnessAdapter, DesktopHostAdapter, DshRuntimeConfig, HostAdapterError,
    OpenAiDotsAdapter, OpenAiDotsRuntimeConfig,
};
use fleet_proto::{CreateSessionRequest, DeliverTurnRequest, DeliveryTier, DesktopHostKind};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    os::unix::net::UnixListener,
    path::{Path, PathBuf},
    process::Command,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::Duration,
};
use tungstenite::Message;

static NEXT: AtomicU64 = AtomicU64::new(0);

const CODEX_STATE_5_DDL: &str = "\
CREATE TABLE projects (\
    id TEXT PRIMARY KEY,\
    name TEXT NOT NULL,\
    metadata TEXT NOT NULL DEFAULT '{}',\
    position INTEGER NOT NULL,\
    created_at_ms INTEGER NOT NULL,\
    updated_at_ms INTEGER NOT NULL\
);\
CREATE INDEX idx_projects_position ON projects(position ASC, id ASC);\
CREATE TABLE project_roots (\
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,\
    position INTEGER NOT NULL,\
    path TEXT NOT NULL,\
    PRIMARY KEY (project_id, position)\
);\
CREATE TABLE threads (\
    id TEXT PRIMARY KEY,\
    rollout_path TEXT NOT NULL,\
    created_at INTEGER NOT NULL,\
    updated_at INTEGER NOT NULL,\
    source TEXT NOT NULL,\
    model_provider TEXT NOT NULL,\
    cwd TEXT NOT NULL,\
    title TEXT NOT NULL,\
    sandbox_policy TEXT NOT NULL,\
    approval_mode TEXT NOT NULL,\
    tokens_used INTEGER NOT NULL DEFAULT 0,\
    has_user_event INTEGER NOT NULL DEFAULT 0,\
    archived INTEGER NOT NULL DEFAULT 0,\
    preview TEXT NOT NULL DEFAULT '',\
    recency_at_ms INTEGER NOT NULL DEFAULT 0,\
    project_id TEXT REFERENCES projects(id) ON DELETE SET NULL,\
    originator TEXT\
);\
CREATE INDEX idx_threads_visible_recency_at_ms ON threads(recency_at_ms DESC) WHERE preview <> '';\
CREATE INDEX idx_threads_project_recency ON threads(project_id, recency_at_ms DESC) WHERE archived = 0;\
";

struct OpenAiDotsFixture {
    root: PathBuf,
    projects_root: PathBuf,
    state_root: PathBuf,
    codex_root: PathBuf,
    db_path: PathBuf,
    global_state_path: PathBuf,
    socket: PathBuf,
    rpc_calls: Arc<Mutex<Vec<Value>>>,
    stop: Arc<AtomicBool>,
    server: Option<JoinHandle<()>>,
}

impl OpenAiDotsFixture {
    fn new(force_null_project_on_thread: bool, force_empty_preview: bool) -> Self {
        let root = std::env::temp_dir().join(format!(
            "fleet-dots-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let projects_root = root.join("Projects");
        let state_root = root.join("state");
        let codex_root = root.join(".codex");
        fs::create_dir_all(&projects_root).unwrap();
        fs::create_dir_all(&state_root).unwrap();
        fs::create_dir_all(&codex_root).unwrap();

        let db_path = codex_root.join("state_5.sqlite");
        let status = Command::new("/usr/bin/sqlite3")
            .arg(&db_path)
            .arg(CODEX_STATE_5_DDL)
            .status()
            .unwrap();
        assert!(status.success(), "failed to initialize state_5.sqlite");

        let global_state_path = codex_root.join(".codex-global-state.json");
        fs::write(
            &global_state_path,
            serde_json::to_vec_pretty(&json!({
                "local-projects": {},
                "project-order": [],
                "thread-project-assignments": {},
                "sidebar-project-thread-orders": {}
            }))
            .unwrap(),
        )
        .unwrap();
        fs::write(
            codex_root.join("config.toml"),
            "model = \"zcode/GLM-5.3-Flash\"\nmodel_reasoning_effort = \"max\"\nopenai_base_url = \"http://127.0.0.1:3425/backend-api/codex\"\n",
        )
        .unwrap();

        let socket = root.join("app-server.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        listener.set_nonblocking(true).unwrap();

        let rpc_calls = Arc::new(Mutex::new(Vec::new()));
        let calls_ref = rpc_calls.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_ref = stop.clone();
        let db_ref = db_path.clone();
        let global_ref = global_state_path.clone();
        let sessions_dir = codex_root.join("sessions/2026/11/05");
        fs::create_dir_all(&sessions_dir).unwrap();

        let server = thread::spawn(move || {
            let mut proj_seq = 0u64;
            let mut thread_seq = 0u64;
            let mut turn_seq = 0u64;
            while !stop_ref.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        stream.set_nonblocking(false).unwrap();
                        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                        let Ok(mut ws) = tungstenite::accept(stream) else {
                            continue;
                        };
                        while let Ok(msg) = ws.read() {
                            let Message::Text(text) = msg else {
                                continue;
                            };
                            let req: Value = serde_json::from_str(&text).unwrap();
                            calls_ref.lock().unwrap().push(req.clone());
                            let Some(method) = req.get("method").and_then(Value::as_str) else {
                                continue;
                            };
                            if req.get("id").is_none() {
                                continue;
                            }
                            let params = &req["params"];
                            let result = match method {
                                "initialize" => json!({"userAgent": "codex-app-server-fixture"}),
                                "project/create" => {
                                    proj_seq += 1;
                                    let project_id = format!("019a5300-0000-7000-8000-{proj_seq:012x}");
                                    let name = params["name"].as_str().unwrap_or("unnamed");
                                    let root_path = params["rootPath"].as_str().unwrap_or("");
                                    let sql = format!(
                                        "INSERT INTO projects (id, name, metadata, position, created_at_ms, updated_at_ms) \
                                         VALUES ('{project_id}', '{}', '{{}}', {proj_seq}, 1762300000000, 1762300000000);\
                                         INSERT INTO project_roots (project_id, position, path) \
                                         VALUES ('{project_id}', 0, '{}');",
                                        sql_escape(name),
                                        sql_escape(root_path),
                                    );
                                    exec_sqlite(&db_ref, &sql);
                                    update_codex_global_state(&global_ref, |state| {
                                        state["local-projects"][&project_id] = json!({
                                            "id": project_id,
                                            "name": name,
                                            "roots": [root_path],
                                        });
                                        state["project-order"]
                                            .as_array_mut()
                                            .unwrap()
                                            .push(json!(project_id));
                                    });
                                    json!({
                                        "project": {
                                            "id": project_id,
                                            "name": name,
                                            "roots": [root_path],
                                        }
                                    })
                                }
                                "thread/start" => {
                                    thread_seq += 1;
                                    let thread_id = format!("019a5301-0000-7000-8000-{thread_seq:012x}");
                                    let project_id = params["projectId"].as_str().unwrap_or("");
                                    let cwd = params["cwd"].as_str().unwrap_or("");
                                    let title = params["title"].as_str().unwrap_or("");
                                    let rollout_file =
                                        sessions_dir.join(format!("rollout-20261105-{thread_id}.jsonl"));
                                    fs::write(&rollout_file, b"").unwrap();
                                    let db_proj_col = if force_null_project_on_thread {
                                        "NULL".to_string()
                                    } else {
                                        format!("'{}'", sql_escape(project_id))
                                    };
                                    let sql = format!(
                                        "INSERT INTO threads \
                                         (id, rollout_path, created_at, updated_at, source, model_provider, cwd, title, \
                                          sandbox_policy, approval_mode, has_user_event, archived, preview, recency_at_ms, project_id, originator) \
                                         VALUES ('{thread_id}', '{}', 1762300000, 1762300000, 'vscode', 'openai', '{}', '{}', \
                                                 'workspace-write', 'never', 1, 0, '', 1762300000000, {db_proj_col}, 'Codex Desktop');",
                                        sql_escape(&rollout_file.display().to_string()),
                                        sql_escape(cwd),
                                        sql_escape(title),
                                    );
                                    exec_sqlite(&db_ref, &sql);
                                    json!({
                                        "thread": {
                                            "id": thread_id,
                                            "projectId": project_id,
                                            "cwd": cwd,
                                            "title": title,
                                        }
                                    })
                                }
                                "thread/resume" => {
                                    let thread_id = params["threadId"].as_str().unwrap_or("");
                                    let project_id = params["projectId"].as_str().unwrap_or("");
                                    let cwd = params["cwd"].as_str().unwrap_or("");
                                    json!({
                                        "thread": {
                                            "id": thread_id,
                                            "projectId": project_id,
                                            "cwd": cwd,
                                            "status": {"type": "idle"}
                                        }
                                    })
                                }
                                "turn/start" => {
                                    turn_seq += 1;
                                    let turn_id = format!("dots-turn-{turn_seq}");
                                    let thread_id = params["threadId"].as_str().unwrap_or("");
                                    let text = params
                                        .pointer("/input/0/text")
                                        .and_then(Value::as_str)
                                        .unwrap_or("");
                                    let preview = if force_empty_preview { "" } else { text };
                                    let sql = format!(
                                        "UPDATE threads SET preview = '{}', updated_at = 1762300010, recency_at_ms = 1762300010000 \
                                         WHERE id = '{}';",
                                        sql_escape(preview),
                                        sql_escape(thread_id),
                                    );
                                    exec_sqlite(&db_ref, &sql);
                                    json!({
                                        "turn": {
                                            "id": turn_id,
                                            "status": "inProgress"
                                        }
                                    })
                                }
                                other => panic!("unexpected codex app-server method: {other}"),
                            };
                            let resp = json!({"id": req["id"], "result": result});
                            if ws.send(Message::Text(resp.to_string().into())).is_err() {
                                break;
                            }
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2));
                    }
                    Err(_) => break,
                }
            }
        });

        Self {
            root,
            projects_root,
            state_root,
            codex_root,
            db_path,
            global_state_path,
            socket,
            rpc_calls,
            stop,
            server: Some(server),
        }
    }

    fn runtime_config(&self) -> OpenAiDotsRuntimeConfig {
        OpenAiDotsRuntimeConfig::new(self.codex_root.clone(), self.socket.clone())
    }
}

impl Drop for OpenAiDotsFixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(server) = self.server.take() {
            let _ = server.join();
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn sql_escape(s: &str) -> String {
    s.replace('\'', "''")
}

fn exec_sqlite(db_path: &Path, sql: &str) {
    let status = Command::new("/usr/bin/sqlite3")
        .arg(db_path)
        .arg(sql)
        .status()
        .unwrap();
    assert!(status.success(), "sqlite3 execution failed: {sql}");
}

fn update_codex_global_state<F>(path: &Path, f: F)
where
    F: FnOnce(&mut Value),
{
    let mut state: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    f(&mut state);
    fs::write(path, serde_json::to_vec_pretty(&state).unwrap()).unwrap();
}

// ============================================================================
// DSH HTTP Gateway Fixture (`/v1/sessions` & `/v1/sessions/<id>/turns`)
// ============================================================================

#[derive(Clone, Debug)]
struct DshRecordedRequest {
    method: String,
    path: String,
    body: Option<Value>,
}

struct DshGatewayFixture {
    root: PathBuf,
    projects_root: PathBuf,
    state_root: PathBuf,
    base_url: String,
    requests: Arc<Mutex<Vec<DshRecordedRequest>>>,
    stop: Arc<AtomicBool>,
    server: Option<JoinHandle<()>>,
}

impl DshGatewayFixture {
    fn new(spoof_wrong_cwd_on_readback: bool) -> Self {
        let root = std::env::temp_dir().join(format!(
            "fleet-dsh-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let projects_root = root.join("Projects");
        let state_root = root.join("state");
        fs::create_dir_all(&projects_root).unwrap();
        fs::create_dir_all(&state_root).unwrap();

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let addr = listener.local_addr().unwrap();
        let base_url = format!("http://{addr}/v1");

        let requests = Arc::new(Mutex::new(Vec::new()));
        let req_ref = requests.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_ref = stop.clone();

        let server = thread::spawn(move || {
            let mut sessions: BTreeMap<String, Value> = BTreeMap::new();
            let mut session_seq = 0u64;
            let mut turn_seq = 0u64;

            while !stop_ref.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream.set_nonblocking(false).unwrap();
                        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                        let Ok((method, path, body)) = read_http_json_request(&mut stream) else {
                            continue;
                        };
                        req_ref.lock().unwrap().push(DshRecordedRequest {
                            method: method.clone(),
                            path: path.clone(),
                            body: body.clone(),
                        });

                        let (code, resp_json) = if method == "POST" && path == "/v1/sessions" {
                            session_seq += 1;
                            let session_id = format!("dsh-sess-{session_seq:04}");
                            let payload = body.unwrap_or_else(|| json!({}));
                            let cwd = payload["cwd"].as_str().unwrap_or("").to_string();
                            let project_id =
                                payload["project_id"].as_str().unwrap_or("").to_string();
                            let title = payload["title"].as_str().unwrap_or("").to_string();
                            let sess_val = json!({
                                "id": session_id,
                                "project_id": project_id,
                                "title": title,
                                "cwd": cwd,
                                "status": "active",
                                "turns": []
                            });
                            sessions.insert(session_id.clone(), sess_val.clone());
                            (201u16, json!({"session": sess_val}))
                        } else if method == "POST"
                            && path.starts_with("/v1/sessions/")
                            && path.ends_with("/turns")
                        {
                            let session_id = path
                                .trim_start_matches("/v1/sessions/")
                                .trim_end_matches("/turns");
                            if let Some(sess) = sessions.get_mut(session_id) {
                                turn_seq += 1;
                                let turn_id = format!("dsh-turn-{turn_seq:04}");
                                let payload = body.unwrap_or_else(|| json!({}));
                                let turn_val = json!({
                                    "id": turn_id,
                                    "delivery_id": payload["delivery_id"],
                                    "prompt": payload["prompt"],
                                    "model": payload.get("model").cloned().unwrap_or(Value::Null),
                                    "status": "running"
                                });
                                sess["turns"].as_array_mut().unwrap().push(turn_val.clone());
                                (200u16, json!({"turn": turn_val, "session_id": session_id}))
                            } else {
                                (404u16, json!({"error": "unknown DSH session"}))
                            }
                        } else if method == "GET" && path.starts_with("/v1/sessions/") {
                            let session_id = path.trim_start_matches("/v1/sessions/");
                            if let Some(sess) = sessions.get(session_id) {
                                let mut cloned = sess.clone();
                                if spoof_wrong_cwd_on_readback {
                                    cloned["cwd"] = json!("/tmp/hijacked-wrong-cwd");
                                }
                                (200u16, json!({"session": cloned}))
                            } else {
                                (404u16, json!({"error": "unknown DSH session"}))
                            }
                        } else {
                            (404u16, json!({"error": "route not found"}))
                        };

                        let body_str = resp_json.to_string();
                        let http_resp = format!(
                            "HTTP/1.1 {code} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body_str}",
                            body_str.len()
                        );
                        let _ = stream.write_all(http_resp.as_bytes());
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2));
                    }
                    Err(_) => break,
                }
            }
        });

        Self {
            root,
            projects_root,
            state_root,
            base_url,
            requests,
            stop,
            server: Some(server),
        }
    }

    fn runtime_config(&self) -> DshRuntimeConfig {
        DshRuntimeConfig::new(self.base_url.clone())
    }
}

impl Drop for DshGatewayFixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(server) = self.server.take() {
            let _ = server.join();
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn read_http_json_request(stream: &mut TcpStream) -> std::io::Result<(String, String, Option<Value>)> {
    let mut head = Vec::new();
    while !head.ends_with(b"\r\n\r\n") {
        let mut byte = [0u8; 1];
        stream.read_exact(&mut byte)?;
        head.push(byte[0]);
    }
    let head_str = String::from_utf8_lossy(&head).into_owned();
    let mut lines = head_str.split("\r\n");
    let first = lines.next().unwrap_or_default();
    let mut parts = first.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let path = parts.next().unwrap_or_default().to_string();
    let mut content_len = 0usize;
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            if k.trim().eq_ignore_ascii_case("content-length") {
                content_len = v.trim().parse().unwrap_or(0);
            }
        }
    }
    let body = if content_len > 0 {
        let mut buf = vec![0u8; content_len];
        stream.read_exact(&mut buf)?;
        serde_json::from_slice(&buf).ok()
    } else {
        None
    };
    Ok((method, path, body))
}

// ============================================================================
// Cindy Gateway Fixture (`sessions.patch`, `sessions.create`, `cindy_session_send`, `sessions.get`)
// ============================================================================

struct CindyGatewayFixture {
    root: PathBuf,
    projects_root: PathBuf,
    state_root: PathBuf,
    cindy_root: PathBuf,
    socket: PathBuf,
    rpc_calls: Arc<Mutex<Vec<Value>>>,
    stop: Arc<AtomicBool>,
    server: Option<JoinHandle<()>>,
}

impl CindyGatewayFixture {
    fn new(spoof_wrong_cwd_on_get: bool) -> Self {
        let root = std::env::temp_dir().join(format!(
            "fleet-cindy-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let projects_root = root.join("Projects");
        let state_root = root.join("state");
        let cindy_root = root.join(".cindy");
        let transcripts_dir = cindy_root.join("transcripts");
        fs::create_dir_all(&projects_root).unwrap();
        fs::create_dir_all(&state_root).unwrap();
        fs::create_dir_all(&transcripts_dir).unwrap();

        let socket = root.join("cindy-gateway.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        listener.set_nonblocking(true).unwrap();

        let rpc_calls = Arc::new(Mutex::new(Vec::new()));
        let calls_ref = rpc_calls.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_ref = stop.clone();
        let transcripts_ref = transcripts_dir.clone();

        let server = thread::spawn(move || {
            let mut sessions: BTreeMap<String, Value> = BTreeMap::new();
            let mut proj_seq = 0u64;
            let mut sess_seq = 0u64;
            let mut turn_seq = 0u64;

            while !stop_ref.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        stream.set_nonblocking(false).unwrap();
                        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                        let Ok(mut ws) = tungstenite::accept(stream) else {
                            continue;
                        };
                        while let Ok(msg) = ws.read() {
                            let Message::Text(text) = msg else {
                                continue;
                            };
                            let req: Value = serde_json::from_str(&text).unwrap();
                            calls_ref.lock().unwrap().push(req.clone());
                            let method = req["method"].as_str().unwrap_or("");
                            let params = &req["params"];
                            let result = match method {
                                "sessions.patch" => {
                                    proj_seq += 1;
                                    let slug = params["slug"].as_str().unwrap_or("proj");
                                    let project_id = format!("cindy-proj-{slug}-{proj_seq}");
                                    json!({
                                        "projectId": project_id,
                                        "cwd": params["cwd"],
                                        "workspaceUris": params["workspaceUris"],
                                    })
                                }
                                "sessions.create" => {
                                    sess_seq += 1;
                                    let session_key = format!("cindy-session-{sess_seq:04}");
                                    let transcript_path =
                                        transcripts_ref.join(format!("{session_key}.jsonl"));
                                    fs::write(&transcript_path, b"").unwrap();
                                    let sess_val = json!({
                                        "sessionKey": session_key,
                                        "projectId": params["projectId"],
                                        "cwd": params["cwd"],
                                        "title": params["title"],
                                        "transcriptPath": transcript_path,
                                        "turns": []
                                    });
                                    sessions.insert(session_key.clone(), sess_val);
                                    json!({
                                        "sessionKey": session_key,
                                        "projectId": params["projectId"],
                                        "cwd": params["cwd"],
                                        "transcriptPath": transcript_path,
                                    })
                                }
                                "cindy_session_send" => {
                                    let session_key =
                                        params["sessionKey"].as_str().unwrap_or("");
                                    let sess = sessions
                                        .get_mut(session_key)
                                        .expect("cindy session must exist before cindy_session_send");
                                    turn_seq += 1;
                                    let turn_id = format!("cindy-run-{turn_seq:04}");
                                    let turn_entry = json!({
                                        "turnId": turn_id,
                                        "deliveryId": params["deliveryId"],
                                        "projectId": params["projectId"],
                                        "cwd": params["cwd"],
                                        "text": params["text"],
                                        "model": params.get("model").cloned().unwrap_or(Value::Null),
                                    });
                                    sess["turns"]
                                        .as_array_mut()
                                        .unwrap()
                                        .push(turn_entry.clone());
                                    let transcript_path = PathBuf::from(
                                        sess["transcriptPath"].as_str().unwrap(),
                                    );
                                    let mut existing =
                                        fs::read_to_string(&transcript_path).unwrap_or_default();
                                    existing.push_str(&serde_json::to_string(&turn_entry).unwrap());
                                    existing.push('\n');
                                    fs::write(&transcript_path, existing).unwrap();
                                    json!({
                                        "sessionKey": session_key,
                                        "turnId": turn_id,
                                        "runId": turn_id,
                                        "status": "accepted"
                                    })
                                }
                                "sessions.get" => {
                                    let session_key =
                                        params["sessionKey"].as_str().unwrap_or("");
                                    let mut sess = sessions
                                        .get(session_key)
                                        .cloned()
                                        .expect("cindy session must exist on sessions.get");
                                    if spoof_wrong_cwd_on_get {
                                        sess["cwd"] = json!("/tmp/cindy-wrong-cwd");
                                    }
                                    json!({"session": sess})
                                }
                                other => panic!("unexpected Cindy gateway method: {other}"),
                            };
                            let resp = json!({"id": req["id"], "result": result});
                            if ws.send(Message::Text(resp.to_string().into())).is_err() {
                                break;
                            }
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2));
                    }
                    Err(_) => break,
                }
            }
        });

        Self {
            root,
            projects_root,
            state_root,
            cindy_root,
            socket,
            rpc_calls,
            stop,
            server: Some(server),
        }
    }

    fn runtime_config(&self) -> CindyRuntimeConfig {
        CindyRuntimeConfig::new(self.cindy_root.clone(), self.socket.clone())
    }
}

impl Drop for CindyGatewayFixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(server) = self.server.take() {
            let _ = server.join();
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn openai_dots_creates_project_starts_thread_resumes_and_verifies_state_5_sqlite() {
    let f = OpenAiDotsFixture::new(false, false);
    let mut adapter = OpenAiDotsAdapter::with_runtime(
        f.projects_root.clone(),
        f.state_root.clone(),
        f.runtime_config(),
    )
    .unwrap();

    // 1. Create directory + mount sidebar project via `codex app-server` `project/create`
    let binding = adapter
        .ensure_sidebar_project("self-dev-dots", Some("OpenAI Dots Self Dev"))
        .unwrap();
    assert_eq!(binding.host, DesktopHostKind::OpenaiDots);
    assert!(binding.project_dir.is_dir());
    assert_eq!(binding.project_dir, f.projects_root.join("self-dev-dots"));
    assert!(binding.project_id.starts_with("019a5300-"));

    // Verify `state_5.sqlite` `projects` & `project_roots`
    let proj_row = read_codex_project(Path::new("/usr/bin/sqlite3"), &f.db_path, &binding.project_id)
        .unwrap()
        .expect("project row must exist in state_5.sqlite");
    assert_eq!(proj_row.project_id, binding.project_id);
    assert_eq!(proj_row.name, "OpenAI Dots Self Dev");
    assert_eq!(proj_row.root_path, binding.project_dir);

    // Verify `.codex-global-state.json` includes the sidebar project
    let global_state: Value =
        serde_json::from_slice(&fs::read(&f.global_state_path).unwrap()).unwrap();
    assert_eq!(
        global_state["local-projects"][&binding.project_id]["roots"][0],
        json!(binding.project_dir)
    );

    // 2. Start sidebar-visible thread + initial turn via `thread/start` + `turn/start`
    let create_req = CreateSessionRequest {
        delivery_id: "dots-deliv-1".into(),
        project_id: binding.project_id.clone(),
        project_dir: binding.project_dir.clone(),
        title: "Slice K OpenAI Dots Thread".into(),
        prompt: "Bootstrap OpenAI Dots sidebar project and run initial turn.".into(),
        model: Some("gpt-5-codex".into()),
    };
    let created = adapter.create_dots_session(&create_req).unwrap();
    assert_eq!(created.host, DesktopHostKind::OpenaiDots);
    assert_eq!(created.project_id, binding.project_id);
    assert!(created.thread_id.starts_with("019a5301-"));
    assert_eq!(created.session_id, created.thread_id);
    assert_eq!(created.turn_id, "dots-turn-1");
    assert_eq!(created.project_dir, binding.project_dir);
    assert_eq!(
        created.deep_link,
        format!("codex://threads/{}", created.thread_id)
    );
    assert_eq!(created.tier, DeliveryTier::ThreadVisible);

    // Verify `threads` row in `state_5.sqlite`
    let thread_row = read_codex_thread(Path::new("/usr/bin/sqlite3"), &f.db_path, &created.thread_id)
        .unwrap()
        .expect("thread row must exist in state_5.sqlite");
    assert_eq!(thread_row.project_id, binding.project_id);
    assert_eq!(thread_row.cwd, binding.project_dir);
    assert_eq!(thread_row.archived, 0);
    assert_eq!(
        thread_row.preview,
        "Bootstrap OpenAI Dots sidebar project and run initial turn."
    );

    // 3. Resume and deliver follow-up turn on the same thread (`thread/resume` + `turn/start`)
    let followup_req = DeliverTurnRequest {
        delivery_id: "dots-deliv-2".into(),
        project_id: binding.project_id.clone(),
        session_id: created.thread_id.clone(),
        project_dir: binding.project_dir.clone(),
        prompt: "Continue on the same OpenAI Dots sidebar thread.".into(),
        model: None,
    };
    let followup = adapter.deliver_dots_turn(&followup_req).unwrap();
    assert_eq!(followup.thread_id, created.thread_id);
    assert_eq!(followup.turn_id, "dots-turn-2");
    assert_eq!(
        followup.preview,
        "Continue on the same OpenAI Dots sidebar thread."
    );

    let methods: Vec<String> = f
        .rpc_calls
        .lock()
        .unwrap()
        .iter()
        .filter_map(|v| {
            if v.get("id").is_some() {
                v.get("method").and_then(Value::as_str).map(str::to_string)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        methods,
        vec![
            "initialize",
            "project/create",
            "initialize",
            "thread/start",
            "turn/start",
            "initialize",
            "thread/resume",
            "turn/start",
        ]
    );
    let last_turn_call = f
        .rpc_calls
        .lock()
        .unwrap()
        .iter()
        .rev()
        .find(|v| v.get("method").and_then(Value::as_str) == Some("turn/start"))
        .cloned()
        .unwrap();
    assert_eq!(last_turn_call["params"]["model"], json!("zcode/GLM-5.3-Flash"));
    assert_eq!(last_turn_call["params"]["effort"], json!("max"));
    println!(
        "OPENAI_DOTS_VERIFIED project_id={} thread_id={} turns=[{}, {}] deep_link={}",
        followup.project_id,
        followup.thread_id,
        created.turn_id,
        followup.turn_id,
        followup.deep_link
    );
}

#[test]
fn openai_dots_rejects_unscoped_null_project_or_empty_preview_in_state_5_sqlite() {
    // Case 1: Thread created with NULL project_id (unscoped CLI thread) must be rejected
    let f_null = OpenAiDotsFixture::new(true, false);
    let mut adapter_null = OpenAiDotsAdapter::with_runtime(
        f_null.projects_root.clone(),
        f_null.state_root.clone(),
        f_null.runtime_config(),
    )
    .unwrap();
    let binding = adapter_null
        .ensure_sidebar_project("dots-guard", Some("Dots Guard"))
        .unwrap();
    let err_null = adapter_null
        .create_dots_session(&CreateSessionRequest {
            delivery_id: "dots-null-proj".into(),
            project_id: binding.project_id.clone(),
            project_dir: binding.project_dir.clone(),
            title: "Should Reject Null Project".into(),
            prompt: "Verify null project_id in threads is rejected.".into(),
            model: None,
        })
        .unwrap_err();
    assert!(matches!(err_null, HostAdapterError::ForbiddenProjectId(_)));

    // Case 2: Thread with empty preview fails `idx_threads_visible_recency_at_ms` visibility check
    let f_empty = OpenAiDotsFixture::new(false, true);
    let mut adapter_empty = OpenAiDotsAdapter::with_runtime(
        f_empty.projects_root.clone(),
        f_empty.state_root.clone(),
        f_empty.runtime_config(),
    )
    .unwrap();
    let binding2 = adapter_empty
        .ensure_sidebar_project("dots-preview-guard", Some("Dots Preview Guard"))
        .unwrap();
    let err_empty = adapter_empty
        .create_dots_session(&CreateSessionRequest {
            delivery_id: "dots-empty-preview".into(),
            project_id: binding2.project_id.clone(),
            project_dir: binding2.project_dir.clone(),
            title: "Should Reject Empty Preview".into(),
            prompt: "Verify empty preview in threads is rejected.".into(),
            model: None,
        })
        .unwrap_err();
    assert!(matches!(err_empty, HostAdapterError::OpenAiDotsProtocol(_)));
}

#[test]
fn dsh_creates_project_dir_binds_session_cwd_and_delivers_followup_turns() {
    let f = DshGatewayFixture::new(false);
    let mut adapter = DeepSeekHarnessAdapter::with_runtime(
        f.projects_root.clone(),
        f.state_root.clone(),
        f.runtime_config(),
    )
    .unwrap();

    // 1. Create project directory & register sidebar project
    let binding = adapter
        .ensure_sidebar_project("self-dev-dsh", Some("DSH Self Dev"))
        .unwrap();
    assert_eq!(binding.host, DesktopHostKind::DeepseekHarness);
    assert!(binding.project_dir.is_dir());
    assert_eq!(binding.project_dir, f.projects_root.join("self-dev-dsh"));

    // 2. Create DSH session (`POST /v1/sessions` with `cwd` + `POST /v1/sessions/<id>/turns`)
    let create_req = CreateSessionRequest {
        delivery_id: "dsh-deliv-1".into(),
        project_id: binding.project_id.clone(),
        project_dir: binding.project_dir.clone(),
        title: "Slice K DSH Session".into(),
        prompt: "Create DSH session bound to newly created project cwd.".into(),
        model: Some("deepseek-r1".into()),
    };
    let created = adapter.create_dsh_session(&create_req).unwrap();
    assert_eq!(created.host, DesktopHostKind::DeepseekHarness);
    assert_eq!(created.project_id, binding.project_id);
    assert_eq!(created.session_id, "dsh-sess-0001");
    assert_eq!(created.turn_id, "dsh-turn-0001");
    assert_eq!(created.project_dir, binding.project_dir);
    assert_eq!(created.turn_count, 1);
    assert_eq!(created.tier, DeliveryTier::ThreadVisible);

    // 3. Deliver follow-up turn to same DSH session (`POST /v1/sessions/<id>/turns`)
    let followup_req = DeliverTurnRequest {
        delivery_id: "dsh-deliv-2".into(),
        project_id: binding.project_id.clone(),
        session_id: created.session_id.clone(),
        project_dir: binding.project_dir.clone(),
        prompt: "Follow up on the same DSH session and verify turn count.".into(),
        model: None,
    };
    let followup = adapter.deliver_dsh_turn(&followup_req).unwrap();
    assert_eq!(followup.session_id, created.session_id);
    assert_eq!(followup.turn_id, "dsh-turn-0002");
    assert_eq!(followup.turn_count, 2);
    assert_eq!(followup.project_dir, binding.project_dir);

    let reqs = f.requests.lock().unwrap().clone();
    assert_eq!(reqs[0].method, "POST");
    assert_eq!(reqs[0].path, "/v1/sessions");
    assert_eq!(
        reqs[0].body.as_ref().unwrap()["cwd"],
        json!(binding.project_dir)
    );
    assert_eq!(reqs[1].method, "POST");
    assert_eq!(reqs[1].path, "/v1/sessions/dsh-sess-0001/turns");
    assert!(reqs.iter().any(|r| {
        r.method == "POST"
            && r.path == "/v1/sessions/dsh-sess-0001/turns"
            && r.body.as_ref().unwrap()["delivery_id"] == "dsh-deliv-2"
    }));
    println!(
        "DSH_VERIFIED project_id={} session_id={} turns=[{}, {}] cwd={}",
        followup.project_id,
        followup.session_id,
        created.turn_id,
        followup.turn_id,
        followup.project_dir.display()
    );
}

#[test]
fn dsh_rejects_mismatched_cwd_on_gateway_readback() {
    let f = DshGatewayFixture::new(true);
    let mut adapter = DeepSeekHarnessAdapter::with_runtime(
        f.projects_root.clone(),
        f.state_root.clone(),
        f.runtime_config(),
    )
    .unwrap();
    let binding = adapter
        .ensure_sidebar_project("dsh-cwd-guard", Some("DSH Cwd Guard"))
        .unwrap();
    let err = adapter
        .create_dsh_session(&CreateSessionRequest {
            delivery_id: "dsh-bad-cwd".into(),
            project_id: binding.project_id.clone(),
            project_dir: binding.project_dir.clone(),
            title: "Should Reject Wrong Cwd".into(),
            prompt: "Verify gateway cwd drift is caught.".into(),
            model: None,
        })
        .unwrap_err();
    assert!(matches!(err, HostAdapterError::ProjectDirMismatch { .. }));
}

#[test]
fn dsh_native_cli_registers_workspace_json_and_verifies_session_projcache_and_archive() {
    use std::os::unix::fs::PermissionsExt;

    let root = std::env::temp_dir().join(format!(
        "fleet-dsh-native-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let projects_root = root.join("Projects");
    let state_root = root.join("state");
    let dsh_root = root.join(".dsh");
    fs::create_dir_all(&projects_root).unwrap();
    fs::create_dir_all(&state_root).unwrap();
    fs::create_dir_all(dsh_root.join("storages/session_projcache/sessions")).unwrap();
    fs::create_dir_all(dsh_root.join("sessions")).unwrap();

    let fake_dsh_bin = root.join("fake-dsh.sh");
    let script = r#"#!/bin/sh
set -eu
SID="session-11111111-2222-4333-8444-555555555555"
TURN=1
while [ $# -gt 0 ]; do
  case "$1" in
    --session-id)
      SID="$2"
      TURN=2
      shift 2
      ;;
    *)
      shift
      ;;
  esac
done
CWD_NOW="$(pwd -P)"
SLUG="--$(printf '%s' "$CWD_NOW" | sed 's|^/||; s|/$||; s|/|-|g')--"
mkdir -p "$DSH_HOME/sessions/$SLUG/$SID"
printf 'mock-zstd-archive\n' > "$DSH_HOME/sessions/$SLUG/$SID/session.v4.jsonl.zstd"
mkdir -p "$DSH_HOME/storages/session_projcache/sessions"
cat > "$DSH_HOME/storages/session_projcache/sessions/$SID.json" <<EOF
{
  "version": 7,
  "record": {
    "identity": {
      "formatVersion": 4,
      "createdAt": 1793850818052,
      "cwd": "$CWD_NOW",
      "isSeeded": false,
      "inheritedEventCount": 0
    },
    "rows": {
      "title": { "ver": 1, "seq": 10, "val": "Native DSH Test Session" },
      "workingDirectory": { "ver": 1, "seq": 10, "val": "$CWD_NOW" },
      "turnBoundary": { "ver": 2, "seq": 10, "val": { "lastTurn": $TURN } }
    }
  }
}
EOF
printf '{"type":"session","sessionId":"%s","cwd":"%s"}\n' "$SID" "$CWD_NOW"
printf '{"type":"status","phase":"turn_start","turn":%s}\n' "$TURN"
printf '{"type":"status","phase":"turn_end","turn":%s,"reason":{"kind":"completed"}}\n' "$TURN"
printf '{"type":"final","text":"READY"}\n'
"#;
    fs::write(&fake_dsh_bin, script).unwrap();
    let mut perms = fs::metadata(&fake_dsh_bin).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&fake_dsh_bin, perms).unwrap();

    let runtime = DshRuntimeConfig::new_native(dsh_root.clone(), fake_dsh_bin);
    let mut adapter = DeepSeekHarnessAdapter::with_runtime(
        projects_root.clone(),
        state_root,
        runtime,
    )
    .unwrap();

    let binding = adapter
        .ensure_sidebar_project("glaux-book", Some("glaux-book"))
        .unwrap();
    assert_eq!(binding.host, DesktopHostKind::DeepseekHarness);
    assert_eq!(binding.project_dir, projects_root.join("glaux-book"));

    let created = adapter
        .create_dsh_session(&CreateSessionRequest {
            delivery_id: "dsh-native-1".into(),
            project_id: binding.project_id.clone(),
            project_dir: binding.project_dir.clone(),
            title: "Translate Maitra Paper".into(),
            prompt: "Resume translation of Code Without Copyright.".into(),
            model: Some("group/ds-flash".into()),
        })
        .unwrap();
    assert_eq!(
        created.session_id,
        "session-11111111-2222-4333-8444-555555555555"
    );
    assert_eq!(created.turn_id, "turn-1");
    assert_eq!(created.turn_count, 1);

    let followup = adapter
        .deliver_dsh_turn(&DeliverTurnRequest {
            delivery_id: "dsh-native-2".into(),
            project_id: binding.project_id.clone(),
            session_id: created.session_id.clone(),
            project_dir: binding.project_dir.clone(),
            prompt: "Verify EPUB and Zotero attachment.".into(),
            model: Some("group/ds-flash".into()),
        })
        .unwrap();
    assert_eq!(followup.session_id, created.session_id);
    assert_eq!(followup.turn_id, "turn-2");
    assert_eq!(followup.turn_count, 2);

    let ws_doc: Value =
        serde_json::from_slice(&fs::read(dsh_root.join("storages/workspace.json")).unwrap())
            .unwrap();
    let sess_ids = ws_doc
        .pointer(&format!("/tables/workspaces/{}/sessionIds", binding.project_id))
        .and_then(Value::as_array)
        .unwrap();
    assert!(sess_ids
        .iter()
        .any(|v| v.as_str() == Some("session-11111111-2222-4333-8444-555555555555")));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn cindy_creates_project_dir_binds_gateway_session_cwd_and_sends_followup_via_cindy_session_send() {
    let f = CindyGatewayFixture::new(false);
    let mut adapter = CindyAdapter::with_runtime(
        f.projects_root.clone(),
        f.state_root.clone(),
        f.runtime_config(),
    )
    .unwrap();

    // 1. Create directory & bind workspace in Cindy gateway (`sessions.patch`)
    let binding = adapter
        .ensure_sidebar_project("self-dev-cindy", Some("Cindy Self Dev"))
        .unwrap();
    assert_eq!(binding.host, DesktopHostKind::Cindy);
    assert!(binding.project_dir.is_dir());
    assert_eq!(binding.project_dir, f.projects_root.join("self-dev-cindy"));
    assert!(binding.project_id.starts_with("cindy-proj-self-dev-cindy-"));

    // 2. Create Cindy session (`sessions.create` + `cindy_session_send` + `sessions.get`)
    let create_req = CreateSessionRequest {
        delivery_id: "cindy-deliv-1".into(),
        project_id: binding.project_id.clone(),
        project_dir: binding.project_dir.clone(),
        title: "Slice K Cindy Session".into(),
        prompt: "Initialize Cindy workspace session and verify transcript.".into(),
        model: Some("cindy-sonnet".into()),
    };
    let created = adapter.create_cindy_session(&create_req).unwrap();
    assert_eq!(created.host, DesktopHostKind::Cindy);
    assert_eq!(created.project_id, binding.project_id);
    assert_eq!(created.session_key, "cindy-session-0001");
    assert_eq!(created.session_id, "cindy-session-0001");
    assert_eq!(created.turn_id, "cindy-run-0001");
    assert_eq!(created.project_dir, binding.project_dir);
    assert_eq!(created.turn_count, 1);
    assert_eq!(created.tier, DeliveryTier::ThreadVisible);
    assert!(created.transcript_path.is_file());

    // 3. Deliver follow-up turn to same `sessionKey` via `cindy_session_send`
    let followup_req = DeliverTurnRequest {
        delivery_id: "cindy-deliv-2".into(),
        project_id: binding.project_id.clone(),
        session_id: created.session_key.clone(),
        project_dir: binding.project_dir.clone(),
        prompt: "Send follow-up turn to the same Cindy session via cindy_session_send.".into(),
        model: None,
    };
    let followup = adapter.deliver_cindy_turn(&followup_req).unwrap();
    assert_eq!(followup.session_key, created.session_key);
    assert_eq!(followup.turn_id, "cindy-run-0002");
    assert_eq!(followup.turn_count, 2);

    // Verify transcript JSONL records both turns with matching `cwd` and `projectId`
    let transcript = read_cindy_transcript(&followup.transcript_path).unwrap();
    assert_eq!(transcript.len(), 2);
    assert_eq!(transcript[0]["turnId"], "cindy-run-0001");
    assert_eq!(transcript[0]["cwd"], json!(binding.project_dir));
    assert_eq!(transcript[1]["turnId"], "cindy-run-0002");
    assert_eq!(transcript[1]["cwd"], json!(binding.project_dir));

    let methods: Vec<String> = f
        .rpc_calls
        .lock()
        .unwrap()
        .iter()
        .filter_map(|v| v.get("method").and_then(Value::as_str).map(str::to_string))
        .collect();
    assert!(methods.contains(&"sessions.patch".to_string()));
    assert!(methods.contains(&"sessions.create".to_string()));
    assert_eq!(
        methods
            .iter()
            .filter(|m| m.as_str() == "cindy_session_send")
            .count(),
        2
    );
    println!(
        "CINDY_VERIFIED project_id={} session_key={} turns=[{}, {}] transcript={}",
        followup.project_id,
        followup.session_key,
        created.turn_id,
        followup.turn_id,
        followup.transcript_path.display()
    );
}

#[test]
fn cindy_rejects_mismatched_cwd_on_gateway_sessions_get() {
    let f = CindyGatewayFixture::new(true);
    let mut adapter = CindyAdapter::with_runtime(
        f.projects_root.clone(),
        f.state_root.clone(),
        f.runtime_config(),
    )
    .unwrap();
    let binding = adapter
        .ensure_sidebar_project("cindy-cwd-guard", Some("Cindy Cwd Guard"))
        .unwrap();
    let err = adapter
        .create_cindy_session(&CreateSessionRequest {
            delivery_id: "cindy-bad-cwd".into(),
            project_id: binding.project_id.clone(),
            project_dir: binding.project_dir.clone(),
            title: "Should Reject Wrong Cwd".into(),
            prompt: "Verify Cindy gateway cwd mismatch is caught.".into(),
            model: None,
        })
        .unwrap_err();
    assert!(matches!(err, HostAdapterError::ProjectDirMismatch { .. }));
}
