use fleet_daemon::{FleetLedgerStore, FleetMailboxStore, HostAdapterError};
use fleet_proto::{
    stable_runtime_session_id, DeliveryTier, DesktopHostKind, InboxMessage, InteractionStatus,
    SessionMeta, SessionNotice, SessionNoticeKind, SidebarProjectBinding, TimelineItem,
    TimelineItemKind, TimelineItemStatus, TimelineOrderSeq, TimelineRole, UpdatedSeq,
};
use serde_json::json;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Barrier,
    },
    thread,
    time::Instant,
};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TempDirFixture {
    path: PathBuf,
}

impl TempDirFixture {
    fn new() -> Self {
        let raw = std::env::temp_dir().join(format!(
            "fleet-ledger-wal-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&raw).expect("create temp dir");
        let path = raw.canonicalize().unwrap_or(raw);
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

fn tempdir() -> Result<TempDirFixture, std::io::Error> {
    Ok(TempDirFixture::new())
}

#[test]
fn legacy_mailboxes_and_host_state_migrate_idempotently_to_wal_mode_0600() {
    let dir = tempdir().expect("tempdir");
    let state_root = dir.path().join("state");
    fs::create_dir_all(&state_root).expect("create state_root");

    let mut legacy_msgs = Vec::new();
    for i in 1..=5 {
        legacy_msgs.push(InboxMessage {
            id: format!("legacy-msg-{i}"),
            from: format!("openai-dots/demo-proj#dots-sess-{i}"),
            to: "grokbot/demo-proj".into(),
            project_slug: Some("demo-proj".into()),
            session_id: None,
            reply_to: None,
            body: format!("Legacy unread message #{i}"),
            files: vec![],
            wake: true,
            notify: false,
            created_at_unix: 1_762_000_000 + i as u64,
            delivered: false,
        });
    }
    fs::write(
        state_root.join("mailboxes.json"),
        serde_json::to_vec_pretty(&json!({ "messages": legacy_msgs })).expect("serialize legacy"),
    )
    .expect("write mailboxes.json");

    let legacy_host_state = json!({
        "projects": {
            "proj-zcode-demo": {
                "host": "zcode",
                "project_id": "proj-zcode-demo",
                "title": "Demo Project",
                "project_dir": "/fixture/Projects/demo",
                "workspace_uris": ["file:///fixture/Projects/demo"]
            }
        },
        "slug_to_project_id": {
            "demo": "proj-zcode-demo"
        },
        "sessions": {
            "zcode-sess-1": {
                "host": "zcode",
                "project_id": "proj-zcode-demo",
                "session_id": "zcode-sess-1",
                "title": "Migrated ZCode Thread",
                "project_dir": "/fixture/Projects/demo",
                "workspace_uris": ["file:///fixture/Projects/demo"],
                "turns": [
                    {
                        "delivery_id": "legacy-claim-1",
                        "turn_id": "turn-1",
                        "prompt": "Initial migrated prompt",
                        "model": null
                    }
                ]
            }
        },
        "claimed_deliveries": ["legacy-claim-1"],
        "next_session_seq": 1,
        "next_turn_seq": 1
    });
    fs::write(
        state_root.join("zcode-state.json"),
        serde_json::to_vec_pretty(&legacy_host_state).expect("serialize host state"),
    )
    .expect("write zcode-state.json");

    let mailbox = FleetMailboxStore::new(state_root.clone()).expect("open FleetMailboxStore");
    let ledger = mailbox.ledger();
    assert_eq!(ledger.journal_mode().expect("journal_mode"), "wal");
    assert_eq!(ledger.integrity_check().expect("integrity_check"), "ok");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(ledger.db_path())
            .expect("db metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "fleet-ledger.sqlite must have mode 0600");
    }

    let pulled = mailbox
        .check_inbox("grokbot", true)
        .expect("first check_inbox");
    assert_eq!(pulled.len(), 5);
    assert!(pulled.iter().all(|m| m.delivered));

    let pulled_again = mailbox
        .check_inbox("grokbot", true)
        .expect("second check_inbox");
    assert!(
        pulled_again.is_empty(),
        "already-delivered migrated messages must not be re-emitted on subsequent opens"
    );

    let sessions = ledger.list_sessions().expect("list migrated sessions");
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].external_session_id, "zcode-sess-1");
    assert_eq!(sessions[0].runtime, "zcode");
}

#[test]
fn duplicate_delivery_rejects_mutated_payload_and_allows_idempotent_replay() {
    let dir = tempdir().expect("tempdir");
    let state_root = dir.path().join("state");
    let mailbox = FleetMailboxStore::new(state_root).expect("open store");

    let msg = InboxMessage {
        id: "deliv-idempotent-1".into(),
        from: "grokbot/agent-fleet".into(),
        to: "openai-dots/agent-fleet#sess-1".into(),
        project_slug: Some("agent-fleet".into()),
        session_id: Some("sess-1".into()),
        reply_to: None,
        body: "Original peer body".into(),
        files: vec![],
        wake: true,
        notify: false,
        created_at_unix: 1_762_000_100,
        delivered: false,
    };

    let first = mailbox.enqueue(msg.clone()).expect("first enqueue");
    let second = mailbox
        .enqueue(msg.clone())
        .expect("identical replay must be idempotent");
    assert_eq!(first.id, second.id);
    assert_eq!(first.body, second.body);

    let mut mutated = msg;
    mutated.body = "Mutated conflicting peer body".into();
    let err = mailbox
        .enqueue(mutated)
        .expect_err("mutated body for same delivery_id must fail");
    assert!(matches!(err, HostAdapterError::DuplicateDelivery(_)));
    assert!(err.to_string().contains("DuplicateDelivery"));
}

#[test]
fn delta_sync_fingerprints_skip_unchanged_items_and_sequence_modifications() {
    let dir = tempdir().expect("tempdir");
    let ledger = FleetLedgerStore::open(dir.path().join("state")).expect("open ledger");
    let sid = stable_runtime_session_id("conn-alpha", "openai-dots", "thread-delta-100")
        .expect("derive sid");

    ledger
        .upsert_session(&SessionMeta {
            session_id: sid.clone(),
            connector_id: "conn-alpha".into(),
            runtime: "openai-dots".into(),
            external_session_id: "thread-delta-100".into(),
            project_slug: "agent-fleet".into(),
            title: "Delta Sync Session".into(),
            latest_updated_seq: UpdatedSeq::new(0),
            delivery_tier: Some(DeliveryTier::ThreadVisible),
            sidebar_binding: Some(SidebarProjectBinding {
                host: DesktopHostKind::OpenaiDots,
                project_id: "proj-dots-fleet".into(),
                title: "agent-fleet".into(),
                project_dir: PathBuf::from("/fixture/Projects/agent-fleet"),
                workspace_uris: vec!["file:///fixture/Projects/agent-fleet".into()],
            }),
        })
        .expect("upsert session");

    let mut items: Vec<TimelineItem> = (0..100)
        .map(|i| {
            TimelineItem::new(
                format!("item-{i:03}"),
                sid.clone(),
                Some("turn-1".into()),
                TimelineItemKind::AssistantMessage,
                TimelineItemStatus::Completed,
                TimelineRole::Assistant,
                format!("Initial item content #{i}"),
                TimelineOrderSeq::new(i),
                UpdatedSeq::new(0),
                1_762_000_000_000 + i as i64,
            )
            .with_delivery_tier(DeliveryTier::ThreadVisible)
        })
        .collect();

    let initial_deltas = ledger
        .upsert_timeline_batch(&items)
        .expect("initial batch upsert");
    assert_eq!(initial_deltas.len(), 100);

    let repeat_deltas = ledger
        .upsert_timeline_batch(&items)
        .expect("unchanged batch upsert");
    assert!(
        repeat_deltas.is_empty(),
        "unchanged items must produce zero delta emissions"
    );

    items[7].content = "Updated item #7".into();
    items[42].content = "Updated item #42".into();
    items[91].content = "Updated item #91".into();

    let modified_deltas = ledger
        .upsert_timeline_batch(&items)
        .expect("modified batch upsert");
    assert_eq!(modified_deltas.len(), 3);
    assert_eq!(modified_deltas[0].item_id, "item-007");
    assert_eq!(modified_deltas[1].item_id, "item-042");
    assert_eq!(modified_deltas[2].item_id, "item-091");
    assert!(modified_deltas[0].updated_seq < modified_deltas[1].updated_seq);
    assert!(modified_deltas[1].updated_seq < modified_deltas[2].updated_seq);
    assert!(
        modified_deltas[0].updated_seq
            > initial_deltas
                .last()
                .expect("last initial")
                .updated_seq
    );

    let cursor = initial_deltas.last().expect("last initial").updated_seq;
    let recovered = ledger
        .list_timeline_items(sid.as_str(), Some(cursor), 50)
        .expect("recover after cursor");
    assert_eq!(recovered.len(), 3);
}

#[test]
fn concurrent_16_threads_enqueue_and_upsert_without_lost_writes_or_corruption() {
    let dir = tempdir().expect("tempdir");
    let state_root = dir.path().join("state");
    let ledger = FleetLedgerStore::open(&state_root).expect("open ledger");
    let sid = stable_runtime_session_id("conn-alpha", "deepseek-harness", "dsh-concurrent-16")
        .expect("derive sid");

    let barrier = Arc::new(Barrier::new(16));
    let mut handles = Vec::new();
    for tid in 0..16 {
        let b = Arc::clone(&barrier);
        let root = state_root.clone();
        let session_id = sid.clone();
        handles.push(thread::spawn(move || {
            let thread_ledger = FleetLedgerStore::open(&root).expect("open thread ledger");
            b.wait();
            for step in 0..10 {
                let msg = InboxMessage {
                    id: format!("conc-msg-t{tid}-s{step}"),
                    from: format!("openai-dots/demo#dots-{tid}"),
                    to: "grokbot/demo".into(),
                    project_slug: Some("demo".into()),
                    session_id: None,
                    reply_to: None,
                    body: format!("Concurrent message from thread {tid} step {step}"),
                    files: vec![],
                    wake: true,
                    notify: false,
                    created_at_unix: 1_762_000_000 + (tid * 10 + step) as u64,
                    delivered: false,
                };
                thread_ledger.enqueue(msg).expect("concurrent enqueue");

                let item = TimelineItem::new(
                    format!("conc-item-t{tid}-s{step}"),
                    session_id.clone(),
                    Some(format!("turn-{tid}")),
                    TimelineItemKind::AssistantMessage,
                    TimelineItemStatus::Completed,
                    TimelineRole::Assistant,
                    format!("Concurrent timeline item t{tid} s{step}"),
                    TimelineOrderSeq::new((tid * 10 + step) as i32),
                    UpdatedSeq::new(0),
                    1_762_000_000_000,
                );
                let delta = thread_ledger
                    .upsert_timeline_item(&item)
                    .expect("concurrent upsert_timeline_item");
                assert!(delta.is_some());
            }
        }));
    }

    for h in handles {
        h.join().expect("thread join");
    }

    let all_msgs = ledger.list_all_messages().expect("list_all_messages");
    assert_eq!(all_msgs.len(), 160);
    let all_items = ledger
        .list_timeline_items(sid.as_str(), None, 500)
        .expect("list_timeline_items");
    assert_eq!(all_items.len(), 160);
    assert_eq!(ledger.integrity_check().expect("integrity_check"), "ok");
}

#[test]
fn wal_reader_isolation_does_not_block_concurrent_writer() {
    let dir = tempdir().expect("tempdir");
    let state_root = dir.path().join("state");
    let writer = FleetLedgerStore::open(&state_root).expect("open writer");
    let reader = FleetLedgerStore::open(&state_root).expect("open reader");
    let sid =
        stable_runtime_session_id("conn-alpha", "zcode", "zcode-wal-iso").expect("derive sid");

    let item1 = TimelineItem::new(
        "iso-item-1",
        sid.clone(),
        Some("turn-1".into()),
        TimelineItemKind::UserMessage,
        TimelineItemStatus::Completed,
        TimelineRole::User,
        "First message before concurrent write",
        TimelineOrderSeq::new(1),
        UpdatedSeq::new(0),
        1_762_000_000_000,
    );
    writer.upsert_timeline_item(&item1).expect("upsert item1");

    let snap_before = reader
        .list_timeline_items(sid.as_str(), None, 10)
        .expect("reader snapshot");
    assert_eq!(snap_before.len(), 1);

    let notice = SessionNotice {
        notice_id: "notice-iso-1".into(),
        session_id: sid.clone(),
        kind: SessionNoticeKind::ToolApproval,
        status: InteractionStatus::Open,
        title: "WAL active check".into(),
        prompt: "Concurrent writer appended while reader open".into(),
        plan_ticket: None,
        decision_payload: None,
        updated_seq: UpdatedSeq::new(0),
    };
    writer.append_notice(&notice).expect("append notice");
    assert_eq!(reader.list_notices(sid.as_str()).expect("notices").len(), 1);
    assert_eq!(writer.integrity_check().expect("integrity"), "ok");
}

#[test]
fn bench_store_10k_append() {
    let dir = tempdir().expect("tempdir");
    let legacy_root = dir.path().join("legacy_state");
    fs::create_dir_all(&legacy_root).expect("create legacy_state");

    let seed_messages: Vec<InboxMessage> = (0..10_000)
        .map(|i| InboxMessage {
            id: format!("seed-msg-{i:05}"),
            from: "openai-dots/agent-fleet#dots-1".into(),
            to: "grokbot/agent-fleet".into(),
            project_slug: Some("agent-fleet".into()),
            session_id: None,
            reply_to: None,
            body: format!("Seeded benchmark message payload #{i:05} with realistic text"),
            files: vec![],
            wake: true,
            notify: false,
            created_at_unix: 1_762_000_000 + i as u64,
            delivered: false,
        })
        .collect();
    fs::write(
        legacy_root.join("mailboxes.json"),
        serde_json::to_vec_pretty(&json!({ "messages": seed_messages })).expect("serialize 10k"),
    )
    .expect("write 10k mailboxes.json");

    let legacy_store = FleetMailboxStore::new(dir.path().join("dummy")).expect("dummy store");
    let legacy_file = legacy_root.join("mailboxes.json");
    let mut trunk_samples_ms = Vec::new();
    for run in 0..5 {
        let t0 = Instant::now();
        let bytes = fs::read(&legacy_file).expect("read legacy");
        let mut parsed: serde_json::Value = serde_json::from_slice(&bytes).expect("parse legacy");
        parsed["messages"]
            .as_array_mut()
            .expect("messages array")
            .push(json!({
                "id": format!("trunk-append-{run}"),
                "from": "openai-dots/agent-fleet#dots-1",
                "to": "grokbot/agent-fleet",
                "project_slug": "agent-fleet",
                "session_id": null,
                "reply_to": null,
                "body": "Appended 10,001st message",
                "files": [],
                "wake": true,
                "notify": false,
                "created_at_unix": 1762099999u64,
                "delivered": false
            }));
        let tmp = legacy_file.with_extension("json.tmp");
        let out = serde_json::to_vec_pretty(&parsed).expect("serialize");
        fs::write(&tmp, out).expect("write tmp");
        fs::rename(&tmp, &legacy_file).expect("rename tmp");
        trunk_samples_ms.push(t0.elapsed().as_secs_f64() * 1000.0);
    }
    let _ = legacy_store;
    trunk_samples_ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let trunk_p99_ms = *trunk_samples_ms.last().expect("trunk p99");

    let wal_root = dir.path().join("wal_state");
    let ledger = FleetLedgerStore::open(&wal_root).expect("open wal ledger");
    let sid = stable_runtime_session_id("conn-alpha", "openai-dots", "bench-10k")
        .expect("derive bench sid");
    let seed_items: Vec<TimelineItem> = (0..10_000)
        .map(|i| {
            TimelineItem::new(
                format!("seed-item-{i:05}"),
                sid.clone(),
                Some("turn-seed".into()),
                TimelineItemKind::AssistantMessage,
                TimelineItemStatus::Completed,
                TimelineRole::Assistant,
                format!("Seeded timeline item #{i:05} with realistic benchmark text"),
                TimelineOrderSeq::new(i),
                UpdatedSeq::new(0),
                1_762_000_000_000 + i as i64,
            )
        })
        .collect();
    ledger
        .upsert_timeline_batch(&seed_items)
        .expect("seed 10k timeline items");

    let mut head_samples_ms = Vec::new();
    for run in 0..25 {
        let msg = InboxMessage {
            id: format!("head-append-{run:03}"),
            from: "openai-dots/agent-fleet#dots-1".into(),
            to: "grokbot/agent-fleet".into(),
            project_slug: Some("agent-fleet".into()),
            session_id: None,
            reply_to: None,
            body: format!("WAL append message #{run}"),
            files: vec![],
            wake: true,
            notify: false,
            created_at_unix: 1_762_100_000 + run as u64,
            delivered: false,
        };
        let item = TimelineItem::new(
            format!("head-item-{run:03}"),
            sid.clone(),
            Some("turn-bench".into()),
            TimelineItemKind::AssistantMessage,
            TimelineItemStatus::Completed,
            TimelineRole::Assistant,
            format!("WAL append timeline item #{run}"),
            TimelineOrderSeq::new(10_000 + run),
            UpdatedSeq::new(0),
            1_762_100_000_000 + run as i64,
        );
        let t0 = Instant::now();
        ledger.enqueue(msg).expect("wal enqueue");
        ledger.upsert_timeline_item(&item).expect("wal upsert");
        head_samples_ms.push(t0.elapsed().as_secs_f64() * 1000.0);
    }
    head_samples_ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let head_p99_ms = head_samples_ms[(head_samples_ms.len() * 99) / 100];
    let speedup = trunk_p99_ms / head_p99_ms.max(0.001);

    println!(
        "BENCH_STORE_10K_APPEND trunk_p99_ms={trunk_p99_ms:.3} head_p99_ms={head_p99_ms:.3} speedup={speedup:.1}x"
    );
    let budget_ms = if cfg!(debug_assertions) { 25.0 } else { 5.0 };
    assert!(
        head_p99_ms < budget_ms,
        "head P99 append latency {head_p99_ms:.3}ms exceeded budget {budget_ms:.1}ms"
    );
    assert!(
        speedup >= 5.0,
        "expected >= 5x speedup over trunk full-JSON rewrite, got {speedup:.2}x (trunk={trunk_p99_ms:.3}ms, head={head_p99_ms:.3}ms)"
    );
}
