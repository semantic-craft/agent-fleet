use fleet_daemon::{
    ConnectorClient, ConnectorConfig, ConnectorError, FleetLedgerStore, TimelineEmitTransport,
    ZcodeRuntimeConfig,
};
use fleet_proto::{
    timeline::{
        timeline_content_hash, CanonicalSessionId, TimelineItem, TimelineItemKind,
        TimelineItemStatus, TimelineOrderSeq, TimelineRole, UpdatedSeq,
    },
    workbench::ConnectorRpcFrame,
};
use serde_json::{json, Value};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    process::Command,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Barrier, Mutex,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tungstenite::{accept, Message};

fn make_temp_dir(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!(
        "fleet-pr4-{prefix}-{}-{nanos}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn sample_timeline_item(
    connector_id: &str,
    runtime: &str,
    ext_session: &str,
    item_id: &str,
    order_seq: i64,
    content: &str,
) -> TimelineItem {
    let session_id =
        CanonicalSessionId::derive(connector_id, runtime, ext_session).expect("valid session id");
    TimelineItem {
        item_id: item_id.to_string(),
        session_id,
        turn_id: Some(format!("turn-{order_seq}")),
        kind: TimelineItemKind::AssistantMessage,
        status: TimelineItemStatus::Completed,
        role: TimelineRole::Assistant,
        content: content.to_string(),
        order_seq: TimelineOrderSeq::try_from(order_seq).expect("positive order_seq"),
        updated_seq: UpdatedSeq::new(0),
        content_hash: timeline_content_hash("assistant_message", "completed", "assistant", content),
        delivery_tier: None,
        sidebar_binding: None,
        peer_sender: None,
        created_at_ms: 1_762_000_000_000 + order_seq,
    }
}

/// Minimal dual-mode HTTP (`/api/v2/connector/ingest`) + WebSocket (`/api/v2/connector/ws`)
/// mock server on a single loopback port.
fn spawn_mock_v2_server(
    generation: u64,
    ingest_response_json: String,
    server_rpc_requests: Vec<ConnectorRpcFrame>,
) -> (
    String,
    Arc<Mutex<Vec<String>>>,
    Arc<Mutex<Vec<ConnectorRpcFrame>>>,
    thread::JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let addr = listener.local_addr().expect("local addr");
    let server_url = format!("http://127.0.0.1:{}", addr.port());
    let http_bodies = Arc::new(Mutex::new(Vec::new()));
    let ws_frames = Arc::new(Mutex::new(Vec::new()));

    let http_bodies_clone = Arc::clone(&http_bodies);
    let ws_frames_clone = Arc::clone(&ws_frames);

    let handle = thread::spawn(move || {
        let _ = listener.set_nonblocking(false);
        for incoming in listener.incoming().flatten() {
            let mut peek_buf = [0u8; 256];
            let n = incoming.peek(&mut peek_buf).unwrap_or(0);
            let head = String::from_utf8_lossy(&peek_buf[..n]);
            if head.starts_with("POST /api/v2/connector/ingest") {
                let mut stream = incoming;
                let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                let mut reader = BufReader::new(&mut stream);
                let mut content_len = 0usize;
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0
                        || line == "\r\n"
                        || line == "\n"
                    {
                        break;
                    }
                    if let Some((k, v)) = line.split_once(':') {
                        if k.trim().eq_ignore_ascii_case("content-length") {
                            content_len = v.trim().parse::<usize>().unwrap_or(0);
                        }
                    }
                }
                let mut body = vec![0u8; content_len];
                let _ = reader.read_exact(&mut body);
                http_bodies_clone
                    .lock()
                    .expect("mutex")
                    .push(String::from_utf8_lossy(&body).into_owned());

                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    ingest_response_json.len(),
                    ingest_response_json
                );
                let _ = stream.write_all(resp.as_bytes());
                let _ = stream.flush();
                break;
            } else if head.starts_with("GET /api/v2/connector/ws") {
                let mut ws = accept(incoming).expect("accept ws");
                if let Ok(Message::Text(hello_raw)) = ws.read() {
                    let hello: ConnectorRpcFrame =
                        serde_json::from_str(&hello_raw).expect("parse hello");
                    ws_frames_clone.lock().expect("mutex").push(hello.clone());
                    let welcome = ConnectorRpcFrame::new_response(
                        generation,
                        hello.id.unwrap_or_else(|| "hello-1".into()),
                        json!({
                            "ok": true,
                            "leasedGeneration": generation,
                        })
                        .to_string(),
                    );
                    let _ = ws.send(Message::Text(
                        serde_json::to_string(&welcome).unwrap().into(),
                    ));
                }
                for req in &server_rpc_requests {
                    let _ = ws.send(Message::Text(
                        serde_json::to_string(req).unwrap().into(),
                    ));
                    if let Ok(Message::Text(resp_raw)) = ws.read() {
                        if let Ok(parsed) = serde_json::from_str::<ConnectorRpcFrame>(&resp_raw) {
                            ws_frames_clone.lock().expect("mutex").push(parsed);
                        }
                    }
                }
                break;
            }
        }
    });

    (server_url, http_bodies, ws_frames, handle)
}

#[test]
fn ws_handshake_claims_lease_and_reports_six_local_peers() {
    let root = make_temp_dir("ws-handshake");
    let projects_root = root.join("Projects");
    let state_root = root.join("state");
    fs::create_dir_all(&projects_root).unwrap();
    fs::create_dir_all(&state_root).unwrap();

    let discover_req =
        ConnectorRpcFrame::new_request(7, "srv-discover-1", "runtime.discover", "{}");
    let (server_url, _http_bodies, ws_frames, handle) = spawn_mock_v2_server(
        7,
        r#"{"accepted":0,"rejected":[]}"#.to_string(),
        vec![discover_req],
    );

    let cfg = ConnectorConfig::new(
        &server_url,
        "conn-alpha",
        "secret-token-1",
        projects_root,
        state_root,
    );
    let mut client = ConnectorClient::new(cfg).expect("create client");
    let gen = client.connect().expect("ws handshake");
    assert_eq!(gen, 7);

    let resp = client
        .process_next_ws_rpc()
        .expect("process runtime.discover");
    handle.join().expect("server thread");

    let recorded = ws_frames.lock().expect("mutex");
    assert_eq!(recorded.len(), 2);
    assert_eq!(recorded[0].method.as_deref(), Some("connector.hello"));
    let result_val: Value =
        serde_json::from_str(resp.result_json.as_deref().unwrap()).expect("parse discover result");
    let providers = result_val["providers"].as_array().expect("providers array");
    assert_eq!(providers.len(), 6, "must report 5 desktop hosts + grokbot");
    println!(
        "WS_HANDSHAKE_OK: leased_generation={gen} providers_count={}",
        providers.len()
    );
}

#[test]
fn timeline_batch_over_900kib_spills_over_to_http_ingest() {
    let root = make_temp_dir("ingest-spillover");
    let projects_root = root.join("Projects");
    let state_root = root.join("state");
    fs::create_dir_all(&projects_root).unwrap();
    fs::create_dir_all(&state_root).unwrap();

    let (server_url, http_bodies, _ws_frames, handle) = spawn_mock_v2_server(
        3,
        r#"{"accepted":12,"rejected":[]}"#.to_string(),
        Vec::new(),
    );

    let cfg = ConnectorConfig::new(
        &server_url,
        "conn-alpha",
        "secret-token-2",
        projects_root,
        state_root,
    );
    let mut client = ConnectorClient::new(cfg).expect("create client");

    // Build a 1.2 MiB timeline snapshot batch (12 items * ~105 KiB each).
    let chunk = "X".repeat(105 * 1024);
    let items: Vec<TimelineItem> = (1..=12)
        .map(|i| {
            sample_timeline_item(
                "conn-alpha",
                "zcode",
                "sess-big",
                &format!("item-{i}"),
                i,
                &chunk,
            )
        })
        .collect();

    let outcome = client
        .emit_timeline_batch(&items)
        .expect("emit 1.2 MiB batch via HTTP ingest spillover");
    handle.join().expect("server thread");

    assert_eq!(outcome.transport, TimelineEmitTransport::HttpIngestSpillover);
    assert!(
        outcome.wire_bytes >= 1_200_000,
        "batch wire bytes {} must be >= 1.2 MiB",
        outcome.wire_bytes
    );
    assert_eq!(outcome.accepted, 12);
    assert!(outcome.rejected.is_empty());
    assert_eq!(http_bodies.lock().expect("mutex").len(), 1);
    println!(
        "INGEST_SPILLOVER_OK: transport={:?} wire_bytes={} accepted={}",
        outcome.transport, outcome.wire_bytes, outcome.accepted
    );
}

#[test]
fn partial_rejection_isolation_advances_accepted_fingerprints_and_isolates_rejected_index() {
    let root = make_temp_dir("partial-reject");
    let projects_root = root.join("Projects");
    let state_root = root.join("state");
    fs::create_dir_all(&projects_root).unwrap();
    fs::create_dir_all(&state_root).unwrap();

    let mock_resp =
        r#"{"accepted":9,"rejected":[{"index":4,"code":"invalid_order_seq","message":"order_seq regression"}]}"#
            .to_string();
    let (server_url, _http_bodies, _ws_frames, handle) =
        spawn_mock_v2_server(5, mock_resp, Vec::new());

    let cfg = ConnectorConfig::new(
        &server_url,
        "conn-alpha",
        "secret-token-3",
        projects_root,
        state_root.clone(),
    );
    let mut client = ConnectorClient::new(cfg).expect("create client");

    let items: Vec<TimelineItem> = (0..10)
        .map(|idx| {
            sample_timeline_item(
                "conn-alpha",
                "zcode",
                "sess-partial",
                &format!("item-{idx}"),
                (idx + 1) as i64,
                &format!("payload-{idx}"),
            )
        })
        .collect();

    let outcome = client
        .ingest_via_http(&items)
        .expect("partial rejection must not crash loop");
    handle.join().expect("server thread");

    assert_eq!(outcome.accepted, 9);
    assert_eq!(outcome.rejected.len(), 1);
    assert_eq!(outcome.rejected[0].index, 4);
    assert_eq!(outcome.rejected[0].code, "invalid_order_seq");

    let ledger = FleetLedgerStore::open(&state_root).expect("open ledger");
    let stored = ledger
        .list_timeline_items(items[0].session_id.as_str(), None, 100)
        .expect("list stored timeline items");
    assert_eq!(
        stored.len(),
        9,
        "only the 9 accepted items may advance in fleet-ledger.sqlite"
    );
    assert!(
        stored.iter().all(|it| it.item_id != "item-4"),
        "rejected index 4 (item-4) must be isolated from ledger sync fingerprints"
    );
    println!(
        "PARTIAL_REJECT_OK: accepted={} rejected_index={} ledger_stored={}",
        outcome.accepted, outcome.rejected[0].index, stored.len()
    );
}

#[test]
fn rpc_session_create_mounts_zcode_sidebar_and_returns_thread_visible() {
    let root = make_temp_dir("rpc-zcode-create");
    let projects_root = root.join("Projects");
    let state_root = root.join("state");
    let zcode_root = root.join("zcode-home");
    fs::create_dir_all(&projects_root).unwrap();
    fs::create_dir_all(&state_root).unwrap();
    fs::create_dir_all(zcode_root.join("v2")).unwrap();
    fs::create_dir_all(zcode_root.join("cli/db")).unwrap();

    // Create a mock zcode binary that writes the session row to cli/db/db.sqlite.
    let zcode_bin = root.join("mock-zcode.sh");
    fs::write(
        &zcode_bin,
        "#!/bin/sh\n\
         echo '{\"type\":\"result\",\"session_id\":\"zcode-rpc-sess-1\",\"result\":\"ready\"}'\n",
    )
    .unwrap();
    fs::set_permissions(&zcode_bin, fs::Permissions::from_mode(0o755)).unwrap();

    let mut cfg = ConnectorConfig::new(
        "http://127.0.0.1:59999",
        "conn-alpha",
        "secret-token-4",
        projects_root.clone(),
        state_root,
    );
    cfg.runtime_options.zcode = Some(ZcodeRuntimeConfig::new(zcode_root, zcode_bin));

    let client = ConnectorClient::new(cfg).expect("create client");
    let create_frame = ConnectorRpcFrame::new_request(
        1,
        "rpc-create-1",
        "session.create",
        json!({
            "target": "zcode/demo-app",
            "title": "Demo App Session",
            "deliveryId": "deliv-rpc-1",
            "prompt": "Hello ZCode from Workbench",
        })
        .to_string(),
    );
    let resp = client
        .handler()
        .handle_frame(&create_frame)
        .expect("handle session.create");
    assert!(
        resp.error_code.is_none(),
        "unexpected error: {:?} {:?}",
        resp.error_code,
        resp.error_message
    );
    let result: Value = serde_json::from_str(resp.result_json.as_deref().unwrap()).unwrap();
    assert_eq!(result["ok"], true);
    assert_eq!(result["projectSlug"], "demo-app");
    assert_eq!(result["deliveryTier"], "thread-visible");
    assert!(projects_root.join("demo-app").is_dir());
    println!(
        "RPC_SESSION_CREATE_OK: session_id={} tier={} project_dir={}",
        result["sessionId"], result["deliveryTier"], result["projectDir"]
    );
}

#[test]
fn sandbox_fs_rejects_traversal_and_anet_secret_paths_with_project_dir_boundary_error() {
    let root = make_temp_dir("sandbox-escape");
    let projects_root = root.join("Projects");
    let state_root = root.join("state");
    let demo_dir = projects_root.join("demo-app");
    fs::create_dir_all(&demo_dir).unwrap();
    fs::create_dir_all(&state_root).unwrap();
    fs::write(demo_dir.join("README.md"), "safe content\n").unwrap();

    let cfg = ConnectorConfig::new(
        "http://127.0.0.1:59998",
        "conn-alpha",
        "secret-token-5",
        projects_root,
        state_root,
    );
    let client = ConnectorClient::new(cfg).expect("create client");

    for forbidden_path in [
        "../../.ssh/id_rsa",
        ".anet/identities",
        ".private_keys/root.pem",
        "/etc/passwd",
    ] {
        let req = ConnectorRpcFrame::new_request(
            1,
            "req-escape",
            "workspace.readFile",
            json!({
                "projectSlug": "demo-app",
                "path": forbidden_path,
            })
            .to_string(),
        );
        let resp = client
            .handler()
            .handle_frame(&req)
            .expect("handle workspace.readFile");
        assert_eq!(
            resp.error_code.as_deref(),
            Some("ProjectDirBoundaryError"),
            "path {forbidden_path} must be rejected with ProjectDirBoundaryError"
        );
        assert!(resp.result_json.is_none());
    }

    // Valid read inside demo-app succeeds, and workspace.exec runs pinned via libc::fchdir.
    let ok_read = ConnectorRpcFrame::new_request(
        1,
        "req-ok-read",
        "workspace.readFile",
        json!({
            "projectSlug": "demo-app",
            "path": "README.md",
        })
        .to_string(),
    );
    let ok_resp = client.handler().handle_frame(&ok_read).unwrap();
    assert!(ok_resp.error_code.is_none());

    let exec_req = ConnectorRpcFrame::new_request(
        1,
        "req-exec",
        "workspace.exec",
        json!({
            "projectSlug": "demo-app",
            "program": "/bin/pwd",
            "args": [],
        })
        .to_string(),
    );
    let exec_resp = client.handler().handle_frame(&exec_req).unwrap();
    let exec_val: Value = serde_json::from_str(exec_resp.result_json.as_deref().unwrap()).unwrap();
    assert_eq!(exec_val["exitCode"], 0);

    // Interrogate finding: dangling symlink inside project pointing to nonexistent file outside
    // project root must be rejected on `workspace.writeFile` with ProjectDirBoundaryError.
    let outside_target = root.join("outside-pwned.txt");
    let dangling_link = demo_dir.join("dangling_out.txt");
    std::os::unix::fs::symlink(&outside_target, &dangling_link).unwrap();
    let dangling_write = ConnectorRpcFrame::new_request(
        1,
        "req-dangling-write",
        "workspace.writeFile",
        json!({
            "projectSlug": "demo-app",
            "path": "dangling_out.txt",
            "content": "should never write outside",
        })
        .to_string(),
    );
    let dangling_resp = client.handler().handle_frame(&dangling_write).unwrap();
    assert_eq!(
        dangling_resp.error_code.as_deref(),
        Some("ProjectDirBoundaryError"),
        "dangling symlink write must fail closed with ProjectDirBoundaryError"
    );
    assert!(
        !outside_target.exists(),
        "outside target must never be created through a dangling symlink"
    );

    println!(
        "SANDBOX_ESCAPE_REJECTED_OK: blocked=[../../.ssh/id_rsa, .anet/identities, dangling_symlink] fchdir_pwd={}",
        exec_val["stdout"].as_str().unwrap_or("").trim()
    );
}

#[test]
fn zero_python_or_anywhere_cli_subprocesses_in_connector_tree() {
    let root = make_temp_dir("zero-python");
    let projects_root = root.join("Projects");
    let state_root = root.join("state");
    fs::create_dir_all(&projects_root).unwrap();
    fs::create_dir_all(&state_root).unwrap();

    let ping_req = ConnectorRpcFrame::new_request(1, "ping-1", "runtime.ping", "{}");
    let (server_url, _http_bodies, _ws_frames, handle) = spawn_mock_v2_server(
        1,
        r#"{"accepted":0,"rejected":[]}"#.to_string(),
        vec![ping_req],
    );

    let mut cfg = ConnectorConfig::new(
        &server_url,
        "conn-pure-rust",
        "tok-rust",
        projects_root,
        state_root,
    );
    cfg.request_timeout = Duration::from_secs(5);
    let out = fleet_daemon::run_connector_daemon(cfg, true, Some(1)).expect("run connector daemon");
    handle.join().expect("server thread");

    assert_eq!(out["ok"], true);
    assert_eq!(out["handledRpcs"], 1);
    let pid = std::process::id().to_string();
    let ps_children = Command::new("/usr/bin/pgrep")
        .args(["-P", &pid, "-fl", "anywhere-cli|python"])
        .output()
        .expect("run pgrep");
    let child_matches = String::from_utf8_lossy(&ps_children.stdout)
        .trim()
        .to_string();
    assert!(
        child_matches.is_empty(),
        "expected zero python or anywhere-cli child processes, found: {child_matches}"
    );
    println!("ZERO_PYTHON_OK: connector_pid={pid} python_children=0 handled_rpcs=1");
}

#[test]
fn critical_semaphore_lane_completes_interaction_respond_without_starving_behind_standard_lane() {
    let root = make_temp_dir("semaphores");
    let projects_root = root.join("Projects");
    let state_root = root.join("state");
    fs::create_dir_all(&projects_root).unwrap();
    fs::create_dir_all(&state_root).unwrap();

    let cfg = ConnectorConfig::new(
        "http://127.0.0.1:59997",
        "conn-alpha",
        "secret-token-6",
        projects_root,
        state_root,
    );
    let client = ConnectorClient::new(cfg).expect("create client");
    let handler = client.handler().clone();

    let barrier = Arc::new(Barrier::new(15));
    let max_standard_observed = Arc::new(AtomicUsize::new(0));
    let mut standard_handles = Vec::new();

    for i in 0..12 {
        let h = handler.clone();
        let b = Arc::clone(&barrier);
        let max_obs = Arc::clone(&max_standard_observed);
        standard_handles.push(thread::spawn(move || {
            b.wait();
            let frame = ConnectorRpcFrame::new_request(
                1,
                format!("std-{i}"),
                "runtime.ping",
                json!({ "simulateHoldMs": 120 }).to_string(),
            );
            let resp = h.handle_frame(&frame).expect("standard RPC");
            let cur = h.gate().standard().in_flight();
            max_obs.fetch_max(cur, Ordering::SeqCst);
            resp
        }));
    }

    let mut critical_handles = Vec::new();
    for i in 0..2 {
        let h = handler.clone();
        let b = Arc::clone(&barrier);
        critical_handles.push(thread::spawn(move || {
            b.wait();
            // Wait 10ms so all 8 standard slots are guaranteed occupied by 120ms holds.
            thread::sleep(Duration::from_millis(10));
            let t0 = Instant::now();
            let frame = ConnectorRpcFrame::new_request(
                1,
                format!("crit-{i}"),
                "interaction.respond",
                json!({
                    "noticeId": format!("notice-{i}"),
                    "sessionId": "conn-alpha:zcode:sess-1",
                    "decision": "approved",
                })
                .to_string(),
            );
            let resp = h.handle_frame(&frame).expect("critical RPC");
            (t0.elapsed(), resp)
        }));
    }

    barrier.wait();

    for ch in critical_handles {
        let (elapsed, resp) = ch.join().expect("critical join");
        assert!(
            elapsed < Duration::from_millis(60),
            "critical interaction.respond took {elapsed:?}, should not starve behind 120ms standard holds"
        );
        let val: Value = serde_json::from_str(resp.result_json.as_deref().unwrap()).unwrap();
        assert_eq!(val["lane"], "critical");
    }

    for sh in standard_handles {
        let resp = sh.join().expect("standard join");
        assert!(resp.error_code.is_none());
    }

    assert!(
        max_standard_observed.load(Ordering::SeqCst) <= 8,
        "standard semaphore must never exceed 8 concurrent permits"
    );
    println!("SEMAPHORES_OK: standard_cap=8 critical_cap=2 critical_starvation=false");
}

#[test]
fn reconnect_fences_stale_generation_and_flushes_pending_outbox_via_http_ingest() {
    let root = make_temp_dir("reconnect-fence");
    let projects_root = root.join("Projects");
    let state_root = root.join("state");
    fs::create_dir_all(&projects_root).unwrap();
    fs::create_dir_all(&state_root).unwrap();

    // First server instance leases generation 10 and then drops.
    let (server_url_1, _http_1, _ws_1, handle_1) = spawn_mock_v2_server(
        10,
        r#"{"accepted":0,"rejected":[]}"#.to_string(),
        Vec::new(),
    );

    let cfg = ConnectorConfig::new(
        &server_url_1,
        "conn-alpha",
        "secret-token-7",
        projects_root.clone(),
        state_root.clone(),
    );
    let mut client = ConnectorClient::new(cfg).expect("create client");
    let gen1 = client.connect().expect("initial connect");
    assert_eq!(gen1, 10);
    handle_1.join().expect("server 1 join");

    // Queue pending outbox items while disconnected.
    client.queue_outbox_items(vec![
        sample_timeline_item("conn-alpha", "zcode", "sess-reconnect", "item-r1", 1, "turn 1"),
        sample_timeline_item("conn-alpha", "zcode", "sess-reconnect", "item-r2", 2, "turn 2"),
    ]);
    assert_eq!(client.pending_outbox_len(), 2);

    // Restart mock server with generation 11 ( accepting WS handshake + HTTP outbox flush ).
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind server 2");
    let addr2 = listener.local_addr().unwrap();
    let server_url_2 = format!("http://127.0.0.1:{}", addr2.port());
    let flushed_batches = Arc::new(Mutex::new(Vec::new()));
    let flushed_clone = Arc::clone(&flushed_batches);

    let handle_2 = thread::spawn(move || {
        for _ in 0..2 {
            if let Ok((stream, _)) = listener.accept() {
                let mut peek_buf = [0u8; 256];
                let n = stream.peek(&mut peek_buf).unwrap_or(0);
                let head = String::from_utf8_lossy(&peek_buf[..n]);
                if head.starts_with("GET /api/v2/connector/ws") {
                    let mut ws = accept(stream).expect("accept ws");
                    if let Ok(Message::Text(hello_raw)) = ws.read() {
                        let hello: ConnectorRpcFrame = serde_json::from_str(&hello_raw).unwrap();
                        let welcome = ConnectorRpcFrame::new_response(
                            11,
                            hello.id.unwrap_or_else(|| "hello".into()),
                            json!({"ok": true, "leasedGeneration": 11}).to_string(),
                        );
                        let _ = ws.send(Message::Text(
                            serde_json::to_string(&welcome).unwrap().into(),
                        ));
                    }
                } else if head.starts_with("POST /api/v2/connector/ingest") {
                    let mut s = stream;
                    let mut reader = BufReader::new(&mut s);
                    let mut content_len = 0usize;
                    loop {
                        let mut line = String::new();
                        if reader.read_line(&mut line).unwrap_or(0) == 0
                            || line == "\r\n"
                            || line == "\n"
                        {
                            break;
                        }
                        if let Some((k, v)) = line.split_once(':') {
                            if k.trim().eq_ignore_ascii_case("content-length") {
                                content_len = v.trim().parse::<usize>().unwrap_or(0);
                            }
                        }
                    }
                    let mut body = vec![0u8; content_len];
                    let _ = reader.read_exact(&mut body);
                    flushed_clone
                        .lock()
                        .unwrap()
                        .push(String::from_utf8_lossy(&body).into_owned());
                    let resp_body = r#"{"accepted":2,"rejected":[]}"#;
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        resp_body.len(),
                        resp_body
                    );
                    let _ = s.write_all(resp.as_bytes());
                }
            }
        }
    });

    // Update client's target URL to server 2 and reconnect.
    let mut cfg2 = ConnectorConfig::new(
        &server_url_2,
        "conn-alpha",
        "secret-token-7",
        projects_root,
        state_root,
    );
    cfg2.request_timeout = Duration::from_secs(5);
    let mut reconnected_client = ConnectorClient::new(cfg2).unwrap();
    reconnected_client.queue_outbox_items(vec![
        sample_timeline_item("conn-alpha", "zcode", "sess-reconnect", "item-r1", 1, "turn 1"),
        sample_timeline_item("conn-alpha", "zcode", "sess-reconnect", "item-r2", 2, "turn 2"),
    ]);
    let gen2 = reconnected_client.connect().expect("reconnect and flush outbox");
    handle_2.join().expect("server 2 join");

    assert_eq!(gen2, 11);
    assert_eq!(reconnected_client.pending_outbox_len(), 0);
    assert_eq!(flushed_batches.lock().unwrap().len(), 1);

    // Verify stale generation 10 frame is fenced!
    let stale_frame =
        ConnectorRpcFrame::new_request(10, "stale-req-1", "runtime.ping", "{}".to_string());
    let err = reconnected_client
        .handler()
        .handle_frame(&stale_frame)
        .expect_err("stale generation 10 must be fenced");
    assert!(matches!(
        err,
        ConnectorError::StaleGeneration {
            active: 11,
            received: 10
        }
    ));
    println!("RECONNECT_FENCE_OK: new_generation={gen2} stale_generation_fenced=10 outbox_flushed=2");
}

#[test]
fn bench_connector_rpc_roundtrip() {
    let root = make_temp_dir("bench-rpc");
    let projects_root = root.join("Projects");
    let state_root = root.join("state");
    fs::create_dir_all(&projects_root).unwrap();
    fs::create_dir_all(&state_root).unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind bench server");
    let addr = listener.local_addr().unwrap();
    let server_url = format!("http://127.0.0.1:{}", addr.port());

    let server_thread = thread::spawn(move || {
        if let Ok((stream, _)) = listener.accept() {
            let _ = stream.set_nodelay(true);
            let mut ws = accept(stream).expect("accept ws");
            if let Ok(Message::Text(hello_raw)) = ws.read() {
                let hello: ConnectorRpcFrame = serde_json::from_str(&hello_raw).unwrap();
                let welcome = ConnectorRpcFrame::new_response(
                    1,
                    hello.id.unwrap_or_else(|| "hello-1".into()),
                    r#"{"ok":true,"leasedGeneration":1}"#.to_string(),
                );
                let _ = ws.send(Message::Text(
                    serde_json::to_string(&welcome).unwrap().into(),
                ));
            }
            for _ in 0..1_000 {
                let Ok(Message::Text(req_raw)) = ws.read() else {
                    break;
                };
                let req: ConnectorRpcFrame = serde_json::from_str(&req_raw).unwrap();
                let resp = ConnectorRpcFrame::new_response(
                    1,
                    req.id.unwrap_or_else(|| "0".into()),
                    r#"{"ok":true}"#.to_string(),
                );
                let _ = ws.send(Message::Text(
                    serde_json::to_string(&resp).unwrap().into(),
                ));
            }
        }
    });

    let cfg = ConnectorConfig::new(
        &server_url,
        "conn-bench",
        "bench-token",
        projects_root,
        state_root,
    );
    let mut client = ConnectorClient::new(cfg).expect("create client");
    client.connect().expect("connect bench");

    let mut samples_us = Vec::with_capacity(1_000);
    for _ in 0..1_000 {
        let t0 = Instant::now();
        let resp = client
            .send_rpc_roundtrip("runtime.ping", "{}".to_string())
            .expect("rpc roundtrip");
        assert!(resp.error_code.is_none());
        samples_us.push(t0.elapsed().as_micros() as u64);
    }
    server_thread.join().expect("server thread");

    samples_us.sort_unstable();
    let median_ms = samples_us[samples_us.len() / 2] as f64 / 1000.0;
    let p99_ms = samples_us[(samples_us.len() * 99) / 100] as f64 / 1000.0;

    let pid = std::process::id().to_string();
    let ps_out = Command::new("/bin/ps")
        .args(["-o", "rss=", "-p", &pid])
        .output()
        .expect("run ps");
    let rss_kib: f64 = String::from_utf8_lossy(&ps_out.stdout)
        .trim()
        .parse()
        .unwrap_or(0.0);
    let rss_mib = rss_kib / 1024.0;

    println!(
        "BENCH_CONNECTOR_RPC_ROUNDTRIP: rounds=1000 median_ms={median_ms:.3} p99_ms={p99_ms:.3} rss_mib={rss_mib:.2} limit_p99_ms=3.0 limit_rss_mib=35.0"
    );
    assert!(
        p99_ms < 3.0,
        "P99 connector RPC roundtrip {p99_ms:.3} ms exceeded 3.0 ms budget"
    );
    assert!(
        rss_mib < 35.0,
        "Connector RSS {rss_mib:.2} MiB exceeded 35.0 MiB budget"
    );
}
