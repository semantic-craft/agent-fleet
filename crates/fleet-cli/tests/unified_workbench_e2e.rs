#![cfg(unix)]

use fleet_daemon::{discover_local_runtime_providers, FleetLedgerStore, HostRuntimeOptions};
use fleet_proto::{
    timeline::{
        timeline_content_hash, CanonicalSessionId, TimelineItem, TimelineItemKind,
        TimelineItemStatus, TimelineOrderSeq, TimelineRole, UpdatedSeq,
    },
    workbench::ProtocolEventEnvelope,
    DeliveryTier,
};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering},
        mpsc::{self, Receiver, Sender},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tungstenite::{accept, client, Message};

static NEXT_DIR_ID: AtomicU64 = AtomicU64::new(1);

fn unique_test_dir(prefix: &str) -> PathBuf {
    let seq = NEXT_DIR_ID.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!(
        "fleet-pr6-{prefix}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).expect("create temp test dir");
    dir
}

struct EmbeddedWorkbenchHarness {
    base_url: String,
    ws_base_url: String,
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl EmbeddedWorkbenchHarness {
    fn start(state_root: PathBuf, projects_root: PathBuf, zcode_bin: PathBuf) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let addr = listener.local_addr().expect("local addr");
        listener.set_nonblocking(true).expect("set nonblocking");

        let ledger = FleetLedgerStore::open(&state_root).expect("open ledger");
        let next_seq = Arc::new(AtomicI64::new(100));
        let subscribers: Arc<Mutex<BTreeMap<String, Vec<Sender<String>>>>> =
            Arc::new(Mutex::new(BTreeMap::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let stop_loop = Arc::clone(&stop);

        let state_root_clone = state_root.clone();
        let projects_root_clone = projects_root.clone();
        let zcode_bin_clone = zcode_bin.clone();

        let join = thread::spawn(move || {
            while !stop_loop.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let _ = stream.set_nonblocking(false);
                        let _ = stream.set_nodelay(true);
                        let ledger_conn = ledger.clone();
                        let seq_conn = Arc::clone(&next_seq);
                        let subs_conn = Arc::clone(&subscribers);
                        let stop_conn = Arc::clone(&stop_loop);
                        let sr = state_root_clone.clone();
                        let pr = projects_root_clone.clone();
                        let zb = zcode_bin_clone.clone();
                        thread::spawn(move || {
                            let mut peek = [0u8; 1024];
                            let n = stream.peek(&mut peek).unwrap_or(0);
                            if n == 0 {
                                return;
                            }
                            let preview = String::from_utf8_lossy(&peek[..n]);
                            let first = preview.lines().next().unwrap_or("");
                            let is_ws = preview
                                .lines()
                                .any(|l| l.to_ascii_lowercase().contains("upgrade: websocket"));
                            if is_ws {
                                let path = first.split_whitespace().nth(1).unwrap_or("/");
                                if let Some(rest) = path.strip_prefix("/api/v2/sessions/") {
                                    if let Some(sid) = rest.strip_suffix("/ws") {
                                        let sid = sid.to_string();
                                        if let Ok(mut ws) = accept(stream) {
                                            let (tx, rx): (Sender<String>, Receiver<String>) =
                                                mpsc::channel();
                                            subs_conn
                                                .lock()
                                                .unwrap()
                                                .entry(sid)
                                                .or_default()
                                                .push(tx);
                                            let _ = ws
                                                .get_ref()
                                                .set_read_timeout(Some(Duration::from_millis(15)));
                                            while !stop_conn.load(Ordering::SeqCst) {
                                                while let Ok(ev) = rx.try_recv() {
                                                    if ws.send(Message::Text(ev.into())).is_err() {
                                                        return;
                                                    }
                                                }
                                                match ws.read() {
                                                    Ok(Message::Close(_)) => break,
                                                    Err(tungstenite::Error::Io(ref e))
                                                        if e.kind()
                                                            == std::io::ErrorKind::WouldBlock
                                                            || e.kind()
                                                                == std::io::ErrorKind::TimedOut => {}
                                                    Err(_) => break,
                                                    _ => {}
                                                }
                                            }
                                        }
                                    }
                                }
                                return;
                            }

                            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                            let mut buf = [0u8; 65536];
                            let read_n = stream.read(&mut buf).unwrap_or(0);
                            if read_n == 0 {
                                return;
                            }
                            let req_str = String::from_utf8_lossy(&buf[..read_n]);
                            let (head, body_str) = req_str.split_once("\r\n\r\n").unwrap_or((&req_str, ""));
                            let req_line = head.lines().next().unwrap_or("");
                            let mut parts = req_line.split_whitespace();
                            let method = parts.next().unwrap_or("GET");
                            let path = parts.next().unwrap_or("/");

                            let resp_json = match (method, path) {
                                ("GET", "/api/v2/health") => json!({"status": "ok"}),
                                ("GET", "/api/v2/providers") => {
                                    let providers = discover_local_runtime_providers(
                                        &HostRuntimeOptions::default(),
                                    );
                                    json!({
                                        "connectors": [{
                                            "connectorId": "conn-local",
                                            "generation": 1,
                                            "providers": providers,
                                        }],
                                        "providers": providers,
                                    })
                                }
                                ("POST", "/api/v2/connector/ingest") => {
                                    let parsed: Value =
                                        serde_json::from_str(body_str).unwrap_or_else(|_| json!({}));
                                    let items: Vec<TimelineItem> = parsed
                                        .get("items")
                                        .and_then(|v| serde_json::from_value(v.clone()).ok())
                                        .unwrap_or_default();
                                    let mut sequenced = Vec::new();
                                    for mut it in items {
                                        let seq = UpdatedSeq::new(
                                            seq_conn.fetch_add(1, Ordering::SeqCst),
                                        );
                                        it.updated_seq = seq;
                                        sequenced.push(it);
                                    }
                                    let _ = ledger_conn.upsert_timeline_batch(&sequenced);
                                    let mut guard = subs_conn.lock().unwrap();
                                    for it in &sequenced {
                                        let env = ProtocolEventEnvelope::new(
                                            format!("evt-{}-{}", it.item_id, it.updated_seq.get()),
                                            "timeline.item_upsert",
                                            it.session_id.clone(),
                                            it.updated_seq,
                                            1_700_000_000_000,
                                            serde_json::to_string(it).unwrap(),
                                        );
                                        let wire = env.to_wire_json();
                                        if let Some(list) = guard.get_mut(it.session_id.as_str()) {
                                            list.retain(|tx| tx.send(wire.clone()).is_ok());
                                        }
                                    }
                                    json!({"accepted": sequenced.len(), "rejected": []})
                                }
                                _ if method == "POST"
                                    && path.starts_with("/api/v2/sessions/")
                                    && path.ends_with("/messages") =>
                                {
                                    let sid_str = path
                                        .strip_prefix("/api/v2/sessions/")
                                        .and_then(|s| s.strip_suffix("/messages"))
                                        .unwrap_or("");
                                    let parsed: Value =
                                        serde_json::from_str(body_str).unwrap_or_else(|_| json!({}));
                                    let prompt = parsed
                                        .get("prompt")
                                        .and_then(Value::as_str)
                                        .unwrap_or("Human follow-up turn");
                                    if let Ok(Some(meta)) = ledger_conn.get_session(sid_str) {
                                        let zroot = sr.join("zcode");
                                        let _ = Command::new(env!("CARGO_BIN_EXE_fleet-cli"))
                                            .args([
                                                "send",
                                                "--to",
                                                &format!(
                                                    "{}/{}#{}",
                                                    meta.runtime,
                                                    meta.project_slug,
                                                    meta.external_session_id
                                                ),
                                                "--prompt",
                                                prompt,
                                                "--projects-root",
                                                pr.to_str().unwrap(),
                                                "--state-root",
                                                sr.to_str().unwrap(),
                                                "--zcode-root",
                                                zroot.to_str().unwrap(),
                                                "--zcode-bin",
                                                zb.to_str().unwrap(),
                                            ])
                                            .output();
                                        let existing = ledger_conn
                                            .list_timeline_items(sid_str, None, 1000)
                                            .unwrap_or_default();
                                        let seq = UpdatedSeq::new(
                                            seq_conn.fetch_add(1, Ordering::SeqCst),
                                        );
                                        let order_seq = (existing.len() + 1) as i64;
                                        let item = TimelineItem {
                                            item_id: format!("human-{}", seq.get()),
                                            session_id: CanonicalSessionId::parse(sid_str).unwrap(),
                                            turn_id: Some(format!("turn-{order_seq}")),
                                            kind: TimelineItemKind::UserMessage,
                                            status: TimelineItemStatus::Completed,
                                            role: TimelineRole::User,
                                            content: prompt.to_string(),
                                            order_seq: TimelineOrderSeq::try_from(order_seq)
                                                .unwrap(),
                                            updated_seq: seq,
                                            content_hash: timeline_content_hash(
                                                "user_message",
                                                "completed",
                                                "user",
                                                prompt,
                                            ),
                                            delivery_tier: Some(DeliveryTier::ThreadVisible),
                                            sidebar_binding: None,
                                            peer_sender: None,
                                            created_at_ms: 1_700_000_000_000,
                                        };
                                        let _ = ledger_conn
                                            .upsert_timeline_batch(std::slice::from_ref(&item));
                                        let mut guard = subs_conn.lock().unwrap();
                                        let env = ProtocolEventEnvelope::new(
                                            format!("evt-{}", seq.get()),
                                            "timeline.item_upsert",
                                            item.session_id.clone(),
                                            seq,
                                            1_700_000_000_000,
                                            serde_json::to_string(&item).unwrap(),
                                        );
                                        let wire = env.to_wire_json();
                                        if let Some(list) = guard.get_mut(sid_str) {
                                            list.retain(|tx| tx.send(wire.clone()).is_ok());
                                        }
                                        json!({
                                            "ok": true,
                                            "sessionId": sid_str,
                                            "externalSessionId": meta.external_session_id,
                                            "orderSeq": order_seq,
                                            "updatedSeq": seq.get(),
                                        })
                                    } else {
                                        json!({"ok": false, "error": "session_not_found"})
                                    }
                                }
                                _ => json!({"status": "ok"}),
                            };
                            let body_bytes = serde_json::to_vec(&resp_json).unwrap();
                            let hdr = format!(
                                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                body_bytes.len()
                            );
                            let _ = stream.write_all(hdr.as_bytes());
                            let _ = stream.write_all(&body_bytes);
                            let _ = stream.flush();
                        });
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(1));
                    }
                    Err(_) => break,
                }
            }
        });

        Self {
            base_url: format!("http://{addr}"),
            ws_base_url: format!("ws://{addr}"),
            stop,
            join: Some(join),
        }
    }
}

impl Drop for EmbeddedWorkbenchHarness {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

fn setup_mock_zcode(base_dir: &Path) -> (PathBuf, PathBuf) {
    let zcode_root = base_dir.join("zcode");
    fs::create_dir_all(&zcode_root).expect("create zcode root");
    let zcode_bin = base_dir.join("mock-zcode.sh");
    fs::write(
        &zcode_bin,
        "#!/bin/sh\necho '{\"type\":\"result\",\"session_id\":\"zcode-e2e-sess-01\",\"result\":\"Mounted in ZCode sidebar\"}'\n",
    )
    .expect("write mock zcode bin");
    let mut perms = fs::metadata(&zcode_bin).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&zcode_bin, perms).unwrap();
    (zcode_root, zcode_bin)
}

#[test]
fn e2e_mcp_peer_dispatch_grokbot_to_zcode_and_reply_streams_to_workbench_ws() {
    let base = unique_test_dir("e2e-mcp");
    let projects_root = base.join("Projects");
    let state_root = base.join("state");
    fs::create_dir_all(projects_root.join("agent-fleet")).unwrap();
    fs::create_dir_all(&state_root).unwrap();
    let (zcode_root, zcode_bin) = setup_mock_zcode(&base);

    let harness = EmbeddedWorkbenchHarness::start(
        state_root.clone(),
        projects_root.clone(),
        zcode_bin.clone(),
    );

    let expected_canonical_sid =
        CanonicalSessionId::derive("local-connector", "zcode", "zcode-e2e-sess-01").unwrap();

    // Connect a live WebSocket subscriber to the session stream before MCP dispatch
    let ws_url = format!(
        "{}/api/v2/sessions/{}/ws",
        harness.ws_base_url,
        expected_canonical_sid.as_str()
    );
    let tcp_addr = harness.base_url.strip_prefix("http://").unwrap();
    let ws_tcp = TcpStream::connect(tcp_addr).expect("connect ws tcp");
    ws_tcp
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let (mut ws_client, _) = client(&ws_url, ws_tcp).expect("connect session ws subscriber");

    // Step 1: Dispatch `grokbot -> zcode/agent-fleet` via `fleet-cli mcp` `send_message`
    let mut mcp_child = Command::new(env!("CARGO_BIN_EXE_fleet-cli"))
        .args([
            "mcp",
            "--projects-root",
            projects_root.to_str().unwrap(),
            "--state-root",
            state_root.to_str().unwrap(),
            "--workbench",
            &harness.base_url,
            "--zcode-root",
            zcode_root.to_str().unwrap(),
            "--zcode-bin",
            zcode_bin.to_str().unwrap(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn fleet-cli mcp");

    {
        let stdin = mcp_child.stdin.as_mut().unwrap();
        let rpc_call = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": "send_message",
                "arguments": {
                    "from": "grokbot",
                    "to": "zcode/agent-fleet",
                    "delivery_id": "mcp-grok-to-zcode-1",
                    "title": "Unified E2E Verification",
                    "prompt": "Verify peer MCP dispatch into ZCode sidebar and Workbench timeline"
                }
            }
        });
        writeln!(stdin, "{rpc_call}").unwrap();
        stdin.flush().unwrap();
    }
    let mcp_out = mcp_child.wait_with_output().expect("wait mcp");
    assert!(mcp_out.status.success());
    let stdout_str = String::from_utf8_lossy(&mcp_out.stdout);
    let mcp_resp: Value = serde_json::from_str(stdout_str.trim()).expect("parse mcp json");
    let structured = &mcp_resp["result"]["structuredContent"];
    assert_eq!(structured["tier"], "thread-visible");
    assert_eq!(structured["peer_framed"], true);
    assert_eq!(structured["workbench_streamed"], true);
    assert_eq!(
        structured["canonical_session_id"],
        expected_canonical_sid.as_str()
    );

    // Verify WebSocket subscriber received the `<fleet-message>` timeline item
    let msg1 = ws_client.read().expect("read ws event 1");
    let Message::Text(txt1) = msg1 else {
        panic!("expected text frame");
    };
    let env1: Value = serde_json::from_str(&txt1).unwrap();
    assert_eq!(env1["type"], "timeline.item_upsert");
    assert_eq!(env1["sessionId"], expected_canonical_sid.as_str());
    assert_eq!(env1["data"]["peerSender"], "grokbot");
    assert_eq!(env1["data"]["deliveryTier"], "thread-visible");
    assert!(env1["data"]["content"]
        .as_str()
        .unwrap()
        .contains("<fleet-message from=\"grokbot\""));

    // Step 2: Reply from `zcode/agent-fleet -> grokbot` via `fleet-cli send --reply-to mcp-grok-to-zcode-1`
    let reply_out = Command::new(env!("CARGO_BIN_EXE_fleet-cli"))
        .args([
            "send",
            "--from",
            "zcode/agent-fleet",
            "--to",
            "grokbot",
            "--reply-to",
            "mcp-grok-to-zcode-1",
            "--delivery-id",
            "reply-zcode-to-grok-1",
            "--prompt",
            "ZCode verified sidebar mount and completed checks.",
            "--projects-root",
            projects_root.to_str().unwrap(),
            "--state-root",
            state_root.to_str().unwrap(),
            "--workbench",
            &harness.base_url,
        ])
        .output()
        .expect("run reply send");
    assert!(reply_out.status.success());
    let reply_json: Value = serde_json::from_slice(&reply_out.stdout).unwrap();
    assert_eq!(
        reply_json["canonical_session_id"],
        expected_canonical_sid.as_str()
    );
    assert_eq!(reply_json["workbench_streamed"], true);

    // Check GrokBot inbox receives the reply
    let inbox_out = Command::new(env!("CARGO_BIN_EXE_fleet-cli"))
        .args([
            "inbox",
            "--for",
            "grokbot",
            "--state-root",
            state_root.to_str().unwrap(),
        ])
        .output()
        .expect("run inbox");
    assert!(inbox_out.status.success());
    let inbox_json: Value = serde_json::from_slice(&inbox_out.stdout).unwrap();
    assert_eq!(inbox_json["count"], 1);
    assert_eq!(
        inbox_json["messages"][0]["reply_to"],
        "mcp-grok-to-zcode-1"
    );

    // Verify WebSocket subscriber received the reply in the SAME session thread
    let msg2 = ws_client.read().expect("read ws event 2");
    let Message::Text(txt2) = msg2 else {
        panic!("expected text frame 2");
    };
    let env2: Value = serde_json::from_str(&txt2).unwrap();
    assert_eq!(env2["type"], "timeline.item_upsert");
    assert_eq!(env2["sessionId"], expected_canonical_sid.as_str());
    assert_eq!(env2["data"]["role"], "assistant");
    assert_eq!(env2["data"]["orderSeq"], 2);
    assert!(env2["data"]["content"]
        .as_str()
        .unwrap()
        .contains("ZCode verified sidebar mount and completed checks."));

    // Step 3: Human intervenes from Web Workbench (`POST /api/v2/sessions/{id}/messages`)
    let sock_addr = harness.base_url.strip_prefix("http://").unwrap();
    let mut http_stream = TcpStream::connect(sock_addr).unwrap();
    let body_bytes = serde_json::to_vec(&json!({
        "prompt": "Human follow-up from Web Workbench: run cargo test",
        "role": "user"
    }))
    .unwrap();
    let req = format!(
        "POST /api/v2/sessions/{}/messages HTTP/1.1\r\nHost: {sock_addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        expected_canonical_sid.as_str(),
        body_bytes.len()
    );
    http_stream.write_all(req.as_bytes()).unwrap();
    http_stream.write_all(&body_bytes).unwrap();
    let mut resp_buf = String::new();
    http_stream.read_to_string(&mut resp_buf).unwrap();
    assert!(resp_buf.contains("200 OK"));

    let msg3 = ws_client.read().expect("read ws event 3");
    let Message::Text(txt3) = msg3 else {
        panic!("expected text frame 3");
    };
    let env3: Value = serde_json::from_str(&txt3).unwrap();
    assert_eq!(env3["sessionId"], expected_canonical_sid.as_str());
    assert!(env3["data"]["content"]
        .as_str()
        .unwrap()
        .contains("Human follow-up from Web Workbench"));

    // Verify no duplicate session was created in ledger
    let ledger = FleetLedgerStore::open(&state_root).unwrap();
    let sessions = ledger.list_sessions().unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].external_session_id, "zcode-e2e-sess-01");
}

#[test]
fn doctor_with_workbench_verifies_health_and_connector_leases() {
    let base = unique_test_dir("doctor-wb");
    let projects_root = base.join("Projects");
    let state_root = base.join("state");
    fs::create_dir_all(&projects_root).unwrap();
    fs::create_dir_all(&state_root).unwrap();
    let (_, zcode_bin) = setup_mock_zcode(&base);

    let harness = EmbeddedWorkbenchHarness::start(
        state_root.clone(),
        projects_root.clone(),
        zcode_bin,
    );

    let out = Command::new(env!("CARGO_BIN_EXE_fleet-cli"))
        .args([
            "doctor",
            "--projects-root",
            projects_root.to_str().unwrap(),
            "--state-root",
            state_root.to_str().unwrap(),
            "--workbench",
            &harness.base_url,
        ])
        .output()
        .expect("run doctor --workbench");
    assert!(out.status.success());
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["workbench"]["reachable"], true);
    assert_eq!(doc["workbench"]["health_status"], "ok");
    assert_eq!(doc["workbench"]["active_connector_leases"], 1);
    assert_eq!(doc["workbench"]["cindy_dispatch_channel"], "anet");
    assert_eq!(doc["workbench"]["ready"], true);

    // Unreachable workbench with --gate must fail closed (exit 2)
    let bad = Command::new(env!("CARGO_BIN_EXE_fleet-cli"))
        .args([
            "doctor",
            "--projects-root",
            projects_root.to_str().unwrap(),
            "--state-root",
            state_root.to_str().unwrap(),
            "--workbench",
            "http://127.0.0.1:9",
            "--gate",
        ])
        .output()
        .expect("run doctor --workbench bad --gate");
    assert_eq!(bad.status.code(), Some(2));
}

#[test]
fn bench_e2e_dispatch_and_workbench_stream() {
    let base = unique_test_dir("bench-e2e");
    let projects_root = base.join("Projects");
    let state_root = base.join("state");
    fs::create_dir_all(projects_root.join("bench-repo")).unwrap();
    fs::create_dir_all(&state_root).unwrap();
    let (_, zcode_bin) = setup_mock_zcode(&base);

    let harness = EmbeddedWorkbenchHarness::start(
        state_root.clone(),
        projects_root.clone(),
        zcode_bin.clone(),
    );

    // Warm up SQLite WAL and binary page cache before timed measurements
    let warm = Command::new(env!("CARGO_BIN_EXE_fleet-cli"))
        .args([
            "send",
            "--from",
            "grokbot",
            "--to",
            "grokbot/bench-repo",
            "--delivery-id",
            "bench-warmup-0",
            "--prompt",
            "warmup probe",
            "--projects-root",
            projects_root.to_str().unwrap(),
            "--state-root",
            state_root.to_str().unwrap(),
            "--workbench",
            &harness.base_url,
        ])
        .output()
        .unwrap();
    assert!(warm.status.success());

    let iterations = 30usize;
    let mut baseline_ms = Vec::with_capacity(iterations);
    let mut streamed_ms = Vec::with_capacity(iterations);

    for i in 0..iterations {
        let t0 = Instant::now();
        let out_base = Command::new(env!("CARGO_BIN_EXE_fleet-cli"))
            .args([
                "send",
                "--from",
                "grokbot",
                "--to",
                "grokbot/bench-repo",
                "--delivery-id",
                &format!("bench-base-{i}"),
                "--prompt",
                "baseline dispatch probe",
                "--projects-root",
                projects_root.to_str().unwrap(),
                "--state-root",
                state_root.to_str().unwrap(),
            ])
            .output()
            .unwrap();
        assert!(out_base.status.success());
        baseline_ms.push(t0.elapsed().as_secs_f64() * 1000.0);

        let t1 = Instant::now();
        let out_stream = Command::new(env!("CARGO_BIN_EXE_fleet-cli"))
            .args([
                "send",
                "--from",
                "grokbot",
                "--to",
                "grokbot/bench-repo",
                "--delivery-id",
                &format!("bench-stream-{i}"),
                "--prompt",
                "workbench streamed dispatch probe",
                "--projects-root",
                projects_root.to_str().unwrap(),
                "--state-root",
                state_root.to_str().unwrap(),
                "--workbench",
                &harness.base_url,
            ])
            .output()
            .unwrap();
        assert!(out_stream.status.success());
        streamed_ms.push(t1.elapsed().as_secs_f64() * 1000.0);
    }

    baseline_ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
    streamed_ms.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let p99_idx = ((iterations as f64) * 0.99).floor() as usize;
    let p99_idx = p99_idx.min(iterations - 1);
    let base_p99 = baseline_ms[p99_idx];
    let stream_p99 = streamed_ms[p99_idx];
    let overhead_p99 = (stream_p99 - base_p99).max(0.0);

    println!(
        "BENCH_E2E_WORKBENCH_STREAM: iterations={} baseline_p99_ms={:.3} streamed_p99_ms={:.3} overhead_p99_ms={:.3} max_overhead_ms=10.0",
        iterations, base_p99, stream_p99, overhead_p99
    );
    assert!(
        overhead_p99 < 10.0,
        "Workbench streaming P99 overhead {:.3} ms exceeded 10.0 ms limit",
        overhead_p99
    );
}
