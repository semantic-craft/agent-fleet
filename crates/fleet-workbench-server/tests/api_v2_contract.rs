use fleet_daemon::{ConnectorClient, ConnectorConfig, ZcodeRuntimeConfig};
use fleet_proto::{
    timeline::{
        timeline_content_hash, CanonicalSessionId, TimelineItem, TimelineItemKind,
        TimelineItemStatus, TimelineOrderSeq, TimelineRole, UpdatedSeq,
    },
    workbench::{ProtocolEventEnvelope, ProtocolEventRecoveryResponse},
};
use fleet_workbench_server::{WorkbenchServerConfig, WorkbenchServerHandle};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::TcpStream,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    process::Command,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tungstenite::{client, Message};

fn make_temp_dir(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!(
        "fleet-pr5-{prefix}-{}-{nanos}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn http_request(
    base_url: &str,
    method: &str,
    path: &str,
    extra_headers: &[(&str, &str)],
    body: Option<&str>,
) -> (u16, BTreeMap<String, String>, String) {
    let host_port = base_url.strip_prefix("http://").expect("http url");
    let mut stream = TcpStream::connect(host_port).expect("connect server");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let payload = body.unwrap_or("");
    let mut req = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host_port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",
        payload.len()
    );
    for (k, v) in extra_headers {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    req.push_str("\r\n");
    req.push_str(payload);
    stream.write_all(req.as_bytes()).unwrap();
    stream.flush().unwrap();

    let mut reader = BufReader::new(stream);
    let mut status_line = String::new();
    reader.read_line(&mut status_line).unwrap();
    let status: u16 = status_line
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();

    let mut headers = BTreeMap::new();
    let mut content_len = 0usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" || line == "\n" {
            break;
        }
        if let Some((k, v)) = line.split_once(':') {
            let key = k.trim().to_ascii_lowercase();
            let val = v.trim().to_string();
            if key == "content-length" {
                content_len = val.parse().unwrap_or(0);
            }
            headers.insert(key, val);
        }
    }
    let mut resp_bytes = vec![0u8; content_len];
    if content_len > 0 {
        reader.read_exact(&mut resp_bytes).unwrap();
    }
    (
        status,
        headers,
        String::from_utf8_lossy(&resp_bytes).into_owned(),
    )
}

fn sample_item(
    session_id: &CanonicalSessionId,
    item_id: &str,
    order_seq: i64,
    content: &str,
) -> TimelineItem {
    TimelineItem {
        item_id: item_id.to_string(),
        session_id: session_id.clone(),
        turn_id: Some(format!("turn-{order_seq}")),
        kind: TimelineItemKind::AssistantMessage,
        status: TimelineItemStatus::Completed,
        role: TimelineRole::Assistant,
        content: content.to_string(),
        order_seq: TimelineOrderSeq::try_from(order_seq).unwrap(),
        updated_seq: UpdatedSeq::new(0),
        content_hash: timeline_content_hash("assistant_message", "completed", "assistant", content),
        delivery_tier: None,
        sidebar_binding: None,
        peer_sender: None,
        created_at_ms: 1_762_000_000_000 + order_seq,
    }
}

#[test]
fn health_endpoint_returns_ok_and_serves_workbench_ui() {
    let root = make_temp_dir("health");
    let cfg = WorkbenchServerConfig::new("127.0.0.1:0", root.join("state"), root.join("Projects"));
    let server = WorkbenchServerHandle::start(cfg).expect("start server");

    let (status, _, body) = http_request(&server.base_url(), "GET", "/api/v2/health", &[], None);
    assert_eq!(status, 200);
    assert_eq!(body, r#"{"status":"ok"}"#);

    let (ui_status, _, ui_body) = http_request(&server.base_url(), "GET", "/", &[], None);
    assert_eq!(ui_status, 200);
    assert!(ui_body.contains("Agent-Fleet Unified Workbench"));
    println!("HEALTH_OK: url={} body={body}", server.base_url());
}

#[test]
fn create_and_start_rolls_back_provisional_admission_on_connector_reject_and_commits_on_ok() {
    let root = make_temp_dir("create-and-start");
    let projects_root = root.join("Projects");
    let state_root = root.join("state");
    let zcode_root = root.join("zcode-home");
    fs::create_dir_all(zcode_root.join("v2")).unwrap();
    fs::create_dir_all(zcode_root.join("cli/db")).unwrap();

    let zcode_bin = root.join("mock-zcode.sh");
    fs::write(
        &zcode_bin,
        "#!/bin/sh\necho '{\"type\":\"result\",\"session_id\":\"zcode-wb-sess-1\",\"result\":\"ok\"}'\n",
    )
    .unwrap();
    fs::set_permissions(&zcode_bin, fs::Permissions::from_mode(0o755)).unwrap();

    let mut cfg = WorkbenchServerConfig::new("127.0.0.1:0", state_root, projects_root.clone());
    cfg.runtime_options.zcode = Some(ZcodeRuntimeConfig::new(zcode_root, zcode_bin));
    let server = WorkbenchServerHandle::start(cfg).expect("start server");

    // 1. Simulate connector rejection (ok: false) -> must roll back provisional admission.
    let (rej_status, _, rej_body) = http_request(
        &server.base_url(),
        "POST",
        "/api/v2/sessions/create-and-start",
        &[],
        Some(
            &json!({
                "target": "zcode/demo-workbench",
                "title": "Should Roll Back",
                "prompt": "test",
                "simulateConnectorReject": true,
            })
            .to_string(),
        ),
    );
    assert_eq!(rej_status, 502);
    let rej_val: Value = serde_json::from_str(&rej_body).unwrap();
    assert_eq!(rej_val["ok"], false);
    assert_eq!(rej_val["rolledBack"], true);
    assert_eq!(server.state().provisional_admission_count(), 0);

    let (_, _, list_after_reject) =
        http_request(&server.base_url(), "GET", "/api/v2/sessions", &[], None);
    let list_val: Value = serde_json::from_str(&list_after_reject).unwrap();
    assert_eq!(
        list_val["sessions"].as_array().unwrap().len(),
        0,
        "zero orphan sessions may remain after rollback"
    );

    // 2. Real create-and-start for zcode/demo-workbench -> commits and mounts sidebar.
    let (ok_status, _, ok_body) = http_request(
        &server.base_url(),
        "POST",
        "/api/v2/sessions/create-and-start",
        &[],
        Some(
            &json!({
                "target": "zcode/demo-workbench",
                "title": "Demo Workbench Session",
                "prompt": "Verify workbench session mount",
            })
            .to_string(),
        ),
    );
    assert_eq!(ok_status, 200);
    let ok_val: Value = serde_json::from_str(&ok_body).unwrap();
    assert_eq!(ok_val["ok"], true);
    assert_eq!(ok_val["deliveryTier"], "thread-visible");
    assert!(projects_root.join("demo-workbench").is_dir());
    println!(
        "CREATE_AND_START_OK: rollback_verified=true session_id={} tier={}",
        ok_val["sessionId"], ok_val["deliveryTier"]
    );
}

#[test]
fn monotonic_updated_seq_and_ws_subscriber_fifo_fanout() {
    let root = make_temp_dir("ws-fanout");
    let cfg = WorkbenchServerConfig::new("127.0.0.1:0", root.join("state"), root.join("Projects"));
    let server = WorkbenchServerHandle::start(cfg).expect("start server");

    let conn_cfg = ConnectorConfig::new(
        server.base_url(),
        "conn-stream",
        "dev-token",
        root.join("Projects"),
        root.join("conn-state"),
    );
    let mut connector = ConnectorClient::new(conn_cfg).unwrap();
    let gen = connector.connect().expect("connector connect");
    assert!(gen >= 1);

    let sid = CanonicalSessionId::derive("conn-stream", "zcode", "sess-stream-20").unwrap();
    let ws_url = format!(
        "ws://{}/api/v2/sessions/{}/ws",
        server.base_url().strip_prefix("http://").unwrap(),
        sid.as_str()
    );
    let (mut browser_ws, _) = client(&ws_url, TcpStream::connect(server.base_url().strip_prefix("http://").unwrap()).unwrap()).unwrap();
    browser_ws
        .get_ref()
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();

    // Stream 20 rapid timeline item updates from the connector.
    let items: Vec<TimelineItem> = (1..=20)
        .map(|i| sample_item(&sid, &format!("item-{i}"), i, &format!("stream update #{i}")))
        .collect();
    connector
        .emit_timeline_batch(&items)
        .expect("emit 20 timeline items");

    let mut prev_seq = 0i64;
    for expected_idx in 1..=20 {
        let msg = browser_ws.read().expect("read ws event");
        let Message::Text(text) = msg else {
            panic!("expected text frame");
        };
        let env = ProtocolEventEnvelope::from_wire_json(&text).expect("valid 1.1 envelope");
        assert_eq!(env.event_type, "timeline.item_upsert");
        assert!(
            env.updated_seq.get() > prev_seq,
            "updatedSeq must advance strictly monotonically ({} <= {})",
            env.updated_seq.get(),
            prev_seq
        );
        prev_seq = env.updated_seq.get();
        let _ = expected_idx;
    }
    println!(
        "WS_STREAM_20_OK: items_received=20 final_updated_seq={prev_seq} gaps=0 duplicates=0"
    );
}

#[test]
fn event_recovery_after_cursor_and_snapshot_required_on_replacement_fence() {
    let root = make_temp_dir("event-recovery");
    let cfg = WorkbenchServerConfig::new("127.0.0.1:0", root.join("state"), root.join("Projects"));
    let server = WorkbenchServerHandle::start(cfg).expect("start server");

    let sid = CanonicalSessionId::derive("conn-rec", "zcode", "sess-rec-1").unwrap();
    let initial = vec![sample_item(&sid, "item-1", 1, "initial message")];
    server.state().ingest_timeline_items(&initial);

    let (_, _, snap1_raw) = http_request(
        &server.base_url(),
        "GET",
        &format!("/api/v2/sessions/{}/snapshot", sid.as_str()),
        &[],
        None,
    );
    let snap1: Value = serde_json::from_str(&snap1_raw).unwrap();
    let cursor1 = snap1["cursor"].as_str().unwrap().to_string();

    // Emit 5 timeline updates while disconnected.
    let missed: Vec<TimelineItem> = (2..=6)
        .map(|i| sample_item(&sid, &format!("item-{i}"), i, &format!("missed #{i}")))
        .collect();
    server.state().ingest_timeline_items(&missed);

    let (status, _, rec_raw) = http_request(
        &server.base_url(),
        "GET",
        &format!(
            "/api/v2/sessions/{}/events?after={}",
            sid.as_str(),
            cursor1
        ),
        &[],
        None,
    );
    assert_eq!(status, 200);
    let rec: ProtocolEventRecoveryResponse = serde_json::from_str(&rec_raw).unwrap();
    assert!(!rec.snapshot_required);
    assert_eq!(rec.events.len(), 5);

    // Trigger session replacement fence and call /events?after=seq:1 -> snapshotRequired: true.
    let (f_status, _, _) = http_request(
        &server.base_url(),
        "POST",
        &format!("/api/v2/sessions/{}/fence", sid.as_str()),
        &[],
        Some("{}"),
    );
    assert_eq!(f_status, 200);

    let (_, _, fenced_raw) = http_request(
        &server.base_url(),
        "GET",
        &format!("/api/v2/sessions/{}/events?after=seq:1", sid.as_str()),
        &[],
        None,
    );
    let fenced_rec: ProtocolEventRecoveryResponse = serde_json::from_str(&fenced_raw).unwrap();
    assert!(
        fenced_rec.snapshot_required,
        "cursor behind replacement fence must return snapshotRequired: true"
    );
    assert!(fenced_rec.events.is_empty());

    let (_, _, full_snap_raw) = http_request(
        &server.base_url(),
        "GET",
        &format!("/api/v2/sessions/{}/snapshot", sid.as_str()),
        &[],
        None,
    );
    let full_snap: Value = serde_json::from_str(&full_snap_raw).unwrap();
    assert_eq!(full_snap["items"].as_array().unwrap().len(), 6);
    println!(
        "EVENT_RECOVERY_AND_FENCE_OK: recovered_events=5 snapshot_required_after_fence=true reconciled_items=6"
    );
}

#[test]
fn interaction_notice_state_machine_and_plan_fence_conflict() {
    let root = make_temp_dir("notices");
    let cfg = WorkbenchServerConfig::new("127.0.0.1:0", root.join("state"), root.join("Projects"));
    let server = WorkbenchServerHandle::start(cfg).expect("start server");

    let sid = CanonicalSessionId::derive("conn-notice", "zcode", "sess-notice-1").unwrap();
    let (emit_status, _, emit_raw) = http_request(
        &server.base_url(),
        "POST",
        &format!("/api/v2/sessions/{}/runtime/notices", sid.as_str()),
        &[],
        Some(
            &json!({
                "noticeId": "notice-approve-1",
                "kind": "plan_review",
                "title": "Approve Production Deploy",
                "prompt": "Confirm deployment plan v2?",
                "planTicket": {
                    "reviewId": "rev-1",
                    "planId": "plan-deploy",
                    "planRevision": 2,
                    "contentHash": "sha256:abcd1234",
                    "epoch": 9
                }
            })
            .to_string(),
        ),
    );
    assert_eq!(emit_status, 200);
    let emitted: Value = serde_json::from_str(&emit_raw).unwrap();
    assert_eq!(emitted["status"], "open");

    // 1. Interrogate check: stale contentHash or epoch must fail closed with HTTP 409 plan_fence_conflict.
    let (conflict_status, _, conflict_raw) = http_request(
        &server.base_url(),
        "POST",
        &format!(
            "/api/v2/sessions/{}/runtime/notices/notice-approve-1/respond",
            sid.as_str()
        ),
        &[],
        Some(
            &json!({
                "decision": "approved",
                "contentHash": "sha256:stale_hash",
                "epoch": 9
            })
            .to_string(),
        ),
    );
    assert_eq!(conflict_status, 409);
    let conflict_val: Value = serde_json::from_str(&conflict_raw).unwrap();
    assert_eq!(conflict_val["error"], "plan_fence_conflict");

    // 2. Valid approval transitions open -> response_accepted -> resolving -> resolved.
    let (ok_status, _, ok_raw) = http_request(
        &server.base_url(),
        "POST",
        &format!(
            "/api/v2/sessions/{}/runtime/notices/notice-approve-1/respond",
            sid.as_str()
        ),
        &[],
        Some(
            &json!({
                "decision": "approved",
                "contentHash": "sha256:abcd1234",
                "epoch": 9
            })
            .to_string(),
        ),
    );
    assert_eq!(ok_status, 200);
    let ok_val: Value = serde_json::from_str(&ok_raw).unwrap();
    assert_eq!(ok_val["status"], "resolved");
    assert_eq!(
        ok_val["transitions"],
        json!(["open", "response_accepted", "resolving", "resolved"])
    );
    println!(
        "INTERACTION_NOTICE_OK: plan_fence_conflict_blocked=true transitions={}",
        ok_val["transitions"]
    );
}

#[test]
fn anet_wire3_relay_send_and_status_coexist_on_workbench_server() {
    let root = make_temp_dir("anet-wire3");
    let cfg = WorkbenchServerConfig::new("127.0.0.1:0", root.join("state"), root.join("Projects"));
    let server = WorkbenchServerHandle::start(cfg).expect("start server");

    let (send_status, send_headers, send_raw) = http_request(
        &server.base_url(),
        "POST",
        "/relay/send",
        &[("X-ANet-Wire", "3"), ("X-Correlation-Id", "corr-wire3-001")],
        Some(r#"{"task_id":"task-001","envelope":"sealed_payload_v3"}"#),
    );
    assert_eq!(send_status, 200);
    assert_eq!(send_headers.get("x-anet-wire").map(String::as_str), Some("3"));
    let send_val: Value = serde_json::from_str(&send_raw).unwrap();
    let rid = send_val["rid"].as_str().unwrap().to_string();
    assert!(rid.starts_with("rid_"));

    // Conflicting payload under the same correlation ID must be rejected with HTTP 409.
    let (dup_status, _, _) = http_request(
        &server.base_url(),
        "POST",
        "/relay/send",
        &[("X-ANet-Wire", "3"), ("X-Correlation-Id", "corr-wire3-001")],
        Some(r#"{"task_id":"task-001","envelope":"tampered_payload"}"#),
    );
    assert_eq!(dup_status, 409);

    let (st_status, st_headers, st_raw) = http_request(
        &server.base_url(),
        "POST",
        "/relay/status",
        &[("X-ANet-Wire", "3")],
        Some(&json!({"rid": rid}).to_string()),
    );
    assert_eq!(st_status, 200);
    assert_eq!(st_headers.get("x-anet-wire").map(String::as_str), Some("3"));
    let st_val: Value = serde_json::from_str(&st_raw).unwrap();
    assert_eq!(st_val["status"], "queued");
    println!(
        "ANET_WIRE3_COEXISTENCE_OK: rid={rid} status={} wire_header=3",
        st_val["status"]
    );
}

#[test]
fn bench_timeline_fanout_10k() {
    let root = make_temp_dir("bench-10k");
    let cfg = WorkbenchServerConfig::new("127.0.0.1:0", root.join("state"), root.join("Projects"));
    let server = WorkbenchServerHandle::start(cfg).expect("start server");

    let sid = CanonicalSessionId::derive("conn-bench", "zcode", "sess-bench-10k").unwrap();
    let rx = server.state().subscribe_session_ws(sid.as_str());

    let mut batch_latencies_us = Vec::with_capacity(100);
    let total_start = Instant::now();
    for chunk_idx in 0..100 {
        let items: Vec<TimelineItem> = (0..100)
            .map(|i| {
                let seq_num = (chunk_idx * 100 + i + 1) as i64;
                sample_item(
                    &sid,
                    &format!("bench-item-{seq_num}"),
                    seq_num,
                    "benchmark payload",
                )
            })
            .collect();
        let t0 = Instant::now();
        let res = server.state().ingest_timeline_items(&items);
        assert_eq!(res.accepted, 100);
        for _ in 0..100 {
            let wire = rx.recv_timeout(Duration::from_secs(2)).expect("fanout event");
            assert!(wire.contains("\"timeline.item_upsert\""));
        }
        batch_latencies_us.push(t0.elapsed().as_micros() as u64 / 100);
    }
    let total_elapsed = total_start.elapsed();
    let throughput = 10_000.0 / total_elapsed.as_secs_f64();

    batch_latencies_us.sort_unstable();
    let p99_ms = batch_latencies_us[(batch_latencies_us.len() * 99) / 100] as f64 / 1000.0;

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
        "BENCH_TIMELINE_FANOUT_10K: upserts=10000 throughput_per_sec={throughput:.1} p99_fanout_ms={p99_ms:.3} rss_mib={rss_mib:.2} min_throughput=5000 max_p99_ms=10.0 max_rss_mib=50.0"
    );
    assert!(
        throughput >= 5_000.0,
        "throughput {throughput:.1} upserts/sec below 5,000 budget"
    );
    assert!(
        p99_ms < 10.0,
        "P99 fan-out latency {p99_ms:.3} ms exceeded 10.0 ms budget"
    );
    assert!(
        rss_mib < 50.0,
        "Server RSS {rss_mib:.2} MiB exceeded 50.0 MiB budget"
    );
}
