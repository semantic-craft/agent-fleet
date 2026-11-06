#![cfg(unix)]

use fleet_daemon::{
    read_cindy_transcript, read_codex_project, read_codex_thread, read_conversation_summary,
    HostRelayClosureReceipt, HostStoreVerificationEvidence,
};
use fleet_proto::{DeliveryTier, DesktopHostKind, HostSessionReceipt, SidebarProjectBinding};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    os::unix::{fs::PermissionsExt, net::UnixListener},
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
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
";

const CONVERSATION_SUMMARIES_DDL: &str = "\
CREATE TABLE `conversation_summaries` (\
`conversation_id` text,\
`title` text NOT NULL DEFAULT \"\",\
`preview` text NOT NULL DEFAULT \"\",\
`step_count` integer NOT NULL DEFAULT 0,\
`last_modified_time` datetime NOT NULL,\
`workspace_uris` text NOT NULL,\
`status` text NOT NULL DEFAULT \"\",\
`source` text NOT NULL DEFAULT \"\",\
`project_id` text NOT NULL DEFAULT \"\",\
`agent_name` text NOT NULL DEFAULT \"\",\
`parent_conversation_id` text NOT NULL DEFAULT \"\",\
`nesting_depth` integer NOT NULL DEFAULT 0,\
`battle_id` text NOT NULL DEFAULT \"\",\
`winning_conversation_id` text NOT NULL DEFAULT \"\",\
`not_fully_idle` numeric NOT NULL DEFAULT false,\
`killed` numeric NOT NULL DEFAULT false,\
`last_user_input_time` datetime NOT NULL,\
`last_user_input_step_index` integer NOT NULL DEFAULT -1,\
`app_data_dir` text NOT NULL DEFAULT \"\",\
`raw_summary` blob,\
`group_id` text NOT NULL DEFAULT \"\",\
PRIMARY KEY (`conversation_id`));\
";

struct FourHostClusterFixture {
    root: PathBuf,
    projects_root: PathBuf,
    state_root: PathBuf,
    receipts_dir: PathBuf,
    // OpenAI Dots
    codex_root: PathBuf,
    codex_db_path: PathBuf,
    codex_global_state_path: PathBuf,
    codex_socket: PathBuf,
    // Cindy
    cindy_root: PathBuf,
    cindy_socket: PathBuf,
    // DSH
    dsh_base_url: String,
    // Antigravity Desktop
    antigravity_root: PathBuf,
    antigravity_db_path: PathBuf,
    agy_bin: PathBuf,
    // Background threads
    stop: Arc<AtomicBool>,
    workers: Vec<JoinHandle<()>>,
}

impl FourHostClusterFixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "fleet-four-hosts-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let projects_root = root.join("Projects");
        let state_root = root.join("state");
        let receipts_dir = root.join("receipts");
        fs::create_dir_all(&projects_root).unwrap();
        fs::create_dir_all(&state_root).unwrap();
        fs::create_dir_all(&receipts_dir).unwrap();

        let stop = Arc::new(AtomicBool::new(false));
        let mut workers = Vec::new();

        // 1. OpenAI Dots (`state_5.sqlite` + `.codex-global-state.json` + `app-server` socket)
        let codex_root = root.join(".codex");
        let codex_sessions_dir = codex_root.join("sessions/2026/11/05");
        fs::create_dir_all(&codex_sessions_dir).unwrap();
        let codex_db_path = codex_root.join("state_5.sqlite");
        exec_sqlite(&codex_db_path, CODEX_STATE_5_DDL);
        let codex_global_state_path = codex_root.join(".codex-global-state.json");
        fs::write(
            &codex_global_state_path,
            serde_json::to_vec_pretty(&json!({
                "local-projects": {},
                "project-order": [],
                "thread-project-assignments": {},
                "sidebar-project-thread-orders": {}
            }))
            .unwrap(),
        )
        .unwrap();
        let codex_socket = root.join("dots-app-server.sock");
        workers.push(spawn_codex_app_server(
            &codex_socket,
            codex_db_path.clone(),
            codex_global_state_path.clone(),
            codex_sessions_dir,
            stop.clone(),
        ));
        std::os::unix::fs::symlink(&codex_socket, codex_root.join("app-server.sock")).unwrap();

        // 2. Cindy (`cindy-gateway.sock` + `.cindy/transcripts`)
        let cindy_root = root.join(".cindy");
        let cindy_transcripts = cindy_root.join("transcripts");
        fs::create_dir_all(&cindy_transcripts).unwrap();
        let cindy_socket = root.join("cindy-gateway.sock");
        workers.push(spawn_cindy_gateway(
            &cindy_socket,
            cindy_transcripts,
            stop.clone(),
        ));
        std::os::unix::fs::symlink(&cindy_socket, cindy_root.join("cindy-gateway.sock")).unwrap();

        // 3. DeepSeek Harness (`/v1/sessions` + `/v1/sessions/<id>/turns`)
        let (dsh_base_url, dsh_worker) = spawn_dsh_gateway(stop.clone());
        workers.push(dsh_worker);

        // 4. Antigravity Desktop (`daemon/hub_52019.json` + `conversation_summaries.db` + `agy`)
        let antigravity_root = root.join(".gemini/antigravity");
        let daemon_dir = antigravity_root.join("daemon");
        fs::create_dir_all(&daemon_dir).unwrap();
        let antigravity_db_path = antigravity_root.join("conversation_summaries.db");
        exec_sqlite(&antigravity_db_path, CONVERSATION_SUMMARIES_DDL);
        fs::write(
            daemon_dir.join("hub_52019.json"),
            serde_json::to_vec_pretty(&json!({
                "pid": 52001,
                "httpsPort": 52018,
                "httpPort": 52019,
                "lsVersion": "1.28.0-antigravity",
                "csrfToken": "secret-csrf-redacted-token-slice-l"
            }))
            .unwrap(),
        )
        .unwrap();
        let agy_bin = root.join("mock_agy_slice_l.py");
        write_mock_agy_binary(&agy_bin);
        let local_bin_dir = root.join(".local/bin");
        fs::create_dir_all(&local_bin_dir).unwrap();
        write_mock_agy_binary(&local_bin_dir.join("agy"));

        // 5. ZCode (`~/.zcode` + `~/bin/zcode`)
        let zcode_root = root.join(".zcode");
        fs::create_dir_all(&zcode_root).unwrap();
        let home_bin_dir = root.join("bin");
        fs::create_dir_all(&home_bin_dir).unwrap();
        write_mock_zcode_binary(&home_bin_dir.join("zcode"));

        Self {
            root,
            projects_root,
            state_root,
            receipts_dir,
            codex_root,
            codex_db_path,
            codex_global_state_path,
            codex_socket,
            cindy_root,
            cindy_socket,
            dsh_base_url,
            antigravity_root,
            antigravity_db_path,
            agy_bin,
            stop,
            workers,
        }
    }

    fn run_cli(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_fleet-cli"))
            .env("HOME", &self.root)
            .env("DSH_BASE_URL", &self.dsh_base_url)
            .args(args)
            .output()
            .unwrap()
    }
}

impl Drop for FourHostClusterFixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        for handle in self.workers.drain(..) {
            let _ = handle.join();
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

fn spawn_codex_app_server(
    socket: &Path,
    db_path: PathBuf,
    global_state_path: PathBuf,
    sessions_dir: PathBuf,
    stop: Arc<AtomicBool>,
) -> JoinHandle<()> {
    let listener = UnixListener::bind(socket).unwrap();
    listener.set_nonblocking(true).unwrap();
    thread::spawn(move || {
        let mut proj_seq = 0u64;
        let mut thread_seq = 0u64;
        let mut turn_seq = 0u64;
        while !stop.load(Ordering::SeqCst) {
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
                        if req.get("id").is_none() {
                            continue;
                        }
                        let method = req["method"].as_str().unwrap_or("");
                        let params = &req["params"];
                        let result = match method {
                            "initialize" => json!({"userAgent": "codex-app-server-slice-l"}),
                            "project/create" => {
                                proj_seq += 1;
                                let project_id =
                                    format!("019a5400-0000-7000-8000-{proj_seq:012x}");
                                let name = params["name"].as_str().unwrap_or("unnamed");
                                let root_path = params["rootPath"].as_str().unwrap_or("");
                                let sql = format!(
                                    "INSERT INTO projects (id, name, metadata, position, created_at_ms, updated_at_ms) \
                                     VALUES ('{project_id}', '{}', '{{}}', {proj_seq}, 1762310000000, 1762310000000);\
                                     INSERT INTO project_roots (project_id, position, path) \
                                     VALUES ('{project_id}', 0, '{}');",
                                    sql_escape(name),
                                    sql_escape(root_path),
                                );
                                exec_sqlite(&db_path, &sql);
                                let mut gs: Value = serde_json::from_slice(
                                    &fs::read(&global_state_path).unwrap(),
                                )
                                .unwrap();
                                gs["local-projects"][&project_id] = json!({
                                    "id": project_id,
                                    "name": name,
                                    "roots": [root_path],
                                });
                                gs["project-order"]
                                    .as_array_mut()
                                    .unwrap()
                                    .push(json!(project_id));
                                fs::write(
                                    &global_state_path,
                                    serde_json::to_vec_pretty(&gs).unwrap(),
                                )
                                .unwrap();
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
                                let thread_id =
                                    format!("019a5401-0000-7000-8000-{thread_seq:012x}");
                                let project_id = params["projectId"].as_str().unwrap_or("");
                                let cwd = params["cwd"].as_str().unwrap_or("");
                                let title = params["title"].as_str().unwrap_or("");
                                let rollout_file = sessions_dir
                                    .join(format!("rollout-20261105-{thread_id}.jsonl"));
                                fs::write(&rollout_file, b"").unwrap();
                                let sql = format!(
                                    "INSERT INTO threads \
                                     (id, rollout_path, created_at, updated_at, source, model_provider, cwd, title, \
                                      sandbox_policy, approval_mode, has_user_event, archived, preview, recency_at_ms, project_id, originator) \
                                     VALUES ('{thread_id}', '{}', 1762310000, 1762310000, 'vscode', 'openai', '{}', '{}', \
                                             'workspace-write', 'never', 1, 0, '', 1762310000000, '{}', 'Codex Desktop');",
                                    sql_escape(&rollout_file.display().to_string()),
                                    sql_escape(cwd),
                                    sql_escape(title),
                                    sql_escape(project_id),
                                );
                                exec_sqlite(&db_path, &sql);
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
                                json!({
                                    "thread": {
                                        "id": params["threadId"],
                                        "projectId": params["projectId"],
                                        "cwd": params["cwd"],
                                        "status": {"type": "idle"}
                                    }
                                })
                            }
                            "turn/start" => {
                                turn_seq += 1;
                                let turn_id = format!("dots-turn-{turn_seq:04}");
                                let thread_id = params["threadId"].as_str().unwrap_or("");
                                let text = params
                                    .pointer("/input/0/text")
                                    .and_then(Value::as_str)
                                    .unwrap_or("");
                                let sql = format!(
                                    "UPDATE threads SET preview = '{}', updated_at = 1762310010, recency_at_ms = 1762310010000 \
                                     WHERE id = '{}';",
                                    sql_escape(text),
                                    sql_escape(thread_id),
                                );
                                exec_sqlite(&db_path, &sql);
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
    })
}

fn spawn_cindy_gateway(
    socket: &Path,
    transcripts_dir: PathBuf,
    stop: Arc<AtomicBool>,
) -> JoinHandle<()> {
    let listener = UnixListener::bind(socket).unwrap();
    listener.set_nonblocking(true).unwrap();
    thread::spawn(move || {
        let mut sessions: BTreeMap<String, Value> = BTreeMap::new();
        let mut proj_seq = 0u64;
        let mut sess_seq = 0u64;
        let mut turn_seq = 0u64;
        while !stop.load(Ordering::SeqCst) {
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
                                    transcripts_dir.join(format!("{session_key}.jsonl"));
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
                                let session_key = params["sessionKey"].as_str().unwrap_or("");
                                let sess = sessions
                                    .get_mut(session_key)
                                    .expect("cindy session must exist");
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
                                let transcript_path =
                                    PathBuf::from(sess["transcriptPath"].as_str().unwrap());
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
                                let session_key = params["sessionKey"].as_str().unwrap_or("");
                                let sess = sessions
                                    .get(session_key)
                                    .cloned()
                                    .expect("cindy session must exist on sessions.get");
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
    })
}

fn spawn_dsh_gateway(stop: Arc<AtomicBool>) -> (String, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}/v1");
    let handle = thread::spawn(move || {
        let mut sessions: BTreeMap<String, Value> = BTreeMap::new();
        let mut session_seq = 0u64;
        let mut turn_seq = 0u64;
        while !stop.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream.set_nonblocking(false).unwrap();
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                    let Ok((method, path, body)) = read_http_json_request(&mut stream) else {
                        continue;
                    };
                    let (code, resp_json) = if method == "POST" && path == "/v1/sessions" {
                        session_seq += 1;
                        let session_id = format!("dsh-sess-{session_seq:04}");
                        let payload = body.unwrap_or_else(|| json!({}));
                        let sess_val = json!({
                            "id": session_id,
                            "project_id": payload["project_id"].as_str().unwrap_or(""),
                            "title": payload["title"].as_str().unwrap_or(""),
                            "cwd": payload["cwd"].as_str().unwrap_or(""),
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
                            (200u16, json!({"session": sess}))
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
    (base_url, handle)
}

fn read_http_json_request(
    stream: &mut TcpStream,
) -> std::io::Result<(String, String, Option<Value>)> {
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

fn write_mock_agy_binary(script_path: &Path) {
    let script = r#"#!/usr/bin/env python3
import hashlib
import json
import os
import sqlite3
import sys
from pathlib import Path

db_path = Path(os.environ["ANTIGRAVITY_SUMMARIES_DB"])
args = sys.argv[1:]

new_project = "--new-project" in args
project_arg = None
conversation_arg = None
add_dirs = []
prompt_arg = None

i = 0
while i < len(args):
    a = args[i]
    if a == "--project" and i + 1 < len(args):
        project_arg = args[i + 1]
        i += 2
    elif a == "--conversation" and i + 1 < len(args):
        conversation_arg = args[i + 1]
        i += 2
    elif a == "--add-dir" and i + 1 < len(args):
        add_dirs.append(args[i + 1])
        i += 2
    elif a in ("-p", "--print", "--prompt") and i + 1 < len(args):
        prompt_arg = args[i + 1]
        i += 2
    elif a == "--model" and i + 1 < len(args):
        i += 2
    else:
        i += 1

if new_project and prompt_arg is None:
    digest = hashlib.sha256(f"{project_arg}:{add_dirs}".encode()).hexdigest()[:12]
    project_id = f"agy-proj-{digest}"
    print(json.dumps({
        "project_id": project_id,
        "title": project_arg or "",
        "workspace_uris": [f"file://{Path(d).resolve()}" for d in add_dirs],
    }))
    sys.exit(0)

conn = sqlite3.connect(str(db_path))
cur = conn.cursor()
effective_project_id = project_arg if project_arg else "default-cli-project"
workspace_uris = json.dumps([f"file://{Path(d).resolve()}" for d in add_dirs])

if conversation_arg is None:
    digest = hashlib.sha256(f"{effective_project_id}:{prompt_arg}:{len(add_dirs)}".encode()).hexdigest()[:16]
    conv_id = f"conv-{digest}"
    turn_id = f"turn-{digest}-1"
    cur.execute(
        """
        INSERT OR REPLACE INTO conversation_summaries
        (conversation_id, title, preview, step_count, last_modified_time, workspace_uris,
         status, source, project_id, last_user_input_time, last_user_input_step_index, app_data_dir)
        VALUES (?, ?, ?, ?, '2026-11-05T10:50:00Z', ?, 'CASCADE_RUN_STATUS_IDLE', '', ?, '2026-11-05T10:50:00Z', 0, 'antigravity')
        """,
        (conv_id, (prompt_arg or "")[:40], prompt_arg or "", 2, workspace_uris, effective_project_id),
    )
    conn.commit()
    conn.close()
    print(json.dumps({
        "conversation_id": conv_id,
        "turn_id": turn_id,
        "project_id": effective_project_id,
    }))
else:
    cur.execute(
        "SELECT step_count FROM conversation_summaries WHERE conversation_id = ?",
        (conversation_arg,),
    )
    row = cur.fetchone()
    if row is None:
        conn.close()
        sys.stderr.write(f"unknown conversation: {conversation_arg}\n")
        sys.exit(2)
    next_steps = int(row[0]) + 2
    cur.execute(
        """
        UPDATE conversation_summaries
        SET preview = ?, step_count = ?, last_modified_time = '2026-11-05T10:51:00Z',
            workspace_uris = ?, project_id = ?
        WHERE conversation_id = ?
        """,
        (prompt_arg or "", next_steps, workspace_uris, effective_project_id, conversation_arg),
    )
    conn.commit()
    conn.close()
    turn_id = f"turn-{conversation_arg}-{next_steps}"
    print(json.dumps({
        "conversation_id": conversation_arg,
        "turn_id": turn_id,
        "project_id": effective_project_id,
    }))
"#;
    fs::write(script_path, script).unwrap();
    let mut perms = fs::metadata(script_path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(script_path, perms).unwrap();
}

fn write_mock_zcode_binary(script_path: &Path) {
    let script = r#"#!/usr/bin/env python3
import hashlib
import json
import os
import sys

args = sys.argv[1:]
resume_id = None
for i, a in enumerate(args):
    if a == "--resume" and i + 1 < len(args):
        resume_id = args[i + 1]

cwd = os.getcwd()
digest = hashlib.sha256(f"{cwd}:{args}".encode()).hexdigest()[:12]

if resume_id:
    turn_id = "msg_zcode_followup_2" if cwd.endswith("self-dev-zcode") else f"msg_zcode_followup_{digest}"
    print(json.dumps({"session_id": resume_id, "turn_id": turn_id, "status": "completed"}))
else:
    if cwd.endswith("self-dev-zcode"):
        print(json.dumps({"session_id": "sess_zcode_relay_1", "turn_id": "msg_zcode_initial_1", "status": "completed"}))
    else:
        print(json.dumps({"session_id": f"sess_zcode_{digest}", "turn_id": f"msg_zcode_initial_{digest}", "status": "completed"}))
"#;
    fs::write(script_path, script).unwrap();
    let mut perms = fs::metadata(script_path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(script_path, perms).unwrap();
}

#[test]
fn poteto_relay_four_hosts_closes_all_hosts_with_sidebar_visibility_and_receipts() {
    let f = FourHostClusterFixture::new();

    let host_cases = [
        (
            "openai-dots",
            DesktopHostKind::OpenaiDots,
            "poteto-relay-openai-dots",
            "OpenAI Dots PStack Relay Self-Dev",
            vec![
                "--codex-root".to_string(),
                f.codex_root.display().to_string(),
                "--app-server-socket".to_string(),
                f.codex_socket.display().to_string(),
            ],
        ),
        (
            "cindy",
            DesktopHostKind::Cindy,
            "poteto-relay-cindy",
            "Cindy PStack Relay Self-Dev",
            vec![
                "--cindy-root".to_string(),
                f.cindy_root.display().to_string(),
                "--gateway-socket".to_string(),
                f.cindy_socket.display().to_string(),
            ],
        ),
        (
            "deepseek-harness",
            DesktopHostKind::DeepseekHarness,
            "poteto-relay-dsh",
            "DeepSeek Harness PStack Relay Self-Dev",
            vec!["--dsh-base-url".to_string(), f.dsh_base_url.clone()],
        ),
        (
            "antigravity-desktop",
            DesktopHostKind::AntigravityDesktop,
            "poteto-relay-antigravity",
            "Antigravity Desktop PStack Relay Self-Dev",
            vec![
                "--antigravity-root".to_string(),
                f.antigravity_root.display().to_string(),
                "--agy-bin".to_string(),
                f.agy_bin.display().to_string(),
            ],
        ),
    ];

    let mut summary_bundle = Vec::new();

    for (host_arg, expected_kind, slug, title, runtime_args) in host_cases {
        let receipt_file = f.receipts_dir.join(format!("{host_arg}-receipt.json"));
        let mut cli_args: Vec<String> = vec![
            "relay".into(),
            "self-dev".into(),
            "--host".into(),
            host_arg.into(),
            "--projects-root".into(),
            f.projects_root.display().to_string(),
            "--state-root".into(),
            f.state_root.display().to_string(),
            "--slug".into(),
            slug.into(),
            "--title".into(),
            title.into(),
            "--slice".into(),
            "L".into(),
            "--goal".into(),
            format!(
                "Execute cross-agent /poteto-mode (pstack-relay) self-development slice on {host_arg} and verify sidebar project binding."
            ),
            "--done-means".into(),
            "cargo check --workspace, cargo test --workspace, and scripts/check pass with verified four-host receipts.".into(),
            "--followup".into(),
            format!(
                "Run follow-up verification turn on the same {host_arg} sidebar session and confirm receipt closure."
            ),
            "--receipt-out".into(),
            receipt_file.display().to_string(),
        ];
        cli_args.extend(runtime_args);

        let arg_refs: Vec<&str> = cli_args.iter().map(String::as_str).collect();
        let output = f.run_cli(&arg_refs);
        assert!(
            output.status.success(),
            "fleet-cli relay self-dev failed for {host_arg}: stderr={}",
            String::from_utf8_lossy(&output.stderr)
        );

        let stdout_receipt: HostRelayClosureReceipt =
            serde_json::from_slice(&output.stdout).unwrap();
        let file_receipt: HostRelayClosureReceipt =
            serde_json::from_slice(&fs::read(&receipt_file).unwrap()).unwrap();
        assert_eq!(stdout_receipt, file_receipt);

        // 1. Directory creation proof
        let expected_dir = f.projects_root.join(slug);
        assert!(expected_dir.is_dir());
        assert!(stdout_receipt.project_dir.created);
        assert_eq!(stdout_receipt.project_dir.path, expected_dir);
        let expected_uri = format!("file://{}", expected_dir.display());
        assert_eq!(stdout_receipt.project_dir.workspace_uri, expected_uri);

        // 2. Sidebar project registration proof
        assert_eq!(stdout_receipt.host, expected_kind);
        assert_eq!(stdout_receipt.sidebar_project.host, expected_kind);
        assert_eq!(stdout_receipt.sidebar_project.project_dir, expected_dir);
        assert_eq!(
            stdout_receipt.sidebar_project.workspace_uris,
            vec![expected_uri.clone()]
        );
        assert!(!matches!(
            stdout_receipt.sidebar_project.project_id.as_str(),
            "" | "default-cli-project" | "outside-of-project" | "null" | "none"
        ));

        // 3. Initial /poteto-mode session & same-session follow-up turn proof
        assert!(stdout_receipt
            .rendered_initial_prompt
            .starts_with("/poteto-mode\n"));
        assert_eq!(stdout_receipt.initial_session.tier, DeliveryTier::ThreadVisible);
        assert_eq!(stdout_receipt.followup_turn.tier, DeliveryTier::ThreadVisible);
        assert_eq!(
            stdout_receipt.initial_session.project_id,
            stdout_receipt.sidebar_project.project_id
        );
        assert_eq!(
            stdout_receipt.initial_session.session_id,
            stdout_receipt.followup_turn.session_id,
            "follow-up turn must stay in the exact same sidebar session"
        );
        assert_ne!(
            stdout_receipt.initial_session.turn_id,
            stdout_receipt.followup_turn.turn_id,
            "follow-up turn must record a distinct turn_id"
        );

        // 4. Host-specific native store verification
        match (&stdout_receipt.store_evidence, expected_kind) {
            (
                HostStoreVerificationEvidence::OpenaiDots {
                    project_row,
                    thread_row,
                    deep_link,
                    turn_ids,
                },
                DesktopHostKind::OpenaiDots,
            ) => {
                assert_eq!(
                    project_row.project_id,
                    stdout_receipt.sidebar_project.project_id
                );
                assert_eq!(project_row.root_path, expected_dir);
                assert_eq!(
                    thread_row.thread_id,
                    stdout_receipt.initial_session.session_id
                );
                assert_eq!(
                    thread_row.project_id,
                    stdout_receipt.sidebar_project.project_id
                );
                assert_eq!(thread_row.cwd, expected_dir);
                assert_eq!(thread_row.archived, 0);
                assert!(!thread_row.preview.is_empty());
                assert_eq!(
                    *deep_link,
                    format!("codex://threads/{}", thread_row.thread_id)
                );
                assert_eq!(turn_ids.len(), 2);

                // Direct SQLite & global state cross-check
                let direct_proj = read_codex_project(
                    Path::new("/usr/bin/sqlite3"),
                    &f.codex_db_path,
                    &project_row.project_id,
                )
                .unwrap()
                .unwrap();
                assert_eq!(&direct_proj, project_row);
                let direct_thread = read_codex_thread(
                    Path::new("/usr/bin/sqlite3"),
                    &f.codex_db_path,
                    &thread_row.thread_id,
                )
                .unwrap()
                .unwrap();
                assert_eq!(&direct_thread, thread_row);
                let gs: Value =
                    serde_json::from_slice(&fs::read(&f.codex_global_state_path).unwrap()).unwrap();
                assert_eq!(
                    gs["local-projects"][&project_row.project_id]["roots"][0],
                    json!(expected_dir)
                );
            }
            (
                HostStoreVerificationEvidence::Cindy {
                    session_snapshot,
                    transcript_entry_count,
                    turn_ids,
                },
                DesktopHostKind::Cindy,
            ) => {
                assert_eq!(
                    session_snapshot.session_key,
                    stdout_receipt.initial_session.session_id
                );
                assert_eq!(
                    session_snapshot.project_id,
                    stdout_receipt.sidebar_project.project_id
                );
                assert_eq!(session_snapshot.cwd, expected_dir);
                assert_eq!(session_snapshot.turn_count, 2);
                assert_eq!(*transcript_entry_count, 2);
                assert_eq!(turn_ids.len(), 2);
                let lines = read_cindy_transcript(&session_snapshot.transcript_path).unwrap();
                assert_eq!(lines.len(), 2);
                assert!(lines[0]["text"]
                    .as_str()
                    .unwrap()
                    .starts_with("/poteto-mode\n"));
            }
            (
                HostStoreVerificationEvidence::DeepseekHarness {
                    session_snapshot,
                    turn_ids,
                },
                DesktopHostKind::DeepseekHarness,
            ) => {
                assert_eq!(
                    session_snapshot.session_id,
                    stdout_receipt.initial_session.session_id
                );
                assert_eq!(
                    session_snapshot.project_id,
                    stdout_receipt.sidebar_project.project_id
                );
                assert_eq!(session_snapshot.cwd, expected_dir);
                assert_eq!(session_snapshot.turn_count, 2);
                assert_eq!(session_snapshot.status, "active");
                assert_eq!(turn_ids.len(), 2);
            }
            (
                HostStoreVerificationEvidence::AntigravityDesktop {
                    summary,
                    hub_http_port,
                    turn_ids,
                },
                DesktopHostKind::AntigravityDesktop,
            ) => {
                assert_eq!(
                    summary.conversation_id,
                    stdout_receipt.initial_session.session_id
                );
                assert_eq!(
                    summary.project_id,
                    stdout_receipt.sidebar_project.project_id
                );
                assert_eq!(summary.workspace_uris, vec![expected_uri.clone()]);
                assert_eq!(summary.step_count, 4);
                assert_eq!(*hub_http_port, Some(52019));
                assert_eq!(turn_ids.len(), 2);

                let direct_summary = read_conversation_summary(
                    Path::new("/usr/bin/sqlite3"),
                    &f.antigravity_db_path,
                    &summary.conversation_id,
                )
                .unwrap()
                .unwrap();
                assert_eq!(&direct_summary, summary);
                let raw_json = fs::read_to_string(&receipt_file).unwrap();
                assert!(
                    !raw_json.contains("secret-csrf-redacted-token-slice-l"),
                    "CSRF token must never appear in Antigravity receipt file"
                );
            }
            other => panic!("unexpected store_evidence variant: {other:?}"),
        }

        println!(
            "SLICE_L_HOST_VERIFIED host={} project_id={} session_id={} turns=[{}, {}] dir={}",
            stdout_receipt.host,
            stdout_receipt.sidebar_project.project_id,
            stdout_receipt.initial_session.session_id,
            stdout_receipt.initial_session.turn_id,
            stdout_receipt.followup_turn.turn_id,
            stdout_receipt.project_dir.path.display()
        );
        summary_bundle.push(stdout_receipt);
    }

    assert_eq!(summary_bundle.len(), 4);

    if let Ok(bundle_dir) = std::env::var("FLEET_SLICE_L_BUNDLE_DIR") {
        let out_dir = PathBuf::from(bundle_dir);
        fs::create_dir_all(&out_dir).unwrap();
        for item in &summary_bundle {
            let p = out_dir.join(format!("{}-receipt.json", item.host.as_str()));
            fs::write(p, serde_json::to_vec_pretty(item).unwrap()).unwrap();
        }
        fs::write(
            out_dir.join("four-hosts-closure-bundle.json"),
            serde_json::to_vec_pretty(&summary_bundle).unwrap(),
        )
        .unwrap();
    }
}

#[test]
fn poteto_relay_four_hosts_subcommands_ensure_project_create_session_and_deliver_turn() {
    let f = FourHostClusterFixture::new();

    // 1. ensure-project via `fleet-cli hosts ensure-project`
    let ensure_out = f.run_cli(&[
        "hosts",
        "ensure-project",
        "--host",
        "openai-dots",
        "--projects-root",
        &f.projects_root.display().to_string(),
        "--state-root",
        &f.state_root.display().to_string(),
        "--slug",
        "direct-cli-dots",
        "--title",
        "Direct CLI Dots Project",
        "--codex-root",
        &f.codex_root.display().to_string(),
        "--app-server-socket",
        &f.codex_socket.display().to_string(),
    ]);
    assert!(ensure_out.status.success());
    let binding: SidebarProjectBinding = serde_json::from_slice(&ensure_out.stdout).unwrap();
    assert_eq!(binding.host, DesktopHostKind::OpenaiDots);
    assert!(binding.project_dir.is_dir());

    // 2. create-session via `fleet-cli hosts create-session`
    let create_out = f.run_cli(&[
        "hosts",
        "create-session",
        "--host",
        "openai-dots",
        "--projects-root",
        &f.projects_root.display().to_string(),
        "--state-root",
        &f.state_root.display().to_string(),
        "--delivery-id",
        "cli-direct-deliv-1",
        "--project-id",
        &binding.project_id,
        "--project-dir",
        &binding.project_dir.display().to_string(),
        "--title",
        "Direct CLI Session",
        "--prompt",
        "Run initial check via fleet-cli hosts create-session.",
        "--codex-root",
        &f.codex_root.display().to_string(),
        "--app-server-socket",
        &f.codex_socket.display().to_string(),
    ]);
    assert!(create_out.status.success());
    let created: HostSessionReceipt = serde_json::from_slice(&create_out.stdout).unwrap();
    assert_eq!(created.project_id, binding.project_id);
    assert_eq!(created.tier, DeliveryTier::ThreadVisible);

    // 3. deliver-turn via `fleet-cli hosts deliver-turn`
    let turn_out = f.run_cli(&[
        "hosts",
        "deliver-turn",
        "--host",
        "openai-dots",
        "--projects-root",
        &f.projects_root.display().to_string(),
        "--state-root",
        &f.state_root.display().to_string(),
        "--delivery-id",
        "cli-direct-deliv-2",
        "--project-id",
        &binding.project_id,
        "--session-id",
        &created.session_id,
        "--project-dir",
        &binding.project_dir.display().to_string(),
        "--prompt",
        "Continue same thread via fleet-cli hosts deliver-turn.",
        "--codex-root",
        &f.codex_root.display().to_string(),
        "--app-server-socket",
        &f.codex_socket.display().to_string(),
    ]);
    assert!(turn_out.status.success());
    let continued: HostSessionReceipt = serde_json::from_slice(&turn_out.stdout).unwrap();
    assert_eq!(continued.session_id, created.session_id);
    assert_ne!(continued.turn_id, created.turn_id);
}

#[test]
fn poteto_relay_four_hosts_rejects_model_wrong_self_attestation_and_path_traversal() {
    let f = FourHostClusterFixture::new();

    // 1. Reject MODEL_WRONG gate in goal
    let bad_brief = f.run_cli(&[
        "relay",
        "self-dev",
        "--host",
        "cindy",
        "--projects-root",
        &f.projects_root.display().to_string(),
        "--state-root",
        &f.state_root.display().to_string(),
        "--slug",
        "should-not-create-1",
        "--goal",
        "If not the requested model, output MODEL_WRONG and stop.",
        "--done-means",
        "scripts/check passes",
    ]);
    assert_eq!(bad_brief.status.code(), Some(2));
    assert!(bad_brief.stdout.is_empty());
    assert!(String::from_utf8_lossy(&bad_brief.stderr).contains("MODEL_WRONG"));
    assert!(!f.projects_root.join("should-not-create-1").exists());

    // 2. Reject self-attestation in followup prompt
    let bad_followup = f.run_cli(&[
        "relay",
        "self-dev",
        "--host",
        "deepseek-harness",
        "--projects-root",
        &f.projects_root.display().to_string(),
        "--state-root",
        &f.state_root.display().to_string(),
        "--slug",
        "should-not-create-2",
        "--goal",
        "Implement feature cleanly.",
        "--done-means",
        "scripts/check passes",
        "--followup",
        "First confirm your model before running checks.",
    ]);
    assert_eq!(bad_followup.status.code(), Some(2));
    assert!(bad_followup.stdout.is_empty());
    assert!(String::from_utf8_lossy(&bad_followup.stderr).contains("self-attestation"));
    assert!(!f.projects_root.join("should-not-create-2").exists());

    // 3. Reject path traversal slug
    let bad_slug = f.run_cli(&[
        "relay",
        "self-dev",
        "--host",
        "antigravity-desktop",
        "--projects-root",
        &f.projects_root.display().to_string(),
        "--state-root",
        &f.state_root.display().to_string(),
        "--slug",
        "../escape-root",
        "--goal",
        "Implement feature cleanly.",
        "--done-means",
        "scripts/check passes",
    ]);
    assert_eq!(bad_slug.status.code(), Some(2));
    assert!(bad_slug.stdout.is_empty());
    assert!(String::from_utf8_lossy(&bad_slug.stderr).contains("traversal"));
}

#[test]
fn five_hosts_and_open_engine_cli_mcp_interoperability() {
    use std::process::Stdio;

    let f = FourHostClusterFixture::new();
    let zcode_root = f.root.join(".zcode");
    let zcode_bin = f.root.join("bin/zcode");

    // 1. Verify 5th host (`zcode`) full /poteto-mode self-dev relay with SQLite verification
    let zcode_receipt_file = f.receipts_dir.join("zcode-receipt.json");
    let relay_out = f.run_cli(&[
        "relay",
        "self-dev",
        "--host",
        "zcode",
        "--projects-root",
        &f.projects_root.display().to_string(),
        "--state-root",
        &f.state_root.display().to_string(),
        "--slug",
        "self-dev-zcode",
        "--title",
        "ZCode Self Dev Workspace",
        "--slice",
        "Q",
        "--goal",
        "Verify ZCode sidebar project and session creation via fleet-cli relay self-dev.",
        "--done-means",
        "ZCode tasks-index.sqlite and cli/db/db.sqlite both contain the bound project and session.",
        "--receipt-out",
        &zcode_receipt_file.display().to_string(),
        "--zcode-root",
        &zcode_root.display().to_string(),
        "--zcode-bin",
        &zcode_bin.display().to_string(),
    ]);
    assert!(
        relay_out.status.success(),
        "zcode relay failed: {}",
        String::from_utf8_lossy(&relay_out.stderr)
    );
    let zcode_receipt: HostRelayClosureReceipt =
        serde_json::from_slice(&relay_out.stdout).unwrap();
    assert_eq!(zcode_receipt.host, DesktopHostKind::Zcode);
    assert_eq!(
        zcode_receipt.initial_session.tier,
        DeliveryTier::ThreadVisible
    );
    assert_eq!(
        zcode_receipt.initial_session.session_id,
        zcode_receipt.followup_turn.session_id
    );
    match &zcode_receipt.store_evidence {
        HostStoreVerificationEvidence::Zcode {
            project_row,
            session_snapshot,
            turn_ids,
        } => {
            assert_eq!(
                project_row.project_id,
                zcode_receipt.sidebar_project.project_id
            );
            assert_eq!(
                project_row.root_path,
                f.projects_root.join("self-dev-zcode")
            );
            assert_eq!(session_snapshot.session_id, "sess_zcode_relay_1");
            assert_eq!(session_snapshot.message_count, 2);
            assert_eq!(turn_ids.len(), 2);
        }
        other => panic!("expected Zcode store evidence, got {other:?}"),
    }

    // 2. Verify `fleet-cli send` across all 5 canonical desktop hosts (auto-detecting local runtimes from HOME)
    for host in DesktopHostKind::ALL {
        let target = format!("{}/mutual-mesh", host.as_str());
        let send_out = f.run_cli(&[
            "send",
            "--to",
            &target,
            "--projects-root",
            &f.projects_root.display().to_string(),
            "--state-root",
            &f.state_root.display().to_string(),
            "--title",
            "Five-Host Mutual Mesh",
            "--prompt",
            "Dispatch task across five desktop hosts and verify sidebar session.",
        ]);
        assert!(
            send_out.status.success(),
            "send failed for {}: {}",
            host,
            String::from_utf8_lossy(&send_out.stderr)
        );
        let receipt: HostSessionReceipt = serde_json::from_slice(&send_out.stdout).unwrap();
        assert_eq!(receipt.host, host);
        assert_eq!(receipt.project_dir, f.projects_root.join("mutual-mesh"));
        assert_eq!(receipt.tier, DeliveryTier::ThreadVisible);
        assert_ne!(receipt.session_id, format!("{}-session-1", host.as_str()));

        // Follow up on the same session
        let followup_out = f.run_cli(&[
            "send",
            "--to",
            &target,
            "--projects-root",
            &f.projects_root.display().to_string(),
            "--state-root",
            &f.state_root.display().to_string(),
            "--session-id",
            &receipt.session_id,
            "--prompt",
            "Continue the same sidebar session on this host.",
        ]);
        assert!(
            followup_out.status.success(),
            "followup failed for {}: {}",
            host,
            String::from_utf8_lossy(&followup_out.stderr)
        );
        let cont: HostSessionReceipt = serde_json::from_slice(&followup_out.stdout).unwrap();
        assert_eq!(cont.session_id, receipt.session_id);
        assert_ne!(cont.turn_id, receipt.turn_id);
        assert_eq!(cont.tier, DeliveryTier::ThreadVisible);
    }

    // 3. Register an external open-source agent via `fleet-cli agents register` and dispatch via `fleet-cli send`
    let reg_out = f.run_cli(&[
        "agents",
        "register",
        "--state-root",
        &f.state_root.display().to_string(),
        "--id",
        "open-coder",
        "--name",
        "OpenCoder CLI",
        "--command",
        "/bin/echo",
        "--args",
        "dispatched-to,{cwd},{prompt}",
        "--description",
        "Community open-source agent plugin",
    ]);
    assert!(reg_out.status.success());

    let ext_send = f.run_cli(&[
        "send",
        "--to",
        "open-coder/oss-plugin-proj",
        "--projects-root",
        &f.projects_root.display().to_string(),
        "--state-root",
        &f.state_root.display().to_string(),
        "--prompt",
        "Hello external open-source agent.",
        "--execute-external",
        "true",
    ]);
    assert!(ext_send.status.success());
    let ext_val: Value = serde_json::from_slice(&ext_send.stdout).unwrap();
    assert_eq!(ext_val["agent_id"], "open-coder");
    assert_eq!(ext_val["tier"], "completed");

    // Register herdr external agent and test default execute_external dispatch
    let herdr_reg = f.run_cli(&[
        "agents",
        "register",
        "--state-root",
        &f.state_root.display().to_string(),
        "--id",
        "herdr",
        "--name",
        "Herdr Terminal Multiplexer",
        "--command",
        "/bin/echo",
        "--args",
        "dispatched-to-herdr,{slug},{prompt}",
        "--description",
        "Herdr terminal pane dispatcher",
    ]);
    assert!(herdr_reg.status.success());

    let herdr_send = f.run_cli(&[
        "send",
        "--to",
        "herdr/agent-fleet",
        "--projects-root",
        &f.projects_root.display().to_string(),
        "--state-root",
        &f.state_root.display().to_string(),
        "--prompt",
        "echo 1",
    ]);
    assert!(herdr_send.status.success());
    let herdr_val: Value = serde_json::from_slice(&herdr_send.stdout).unwrap();
    assert_eq!(herdr_val["agent_id"], "herdr");
    assert_eq!(herdr_val["tier"], "completed");
    assert!(herdr_val["stdout_preview"]
        .as_str()
        .unwrap()
        .contains("dispatched-to-herdr"));

    // 4. Verify `fleet-cli who`, `fleet-cli setup`, and `fleet-cli doctor`
    let who_out = f.run_cli(&[
        "who",
        "--state-root",
        &f.state_root.display().to_string(),
    ]);
    assert!(who_out.status.success());
    let who_val: Value = serde_json::from_slice(&who_out.stdout).unwrap();
    assert_eq!(
        who_val["canonical_desktop_hosts"].as_array().unwrap().len(),
        5
    );
    assert_eq!(who_val["external_agents"].as_array().unwrap().len(), 2);

    let doctor_out = f.run_cli(&[
        "doctor",
        "--projects-root",
        &f.projects_root.display().to_string(),
        "--state-root",
        &f.state_root.display().to_string(),
    ]);
    assert!(doctor_out.status.success());
    let doctor_val: Value = serde_json::from_slice(&doctor_out.stdout).unwrap();
    assert_eq!(doctor_val["status"], "ok");
    assert_eq!(doctor_val["canonical_hosts"].as_array().unwrap().len(), 5);

    let doctor_live_out = f.run_cli(&[
        "doctor",
        "--live",
        "--projects-root",
        &f.projects_root.display().to_string(),
        "--state-root",
        &f.state_root.display().to_string(),
    ]);
    assert!(doctor_live_out.status.success());
    let doc_live_val: Value = serde_json::from_slice(&doctor_live_out.stdout).unwrap();
    assert_eq!(doc_live_val["status"], "ok");
    assert_eq!(doc_live_val["live_mode"], true);
    assert_eq!(doc_live_val["canonical_hosts"].as_array().unwrap().len(), 5);

    // 5. Verify `fleet-cli mcp` JSON-RPC 2.0 stdio server (`initialize`, `tools/list`, `tools/call`)
    let mut mcp_child = Command::new(env!("CARGO_BIN_EXE_fleet-cli"))
        .args([
            "mcp",
            "--projects-root",
            &f.projects_root.display().to_string(),
            "--state-root",
            &f.state_root.display().to_string(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_clear()
        .env("HOME", &f.root)
        .env("DSH_BASE_URL", &f.dsh_base_url)
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .spawn()
        .unwrap();

    let requests = [
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}),
        json!({"jsonrpc": "2.0", "method": "notifications/initialized", "params": {}}),
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}}),
        json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "send_message",
                "arguments": {
                    "to": "zcode/mcp-dispatched-proj",
                    "title": "MCP Dispatched ZCode Project",
                    "prompt": "Created via MCP send_message tool."
                }
            }
        }),
        json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "tools/call",
            "params": {
                "name": "list_team",
                "arguments": {}
            }
        }),
    ];
    {
        let stdin = mcp_child.stdin.as_mut().unwrap();
        for req in &requests {
            writeln!(stdin, "{req}").unwrap();
        }
    }
    let mcp_output = mcp_child.wait_with_output().unwrap();
    assert!(mcp_output.status.success());
    let lines: Vec<Value> = String::from_utf8_lossy(&mcp_output.stdout)
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines.len(), 4);
    assert_eq!(lines[0]["id"], 1);
    assert_eq!(lines[0]["result"]["serverInfo"]["name"], "agent-fleet-mcp");
    assert_eq!(lines[1]["id"], 2);
    assert_eq!(lines[1]["result"]["tools"].as_array().unwrap().len(), 5);
    assert_eq!(lines[2]["id"], 3);
    assert_eq!(lines[2]["result"]["isError"], false);
    assert_eq!(
        lines[2]["result"]["structuredContent"]["host"],
        "zcode"
    );
    assert_eq!(
        lines[2]["result"]["structuredContent"]["tier"],
        "thread-visible"
    );
    assert_ne!(
        lines[2]["result"]["structuredContent"]["session_id"],
        "zcode-session-1"
    );
    assert_eq!(lines[3]["id"], 4);
    assert_eq!(lines[3]["result"]["isError"], false);
}

#[test]
fn grokbot_bidirectional_interoperability_with_all_five_desktop_hosts_and_agent_wiring() {
    use std::process::Stdio;

    let f = FourHostClusterFixture::new();
    let config_home = f.root.join("config-home");
    fs::create_dir_all(&config_home).unwrap();

    // 1. Run `fleet-cli setup --wire-agents true --config-home <dir>` and verify idempotent wiring
    let setup_out = f.run_cli(&[
        "setup",
        "--projects-root",
        &f.projects_root.display().to_string(),
        "--state-root",
        &f.state_root.display().to_string(),
        "--wire-agents",
        "true",
        "--config-home",
        &config_home.display().to_string(),
    ]);
    assert!(
        setup_out.status.success(),
        "setup failed: {}",
        String::from_utf8_lossy(&setup_out.stderr)
    );
    let setup_val: Value = serde_json::from_slice(&setup_out.stdout).unwrap();
    assert_eq!(setup_val["status"], "ok");
    assert_eq!(setup_val["grokbot"]["ready"], true);
    assert_eq!(setup_val["grokbot"]["bundle_id"], "com.anysphere.sand");
    assert_eq!(
        setup_val["canonical_hosts"].as_array().unwrap().len(),
        5
    );
    assert_eq!(
        setup_val["wired_agents"].as_array().unwrap().len(),
        6
    );
    assert!(config_home.join(".grok/config.toml").exists());
    assert!(config_home.join(".grok/hooks/agent-fleet.json").exists());
    assert!(config_home.join(".gemini/config/mcp_config.json").exists());
    assert!(config_home.join(".codex/config.toml").exists());
    assert!(config_home.join(".dsh/cordis.patch.yml").exists());
    assert!(config_home.join(".zcode/cli/config.json").exists());
    assert!(config_home.join(".cindy/mcp.json").exists());

    let skill_out = f.run_cli(&["skill"]);
    assert!(skill_out.status.success());
    let skill_text = String::from_utf8_lossy(&skill_out.stdout);
    assert!(skill_text.contains("com.anysphere.sand"));
    assert!(skill_text.contains("com.google.antigravity"));
    assert!(skill_text.contains("dev.zcode.app"));

    let help_out = f.run_cli(&["--help"]);
    assert!(help_out.status.success());

    // 2. GrokBot dispatches tasks to all 5 desktop hosts via `fleet-cli send --from grokbot`
    let hosts = [
        ("antigravity-desktop", "grok-to-agy"),
        ("openai-dots", "grok-to-dots"),
        ("deepseek-harness", "grok-to-dsh"),
        ("cindy", "grok-to-cindy"),
        ("zcode", "grok-to-zcode"),
    ];

    let mut outbound_ids = Vec::new();
    for (host, slug) in hosts {
        let out = f.run_cli(&[
            "send",
            "--to",
            &format!("{host}/{slug}"),
            "--from",
            "grokbot",
            "--title",
            &format!("GrokBot Task for {host}"),
            "--prompt",
            &format!("Please inspect {slug} and reply to grokbot."),
            "--projects-root",
            &f.projects_root.display().to_string(),
            "--state-root",
            &f.state_root.display().to_string(),
        ]);
        assert!(
            out.status.success(),
            "grokbot -> {host} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let val: Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(val["from"], "grokbot");
        assert_eq!(val["peer_framed"], true);
        assert_eq!(val["host"], host);
        assert_eq!(val["tier"], "thread-visible");
        assert_ne!(val["session_id"], format!("{host}-session-1"));
        assert_eq!(
            val["project_dir"],
            f.projects_root.join(slug).display().to_string()
        );
        assert!(val["session_id"].as_str().is_some());
        let msg_id = val["delivery_id"].as_str().unwrap().to_string();
        outbound_ids.push((host, slug, msg_id));
    }

    // Verify each desktop host's unread mailbox entry via `fleet-cli hook <host> stop` contains `<fleet-message from="grokbot" ...>`
    for (host, _slug, _msg_id) in &outbound_ids {
        let inbox_out = f.run_cli(&[
            "inbox",
            "--for",
            host,
            "--state-root",
            &f.state_root.display().to_string(),
            "--ack",
            "false",
        ]);
        assert!(inbox_out.status.success());
        let inbox_val: Value = serde_json::from_slice(&inbox_out.stdout).unwrap();
        assert_eq!(inbox_val["count"], 1);
        assert_eq!(inbox_val["messages"][0]["from"], "grokbot");

        let hook_out = f.run_cli(&[
            "hook",
            host,
            "stop",
            "--state-root",
            &f.state_root.display().to_string(),
        ]);
        assert!(hook_out.status.success());
        let hook_stdout = String::from_utf8_lossy(&hook_out.stdout);
        assert!(hook_stdout.contains("<fleet-message from=\"grokbot\""));
        assert!(hook_stdout.contains("</fleet-message>"));
    }

    // 3. All 5 desktop hosts reply back to `grokbot` using `--to grokbot --from <host>/<slug> --reply-to <id>`
    for (host, slug, msg_id) in &outbound_ids {
        let reply_out = f.run_cli(&[
            "send",
            "--to",
            "grokbot",
            "--from",
            &format!("{host}/{slug}"),
            "--reply-to",
            msg_id,
            "--title",
            &format!("Reply from {host}"),
            "--prompt",
            &format!("Completed {slug} on {host}."),
            "--projects-root",
            &f.projects_root.display().to_string(),
            "--state-root",
            &f.state_root.display().to_string(),
        ]);
        assert!(
            reply_out.status.success(),
            "{host} -> grokbot reply failed: {}",
            String::from_utf8_lossy(&reply_out.stderr)
        );
        let reply_val: Value = serde_json::from_slice(&reply_out.stdout).unwrap();
        assert_eq!(reply_val["host"], "grokbot");
        assert_eq!(reply_val["to"], "grokbot/agent-fleet");
        assert_eq!(reply_val["reply_to"], *msg_id);
        assert_eq!(reply_val["tier"], "accepted");
    }

    // 4. Verify `fleet-cli who` reports 5 unread messages in `grokbot` inbox
    let who_out = f.run_cli(&[
        "who",
        "--state-root",
        &f.state_root.display().to_string(),
    ]);
    assert!(who_out.status.success());
    let who_val: Value = serde_json::from_slice(&who_out.stdout).unwrap();
    assert_eq!(who_val["grokbot"]["unread_inbox_count"], 5);
    assert_eq!(who_val["canonical_peers"].as_array().unwrap().len(), 6);

    // 5. Verify GrokBot can pull its 5 replies via MCP `check_inbox` tool (and idempotently gets 0 on second call)
    let mut mcp_child = Command::new(env!("CARGO_BIN_EXE_fleet-cli"))
        .args([
            "mcp",
            "--projects-root",
            &f.projects_root.display().to_string(),
            "--state-root",
            &f.state_root.display().to_string(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_clear()
        .spawn()
        .unwrap();

    let requests = [
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": "check_inbox",
                "arguments": {
                    "for_agent": "grokbot",
                    "ack": true
                }
            }
        }),
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "check_inbox",
                "arguments": {
                    "for_agent": "grokbot",
                    "ack": true
                }
            }
        }),
    ];
    {
        let stdin = mcp_child.stdin.as_mut().unwrap();
        for req in &requests {
            writeln!(stdin, "{req}").unwrap();
        }
    }
    let mcp_out = mcp_child.wait_with_output().unwrap();
    assert!(mcp_out.status.success());
    let lines: Vec<Value> = String::from_utf8_lossy(&mcp_out.stdout)
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(
        lines[0]["result"]["structuredContent"]["count"],
        5
    );
    assert_eq!(
        lines[0]["result"]["structuredContent"]["framed_messages"]
            .as_array()
            .unwrap()
            .len(),
        5
    );
    assert_eq!(
        lines[1]["result"]["structuredContent"]["count"],
        0
    );
}

#[test]
fn auto_detects_local_runtimes_and_rejects_silent_accepted_fallback_in_send_and_mcp() {
    use std::process::Stdio;

    let f = FourHostClusterFixture::new();
    let empty_home = f.root.join("empty-home");
    let isolated_state = f.root.join("isolated-state");
    fs::create_dir_all(&empty_home).unwrap();
    fs::create_dir_all(&isolated_state).unwrap();

    // 1. When HOME has no host runtimes and no explicit flags are passed, `fleet-cli send` MUST fail
    // for all 5 desktop hosts and NEVER write `<state_root>/<host>-state.json` or fake `<host>-session-1`.
    for host in DesktopHostKind::ALL {
        let out = Command::new(env!("CARGO_BIN_EXE_fleet-cli"))
            .env_clear()
            .env("HOME", &empty_home)
            .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
            .args([
                "send",
                "--to",
                &format!("{}/no-runtime-slug", host.as_str()),
                "--projects-root",
                &f.projects_root.display().to_string(),
                "--state-root",
                &isolated_state.display().to_string(),
                "--prompt",
                "Must fail without silent accepted fallback.",
            ])
            .output()
            .unwrap();
        assert!(
            !out.status.success(),
            "expected send to {} without runtime to fail, but succeeded: {}",
            host,
            String::from_utf8_lossy(&out.stdout)
        );
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains("refusing silent accepted fallback"),
            "unexpected stderr for {host}: {stderr}"
        );
        assert!(
            !isolated_state
                .join(format!("{}-state.json", host.as_str()))
                .exists(),
            "{}-state.json must not be created when runtime is missing",
            host.as_str()
        );
    }

    // 2. Similarly, `fleet-cli mcp` `send_message` with an empty HOME must return `isError: true`
    // and never fabricate `zcode-session-1` or `tier: accepted`.
    let mut mcp_child = Command::new(env!("CARGO_BIN_EXE_fleet-cli"))
        .args([
            "mcp",
            "--projects-root",
            &f.projects_root.display().to_string(),
            "--state-root",
            &isolated_state.display().to_string(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_clear()
        .env("HOME", &empty_home)
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .spawn()
        .unwrap();
    {
        let stdin = mcp_child.stdin.as_mut().unwrap();
        let call = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": "send_message",
                "arguments": {
                    "to": "zcode/agent-fleet",
                    "prompt": "Must fail when zcode runtime is absent."
                }
            }
        });
        writeln!(stdin, "{call}").unwrap();
    }
    let mcp_out = mcp_child.wait_with_output().unwrap();
    assert!(mcp_out.status.success());
    let resp: Value =
        serde_json::from_str(String::from_utf8_lossy(&mcp_out.stdout).trim()).unwrap();
    assert_eq!(resp["result"]["isError"], true);
    assert!(resp["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or("")
        .contains("refusing silent accepted fallback"));
    assert!(!isolated_state.join("zcode-state.json").exists());
}

#[test]
fn slice_x_session_scoped_peer_addressing_and_bidirectional_no_orphan_delivery() {
    use fleet_daemon::{inspect_host_state, FleetMailboxStore};

    let f = FourHostClusterFixture::new();

    let agy_init_out = f.run_cli(&[
        "send",
        "--to",
        "antigravity-desktop/agent-fleet",
        "--title",
        "Antigravity Slice X Origin Session",
        "--prompt",
        "Coordinate Slice X across DSH and ZCode from this Antigravity session.",
        "--delivery-id",
        "agy-origin-seed-1",
        "--projects-root",
        &f.projects_root.display().to_string(),
        "--state-root",
        &f.state_root.display().to_string(),
        "--antigravity-root",
        &f.antigravity_root.display().to_string(),
        "--agy-bin",
        &f.agy_bin.display().to_string(),
    ]);
    assert!(
        agy_init_out.status.success(),
        "agy seed failed: {}",
        String::from_utf8_lossy(&agy_init_out.stderr)
    );
    let agy_seed_receipt: HostSessionReceipt =
        serde_json::from_slice(&agy_init_out.stdout).unwrap();
    let agy_conv_id = agy_seed_receipt.session_id.clone();

    let (_, agy_sessions_t1) =
        inspect_host_state(DesktopHostKind::AntigravityDesktop, &f.state_root).unwrap();
    assert_eq!(agy_sessions_t1.len(), 1);
    assert_eq!(agy_sessions_t1[0].session_id, agy_conv_id);
    assert_eq!(agy_sessions_t1[0].turns.len(), 1);

    let sender_with_session = format!("antigravity-desktop/agent-fleet#{agy_conv_id}");
    let dsh_dispatch_out = f.run_cli(&[
        "send",
        "--from",
        &sender_with_session,
        "--to",
        "deepseek-harness/agent-fleet",
        "--delivery-id",
        "msg-agy-to-dsh-1",
        "--title",
        "DSH Slice X Worker",
        "--prompt",
        "Run DSH verification and reply back to Antigravity session.",
        "--projects-root",
        &f.projects_root.display().to_string(),
        "--state-root",
        &f.state_root.display().to_string(),
        "--dsh-base-url",
        &f.dsh_base_url,
    ]);
    assert!(
        dsh_dispatch_out.status.success(),
        "agy -> dsh dispatch failed: {}",
        String::from_utf8_lossy(&dsh_dispatch_out.stderr)
    );
    let dsh_dispatch_val: Value = serde_json::from_slice(&dsh_dispatch_out.stdout).unwrap();
    let dsh_session_id = dsh_dispatch_val["session_id"].as_str().unwrap().to_string();
    assert_eq!(dsh_dispatch_val["from"], sender_with_session);
    assert_eq!(dsh_dispatch_val["to"], "deepseek-harness/agent-fleet");

    let mailbox = FleetMailboxStore::new(f.state_root.clone()).unwrap();
    let persisted_msg = mailbox.get_message("msg-agy-to-dsh-1").unwrap().unwrap();
    assert_eq!(persisted_msg.from, sender_with_session);
    assert_eq!(persisted_msg.to, "deepseek-harness/agent-fleet");
    assert_eq!(
        persisted_msg.session_id.as_deref(),
        Some(dsh_session_id.as_str())
    );
    let rendered_frame = persisted_msg.format_peer_frame();
    assert!(rendered_frame.contains(&format!(
        "<fleet-message from=\"{sender_with_session}\" to=\"deepseek-harness/agent-fleet\" id=\"msg-agy-to-dsh-1\" repo=\"agent-fleet\">"
    )));
    assert!(rendered_frame.contains(&format!(
        "Reply with the agent-fleet send_message tool to \"{sender_with_session}\" with reply_to=\"msg-agy-to-dsh-1\"."
    )));

    let dsh_reply_out = f.run_cli(&[
        "send",
        "--from",
        "deepseek-harness/agent-fleet",
        "--to",
        "antigravity-desktop/agent-fleet",
        "--reply-to",
        "msg-agy-to-dsh-1",
        "--delivery-id",
        "msg-dsh-reply-1",
        "--prompt",
        "DSH completed Slice X verification.",
        "--projects-root",
        &f.projects_root.display().to_string(),
        "--state-root",
        &f.state_root.display().to_string(),
        "--antigravity-root",
        &f.antigravity_root.display().to_string(),
        "--agy-bin",
        &f.agy_bin.display().to_string(),
    ]);
    assert!(
        dsh_reply_out.status.success(),
        "dsh -> agy reply failed: {}",
        String::from_utf8_lossy(&dsh_reply_out.stderr)
    );
    let dsh_reply_val: Value = serde_json::from_slice(&dsh_reply_out.stdout).unwrap();
    assert_eq!(dsh_reply_val["session_id"], agy_conv_id);
    assert_eq!(dsh_reply_val["to"], sender_with_session);
    assert_eq!(
        dsh_reply_val["from"],
        format!("deepseek-harness/agent-fleet#{dsh_session_id}")
    );
    assert_eq!(dsh_reply_val["reply_to"], "msg-agy-to-dsh-1");

    let (_, agy_sessions_t2) =
        inspect_host_state(DesktopHostKind::AntigravityDesktop, &f.state_root).unwrap();
    assert_eq!(
        agy_sessions_t2.len(),
        1,
        "reply must not create an orphan Antigravity session"
    );
    assert_eq!(agy_sessions_t2[0].session_id, agy_conv_id);
    assert_eq!(agy_sessions_t2[0].turns.len(), 2);

    let zcode_reply_out = f.run_cli(&[
        "send",
        "--from",
        "zcode/agent-fleet#zcode-sess-77",
        "--to",
        &sender_with_session,
        "--delivery-id",
        "msg-zcode-reply-2",
        "--prompt",
        "ZCode also verified Slice X on the same Antigravity thread.",
        "--projects-root",
        &f.projects_root.display().to_string(),
        "--state-root",
        &f.state_root.display().to_string(),
        "--antigravity-root",
        &f.antigravity_root.display().to_string(),
        "--agy-bin",
        &f.agy_bin.display().to_string(),
    ]);
    assert!(
        zcode_reply_out.status.success(),
        "zcode -> agy#session reply failed: {}",
        String::from_utf8_lossy(&zcode_reply_out.stderr)
    );
    let zcode_reply_val: Value = serde_json::from_slice(&zcode_reply_out.stdout).unwrap();
    assert_eq!(zcode_reply_val["session_id"], agy_conv_id);
    assert_eq!(zcode_reply_val["to"], sender_with_session);

    let (_, agy_sessions_t3) =
        inspect_host_state(DesktopHostKind::AntigravityDesktop, &f.state_root).unwrap();
    assert_eq!(
        agy_sessions_t3.len(),
        1,
        "direct #session_id delivery must not create an orphan Antigravity session"
    );
    assert_eq!(agy_sessions_t3[0].session_id, agy_conv_id);
    assert_eq!(agy_sessions_t3[0].turns.len(), 3);

    let unscoped_pull = mailbox
        .check_inbox("antigravity-desktop/agent-fleet", true)
        .unwrap();
    assert!(
        unscoped_pull.is_empty(),
        "unscoped inbox poll must not steal session-addressed replies"
    );

    let sibling_pull = mailbox
        .check_inbox("antigravity-desktop/agent-fleet#conv-other", true)
        .unwrap();
    assert!(
        sibling_pull.is_empty(),
        "sibling session poll must not steal another session's replies"
    );

    let exact_pull = mailbox.check_inbox(&sender_with_session, true).unwrap();
    assert_eq!(exact_pull.len(), 2);
    assert_eq!(exact_pull[0].id, "msg-dsh-reply-1");
    assert_eq!(exact_pull[1].id, "msg-zcode-reply-2");

    let agy_outbox = mailbox.check_outbox(&sender_with_session).unwrap();
    assert_eq!(agy_outbox.len(), 1);
    assert_eq!(agy_outbox[0].id, "msg-agy-to-dsh-1");

    let by_session = mailbox.list_by_session(&agy_conv_id).unwrap();
    assert_eq!(by_session.len(), 3);

    let failed_send = f.run_cli(&[
        "send",
        "--from",
        "deepseek-harness/agent-fleet",
        "--to",
        "antigravity-desktop/agent-fleet#nonexistent-conv-999",
        "--delivery-id",
        "msg-should-not-persist",
        "--prompt",
        "Attempting delivery to nonexistent session must fail before mailbox.enqueue.",
        "--projects-root",
        &f.projects_root.display().to_string(),
        "--state-root",
        &f.state_root.display().to_string(),
        "--antigravity-root",
        &f.antigravity_root.display().to_string(),
        "--agy-bin",
        &f.agy_bin.display().to_string(),
    ]);
    assert!(
        !failed_send.status.success(),
        "delivery to nonexistent session must fail"
    );
    assert!(
        mailbox
            .get_message("msg-should-not-persist")
            .unwrap()
            .is_none(),
        "failed adapter delivery must not leave a phantom message in mailboxes.json"
    );
}
