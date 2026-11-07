use fleet_daemon::{
    ensure_no_active_native_writer, extract_fleet_message_metadata, project_host_timelines,
    AntigravityTimelineObserver, CindyTimelineObserver, DeepSeekHarnessTimelineObserver,
    DesktopHostAdapter, FleetLedgerStore, GrokBotTimelineObserver, HostAdapterError,
    HostTimelineObserver, NativeWriterLockGuard, OpenAiDotsAdapter, OpenAiDotsRuntimeConfig,
    OpenAiDotsTimelineObserver, ZcodeTimelineObserver,
};
use fleet_proto::{
    CanonicalSessionId, CreateSessionRequest, DeliverTurnRequest, DeliveryTier, DesktopHostKind,
    InboxMessage, TimelineItemKind,
};
use serde_json::json;
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

static NEXT_TMP_ID: AtomicU64 = AtomicU64::new(1);

struct TempDirFixture {
    path: PathBuf,
}

impl TempDirFixture {
    fn new(prefix: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let seq = NEXT_TMP_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "fleet-observer-{prefix}-{}-{nanos}-{seq}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("create temp dir");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDirFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn run_sqlite_ddl(db_path: &Path, sql: &str) {
    if let Some(parent) = db_path.parent() {
        fs::create_dir_all(parent).expect("create sqlite parent");
    }
    let out = Command::new("/usr/bin/sqlite3")
        .arg(db_path)
        .arg(sql)
        .output()
        .expect("run sqlite3");
    assert!(
        out.status.success(),
        "sqlite3 failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn compress_zstd_bytes(input: &[u8]) -> Vec<u8> {
    let zstd_bin = if Path::new("/opt/homebrew/bin/zstd").exists() {
        "/opt/homebrew/bin/zstd"
    } else if Path::new("/usr/local/bin/zstd").exists() {
        "/usr/local/bin/zstd"
    } else {
        "zstd"
    };
    let mut child = Command::new(zstd_bin)
        .arg("-q")
        .arg("-c")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn zstd");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(input)
        .expect("write zstd input");
    let out = child.wait_with_output().expect("wait zstd");
    assert!(
        out.status.success(),
        "zstd failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    out.stdout
}

#[test]
fn openai_dots_incremental_checkpoint_skips_unchanged_turn_bodies() {
    let tmp = TempDirFixture::new("dots");
    let state_root = tmp.path().join("state");
    let codex_root = tmp.path().join("codex");
    fs::create_dir_all(codex_root.join("rollouts")).expect("create rollouts dir");

    let rollout_path = codex_root.join("rollouts").join("thr-dots-1.jsonl");
    let rollout_lines = [
        json!({"id": "u-1", "turn_id": "turn-1", "role": "user", "text": "Inspect the timeline schema"}),
        json!({"id": "a-1", "turn_id": "turn-1", "role": "assistant", "text": "Reading timeline.rs and workbench.rs"}),
        json!({"id": "t-1", "turn_id": "turn-1", "type": "tool_call", "tool": "cargo test", "text": "cargo test -p fleet-proto"}),
    ];
    let mut rollout_body = String::new();
    for line in &rollout_lines {
        rollout_body.push_str(&line.to_string());
        rollout_body.push('\n');
    }
    fs::write(&rollout_path, &rollout_body).expect("write rollout jsonl");

    let db_path = codex_root.join("state_5.sqlite");
    run_sqlite_ddl(
        &db_path,
        &format!(
            "CREATE TABLE projects (id TEXT PRIMARY KEY, name TEXT, position INTEGER);
             CREATE TABLE project_roots (project_id TEXT, path TEXT, position INTEGER);
             CREATE TABLE threads (id TEXT PRIMARY KEY, project_id TEXT, cwd TEXT, title TEXT, preview TEXT, archived INTEGER, rollout_path TEXT);
             INSERT INTO projects VALUES ('proj-dots-alpha', 'Alpha Repo', 0);
             INSERT INTO project_roots VALUES ('proj-dots-alpha', '/fixture/Projects/alpha', 0);
             INSERT INTO threads VALUES ('thr-dots-1', 'proj-dots-alpha', '/fixture/Projects/alpha', 'Timeline Audit', 'Inspect the timeline schema', 0, '{}');",
            rollout_path.display()
        ),
    );

    let ledger = FleetLedgerStore::open(&state_root).expect("open ledger");
    let mut observer = OpenAiDotsTimelineObserver::new("conn-alpha", codex_root.clone());

    let scan1 = observer.project_into_ledger(&ledger).expect("scan 1");
    assert_eq!(scan1.sessions_scanned, 1);
    assert_eq!(scan1.sessions_skipped_unchanged, 0);
    assert_eq!(scan1.turn_files_opened, 1);
    assert!(scan1.turn_bytes_read > 0);
    assert_eq!(scan1.items_emitted, 3);

    // Scan 2 on identical state must skip reading unchanged turn bodies completely
    let scan2 = observer.project_into_ledger(&ledger).expect("scan 2");
    assert_eq!(scan2.sessions_scanned, 1);
    assert_eq!(scan2.sessions_skipped_unchanged, 1);
    assert_eq!(scan2.turn_files_opened, 0);
    assert_eq!(scan2.turn_bytes_read, 0);
    assert_eq!(scan2.items_emitted, 0);

    // Even a fresh observer instance reloads checkpoints from disk and skips reading turn bodies
    let mut observer_reloaded = OpenAiDotsTimelineObserver::new("conn-alpha", codex_root);
    let scan3 = observer_reloaded
        .project_into_ledger(&ledger)
        .expect("scan 3");
    assert_eq!(scan3.sessions_skipped_unchanged, 1);
    assert_eq!(scan3.turn_files_opened, 0);
    assert_eq!(scan3.items_emitted, 0);
}

#[test]
fn deepseek_harness_two_frame_zstd_projects_ordered_timeline_items() {
    let tmp = TempDirFixture::new("dsh");
    let state_root = tmp.path().join("state");
    let dsh_root = tmp.path().join("dsh");

    let ws_dir = dsh_root.join("storages");
    let pc_dir = ws_dir.join("session_projcache").join("sessions");
    fs::create_dir_all(&pc_dir).expect("create dsh storages");

    fs::write(
        ws_dir.join("workspace.json"),
        serde_json::to_vec_pretty(&json!({
            "tables": {
                "workspaces": {
                    "ws-dsh-alpha": {
                        "path": "/fixture/Projects/dsh-alpha",
                        "title": "DSH Alpha",
                        "sessionIds": ["sess-dsh-1"]
                    }
                }
            }
        }))
        .expect("serialize workspace.json"),
    )
    .expect("write workspace.json");

    fs::write(
        pc_dir.join("sess-dsh-1.json"),
        serde_json::to_vec_pretty(&json!({
            "record": {
                "identity": { "cwd": "/fixture/Projects/dsh-alpha" },
                "rows": {
                    "title": { "ver": 1, "seq": 1, "val": "DSH Two-Frame Verification" }
                }
            }
        }))
        .expect("serialize projcache"),
    )
    .expect("write projcache");

    let archive_dir = dsh_root
        .join("sessions")
        .join("--fixture-Projects-dsh-alpha--")
        .join("sess-dsh-1");
    fs::create_dir_all(&archive_dir).expect("create archive dir");
    let archive_path = archive_dir.join("session.v4.jsonl.zstd");

    // Frame 0: session header JSON
    let frame0_json = format!(
        "{}\n",
        json!({"type": "session/header", "version": 4, "sessionId": "sess-dsh-1", "cwd": "/fixture/Projects/dsh-alpha"})
    );
    // Frame 1: multi-line JSONL turn events
    let frame1_jsonl = format!(
        "{}\n{}\n{}\n",
        json!({"type": "session/title", "data": {"title": "DSH Two-Frame Verification"}}),
        json!({"id": "dsh-u1", "turnId": "turn-1", "role": "user", "text": "Verify two-frame zstd decoding"}),
        json!({"id": "dsh-a1", "turnId": "turn-1", "role": "assistant", "text": "Both zstd frames decoded cleanly"})
    );

    let mut two_frame_bytes = compress_zstd_bytes(frame0_json.as_bytes());
    two_frame_bytes.extend_from_slice(&compress_zstd_bytes(frame1_jsonl.as_bytes()));
    fs::write(&archive_path, &two_frame_bytes).expect("write two-frame zstd archive");

    let ledger = FleetLedgerStore::open(&state_root).expect("open ledger");
    let mut observer = DeepSeekHarnessTimelineObserver::new("conn-alpha", dsh_root);
    let report = observer.project_into_ledger(&ledger).expect("project dsh");
    assert_eq!(report.sessions_scanned, 1);
    assert_eq!(report.items_emitted, 2);
    assert!(report.errors.is_empty());

    let canonical_sid =
        CanonicalSessionId::derive("conn-alpha", "deepseek-harness", "sess-dsh-1").unwrap();
    let items = ledger
        .list_timeline_items(canonical_sid.as_str(), None, 50)
        .expect("list dsh items");
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].order_seq.get(), 10);
    assert_eq!(items[0].kind, TimelineItemKind::UserMessage);
    assert_eq!(items[0].delivery_tier, Some(DeliveryTier::ThreadVisible));
    assert_eq!(items[1].order_seq.get(), 20);
    assert_eq!(items[1].kind, TimelineItemKind::AssistantMessage);
    assert_eq!(
        items[1]
            .sidebar_binding
            .as_ref()
            .map(|b| b.project_id.as_str()),
        Some("ws-dsh-alpha")
    );
}

#[test]
fn zcode_observer_extracts_fleet_message_peer_sender_and_thread_visible_tier() {
    let tmp = TempDirFixture::new("zcode");
    let state_root = tmp.path().join("state");
    let zcode_root = tmp.path().join("zcode");

    let tasks_db = zcode_root.join("v2").join("tasks-index.sqlite");
    let cli_db = zcode_root.join("cli").join("db").join("db.sqlite");

    run_sqlite_ddl(
        &tasks_db,
        "CREATE TABLE tasks (
            workspace_key TEXT, workspace_path TEXT, task_id TEXT, title TEXT,
            task_status TEXT, provider TEXT, root_path TEXT, project_id TEXT,
            archived INTEGER, deleted INTEGER
        );
        INSERT INTO tasks VALUES (
            '/fixture/Projects/zcode-repo', '/fixture/Projects/zcode-repo',
            'ses_zcode_peer_1', 'Peer Audit Task', 'normal', 'glm',
            '/fixture/Projects/zcode-repo', 'proj-zcode-peer', 0, 0
        );",
    );

    let fleet_envelope = "<fleet-message from=\"grokbot\" to=\"zcode/zcode-repo\" id=\"deliv-99\" repo=\"/fixture/Projects/zcode-repo\">\nPlease audit observer.rs\n</fleet-message>";
    let msg1_data = json!({
        "role": "user",
        "turn_id": "turn-1",
        "text": fleet_envelope
    })
    .to_string();
    let msg2_data = json!({
        "role": "assistant",
        "turn_id": "turn-1",
        "text": "Audit complete: all read-only flags verified."
    })
    .to_string();

    run_sqlite_ddl(
        &cli_db,
        &format!(
            "CREATE TABLE session (id TEXT PRIMARY KEY, project_id TEXT, directory TEXT, title TEXT);
             CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT, sequence INTEGER, time_created INTEGER, time_updated INTEGER, data TEXT);
             CREATE TABLE part (id TEXT PRIMARY KEY, message_id TEXT, session_id TEXT, time_created INTEGER, time_updated INTEGER, data TEXT);
             INSERT INTO session VALUES ('ses_zcode_peer_1', 'proj_zcode', '/fixture/Projects/zcode-repo', 'Peer Audit Task');
             INSERT INTO message VALUES ('m-1', 'ses_zcode_peer_1', 1, 1700000001000, 1700000001000, '{}');
             INSERT INTO message VALUES ('m-2', 'ses_zcode_peer_1', 2, 1700000002000, 1700000002000, '{}');",
            msg1_data.replace('\'', "''"),
            msg2_data.replace('\'', "''")
        ),
    );

    let parsed = extract_fleet_message_metadata(fleet_envelope).expect("parse fleet-message");
    assert_eq!(parsed.from.as_deref(), Some("grokbot"));
    assert_eq!(parsed.to.as_deref(), Some("zcode/zcode-repo"));
    assert_eq!(parsed.id.as_deref(), Some("deliv-99"));

    let ledger = FleetLedgerStore::open(&state_root).expect("open ledger");
    let mut observer = ZcodeTimelineObserver::new("conn-alpha", zcode_root);
    let report = observer.project_into_ledger(&ledger).expect("project zcode");
    assert_eq!(report.sessions_scanned, 1);
    assert_eq!(report.items_emitted, 2);

    let canonical_sid =
        CanonicalSessionId::derive("conn-alpha", "zcode", "ses_zcode_peer_1").unwrap();
    let items = ledger
        .list_timeline_items(canonical_sid.as_str(), None, 50)
        .expect("list zcode items");
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].kind, TimelineItemKind::PeerMessage);
    assert_eq!(items[0].delivery_tier, Some(DeliveryTier::ThreadVisible));
    assert_eq!(items[0].peer_sender.as_deref(), Some("grokbot"));
    assert_eq!(items[1].kind, TimelineItemKind::AssistantMessage);
    assert_eq!(items[1].delivery_tier, Some(DeliveryTier::ThreadVisible));
}

#[test]
fn antigravity_and_grokbot_observers_map_sidebar_binding_and_peer_mailboxes() {
    let tmp = TempDirFixture::new("agy-grok");
    let state_root = tmp.path().join("state");
    let agy_root = tmp.path().join("agy");
    fs::create_dir_all(&agy_root).expect("create agy root");

    let summaries_db = agy_root.join("conversation_summaries.db");
    run_sqlite_ddl(
        &summaries_db,
        "CREATE TABLE conversation_summaries (
            conversation_id TEXT PRIMARY KEY,
            title TEXT,
            preview TEXT,
            step_count INTEGER,
            last_modified_time TEXT,
            workspace_uris TEXT,
            status TEXT,
            source TEXT,
            project_id TEXT
        );
        INSERT INTO conversation_summaries VALUES (
            'conv-agy-101',
            'AGY Workbench Integration',
            'Verified SidebarProjectBinding mapping',
            4,
            '2026-11-08T00:00:00Z',
            '[\"file:///fixture/Projects/agent-fleet\"]',
            'CASCADE_RUN_STATUS_IDLE',
            'desktop',
            'proj-agy-fleet'
        );",
    );

    let ledger = FleetLedgerStore::open(&state_root).expect("open ledger");
    let mut agy_observer = AntigravityTimelineObserver::new("conn-alpha", agy_root);
    let agy_report = agy_observer
        .project_into_ledger(&ledger)
        .expect("project agy");
    assert_eq!(agy_report.sessions_scanned, 1);
    assert_eq!(agy_report.items_emitted, 1);

    let agy_sid =
        CanonicalSessionId::derive("conn-alpha", "antigravity-desktop", "conv-agy-101").unwrap();
    let agy_meta = ledger
        .get_session(agy_sid.as_str())
        .expect("get agy session")
        .expect("agy session exists");
    let binding = agy_meta.sidebar_binding.expect("agy sidebar binding");
    assert_eq!(binding.host, DesktopHostKind::AntigravityDesktop);
    assert_eq!(binding.project_id, "proj-agy-fleet");
    assert_eq!(
        binding.workspace_uris,
        vec!["file:///fixture/Projects/agent-fleet".to_string()]
    );

    // Enqueue a GrokBot message and project via GrokBotTimelineObserver
    ledger
        .enqueue(InboxMessage {
            id: "grok-msg-1".into(),
            from: "grokbot/agent-fleet".into(),
            to: "zcode/agent-fleet#ses-1".into(),
            reply_to: None,
            project_slug: Some("agent-fleet".into()),
            session_id: Some("ses-1".into()),
            body: "GrokBot peer status ping".into(),
            files: vec![],
            wake: true,
            notify: false,
            created_at_unix: 1_762_000_000,
            delivered: false,
        })
        .expect("enqueue grokbot msg");

    let mut grok_observer = GrokBotTimelineObserver::new("conn-alpha");
    let grok_report = grok_observer
        .project_into_ledger(&ledger)
        .expect("project grokbot");
    assert_eq!(grok_report.sessions_scanned, 1);
    assert_eq!(grok_report.items_emitted, 1);
}

#[test]
fn cindy_incremental_append_emits_only_newly_appended_lines() {
    let tmp = TempDirFixture::new("cindy");
    let state_root = tmp.path().join("state");
    let cindy_root = tmp.path().join("cindy");
    let transcripts_dir = cindy_root.join("transcripts");
    fs::create_dir_all(&transcripts_dir).expect("create transcripts dir");

    let transcript_path = transcripts_dir.join("cindy-sess-1.jsonl");
    let initial = format!(
        "{}\n{}\n",
        json!({"id": "c-1", "turnId": "t-1", "role": "user", "cwd": "/fixture/Projects/cindy-app", "projectId": "proj-cindy-app", "title": "Cindy Session", "text": "Line 1 user prompt"}),
        json!({"id": "c-2", "turnId": "t-1", "role": "assistant", "text": "Line 2 assistant reply"})
    );
    fs::write(&transcript_path, &initial).expect("write initial cindy transcript");

    let ledger = FleetLedgerStore::open(&state_root).expect("open ledger");
    let mut observer = CindyTimelineObserver::new("conn-alpha", cindy_root);

    let scan1 = observer.project_into_ledger(&ledger).expect("scan 1");
    assert_eq!(scan1.items_emitted, 2);

    // Append 2 new lines to the same Cindy transcript JSONL
    let mut file = OpenOptions::new()
        .append(true)
        .open(&transcript_path)
        .expect("open transcript for append");
    writeln!(
        file,
        "{}",
        json!({"id": "c-3", "turnId": "t-2", "role": "user", "text": "Line 3 follow-up"})
    )
    .expect("append line 3");
    writeln!(
        file,
        "{}",
        json!({"id": "c-4", "turnId": "t-2", "role": "assistant", "text": "Line 4 follow-up response"})
    )
    .expect("append line 4");
    file.flush().expect("flush");
    drop(file);

    let scan2 = observer.project_into_ledger(&ledger).expect("scan 2");
    assert_eq!(
        scan2.items_emitted, 2,
        "incremental scan after appending 2 lines must emit only the 2 new items"
    );
    assert_eq!(scan2.emitted_items[0].order_seq.get(), 30);
    assert_eq!(scan2.emitted_items[1].order_seq.get(), 40);
}

#[test]
fn writer_lock_conflict_returns_native_writer_active_before_mutating_session() {
    let tmp = TempDirFixture::new("writer-lock");
    let projects_root = tmp.path().join("projects");
    let state_root = tmp.path().join("state");
    let codex_root = tmp.path().join("codex");
    fs::create_dir_all(&projects_root).expect("create projects_root");
    fs::create_dir_all(&state_root).expect("create state_root");
    fs::create_dir_all(&codex_root).expect("create codex_root");

    let mut adapter =
        OpenAiDotsAdapter::new(projects_root.clone(), state_root.clone()).expect("create adapter");
    let binding = adapter
        .ensure_sidebar_project("lock-demo", Some("Lock Demo"))
        .expect("ensure project");
    let created = adapter
        .create_session(&CreateSessionRequest {
            delivery_id: "deliv-init-1".into(),
            project_id: binding.project_id.clone(),
            title: "Initial Session".into(),
            project_dir: binding.project_dir.clone(),
            prompt: "Initial prompt".into(),
            model: None,
        })
        .expect("create initial session");

    // Simulate an active native writer lock on the session
    let lock_guard =
        NativeWriterLockGuard::acquire(&state_root, Some(&created.session_id)).expect("lock");
    let direct_check = ensure_no_active_native_writer(
        DesktopHostKind::OpenaiDots,
        &state_root,
        Some(&created.session_id),
    );
    assert!(matches!(
        direct_check,
        Err(HostAdapterError::NativeWriterActive { .. })
    ));

    let takeover_attempt = adapter.deliver_turn(&DeliverTurnRequest {
        delivery_id: "deliv-takeover-2".into(),
        project_id: binding.project_id.clone(),
        session_id: created.session_id.clone(),
        project_dir: binding.project_dir.clone(),
        prompt: "Remote takeover prompt while native writer active".into(),
        model: None,
    });
    match takeover_attempt {
        Err(err @ HostAdapterError::NativeWriterActive { .. }) => {
            assert!(err.to_string().contains("NativeWriterActive"));
        }
        other => panic!("expected Err(NativeWriterActive), got {other:?}"),
    }

    // Verify session was not mutated while lock was held
    let session_record = adapter.session(&created.session_id).expect("session exists");
    assert_eq!(session_record.turns.len(), 1);

    drop(lock_guard);
    let after_unlock = adapter
        .deliver_turn(&DeliverTurnRequest {
            delivery_id: "deliv-takeover-2".into(),
            project_id: binding.project_id,
            session_id: created.session_id,
            project_dir: binding.project_dir,
            prompt: "Remote takeover prompt after unlock".into(),
            model: None,
        })
        .expect("deliver turn after unlock");
    assert_eq!(after_unlock.delivery_id, "deliv-takeover-2");
}

#[test]
fn corrupt_session_file_is_isolated_without_blocking_healthy_sessions() {
    let tmp = TempDirFixture::new("fault-isolation");
    let state_root = tmp.path().join("state");
    let cindy_root = tmp.path().join("cindy");
    let transcripts_dir = cindy_root.join("transcripts");
    fs::create_dir_all(&transcripts_dir).expect("create transcripts dir");

    fs::write(
        transcripts_dir.join("01-healthy.jsonl"),
        format!(
            "{}\n",
            json!({"id": "h-1", "role": "user", "text": "Healthy session 1"})
        ),
    )
    .expect("write healthy 1");
    fs::write(
        transcripts_dir.join("02-corrupt.jsonl"),
        "{\"id\": \"bad-1\", \"role\": \"user\", THIS_IS_CORRUPT_JSON\n",
    )
    .expect("write corrupt");
    fs::write(
        transcripts_dir.join("03-healthy.jsonl"),
        format!(
            "{}\n",
            json!({"id": "h-2", "role": "assistant", "text": "Healthy session 3"})
        ),
    )
    .expect("write healthy 3");

    let ledger = FleetLedgerStore::open(&state_root).expect("open ledger");
    let report =
        project_host_timelines(&ledger, "conn-alpha", "cindy", Some(&cindy_root)).expect("scan");
    assert_eq!(report.sessions_scanned, 3);
    assert_eq!(report.items_emitted, 2);
    assert_eq!(report.errors.len(), 1);
    assert_eq!(report.errors[0].session_id, "02-corrupt");
}

#[test]
fn bench_incremental_scan_50_sessions() {
    let tmp = TempDirFixture::new("bench-50");
    let state_root = tmp.path().join("state");
    let cindy_root = tmp.path().join("cindy");
    let transcripts_dir = cindy_root.join("transcripts");
    fs::create_dir_all(&transcripts_dir).expect("create transcripts dir");

    // 50 sessions x 100 turns = 5,000 total turns
    for s in 0..50 {
        let path = transcripts_dir.join(format!("sess-{s:03}.jsonl"));
        let mut buf = String::with_capacity(16 * 1024);
        for t in 0..100 {
            let role = if t % 2 == 0 { "user" } else { "assistant" };
            let line = json!({
                "id": format!("item-{s}-{t}"),
                "turnId": format!("turn-{}", t / 2 + 1),
                "role": role,
                "cwd": "/fixture/Projects/bench-repo",
                "projectId": "proj-cindy-bench",
                "title": format!("Bench Session {s}"),
                "text": format!("Session {s} turn {t} payload body for incremental scan benchmark")
            });
            buf.push_str(&line.to_string());
            buf.push('\n');
        }
        fs::write(&path, buf).expect("write session jsonl");
    }

    // Baseline: full uncheckpointed parse + hash of all 50 sessions / 5,000 turns
    let baseline_start = Instant::now();
    let baseline_ledger = FleetLedgerStore::open(&state_root).expect("open ledger");
    let mut observer = CindyTimelineObserver::new("conn-alpha", cindy_root.clone());
    let first_scan = observer
        .project_into_ledger(&baseline_ledger)
        .expect("initial scan of 50 sessions");
    let baseline_ms = baseline_start.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(first_scan.sessions_scanned, 50);
    assert_eq!(first_scan.items_emitted, 5_000);

    // Probe: checkpointed no-change rescan of 50 sessions / 5,000 turns
    let mut samples_ms = Vec::with_capacity(25);
    for _ in 0..25 {
        let t0 = Instant::now();
        let rescan = observer
            .project_into_ledger(&baseline_ledger)
            .expect("checkpointed rescan");
        let elapsed_ms = t0.elapsed().as_secs_f64() * 1000.0;
        assert_eq!(rescan.sessions_scanned, 50);
        assert_eq!(rescan.sessions_skipped_unchanged, 50);
        assert_eq!(rescan.items_emitted, 0);
        assert_eq!(rescan.turn_files_opened, 0);
        samples_ms.push(elapsed_ms);
    }
    samples_ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median_ms = samples_ms[samples_ms.len() / 2];
    let p99_ms = samples_ms[samples_ms.len() - 1];

    // Also measure a cold observer instance loading checkpoints from disk
    let cold_t0 = Instant::now();
    let mut cold_observer = CindyTimelineObserver::new("conn-alpha", cindy_root);
    let cold_rescan = cold_observer
        .project_into_ledger(&baseline_ledger)
        .expect("cold checkpointed rescan");
    let cold_ms = cold_t0.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(cold_rescan.sessions_skipped_unchanged, 50);
    assert_eq!(cold_rescan.items_emitted, 0);

    println!(
        "BENCH_INCREMENTAL_SCAN_50_SESSIONS: baseline_full_scan_ms={baseline_ms:.3} rescan_median_ms={median_ms:.3} rescan_p99_ms={p99_ms:.3} cold_rescan_ms={cold_ms:.3} limit_ms=25.0"
    );
    assert!(
        p99_ms < 25.0 && cold_ms < 25.0,
        "checkpointed no-change rescan of 50 sessions must complete in < 25 ms (got p99={p99_ms:.3} ms, cold={cold_ms:.3} ms)"
    );

    let _ = OpenAiDotsRuntimeConfig::new(
        PathBuf::from("/fixture/codex"),
        PathBuf::from("/fixture/codex/app-server.sock"),
    );
}

#[test]
fn live_zcode_readonly_projection_preserves_mtimes() {
    let tmp = TempDirFixture::new("live-zcode");
    let state_root = tmp.path().join("state");
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let live_zcode = home.as_ref().map(|h| h.join(".zcode"));

    let zcode_root = if let Some(ref zr) = live_zcode {
        if zr.join("v2").join("tasks-index.sqlite").exists() {
            zr.clone()
        } else {
            let fallback = tmp.path().join("zcode-fixture");
            run_sqlite_ddl(
                &fallback.join("v2").join("tasks-index.sqlite"),
                "CREATE TABLE tasks (workspace_key TEXT, workspace_path TEXT, task_id TEXT, title TEXT, task_status TEXT, provider TEXT, root_path TEXT, project_id TEXT, archived INTEGER, deleted INTEGER);
                 INSERT INTO tasks VALUES ('/fixture/Projects/demo', '/fixture/Projects/demo', 'ses_ro_1', 'Readonly Check', 'normal', 'glm', '/fixture/Projects/demo', 'proj-ro-1', 0, 0);",
            );
            fallback
        }
    } else {
        return;
    };

    let tasks_db = zcode_root.join("v2").join("tasks-index.sqlite");
    let cli_db = zcode_root.join("cli").join("db").join("db.sqlite");
    let mtime_before_tasks = fs::metadata(&tasks_db).unwrap().modified().unwrap();
    let mtime_before_cli = cli_db
        .exists()
        .then(|| fs::metadata(&cli_db).unwrap().modified().unwrap());

    let ledger = FleetLedgerStore::open(&state_root).expect("open temp ledger");
    let report = project_host_timelines(&ledger, "conn-alpha", "zcode", Some(&zcode_root))
        .expect("readonly zcode projection");
    println!(
        "LIVE_ZCODE_READONLY: sessions_scanned={} items_emitted={} errors={}",
        report.sessions_scanned,
        report.items_emitted,
        report.errors.len()
    );
    assert!(report.sessions_scanned > 0);

    let mtime_after_tasks = fs::metadata(&tasks_db).unwrap().modified().unwrap();
    assert_eq!(
        mtime_before_tasks, mtime_after_tasks,
        "readonly projection must not modify tasks-index.sqlite mtime"
    );
    if let Some(before_cli) = mtime_before_cli {
        let after_cli = fs::metadata(&cli_db).unwrap().modified().unwrap();
        assert_eq!(
            before_cli, after_cli,
            "readonly projection must not modify cli/db/db.sqlite mtime"
        );
    }
}

