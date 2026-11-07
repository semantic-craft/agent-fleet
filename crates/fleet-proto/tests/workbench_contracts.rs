use fleet_proto::{
    canonical_timeline_content_json_utf16, resolve_project_dir_in_root,
    resolve_workbench_file_path, sha256_hex, stable_runtime_session_id, timeline_content_hash,
    timeline_content_hash_utf16, turn_position_item_id, validate_brief, BriefPolicyError,
    ConnectorRpcFrame, DeliveryTier, DesktopHostKind, EffectiveCapabilities, InboxMessage,
    InteractionStatus, InvalidOrderSeq, PlanFenceConflict, PlanReviewTicket, ProtocolEventEnvelope,
    SessionMeta, SessionNotice, SessionNoticeKind, SidebarProjectBinding, TimelineItem,
    TimelineItemKind, TimelineItemStatus, TimelineOrderSeq, TimelineRole, UpdatedSeq,
    MAX_CONNECTOR_WS_FRAME_BYTES,
};
use std::{
    fs,
    path::PathBuf,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

#[test]
fn timeline_content_hash_escapes_lone_surrogates_and_matches_protocol_v1_1_vectors() {
    let h_ascii = timeline_content_hash(
        "assistant_message",
        "completed",
        "assistant",
        "All checks passed.",
    );
    let expected_json =
        r#"{"type":"assistant_message","status":"completed","role":"assistant","content":"All checks passed."}"#;
    assert_eq!(
        h_ascii.as_str(),
        format!("sha256:{}", sha256_hex(expected_json.as_bytes()))
    );

    let utf16_lone_high: [u16; 7] = [
        b'a' as u16,
        b':' as u16,
        0xD83D,
        b':' as u16,
        0xDC00,
        b':' as u16,
        b'z' as u16,
    ];
    let canonical_json = canonical_timeline_content_json_utf16(
        "assistant_message",
        "in_progress",
        "assistant",
        &utf16_lone_high,
    );
    assert_eq!(
        canonical_json,
        r#"{"type":"assistant_message","status":"in_progress","role":"assistant","content":"a:\ud83d:\udc00:z"}"#
    );
    let h_utf16 = timeline_content_hash_utf16(
        "assistant_message",
        "in_progress",
        "assistant",
        &utf16_lone_high,
    );
    let h_str = timeline_content_hash(
        "assistant_message",
        "in_progress",
        "assistant",
        r"a:\uD83D:\uDC00:z",
    );
    assert_eq!(h_utf16, h_str);
    assert_eq!(
        h_str.as_str(),
        "sha256:83fa1ae017f281cd33926c007a45797f680e0c689fca469024c25efd38f01fb1"
    );

    let utf16_valid_pair: [u16; 4] = [b'o' as u16, b'k' as u16, 0xD83D, 0xDE00];
    let h_pair_utf16 = timeline_content_hash_utf16(
        "user_message",
        "completed",
        "user",
        &utf16_valid_pair,
    );
    let h_pair_str = timeline_content_hash("user_message", "completed", "user", "ok😀");
    let h_pair_escaped =
        timeline_content_hash("user_message", "completed", "user", r"ok\uD83D\uDE00");
    assert_eq!(h_pair_utf16, h_pair_str);
    assert_eq!(h_pair_str, h_pair_escaped);
}

#[test]
fn stable_runtime_session_id_and_turn_position_item_id_are_deterministic() {
    let s1 = stable_runtime_session_id("conn-alpha", "zcode", "sess_abc123").unwrap();
    let s2 = stable_runtime_session_id("conn-alpha", "ZCode", "sess_abc123").unwrap();
    assert_eq!(s1, s2);
    assert!(s1.as_str().starts_with("sess_zcode_"));
    assert_eq!(s1.as_str().len(), "sess_zcode_".len() + 24);

    let s_other = stable_runtime_session_id("conn-alpha", "openai-dots", "sess_abc123").unwrap();
    assert_ne!(s1, s_other);
    assert!(s_other.as_str().starts_with("sess_openai-dots_"));

    let i1 = turn_position_item_id(s1.as_str(), "turn-01", 0, TimelineItemKind::UserMessage);
    let i2 = turn_position_item_id(s1.as_str(), "turn-01", 0, TimelineItemKind::UserMessage);
    let i3 = turn_position_item_id(
        s1.as_str(),
        "turn-01",
        1,
        TimelineItemKind::AssistantMessage,
    );
    assert_eq!(i1, i2);
    assert_ne!(i1, i3);
    assert!(i1.starts_with("item_"));
    assert_eq!(i1.len(), 29);
}

#[test]
fn envelope_wire_roundtrip() {
    let sample_timeline = r#"{"protocolVersion":"1.1","eventId":"evt_20261107_001","type":"timeline.item_updated","sessionId":"sess_zcode_0123456789abcdef01234567","updatedSeq":42,"cursor":"seq:42","timestampMs":1730995200000,"data":{"itemId":"item_0123456789abcdef01234567","orderSeq":1,"status":"completed","deliveryTier":"thread-visible"}}"#;
    let parsed = ProtocolEventEnvelope::from_wire_json(sample_timeline).unwrap();
    assert_eq!(parsed.protocol_version, "1.1");
    assert_eq!(parsed.event_id, "evt_20261107_001");
    assert_eq!(parsed.event_type, "timeline.item_updated");
    assert_eq!(parsed.updated_seq, UpdatedSeq::new(42));
    assert_eq!(parsed.cursor, "seq:42");
    assert!(parsed.deduplicates_by_event_id());
    assert_eq!(parsed.to_wire_json(), sample_timeline);

    let sample_cap = r#"{"protocolVersion":"1.1","eventId":"evt_cap_01","type":"runtime.capability.updated","sessionId":"sess_cindy_fedcba9876543210fedcba98","updatedSeq":43,"cursor":"seq:43","timestampMs":1730995200100,"data":{"session.send_message":false,"dispatch_channel":"anet"}}"#;
    let parsed_cap = ProtocolEventEnvelope::from_wire_json(sample_cap).unwrap();
    assert!(!parsed_cap.deduplicates_by_event_id());
    assert_eq!(parsed_cap.to_wire_json(), sample_cap);
}

#[test]
fn order_seq_rejects_out_of_bounds_signed_32bit_values() {
    assert_eq!(
        TimelineOrderSeq::try_from(TimelineOrderSeq::MIN).unwrap().get(),
        i32::MIN
    );
    assert_eq!(
        TimelineOrderSeq::try_from(TimelineOrderSeq::MAX).unwrap().get(),
        i32::MAX
    );

    let err_max = TimelineOrderSeq::try_from(i64::MAX).unwrap_err();
    assert_eq!(err_max, InvalidOrderSeq(i64::MAX));
    assert_eq!(err_max.code(), "invalid_order_seq");

    let err_min = TimelineOrderSeq::try_from(i64::MIN).unwrap_err();
    assert_eq!(err_min, InvalidOrderSeq(i64::MIN));
    assert_eq!(err_min.code(), "invalid_order_seq");

    let err_over = TimelineOrderSeq::try_from(TimelineOrderSeq::MAX + 1).unwrap_err();
    assert_eq!(err_over.code(), "invalid_order_seq");
}

#[test]
fn interaction_status_can_transition_to_covers_all_16_state_pairs() {
    let mut valid_pairs = Vec::new();
    for from in InteractionStatus::ALL {
        for to in InteractionStatus::ALL {
            if from.can_transition_to(to) {
                valid_pairs.push((from, to));
            }
        }
    }
    assert_eq!(InteractionStatus::ALL.len() * InteractionStatus::ALL.len(), 16);
    assert_eq!(
        valid_pairs,
        vec![
            (InteractionStatus::Open, InteractionStatus::ResponseAccepted),
            (
                InteractionStatus::ResponseAccepted,
                InteractionStatus::Resolving
            ),
            (InteractionStatus::Resolving, InteractionStatus::Resolved),
        ]
    );

    let sid = stable_runtime_session_id("conn-1", "openai-dots", "thread-1").unwrap();
    let mut notice = SessionNotice {
        notice_id: "notice-1".into(),
        session_id: sid,
        kind: SessionNoticeKind::ToolApproval,
        status: InteractionStatus::Open,
        title: "Run cargo test".into(),
        prompt: "Allow command execution in workspace?".into(),
        plan_ticket: None,
        decision_payload: None,
        updated_seq: UpdatedSeq::new(1),
    };
    assert!(notice.transition_to(InteractionStatus::Resolved).is_err());
    notice
        .transition_to(InteractionStatus::ResponseAccepted)
        .unwrap();
    notice.transition_to(InteractionStatus::Resolving).unwrap();
    notice.transition_to(InteractionStatus::Resolved).unwrap();
    assert!(notice
        .transition_to(InteractionStatus::ResponseAccepted)
        .is_err());
}

#[test]
fn plan_review_ticket_verify_approval_fails_closed_on_1_byte_mutation() {
    let ticket = PlanReviewTicket {
        review_id: "rev-01".into(),
        plan_id: "plan-workbench".into(),
        plan_revision: 3,
        content_hash: "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
            .into(),
        epoch: 17,
    };
    assert_eq!(
        ticket.verify_approval(&ticket.content_hash, 17),
        Ok(())
    );
    let mutated_hash =
        "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdee";
    assert_eq!(
        ticket.verify_approval(mutated_hash, 17),
        Err(PlanFenceConflict)
    );
    assert_eq!(
        ticket.verify_approval(&ticket.content_hash, 18),
        Err(PlanFenceConflict)
    );
    assert_eq!(
        ticket.verify_revision_and_approval(4, &ticket.content_hash, 17),
        Err(PlanFenceConflict)
    );
}

#[test]
fn delivery_tier_lattice_and_sidebar_binding_projection() {
    assert!(DeliveryTier::ThreadVisible.proves_sidebar_visibility());
    assert!(!DeliveryTier::Accepted.proves_sidebar_visibility());
    assert!(!DeliveryTier::Completed.proves_sidebar_visibility());
    assert!(DeliveryTier::ThreadVisible.lattice_satisfies(DeliveryTier::Accepted));
    assert!(DeliveryTier::Completed.lattice_satisfies(DeliveryTier::Accepted));
    assert!(!DeliveryTier::Completed.lattice_satisfies(DeliveryTier::ThreadVisible));
    assert!(!DeliveryTier::Accepted.lattice_satisfies(DeliveryTier::ThreadVisible));

    let sid = stable_runtime_session_id("conn-1", "zcode", "sess_77").unwrap();
    let binding = SidebarProjectBinding {
        host: DesktopHostKind::Zcode,
        project_id: "proj-uuid-1".into(),
        title: "agent-fleet".into(),
        project_dir: PathBuf::from("/fixture/Projects/agent-fleet"),
        workspace_uris: vec!["file:///fixture/Projects/agent-fleet".into()],
    };
    let item = TimelineItem::new(
        "item-1",
        sid.clone(),
        Some("turn-1".into()),
        TimelineItemKind::PeerMessage,
        TimelineItemStatus::Completed,
        TimelineRole::Peer,
        "<fleet-message from=\"grokbot\" to=\"zcode/agent-fleet\" id=\"m1\">Hello</fleet-message>",
        TimelineOrderSeq::new(1),
        UpdatedSeq::new(10),
        1_730_995_200_000,
    )
    .with_delivery_tier(DeliveryTier::ThreadVisible)
    .with_sidebar_binding(binding.clone())
    .with_peer_sender("grokbot");

    assert!(item.verify_content_hash());
    assert_eq!(item.delivery_tier, Some(DeliveryTier::ThreadVisible));
    assert_eq!(item.sidebar_binding, Some(binding.clone()));

    let meta = SessionMeta {
        session_id: sid,
        connector_id: "conn-1".into(),
        runtime: "zcode".into(),
        external_session_id: "sess_77".into(),
        project_slug: "agent-fleet".into(),
        title: "agent-fleet".into(),
        latest_updated_seq: UpdatedSeq::new(10),
        delivery_tier: Some(DeliveryTier::ThreadVisible),
        sidebar_binding: Some(binding),
    };
    assert!(meta.validate().is_ok());

    let cindy_caps = EffectiveCapabilities::for_desktop_host(DesktopHostKind::Cindy, false);
    assert!(!cindy_caps.session_send_message);
    assert_eq!(cindy_caps.dispatch_channel.as_deref(), Some("anet"));

    let oversize_frame = ConnectorRpcFrame::new_notification(
        1,
        "timeline.itemUpsert",
        "x".repeat(MAX_CONNECTOR_WS_FRAME_BYTES + 128),
    );
    assert!(oversize_frame.requires_http_ingest_spillover());
    let critical_frame = ConnectorRpcFrame::new_request(1, "req-1", "interaction.respond", "{}");
    assert!(critical_frame.is_critical_lane());
}

#[test]
fn workbench_path_boundary_rejects_symlinks_traversal_and_cache_paths() {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let base = std::env::temp_dir().join(format!(
        "fleet-proto-boundary-{}-{stamp}",
        std::process::id()
    ));
    let projects_root = base.join("Projects");
    let outside = base.join("Outside");
    fs::create_dir_all(projects_root.join("demo-app/src")).unwrap();
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("secret.txt"), "top-secret").unwrap();

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        symlink(&outside, projects_root.join("symlink-repo")).unwrap();
        symlink(
            outside.join("secret.txt"),
            projects_root.join("demo-app/src/escaped.txt"),
        )
        .unwrap();
        assert!(resolve_project_dir_in_root(&projects_root, "symlink-repo").is_err());
        assert!(
            resolve_workbench_file_path(&projects_root, "demo-app", "src/escaped.txt").is_err()
        );
    }

    for invalid_rel in [
        "",
        "..",
        "../secret.txt",
        "src/../../secret.txt",
        "/etc/passwd",
        ".anet/id_ed25519",
        ".cache/state",
        ".ssh/id_rsa",
        "PRIVATE/keys",
    ] {
        assert!(
            resolve_workbench_file_path(&projects_root, "demo-app", invalid_rel).is_err(),
            "expected rejection for relative path {invalid_rel:?}"
        );
    }

    let valid_file =
        resolve_workbench_file_path(&projects_root, "demo-app", "src/lib.rs").unwrap();
    assert_eq!(valid_file, projects_root.join("demo-app/src/lib.rs"));

    let _ = fs::remove_dir_all(&base);
}

#[test]
fn workbench_create_and_start_brief_policy_rejects_model_wrong() {
    let bad_prompt = "First inspect your own model and if not GPT-5 print MODEL_WRONG and stop.";
    assert_eq!(
        validate_brief(bad_prompt),
        Err(BriefPolicyError::ModelWrongGate)
    );
    let self_attest_prompt = "Before starting, report your actual model and provider.";
    assert_eq!(
        validate_brief(self_attest_prompt),
        Err(BriefPolicyError::ModelSelfAttestation)
    );
    let good_prompt =
        "Implement TimelineOrderSeq bounds checking in crates/fleet-proto/src/timeline.rs.";
    assert_eq!(validate_brief(good_prompt), Ok(()));
}

#[test]
fn bench_timeline_hash_10k() {
    let sid = stable_runtime_session_id("conn-bench", "zcode", "sess_bench_01").unwrap();
    let count = 10_000usize;

    let mut trunk_durations_ms = Vec::new();
    let mut head_durations_ms = Vec::new();

    for run in 0..5 {
        let t0 = Instant::now();
        let mut trunk_bytes = 0usize;
        for i in 0..count {
            let msg = InboxMessage {
                id: format!("msg-{run}-{i}"),
                from: "grokbot/agent-fleet".into(),
                to: "zcode/agent-fleet#sess_bench_01".into(),
                project_slug: Some("agent-fleet".into()),
                session_id: Some("sess_bench_01".into()),
                reply_to: None,
                body: format!("Execute verification probe {i} on slice target."),
                files: Vec::new(),
                wake: true,
                notify: false,
                created_at_unix: 1_730_995_200,
                delivered: false,
            };
            let framed = msg.format_peer_frame();
            let digest = sha256_hex(framed.as_bytes());
            trunk_bytes += framed.len() + digest.len();
        }
        let trunk_ms = t0.elapsed().as_secs_f64() * 1000.0;
        assert!(trunk_bytes > 0);
        trunk_durations_ms.push(trunk_ms);

        let t1 = Instant::now();
        let mut head_bytes = 0usize;
        for i in 0..count {
            let item = TimelineItem::new(
                format!("item-{run}-{i}"),
                sid.clone(),
                Some("turn-bench".into()),
                TimelineItemKind::AssistantMessage,
                TimelineItemStatus::Completed,
                TimelineRole::Assistant,
                format!("Execute verification probe {i} on slice target."),
                TimelineOrderSeq::new(i as i32),
                UpdatedSeq::new(i as i64),
                1_730_995_200_000,
            )
            .with_delivery_tier(DeliveryTier::ThreadVisible);
            let json = item.to_json_string();
            head_bytes += json.len() + item.content_hash.len();
        }
        let head_ms = t1.elapsed().as_secs_f64() * 1000.0;
        assert!(head_bytes > 0);
        head_durations_ms.push(head_ms);
    }

    trunk_durations_ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
    head_durations_ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let trunk_median = trunk_durations_ms[2];
    let head_median = head_durations_ms[2];
    println!(
        "BENCH_TIMELINE_HASH_10K trunk_median_ms={trunk_median:.2} head_median_ms={head_median:.2} runs=5 items={count}"
    );
    let budget_ms = if cfg!(debug_assertions) { 250.0 } else { 50.0 };
    assert!(
        head_median < budget_ms,
        "expected head 10k timeline hash + serialize < {budget_ms:.1} ms, got {head_median:.2} ms"
    );
}
