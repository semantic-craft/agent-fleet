#![cfg(unix)]

use fleet_daemon::{
    connector_ws::{ConnectorClient, ConnectorConfig, ConnectorError},
    discover_local_runtime_providers, FleetLedgerStore, HostRuntimeOptions,
};
use fleet_proto::{
    timeline::{
        timeline_content_hash, CanonicalSessionId, SessionMeta, TimelineItem, TimelineItemKind,
        TimelineItemStatus, TimelineOrderSeq, TimelineRole, UpdatedSeq,
    },
    workbench::{ConnectorRpcFrame, ProtocolEventEnvelope},
    DeliveryTier,
};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
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
        let next_generation = Arc::new(AtomicU64::new(1));
        let connector_leases: Arc<Mutex<BTreeMap<String, Value>>> =
            Arc::new(Mutex::new(BTreeMap::new()));
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
                        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                        let ledger_conn = ledger.clone();
                        let seq_conn = Arc::clone(&next_seq);
                        let gen_conn = Arc::clone(&next_generation);
                        let leases_conn = Arc::clone(&connector_leases);
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
                                if path == "/api/v2/connector/ws" || path == "/ws/v2/connector" {
                                    if let Ok(mut ws) = accept(stream) {
                                        let _ = ws
                                            .get_ref()
                                            .set_read_timeout(Some(Duration::from_millis(50)));
                                        while !stop_conn.load(Ordering::SeqCst) {
                                            match ws.read() {
                                                Ok(Message::Text(txt)) => {
                                                    let Ok(frame) =
                                                        serde_json::from_str::<ConnectorRpcFrame>(
                                                            &txt,
                                                        )
                                                    else {
                                                        continue;
                                                    };
                                                    match frame.method.as_deref() {
                                                        Some("connector.hello") => {
                                                            let leased = gen_conn
                                                                .fetch_add(1, Ordering::SeqCst);
                                                            let params: Value = frame
                                                                .params_json
                                                                .as_deref()
                                                                .and_then(|s| {
                                                                    serde_json::from_str(s).ok()
                                                                })
                                                                .unwrap_or_else(|| json!({}));
                                                            let cid = params
                                                                .get("connectorId")
                                                                .and_then(Value::as_str)
                                                                .unwrap_or("conn-local")
                                                                .to_string();
                                                            let providers = params
                                                                .get("providers")
                                                                .cloned()
                                                                .unwrap_or_else(|| json!([]));
                                                            leases_conn.lock().unwrap().insert(
                                                                cid.clone(),
                                                                json!({
                                                                    "connectorId": cid,
                                                                    "generation": leased,
                                                                    "providers": providers,
                                                                }),
                                                            );
                                                            let welcome =
                                                                ConnectorRpcFrame::new_response(
                                                                    leased,
                                                                    frame
                                                                        .id
                                                                        .unwrap_or_else(|| {
                                                                            "hello-1".into()
                                                                        }),
                                                                    json!({
                                                                        "connectorId": cid,
                                                                        "leasedGeneration": leased,
                                                                    })
                                                                    .to_string(),
                                                                );
                                                            let wire =
                                                                serde_json::to_string(&welcome)
                                                                    .unwrap();
                                                            if ws
                                                                .send(Message::Text(wire.into()))
                                                                .is_err()
                                                            {
                                                                break;
                                                            }
                                                        }
                                                        Some("timeline.itemUpsert") => {
                                                            let params: Value = frame
                                                                .params_json
                                                                .as_deref()
                                                                .and_then(|s| {
                                                                    serde_json::from_str(s).ok()
                                                                })
                                                                .unwrap_or_else(|| json!({}));
                                                            let items: Vec<TimelineItem> = params
                                                                .get("items")
                                                                .and_then(|v| {
                                                                    serde_json::from_value(
                                                                        v.clone(),
                                                                    )
                                                                    .ok()
                                                                })
                                                                .unwrap_or_default();
                                                            let mut sequenced = Vec::new();
                                                            for mut it in items {
                                                                let seq = UpdatedSeq::new(
                                                                    seq_conn.fetch_add(
                                                                        1,
                                                                        Ordering::SeqCst,
                                                                    ),
                                                                );
                                                                it.updated_seq = seq;
                                                                let _ =
                                                                    ledger_conn.upsert_session(
                                                                        &SessionMeta {
                                                                            session_id: it
                                                                                .session_id
                                                                                .clone(),
                                                                            connector_id: params
                                                                                .get("connectorId")
                                                                                .and_then(
                                                                                    Value::as_str,
                                                                                )
                                                                                .unwrap_or(
                                                                                    "conn-local",
                                                                                )
                                                                                .to_string(),
                                                                            runtime: "zcode"
                                                                                .to_string(),
                                                                            external_session_id:
                                                                                "zcode-e2e-sess-01"
                                                                                    .to_string(),
                                                                            project_slug:
                                                                                "agent-fleet"
                                                                                    .to_string(),
                                                                            title: "Aliyun Cutover"
                                                                                .to_string(),
                                                                            latest_updated_seq: seq,
                                                                            delivery_tier: it
                                                                                .delivery_tier,
                                                                            sidebar_binding: it
                                                                                .sidebar_binding
                                                                                .clone(),
                                                                        },
                                                                    );
                                                                sequenced.push(it);
                                                            }
                                                            let _ = ledger_conn
                                                                .upsert_timeline_batch(&sequenced);
                                                            let mut guard =
                                                                subs_conn.lock().unwrap();
                                                            for it in &sequenced {
                                                                let env =
                                                                    ProtocolEventEnvelope::new(
                                                                        format!(
                                                                            "evt-{}-{}",
                                                                            it.item_id,
                                                                            it.updated_seq.get()
                                                                        ),
                                                                        "timeline.item_upsert",
                                                                        it.session_id.clone(),
                                                                        it.updated_seq,
                                                                        1_700_000_000_000,
                                                                        serde_json::to_string(it)
                                                                            .unwrap(),
                                                                    );
                                                                let wire = env.to_wire_json();
                                                                if let Some(list) = guard
                                                                    .get_mut(it.session_id.as_str())
                                                                {
                                                                    list.retain(|tx| {
                                                                        tx.send(wire.clone())
                                                                            .is_ok()
                                                                    });
                                                                }
                                                            }
                                                        }
                                                        _ => {}
                                                    }
                                                }
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
                                    return;
                                }
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

                            let mut raw = Vec::with_capacity(8192);
                            let mut chunk = [0u8; 16384];
                            let mut header_end = None;
                            let mut expected_len = 0usize;
                            loop {
                                match stream.read(&mut chunk) {
                                    Ok(0) => break,
                                    Ok(n) => {
                                        raw.extend_from_slice(&chunk[..n]);
                                        if header_end.is_none() {
                                            if let Some(pos) = raw
                                                .windows(4)
                                                .position(|w| w == b"\r\n\r\n")
                                            {
                                                let end = pos + 4;
                                                header_end = Some(end);
                                                let head_txt =
                                                    String::from_utf8_lossy(&raw[..pos]);
                                                for line in head_txt.lines() {
                                                    if let Some((k, v)) = line.split_once(':') {
                                                        if k.trim()
                                                            .eq_ignore_ascii_case("content-length")
                                                        {
                                                            expected_len =
                                                                v.trim().parse().unwrap_or(0);
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                        if let Some(end) = header_end {
                                            if raw.len() >= end + expected_len {
                                                break;
                                            }
                                        }
                                    }
                                    Err(_) => break,
                                }
                            }
                            if raw.is_empty() {
                                return;
                            }
                            let req_str = String::from_utf8_lossy(&raw);
                            let (head, body_str) =
                                req_str.split_once("\r\n\r\n").unwrap_or((&req_str, ""));
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
                                    let recorded: Vec<Value> = leases_conn
                                        .lock()
                                        .unwrap()
                                        .values()
                                        .cloned()
                                        .collect();
                                    let connectors = if recorded.is_empty() {
                                        vec![json!({
                                            "connectorId": "conn-local",
                                            "generation": 1,
                                            "providers": providers,
                                        })]
                                    } else {
                                        recorded
                                    };
                                    json!({
                                        "connectors": connectors,
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

struct TlsGatewayHarness {
    https_base_url: String,
    wss_connector_url: String,
    cert_path: PathBuf,
    child: Child,
}

impl TlsGatewayHarness {
    fn start(dir: &Path, backend_http_url: &str) -> Self {
        let cert_path = dir.join("aliyun-gateway-cert.pem");
        let key_path = dir.join("aliyun-gateway-key.pem");
        let cert_status = Command::new("openssl")
            .args([
                "req",
                "-x509",
                "-newkey",
                "rsa:2048",
                "-keyout",
                key_path.to_str().unwrap(),
                "-out",
                cert_path.to_str().unwrap(),
                "-days",
                "1",
                "-nodes",
                "-subj",
                "/CN=127.0.0.1",
                "-addext",
                "subjectAltName=IP:127.0.0.1,DNS:localhost",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("generate self-signed X.509 cert via openssl");
        assert!(cert_status.success(), "openssl cert generation succeeded");

        let backend_addr = backend_http_url
            .strip_prefix("http://")
            .expect("backend http url");
        let (backend_host, backend_port) = backend_addr.split_once(':').expect("host:port");

        let proxy_py = r#"
import socket, ssl, sys, threading

backend_host = sys.argv[1]
backend_port = int(sys.argv[2])
cert_file = sys.argv[3]
key_file = sys.argv[4]

ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
ctx.load_cert_chain(certfile=cert_file, keyfile=key_file)

srv = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
srv.bind(("127.0.0.1", 0))
srv.listen(64)
port = srv.getsockname()[1]
print(f"READY {port}", flush=True)

def pipe(src, dst):
    try:
        while True:
            data = src.recv(16384)
            if not data:
                break
            dst.sendall(data)
    except Exception:
        pass
    finally:
        try:
            dst.shutdown(socket.SHUT_WR)
        except Exception:
            pass

def handle(client_sock):
    tls_conn = None
    backend = None
    try:
        tls_conn = ctx.wrap_socket(client_sock, server_side=True)
        backend = socket.create_connection((backend_host, backend_port), timeout=5.0)
        t1 = threading.Thread(target=pipe, args=(tls_conn, backend), daemon=True)
        t2 = threading.Thread(target=pipe, args=(backend, tls_conn), daemon=True)
        t1.start()
        t2.start()
        t1.join()
        t2.join()
    except Exception:
        pass
    finally:
        for s in (tls_conn, backend, client_sock):
            if s is not None:
                try:
                    s.close()
                except Exception:
                    pass

while True:
    try:
        c, _ = srv.accept()
        threading.Thread(target=handle, args=(c,), daemon=True).start()
    except Exception:
        break
"#;

        let mut child = Command::new("python3")
            .args([
                "-u",
                "-c",
                proxy_py,
                backend_host,
                backend_port,
                cert_path.to_str().unwrap(),
                key_path.to_str().unwrap(),
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn python3 TLS reverse proxy");

        let stdout = child.stdout.as_mut().expect("python3 stdout");
        let mut reader = BufReader::new(stdout);
        let mut ready_line = String::new();
        reader
            .read_line(&mut ready_line)
            .expect("read READY line from TLS gateway");
        let tls_port: u16 = ready_line
            .trim()
            .strip_prefix("READY ")
            .expect("READY prefix from TLS gateway")
            .parse()
            .expect("parse TLS port");

        Self {
            https_base_url: format!("https://127.0.0.1:{tls_port}"),
            wss_connector_url: format!("wss://127.0.0.1:{tls_port}/ws/v2/connector"),
            cert_path,
            child,
        }
    }
}

impl Drop for TlsGatewayHarness {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn e2e_aliyun_tls_gateway_cutover_connector_lease_handoff_and_doctor_gate() {
    let base = unique_test_dir("aliyun-tls-cutover");
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
    let tls_gateway = TlsGatewayHarness::start(&base, &harness.base_url);

    let conn1_out = Command::new(env!("CARGO_BIN_EXE_fleet-cli"))
        .env("FLEET_TLS_CA_CERT", &tls_gateway.cert_path)
        .args([
            "connector",
            "--server",
            &tls_gateway.wss_connector_url,
            "--id",
            "conn-aliyun-1",
            "--token",
            "aliyun-secret-token",
            "--projects-root",
            projects_root.to_str().unwrap(),
            "--state-root",
            state_root.to_str().unwrap(),
            "--once",
        ])
        .output()
        .expect("run fleet-cli connector --server wss://.../ws/v2/connector --once");
    assert!(
        conn1_out.status.success(),
        "outbound wss connector handshake must succeed: {}",
        String::from_utf8_lossy(&conn1_out.stderr)
    );
    let conn1_json: Value = serde_json::from_slice(&conn1_out.stdout).unwrap();
    assert_eq!(conn1_json["ok"], true);
    assert_eq!(conn1_json["connectorId"], "conn-aliyun-1");
    assert_eq!(conn1_json["leasedGeneration"], 1);

    let conn2_out = Command::new(env!("CARGO_BIN_EXE_fleet-cli"))
        .env("FLEET_TLS_CA_CERT", &tls_gateway.cert_path)
        .args([
            "connector",
            "--server",
            &tls_gateway.wss_connector_url,
            "--id",
            "conn-aliyun-1",
            "--token",
            "aliyun-secret-token",
            "--projects-root",
            projects_root.to_str().unwrap(),
            "--state-root",
            state_root.to_str().unwrap(),
            "--once",
        ])
        .output()
        .expect("run second connector handshake for lease handoff");
    assert!(conn2_out.status.success());
    let conn2_json: Value = serde_json::from_slice(&conn2_out.stdout).unwrap();
    assert_eq!(conn2_json["leasedGeneration"], 2);

    let cfg = ConnectorConfig::new(
        tls_gateway.wss_connector_url.clone(),
        "conn-aliyun-1",
        "aliyun-secret-token",
        projects_root.clone(),
        state_root.clone(),
    );
    let mut live_client = ConnectorClient::new(cfg).unwrap();
    let leased_gen = live_client.connect().expect("connect live wss ConnectorClient");
    assert_eq!(leased_gen, 3, "third reconnect must advance lease generation to 3");

    let stale_frame = ConnectorRpcFrame::new_request(1, "stale-req-1", "runtime.ping", "{}");
    let stale_err = live_client
        .handler()
        .handle_frame(&stale_frame)
        .expect_err("stale generation 1 frame must be fenced when active generation is 3");
    assert!(
        matches!(
            stale_err,
            ConnectorError::StaleGeneration {
                active: 3,
                received: 1
            }
        ),
        "expected StaleGeneration {{ active: 3, received: 1 }}, got {stale_err:?}"
    );

    let expected_canonical_sid =
        CanonicalSessionId::derive("local-connector", "zcode", "zcode-e2e-sess-01").unwrap();
    let ws_url = format!(
        "{}/api/v2/sessions/{}/ws",
        harness.ws_base_url,
        expected_canonical_sid.as_str()
    );
    let tcp_addr = harness.base_url.strip_prefix("http://").unwrap();
    let ws_tcp = TcpStream::connect(tcp_addr).expect("connect ws subscriber");
    ws_tcp
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let (mut ws_client, _) = client(&ws_url, ws_tcp).expect("subscribe session ws");

    let send_out = Command::new(env!("CARGO_BIN_EXE_fleet-cli"))
        .env("FLEET_TLS_CA_CERT", &tls_gateway.cert_path)
        .args([
            "send",
            "--from",
            "grokbot",
            "--to",
            "zcode/agent-fleet",
            "--delivery-id",
            "aliyun-https-dispatch-1",
            "--title",
            "Aliyun Cutover Verification",
            "--prompt",
            "Verify HTTPS/WSS bidirectional timeline sync over Aliyun gateway",
            "--projects-root",
            projects_root.to_str().unwrap(),
            "--state-root",
            state_root.to_str().unwrap(),
            "--zcode-root",
            zcode_root.to_str().unwrap(),
            "--zcode-bin",
            zcode_bin.to_str().unwrap(),
            "--workbench",
            &tls_gateway.https_base_url,
        ])
        .output()
        .expect("send over HTTPS workbench gateway");
    assert!(send_out.status.success());
    let send_json: Value = serde_json::from_slice(&send_out.stdout).unwrap();
    assert_eq!(send_json["workbench_streamed"], true);

    let ws_msg = ws_client.read().expect("read timeline event from session ws");
    let Message::Text(ws_txt) = ws_msg else {
        panic!("expected text frame on session ws");
    };
    let ws_env: Value = serde_json::from_str(&ws_txt).unwrap();
    assert_eq!(ws_env["type"], "timeline.item_upsert");
    assert_eq!(ws_env["sessionId"], expected_canonical_sid.as_str());

    let followup_item = TimelineItem {
        item_id: "connector-ws-emit-1".to_string(),
        session_id: expected_canonical_sid.clone(),
        turn_id: Some("turn-ws-2".to_string()),
        kind: TimelineItemKind::AssistantMessage,
        status: TimelineItemStatus::Completed,
        role: TimelineRole::Assistant,
        content: "Desktop connector streamed assistant turn over wss://".to_string(),
        order_seq: TimelineOrderSeq::try_from(2).unwrap(),
        updated_seq: UpdatedSeq::new(2),
        content_hash: timeline_content_hash(
            "assistant_message",
            "completed",
            "assistant",
            "Desktop connector streamed assistant turn over wss://",
        ),
        delivery_tier: Some(DeliveryTier::ThreadVisible),
        sidebar_binding: None,
        peer_sender: Some("zcode".to_string()),
        created_at_ms: 1_700_000_000_500,
    };
    let emit_outcome = live_client
        .emit_timeline_batch(std::slice::from_ref(&followup_item))
        .expect("emit timeline batch over wss connector");
    assert_eq!(emit_outcome.accepted, 1);

    let ws_msg2 = ws_client
        .read()
        .expect("read wss-emitted timeline event from session ws");
    let Message::Text(ws_txt2) = ws_msg2 else {
        panic!("expected second text frame on session ws");
    };
    let ws_env2: Value = serde_json::from_str(&ws_txt2).unwrap();
    assert_eq!(ws_env2["type"], "timeline.item_upsert");
    assert_eq!(ws_env2["data"]["itemId"], "connector-ws-emit-1");

    let web_post = Command::new("curl")
        .args([
            "-sS",
            "--cacert",
            tls_gateway.cert_path.to_str().unwrap(),
            "-X",
            "POST",
            "-H",
            "Content-Type: application/json",
            "-d",
            r#"{"prompt":"Mobile/Web follow-up turn over Aliyun HTTPS","role":"user"}"#,
            &format!(
                "{}/api/v2/sessions/{}/messages",
                tls_gateway.https_base_url,
                expected_canonical_sid.as_str()
            ),
        ])
        .output()
        .expect("POST /api/v2/sessions/{id}/messages over HTTPS");
    assert!(web_post.status.success());
    let web_post_json: Value = serde_json::from_slice(&web_post.stdout).unwrap();
    assert_eq!(web_post_json["ok"], true);

    let ws_msg3 = ws_client
        .read()
        .expect("read mobile/web follow-up turn from session ws");
    let Message::Text(ws_txt3) = ws_msg3 else {
        panic!("expected third text frame on session ws");
    };
    let ws_env3: Value = serde_json::from_str(&ws_txt3).unwrap();
    assert!(ws_env3["data"]["content"]
        .as_str()
        .unwrap()
        .contains("Mobile/Web follow-up turn over Aliyun HTTPS"));

    let doctor_out = Command::new(env!("CARGO_BIN_EXE_fleet-cli"))
        .env("HOME", &base)
        .env("FLEET_TLS_CA_CERT", &tls_gateway.cert_path)
        .args([
            "doctor",
            "--projects-root",
            projects_root.to_str().unwrap(),
            "--state-root",
            state_root.to_str().unwrap(),
            "--workbench",
            &tls_gateway.https_base_url,
            "--gate",
        ])
        .output()
        .expect("run fleet-cli doctor --workbench https://... --gate");
    assert!(
        doctor_out.status.success(),
        "doctor --workbench https://... --gate must exit 0: stderr={}",
        String::from_utf8_lossy(&doctor_out.stderr)
    );
    let doctor_json: Value = serde_json::from_slice(&doctor_out.stdout).unwrap();
    assert_eq!(doctor_json["workbench"]["reachable"], true);
    assert_eq!(doctor_json["workbench"]["health_status"], "ok");
    assert_eq!(doctor_json["workbench"]["active_connector_leases"], 1);
    assert_eq!(
        doctor_json["workbench"]["connectors"][0]["connectorId"],
        "conn-aliyun-1"
    );
    assert_eq!(
        doctor_json["workbench"]["connectors"][0]["generation"],
        3
    );
    assert_eq!(doctor_json["workbench"]["ready"], true);
}
