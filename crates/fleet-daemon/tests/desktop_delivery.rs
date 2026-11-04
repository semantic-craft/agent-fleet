use fleet_daemon::{DesktopDelivery, DesktopError};
use fleet_proto::{SessionId, SessionRecord, SessionSelector, SessionState};
use serde_json::{json, Value};
use std::{
    fs,
    os::unix::net::UnixListener,
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tungstenite::Message;

#[derive(Default)]
struct Behavior {
    missing: bool,
    bad_cwd: bool,
    disconnect_send: bool,
    wrong_response: bool,
    paginate: bool,
    change_session_on_send: bool,
    no_notifications: bool,
    observed_status: Option<&'static str>,
    wrong_turn: bool,
    duplicate_turn: bool,
    reject_start: bool,
    reject_readback: bool,
    reused_turn_id: bool,
}
struct Fixture {
    root: PathBuf,
    socket: PathBuf,
    record: SessionRecord,
    calls: Arc<Mutex<Vec<Value>>>,
    server: Option<thread::JoinHandle<()>>,
}
impl Fixture {
    fn new(behavior: Behavior) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = PathBuf::from(format!(
            "/tmp/fleet-e-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join("sessions")).unwrap();
        fs::create_dir_all(root.join("project")).unwrap();
        let socket = root.join("control.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let record = SessionRecord {
            id: SessionId {
                host: "fixture-host".into(),
                desktop: "codex".into(),
                thread_id: "existing-thread".into(),
            },
            project_cwd: root.join("project"),
            state: SessionState::Open,
            observed_at: now - 10,
            expires_at: now + 600,
        };
        fs::write(
            root.join("sessions/target.json"),
            serde_json::to_vec(&record).unwrap(),
        )
        .unwrap();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let recorded = calls.clone();
        let cwd = record.project_cwd.clone();
        let record_path = root.join("sessions/target.json");
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut ws = tungstenite::accept(stream).unwrap();
            let mut turn_count = 0;
            let mut metadata_reads = 0;
            let mut turns: Vec<Value> = Vec::new();
            while let Ok(message) = ws.read() {
                let Message::Text(text) = message else {
                    continue;
                };
                let request: Value = serde_json::from_str(&text).unwrap();
                recorded.lock().unwrap().push(request.clone());
                let method = request["method"].as_str().unwrap();
                if method == "thread/read" {
                    metadata_reads += 1;
                    if behavior.change_session_on_send && metadata_reads == 2 {
                        let mut v: Value =
                            serde_json::from_slice(&fs::read(&record_path).unwrap()).unwrap();
                        v["id"]["thread_id"] = json!("another-thread");
                        fs::write(&record_path, serde_json::to_vec(&v).unwrap()).unwrap();
                    }
                }
                if (behavior.reject_start && method == "turn/start")
                    || (behavior.reject_readback && method == "thread/turns/list")
                {
                    ws.send(Message::Text(
                        json!({"id":request["id"],"error":{"code":-1,"message":"fixture denied"}})
                            .to_string()
                            .into(),
                    ))
                    .unwrap();
                    continue;
                }
                let result = match method {
                    "initialize" => json!({"userAgent":"fixture"}),
                    "initialized" => continue,
                    "thread/loaded/list" if behavior.missing => {
                        json!({"data":[],"nextCursor":null})
                    }
                    "thread/loaded/list"
                        if behavior.paginate && request["params"]["cursor"].is_null() =>
                    {
                        json!({"data":["unrelated"],"nextCursor":"page-two"})
                    }
                    "thread/loaded/list" => json!({"data":["existing-thread"],"nextCursor":null}),
                    "thread/read" => {
                        json!({"thread":{"id":"existing-thread","cwd":if behavior.bad_cwd {"/wrong".to_string()} else {cwd.to_string_lossy().to_string()},"status":{"type":"idle"},"turns":turns}})
                    }
                    "thread/turns/list"
                        if behavior.paginate
                            && request["params"]["cursor"].is_null()
                            && turns.len() > 1 =>
                    {
                        json!({"data":[turns[0]],"nextCursor":"turn-page-two"})
                    }
                    "thread/turns/list" if request["params"]["cursor"] == "turn-page-two" => {
                        json!({"data":turns[1..],"nextCursor":null})
                    }
                    "thread/turns/list" => {
                        let mut observed = turns.clone();
                        for turn in &mut observed {
                            if let Some(status) = behavior.observed_status {
                                turn["status"] = json!(status);
                            }
                            if behavior.wrong_turn {
                                turn["id"] = json!("foreign-turn");
                            }
                        }
                        if behavior.duplicate_turn && !observed.is_empty() {
                            observed.push(observed[0].clone());
                        }
                        json!({"data":observed,"nextCursor":null})
                    }
                    "turn/start" => {
                        if behavior.disconnect_send {
                            break;
                        }
                        turn_count += 1;
                        let id = if behavior.reused_turn_id {
                            "turn-1".to_owned()
                        } else {
                            format!("turn-{turn_count}")
                        };
                        turns.push(json!({"id":id,"status":"inProgress","items":[{"type":"agentMessage","text":"fixture answer"}]}));
                        if !behavior.no_notifications {
                            ws.send(Message::Text(json!({"method":"item/completed","params":{"threadId":"unrelated-thread","item":{"text":"private-other-thread"}}}).to_string().into())).unwrap();
                            ws.send(Message::Text(json!({"method":"turn/started","params":{"threadId":"existing-thread","turn":{"id":id}}}).to_string().into())).unwrap();
                        }
                        json!({"turn":{"id":id,"status":"inProgress"}})
                    }
                    "turn/interrupt" => {
                        for turn in &mut turns {
                            if turn["id"] == request["params"]["turnId"] {
                                turn["status"] = json!("interrupted");
                            }
                        }
                        json!({})
                    }
                    other => panic!("unexpected method {other}"),
                };
                let id = if behavior.wrong_response && method == "turn/start" {
                    json!(9999)
                } else {
                    request["id"].clone()
                };
                if ws
                    .send(Message::Text(
                        json!({"id":id,"result":result}).to_string().into(),
                    ))
                    .is_err()
                {
                    break;
                }
            }
        });
        Self {
            root,
            socket,
            record,
            calls,
            server: Some(server),
        }
    }
    fn connect(&self) -> Result<DesktopDelivery, DesktopError> {
        DesktopDelivery::connect(
            &self.socket,
            self.root.join("sessions"),
            SessionSelector {
                thread_id: Some(self.record.id.thread_id.clone()),
                ..Default::default()
            },
            vec![],
            Duration::from_secs(2),
        )
    }
    fn methods(&self) -> Vec<String> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .map(|v| v["method"].as_str().unwrap().to_owned())
            .collect()
    }
    fn save(&self, record: &SessionRecord) {
        fs::write(
            self.root.join("sessions/target.json"),
            serde_json::to_vec(record).unwrap(),
        )
        .unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(server) = self.server.take() {
            server.join().unwrap();
        }
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn desktop_delivery_keeps_send_followup_read_interrupt_and_events_on_existing_thread() {
    let f = Fixture::new(Behavior {
        paginate: true,
        ..Default::default()
    });
    let mut d = f.connect().unwrap();
    let first = d.send("delivery-1", "hello").unwrap();
    assert_eq!(first.session(), &f.record.id);
    assert_eq!(first.cwd(), f.record.project_cwd);
    assert_eq!(first.turn_id(), "turn-1");
    let second = d.send("delivery-2", "follow up").unwrap();
    assert_eq!(second.turn_id(), "turn-2");
    let read = d.read().unwrap();
    assert_eq!(read["turns"][0]["items"][0]["text"], "fixture answer");
    let events = d.events().unwrap();
    assert_eq!(events.len(), 3);
    assert_eq!(events.iter().filter(|e| e["source"] == "poll").count(), 1);
    assert!(events
        .iter()
        .all(|e| e["params"]["threadId"] == "existing-thread"));
    d.interrupt(second.turn_id()).unwrap();
    assert_eq!(d.read().unwrap()["turns"][1]["status"], "interrupted");
    assert!(d.interrupt("foreign-turn").is_err());
    drop(d);
    for call in f.calls.lock().unwrap().iter() {
        if call["params"]["threadId"].is_string() {
            assert_eq!(call["params"]["threadId"], "existing-thread");
        }
        assert!(!matches!(
            call["method"].as_str(),
            Some("thread/start" | "thread/resume" | "thread/fork")
        ));
        if call["method"] == "turn/start" {
            assert!(call["params"].get("cwd").is_none());
            assert!(call["params"].get("model").is_none());
        }
    }
    println!("same native thread: existing-thread; two accepted turns, output read, scoped events, second turn interrupted; cwd={}", f.record.project_cwd.display());
}

#[test]
fn desktop_delivery_missing_loaded_thread_never_starts_or_resumes() {
    let f = Fixture::new(Behavior {
        missing: true,
        ..Default::default()
    });
    assert!(f.connect().is_err());
    assert_eq!(
        f.methods(),
        ["initialize", "initialized", "thread/loaded/list"]
    );
}

#[test]
fn desktop_delivery_rejects_native_cwd_mismatch() {
    let f = Fixture::new(Behavior {
        bad_cwd: true,
        ..Default::default()
    });
    assert!(f.connect().is_err());
    assert!(!f.methods().contains(&"turn/start".to_string()));
}

#[test]
fn desktop_delivery_revalidates_expired_changed_and_replaced_project() {
    let f = Fixture::new(Behavior::default());
    let mut d = f.connect().unwrap();
    let mut expired = f.record.clone();
    expired.expires_at = expired.observed_at + 1;
    f.save(&expired);
    assert!(d.send("expired", "hello").is_err());
    let mut changed = f.record.clone();
    changed.observed_at -= 1;
    f.save(&changed);
    assert!(d.send("changed", "hello").is_err());
    f.save(&f.record);
    fs::rename(&f.record.project_cwd, f.root.join("old-project")).unwrap();
    fs::create_dir(&f.record.project_cwd).unwrap();
    assert!(d.send("replaced", "hello").is_err());
    drop(d);
    assert!(!f.methods().contains(&"turn/start".to_string()));
}

#[test]
fn desktop_delivery_duplicate_id_is_never_submitted_twice() {
    let f = Fixture::new(Behavior::default());
    let mut d = f.connect().unwrap();
    d.send("same", "hello").unwrap();
    assert!(d.send("same", "changed text").is_err());
    drop(d);
    assert_eq!(f.methods().iter().filter(|m| *m == "turn/start").count(), 1);
}

#[test]
fn desktop_delivery_disconnect_after_send_is_unknown_and_never_retried() {
    let f = Fixture::new(Behavior {
        disconnect_send: true,
        ..Default::default()
    });
    let mut d = f.connect().unwrap();
    assert!(matches!(
        d.send("first", "hello"),
        Err(DesktopError::Unknown(_))
    ));
    assert!(d.send("second", "try again").is_err());
    drop(d);
    assert_eq!(f.methods().iter().filter(|m| *m == "turn/start").count(), 1);
}

#[test]
fn desktop_delivery_mismatched_rpc_response_poisoned_after_send() {
    let f = Fixture::new(Behavior {
        wrong_response: true,
        ..Default::default()
    });
    let mut d = f.connect().unwrap();
    assert!(matches!(
        d.send("first", "hello"),
        Err(DesktopError::Unknown(_))
    ));
    assert!(d.send("second", "hello").is_err());
    drop(d);
    assert_eq!(f.methods().iter().filter(|m| *m == "turn/start").count(), 1);
}

#[test]
fn desktop_delivery_session_changed_during_native_preflight_never_sends() {
    let f = Fixture::new(Behavior {
        change_session_on_send: true,
        ..Default::default()
    });
    let mut d = f.connect().unwrap();
    assert!(d.send("changed-during-preflight", "hello").is_err());
    drop(d);
    assert!(!f.methods().contains(&"turn/start".to_string()));
}

#[test]
fn desktop_delivery_poll_events_observe_changes_without_native_subscription() {
    let f = Fixture::new(Behavior {
        no_notifications: true,
        ..Default::default()
    });
    let mut d = f.connect().unwrap();
    let initial = d.events().unwrap();
    assert_eq!(initial.len(), 1);
    assert_eq!(initial[0]["source"], "poll");
    assert_eq!(initial[0]["method"], "fleet/thread/observed");
    assert!(d.events().unwrap().is_empty());
    let receipt = d.send("polled", "hello").unwrap();
    let running = d.events().unwrap();
    assert_eq!(running.len(), 1);
    assert_eq!(running[0]["params"]["threadId"], "existing-thread");
    assert_eq!(
        running[0]["params"]["thread"]["turns"][0]["status"],
        "inProgress"
    );
    d.interrupt(receipt.turn_id()).unwrap();
    let stopped = d.events().unwrap();
    assert_eq!(stopped.len(), 1);
    assert_eq!(stopped[0]["source"], "poll");
    assert_eq!(
        stopped[0]["params"]["thread"]["turns"][0]["status"],
        "interrupted"
    );
    assert!(d.events().unwrap().is_empty());
    drop(d);
}

#[test]
fn delivery_tier_acceptance_loaded_list_and_answer_text_do_not_prove_visibility_or_completion() {
    use fleet_proto::DeliveryTier;
    let f = Fixture::new(Behavior::default());
    let mut d = f.connect().unwrap();
    let receipt = d.send("tier-test", "hello").unwrap();
    assert_eq!(receipt.tier(), DeliveryTier::Accepted);
    assert!(!receipt.confirms(DeliveryTier::ThreadVisible));
    assert!(!receipt.confirms(DeliveryTier::Completed));
    assert_eq!(serde_json::to_value(&receipt).unwrap()["tier"], "accepted");
    let observed = d.observe(&receipt).unwrap();
    assert_eq!(observed.tier(), DeliveryTier::Running);
    assert!(!observed.confirms(DeliveryTier::ThreadVisible));
    assert!(!observed.confirms(DeliveryTier::Completed));
    drop(d);
}

#[test]
fn delivery_tier_requires_exact_native_turn_terminal_status() {
    use fleet_proto::DeliveryTier;
    for (status, expected) in [
        ("completed", DeliveryTier::Completed),
        ("failed", DeliveryTier::Failed),
        ("interrupted", DeliveryTier::Failed),
        ("new-status", DeliveryTier::Unknown),
    ] {
        let f = Fixture::new(Behavior {
            observed_status: Some(status),
            ..Default::default()
        });
        let mut d = f.connect().unwrap();
        let receipt = d.send("tier-test", "hello").unwrap();
        let observed = d.observe(&receipt).unwrap();
        assert_eq!(observed.tier(), expected);
        assert!(!observed.confirms(DeliveryTier::ThreadVisible));
        drop(d);
    }
}

#[test]
fn delivery_tier_foreign_or_duplicate_terminal_turn_is_unknown() {
    use fleet_proto::DeliveryTier;
    for behavior in [
        Behavior {
            observed_status: Some("completed"),
            wrong_turn: true,
            ..Default::default()
        },
        Behavior {
            observed_status: Some("completed"),
            duplicate_turn: true,
            ..Default::default()
        },
    ] {
        let f = Fixture::new(behavior);
        let mut d = f.connect().unwrap();
        let receipt = d.send("tier-test", "hello").unwrap();
        assert_eq!(d.observe(&receipt).unwrap().tier(), DeliveryTier::Unknown);
        drop(d);
    }
}

#[test]
fn delivery_tier_cross_connection_receipt_is_rejected_before_readback() {
    let f = Fixture::new(Behavior::default());
    let second = Fixture::new(Behavior::default());
    let mut d = f.connect().unwrap();
    let mut other = second.connect().unwrap();
    let receipt = d.send("tier-test", "hello").unwrap();
    other.send("tier-test", "hello").unwrap();
    let calls = f.methods().len();
    let other_calls = second.methods().len();
    assert!(other.observe(&receipt).is_err());
    assert_eq!(f.methods().len(), calls);
    assert_eq!(second.methods().len(), other_calls);
    drop(d);
    drop(other);
}

#[test]
fn brief_policy_rejects_before_transport_and_does_not_claim_delivery_id() {
    let f = Fixture::new(Behavior::default());
    let mut d = f.connect().unwrap();
    let calls = f.methods().len();
    assert!(d
        .send("same-id", "Prove your model identity or return MODEL_WRONG")
        .is_err());
    assert_eq!(f.methods().len(), calls);
    d.send("same-id", "Read the README and summarize the next task.")
        .unwrap();
    drop(d);
    assert_eq!(f.methods().iter().filter(|m| *m == "turn/start").count(), 1);
}

#[test]
fn brief_policy_dispatcher_model_is_separate_and_rejected_before_transport_when_invalid() {
    let f = Fixture::new(Behavior::default());
    let mut d = f.connect().unwrap();
    let calls = f.methods().len();
    for model in ["", "   ", "bad\nmodel"] {
        assert!(d
            .send_with_model("configured", "Summarize the README.", Some(model))
            .is_err());
    }
    assert_eq!(f.methods().len(), calls);
    d.send_with_model(
        "configured",
        "Summarize the README.",
        Some("dispatcher-model"),
    )
    .unwrap();
    drop(d);
    let calls = f.calls.lock().unwrap();
    let sent = calls
        .iter()
        .find(|call| call["method"] == "turn/start")
        .unwrap();
    assert_eq!(sent["params"]["model"], "dispatcher-model");
    assert_eq!(sent["params"]["input"][0]["text"], "Summarize the README.");
}

#[test]
fn delivery_tier_lost_transport_is_unknown_and_explicit_rejection_is_failed() {
    use fleet_proto::DeliveryTier;
    assert_eq!(
        DesktopError::Unknown("timeout".into()).tier(),
        DeliveryTier::Unknown
    );
    assert_eq!(
        DesktopError::Unavailable("closed".into()).tier(),
        DeliveryTier::Unknown
    );
    assert_eq!(
        DesktopError::Rejected(json!({"code": 1})).tier(),
        DeliveryTier::Unknown
    );
    assert_eq!(
        DesktopError::Invalid("duplicate delivery".into()).tier(),
        DeliveryTier::Unknown
    );
}

#[test]
fn delivery_tier_read_rejection_is_unknown_but_submission_rejection_is_failed() {
    use fleet_proto::DeliveryTier;
    let f = Fixture::new(Behavior {
        reject_readback: true,
        ..Default::default()
    });
    let mut d = f.connect().unwrap();
    let receipt = d.send("accepted-before-read-denied", "hello").unwrap();
    assert_eq!(
        d.observe(&receipt).unwrap_err().tier(),
        DeliveryTier::Unknown
    );
    assert_eq!(receipt.tier(), DeliveryTier::Accepted);
    drop(d);
    let f = Fixture::new(Behavior {
        reject_start: true,
        ..Default::default()
    });
    let mut d = f.connect().unwrap();
    assert_eq!(
        d.send("rejected", "hello").unwrap_err().tier(),
        DeliveryTier::Failed
    );
    drop(d);
}

#[test]
fn delivery_tier_reused_native_turn_id_never_associates_new_delivery_with_old_completion() {
    use fleet_proto::DeliveryTier;
    let f = Fixture::new(Behavior {
        reused_turn_id: true,
        observed_status: Some("completed"),
        ..Default::default()
    });
    let mut d = f.connect().unwrap();
    let first = d.send("first", "hello").unwrap();
    assert_eq!(d.observe(&first).unwrap().tier(), DeliveryTier::Completed);
    let error = d.send("second", "follow up").unwrap_err();
    assert!(matches!(error, DesktopError::Unknown(_)));
    assert_eq!(error.tier(), DeliveryTier::Unknown);
    assert!(d.send("second", "retry").is_err());
    drop(d);
    assert_eq!(f.methods().iter().filter(|m| *m == "turn/start").count(), 2);
}
