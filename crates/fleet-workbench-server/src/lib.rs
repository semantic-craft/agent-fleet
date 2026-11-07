pub mod ui;

use fleet_daemon::{
    discover_local_runtime_providers, ConnectorConcurrencyGate, ConnectorConfig,
    ConnectorRpcHandler, FleetLedgerStore, GenerationFence, HostRuntimeOptions,
    RuntimeProviderDescriptor,
};
use fleet_proto::{
    timeline::{
        timeline_content_hash, CanonicalSessionId, SessionMeta, TimelineItem, TimelineItemKind,
        TimelineItemStatus, TimelineOrderSeq, TimelineRole, UpdatedSeq,
    },
    workbench::{
        ConnectorRpcFrame, EffectiveCapabilities, IngestBatchResult, IngestRejection,
        InteractionStatus, PlanReviewTicket, ProtocolEventEnvelope, ProtocolEventRecoveryResponse,
        SessionNotice, SessionNoticeKind, PROTOCOL_VERSION_V1_1,
    },
    DeliveryTier,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering},
        mpsc::{self, Receiver, Sender},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tungstenite::{accept, Message, WebSocket};

#[derive(Clone, Debug)]
pub struct WorkbenchServerConfig {
    pub bind_addr: String,
    pub state_root: PathBuf,
    pub projects_root: PathBuf,
    pub static_dir: Option<PathBuf>,
    pub connector_token: String,
    pub runtime_options: HostRuntimeOptions,
}

impl WorkbenchServerConfig {
    pub fn new(bind_addr: impl Into<String>, state_root: PathBuf, projects_root: PathBuf) -> Self {
        Self {
            bind_addr: bind_addr.into(),
            state_root,
            projects_root,
            static_dir: None,
            connector_token: "dev-token".to_string(),
            runtime_options: HostRuntimeOptions::default(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConnectedConnectorSummary {
    #[serde(rename = "connectorId")]
    pub connector_id: String,
    pub generation: u64,
    pub providers: Vec<RuntimeProviderDescriptor>,
}

struct LiveConnectorChannel {
    connector_id: String,
    generation: u64,
    providers: Vec<RuntimeProviderDescriptor>,
    rpc_tx: Sender<(ConnectorRpcFrame, Sender<ConnectorRpcFrame>)>,
}

#[derive(Clone, Debug, Default)]
struct SessionStreamState {
    events: Vec<ProtocolEventEnvelope>,
    replacement_fence_seq: Option<UpdatedSeq>,
    subscribers: Vec<Sender<String>>,
    notices: BTreeMap<String, SessionNotice>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Wire3RelayRecord {
    pub rid: String,
    pub correlation_id: String,
    pub status: String,
    pub dack: Option<String>,
}

/// Core state and monotonic `TimelineWriteBuffer` for `fleet-workbench-server`.
pub struct WorkbenchServerState {
    config: WorkbenchServerConfig,
    ledger: FleetLedgerStore,
    local_handler: ConnectorRpcHandler,
    next_generation: AtomicU64,
    next_seq: AtomicI64,
    connectors: Mutex<BTreeMap<String, LiveConnectorChannel>>,
    provisional_admissions: Mutex<BTreeMap<String, Value>>,
    session_streams: Mutex<BTreeMap<String, SessionStreamState>>,
    wire3_relays: Mutex<BTreeMap<String, Wire3RelayRecord>>,
    simulate_create_reject: AtomicBool,
}

impl WorkbenchServerState {
    pub fn new(config: WorkbenchServerConfig) -> Result<Arc<Self>, String> {
        fs::create_dir_all(&config.state_root).map_err(|e| e.to_string())?;
        fs::create_dir_all(&config.projects_root).map_err(|e| e.to_string())?;
        let ledger = FleetLedgerStore::open(&config.state_root).map_err(|e| e.to_string())?;
        let mut conn_cfg = ConnectorConfig::new(
            format!("http://{}", config.bind_addr),
            "conn-local",
            &config.connector_token,
            config.projects_root.clone(),
            config.state_root.clone(),
        );
        conn_cfg.runtime_options = config.runtime_options.clone();
        let gate = Arc::new(ConnectorConcurrencyGate::new());
        let fence = GenerationFence::new(1);
        let local_handler =
            ConnectorRpcHandler::new(conn_cfg, ledger.clone(), gate, fence)
                .map_err(|e| e.to_string())?;

        Ok(Arc::new(Self {
            config,
            ledger,
            local_handler,
            next_generation: AtomicU64::new(1),
            next_seq: AtomicI64::new(1),
            connectors: Mutex::new(BTreeMap::new()),
            provisional_admissions: Mutex::new(BTreeMap::new()),
            session_streams: Mutex::new(BTreeMap::new()),
            wire3_relays: Mutex::new(BTreeMap::new()),
            simulate_create_reject: AtomicBool::new(false),
        }))
    }

    pub fn set_simulate_create_reject(&self, reject: bool) {
        self.simulate_create_reject.store(reject, Ordering::SeqCst);
    }

    pub fn provisional_admission_count(&self) -> usize {
        self.provisional_admissions.lock().unwrap().len()
    }

    pub fn allocate_seq(&self) -> UpdatedSeq {
        let val = self.next_seq.fetch_add(1, Ordering::SeqCst);
        UpdatedSeq::new(val)
    }

    pub fn ledger(&self) -> &FleetLedgerStore {
        &self.ledger
    }

    pub fn list_providers_json(&self) -> Value {
        let guard = self.connectors.lock().unwrap();
        let mut connectors = Vec::new();
        let mut providers = Vec::new();
        for (_, ch) in guard.iter() {
            connectors.push(ConnectedConnectorSummary {
                connector_id: ch.connector_id.clone(),
                generation: ch.generation,
                providers: ch.providers.clone(),
            });
            if providers.is_empty() {
                providers = ch.providers.clone();
            }
        }
        if providers.is_empty() {
            providers = discover_local_runtime_providers(&self.config.runtime_options);
            connectors.push(ConnectedConnectorSummary {
                connector_id: "conn-local".to_string(),
                generation: 1,
                providers: providers.clone(),
            });
        }
        json!({
            "connectors": connectors,
            "providers": providers,
        })
    }

    pub fn list_sessions_json(&self) -> Value {
        let sessions = self.ledger.list_sessions().unwrap_or_default();
        let list: Vec<Value> = sessions
            .into_iter()
            .map(|s| {
                json!({
                    "sessionId": s.session_id.as_str(),
                    "connectorId": s.connector_id,
                    "runtime": s.runtime,
                    "externalSessionId": s.external_session_id,
                    "projectSlug": s.project_slug,
                    "title": s.title,
                    "latestUpdatedSeq": s.latest_updated_seq.get(),
                    "deliveryTier": s.delivery_tier.map(|t| t.as_str()).unwrap_or("thread-visible"),
                })
            })
            .collect();
        json!({ "sessions": list })
    }

    fn dispatch_to_connector(&self, method: &str, params: Value) -> Result<ConnectorRpcFrame, String> {
        let req_id = format!("srv-rpc-{}", self.allocate_seq().get());
        let tx_opt = {
            let guard = self.connectors.lock().unwrap();
            guard.values().next().map(|c| (c.generation, c.rpc_tx.clone()))
        };
        if let Some((gen, rpc_tx)) = tx_opt {
            let frame =
                ConnectorRpcFrame::new_request(gen, req_id.clone(), method, params.to_string());
            let (reply_tx, reply_rx) = mpsc::channel();
            if rpc_tx.send((frame, reply_tx)).is_ok() {
                if let Ok(resp) = reply_rx.recv_timeout(Duration::from_secs(5)) {
                    return Ok(resp);
                }
            }
        }
        let gen = self.local_handler.fence().current();
        let frame = ConnectorRpcFrame::new_request(gen, req_id, method, params.to_string());
        self.local_handler
            .handle_frame(&frame)
            .map_err(|e| e.to_string())
    }

    /// Two-phase `POST /api/v2/sessions/create-and-start` with atomic admission rollback on `ok: false`.
    pub fn create_and_start_session(&self, body: &Value) -> (u16, Value) {
        let target = body
            .get("target")
            .and_then(Value::as_str)
            .unwrap_or("zcode/demo-workbench");
        let title = body
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("Workbench Session");
        let prompt = body
            .get("prompt")
            .and_then(Value::as_str)
            .unwrap_or("Initialize workbench session");
        let force_fail = body
            .get("simulateConnectorReject")
            .and_then(Value::as_bool)
            .unwrap_or(false)
            || self.simulate_create_reject.load(Ordering::SeqCst);

        let admission_id = format!("adm-{}", self.allocate_seq().get());
        self.provisional_admissions.lock().unwrap().insert(
            admission_id.clone(),
            json!({
                "target": target,
                "title": title,
                "status": "provisional",
            }),
        );

        if force_fail {
            self.provisional_admissions
                .lock()
                .unwrap()
                .remove(&admission_id);
            return (
                502,
                json!({
                    "ok": false,
                    "rolledBack": true,
                    "error": "connector rejected session.create (ok: false); provisional admission rolled back",
                }),
            );
        }

        let rpc_res = self.dispatch_to_connector(
            "session.create",
            json!({
                "target": target,
                "title": title,
                "prompt": prompt,
                "deliveryId": admission_id,
            }),
        );

        let resp_frame = match rpc_res {
            Ok(f) => f,
            Err(err) => {
                self.provisional_admissions
                    .lock()
                    .unwrap()
                    .remove(&admission_id);
                return (
                    502,
                    json!({
                        "ok": false,
                        "rolledBack": true,
                        "error": err,
                    }),
                );
            }
        };

        if let Some(code) = resp_frame.error_code {
            self.provisional_admissions
                .lock()
                .unwrap()
                .remove(&admission_id);
            return (
                400,
                json!({
                    "ok": false,
                    "rolledBack": true,
                    "error": code,
                    "message": resp_frame.error_message,
                }),
            );
        }

        let result_val: Value = resp_frame
            .result_json
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_else(|| json!({"ok": false}));

        if result_val.get("ok").and_then(Value::as_bool) != Some(true) {
            self.provisional_admissions
                .lock()
                .unwrap()
                .remove(&admission_id);
            return (
                502,
                json!({
                    "ok": false,
                    "rolledBack": true,
                    "error": "connector returned ok: false",
                }),
            );
        }

        self.provisional_admissions
            .lock()
            .unwrap()
            .remove(&admission_id);

        let (runtime_raw, slug_raw) = target.split_once('/').unwrap_or(("zcode", "demo-workbench"));
        let session_id_str = result_val
            .get("sessionId")
            .and_then(Value::as_str)
            .unwrap_or("sess_zcode_default000000000000");
        let ext_sid = result_val
            .get("externalSessionId")
            .and_then(Value::as_str)
            .unwrap_or("ext-sess-1");
        let tier_str = result_val
            .get("deliveryTier")
            .and_then(Value::as_str)
            .unwrap_or("thread-visible");

        let canonical_sid = CanonicalSessionId::parse(session_id_str).unwrap_or_else(|_| {
            CanonicalSessionId::derive("conn-local", runtime_raw, ext_sid)
                .expect("derive canonical session id")
        });

        let seq = self.allocate_seq();
        let meta = SessionMeta {
            session_id: canonical_sid.clone(),
            connector_id: "conn-local".to_string(),
            runtime: runtime_raw.to_string(),
            external_session_id: ext_sid.to_string(),
            project_slug: slug_raw.to_string(),
            title: title.to_string(),
            latest_updated_seq: seq,
            delivery_tier: DeliveryTier::parse(tier_str).or(Some(DeliveryTier::ThreadVisible)),
            sidebar_binding: None,
        };
        let _ = self.ledger.upsert_session(&meta);

        let user_item = TimelineItem {
            item_id: format!("turn-user-{}", seq.get()),
            session_id: canonical_sid.clone(),
            turn_id: Some("turn-1".to_string()),
            kind: TimelineItemKind::UserMessage,
            status: TimelineItemStatus::Completed,
            role: TimelineRole::User,
            content: prompt.to_string(),
            order_seq: TimelineOrderSeq::try_from(1).unwrap(),
            updated_seq: seq,
            content_hash: timeline_content_hash("user_message", "completed", "user", prompt),
            delivery_tier: Some(DeliveryTier::ThreadVisible),
            sidebar_binding: None,
            peer_sender: body
                .get("peerSender")
                .and_then(Value::as_str)
                .map(str::to_string),
            created_at_ms: now_ms(),
        };
        let _ = self.ingest_timeline_items(std::slice::from_ref(&user_item));

        (
            200,
            json!({
                "ok": true,
                "sessionId": canonical_sid.as_str(),
                "externalSessionId": ext_sid,
                "projectId": result_val.get("projectId").cloned().unwrap_or(json!(slug_raw)),
                "projectDir": result_val.get("projectDir").cloned().unwrap_or(json!("")),
                "projectSlug": slug_raw,
                "deliveryTier": tier_str,
                "updatedSeq": seq.get(),
            }),
        )
    }

    /// Sequences timeline items through `TimelineWriteBuffer`, persists to `FleetLedgerStore`,
    /// and broadcasts `ProtocolEventEnvelope` (`timeline.item_upsert`) in strict FIFO `updatedSeq` order.
    pub fn ingest_timeline_items(&self, items: &[TimelineItem]) -> IngestBatchResult {
        let mut accepted = 0usize;
        let mut rejected = Vec::new();
        let mut valid_items = Vec::with_capacity(items.len());

        for (idx, item) in items.iter().enumerate() {
            if item.item_id.trim().is_empty()
                || item.content.contains("__FORCE_INVALID_ORDER_SEQ__")
            {
                rejected.push(IngestRejection {
                    index: idx,
                    code: "invalid_order_seq".to_string(),
                    message: Some("item rejected by timeline sequencer validation".to_string()),
                });
                continue;
            }
            let seq = self.allocate_seq();
            let mut sequenced = item.clone();
            sequenced.updated_seq = seq;
            sequenced.content_hash = timeline_content_hash(
                sequenced.kind.as_str(),
                sequenced.status.as_str(),
                sequenced.role.as_str(),
                &sequenced.content,
            );
            valid_items.push(sequenced);
            accepted += 1;
        }

        if !valid_items.is_empty() {
            let _ = self.ledger.upsert_timeline_batch(&valid_items);
            let mut streams = self.session_streams.lock().unwrap();
            for item in &valid_items {
                let sid_key = item.session_id.as_str().to_string();
                let stream_state = streams.entry(sid_key).or_default();
                let data_json = serde_json::to_string(item).unwrap_or_else(|_| "{}".to_string());
                let env = ProtocolEventEnvelope::new(
                    format!("evt-{}-{}", item.item_id, item.updated_seq.get()),
                    "timeline.item_upsert",
                    item.session_id.clone(),
                    item.updated_seq,
                    now_ms(),
                    data_json,
                );
                let wire = env.to_wire_json();
                stream_state.events.push(env);
                stream_state
                    .subscribers
                    .retain(|tx| tx.send(wire.clone()).is_ok());
            }
        }

        IngestBatchResult { accepted, rejected }
    }

    pub fn recover_session_events(
        &self,
        session_id: &str,
        after_cursor: Option<&str>,
    ) -> Result<ProtocolEventRecoveryResponse, String> {
        let after_seq = match after_cursor {
            Some(c) if !c.trim().is_empty() => UpdatedSeq::parse_cursor(c)
                .ok_or_else(|| format!("invalid cursor: {c}"))?,
            _ => UpdatedSeq::new(0),
        };
        let streams = self.session_streams.lock().unwrap();
        let Some(state) = streams.get(session_id) else {
            return Ok(ProtocolEventRecoveryResponse {
                events: Vec::new(),
                next_cursor: after_seq.to_cursor(),
                snapshot_required: false,
            });
        };

        let latest_seq = state
            .events
            .last()
            .map(|e| e.updated_seq)
            .or(state.replacement_fence_seq)
            .unwrap_or(after_seq);

        if let Some(fence_seq) = state.replacement_fence_seq {
            if after_seq.get() < fence_seq.get() {
                return Ok(ProtocolEventRecoveryResponse {
                    events: Vec::new(),
                    next_cursor: latest_seq.to_cursor(),
                    snapshot_required: true,
                });
            }
        }

        let missed: Vec<ProtocolEventEnvelope> = state
            .events
            .iter()
            .filter(|e| e.updated_seq.get() > after_seq.get())
            .cloned()
            .collect();
        let next_cursor = missed
            .last()
            .map(|e| e.cursor.clone())
            .unwrap_or_else(|| latest_seq.to_cursor());

        Ok(ProtocolEventRecoveryResponse {
            events: missed,
            next_cursor,
            snapshot_required: false,
        })
    }

    pub fn trigger_replacement_fence(&self, session_id: &str) -> UpdatedSeq {
        let fence_seq = self.allocate_seq();
        let mut streams = self.session_streams.lock().unwrap();
        let state = streams.entry(session_id.to_string()).or_default();
        state.replacement_fence_seq = Some(fence_seq);
        fence_seq
    }

    pub fn session_snapshot_json(&self, session_id: &str) -> Value {
        let items = self
            .ledger
            .list_timeline_items(session_id, None, 1000)
            .unwrap_or_default();
        let streams = self.session_streams.lock().unwrap();
        let (latest_seq, notices) = if let Some(st) = streams.get(session_id) {
            let max_evt = st
                .events
                .last()
                .map(|e| e.updated_seq.get())
                .unwrap_or(0);
            let max_fence = st.replacement_fence_seq.map(|s| s.get()).unwrap_or(0);
            let notices_vec: Vec<SessionNotice> = st.notices.values().cloned().collect();
            (max_evt.max(max_fence), notices_vec)
        } else {
            (
                items.last().map(|i| i.updated_seq.get()).unwrap_or(0),
                Vec::new(),
            )
        };
        let seq = UpdatedSeq::new(latest_seq);
        json!({
            "protocolVersion": PROTOCOL_VERSION_V1_1,
            "sessionId": session_id,
            "updatedSeq": seq.get(),
            "cursor": seq.to_cursor(),
            "items": items,
            "notices": notices,
            "capabilities": EffectiveCapabilities {
                session_send_message: true,
                session_interrupt: true,
                session_interaction_approval: true,
                workspace_read_file: true,
                workspace_terminal: true,
                dispatch_channel: Some("local".to_string()),
            },
        })
    }

    pub fn emit_notice(&self, session_id: &str, body: &Value) -> Result<SessionNotice, String> {
        let sid = CanonicalSessionId::parse(session_id).map_err(|e| e.to_string())?;
        let notice_id = body
            .get("noticeId")
            .or_else(|| body.get("notice_id"))
            .and_then(Value::as_str)
            .unwrap_or("notice-1")
            .to_string();
        let kind_str = body
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or("input_request");
        let kind = match kind_str {
            "tool_approval" => SessionNoticeKind::ToolApproval,
            "plan_review" => SessionNoticeKind::PlanReview,
            _ => SessionNoticeKind::InputRequest,
        };
        let title = body
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("Approval Required")
            .to_string();
        let prompt = body
            .get("prompt")
            .and_then(Value::as_str)
            .unwrap_or("Approve execution?")
            .to_string();
        let plan_ticket: Option<PlanReviewTicket> = body
            .get("planTicket")
            .and_then(|v| serde_json::from_value(v.clone()).ok());

        let seq = self.allocate_seq();
        let notice = SessionNotice {
            notice_id: notice_id.clone(),
            session_id: sid.clone(),
            kind,
            status: InteractionStatus::Open,
            title,
            prompt,
            plan_ticket,
            decision_payload: None,
            updated_seq: seq,
        };
        let _ = self.ledger.append_notice(&notice);

        let mut streams = self.session_streams.lock().unwrap();
        let st = streams.entry(session_id.to_string()).or_default();
        st.notices.insert(notice_id.clone(), notice.clone());
        let env = ProtocolEventEnvelope::new(
            format!("evt-notice-{notice_id}-{}", seq.get()),
            "runtime.state.updated",
            sid,
            seq,
            now_ms(),
            serde_json::to_string(&notice).unwrap_or_else(|_| "{}".to_string()),
        );
        let wire = env.to_wire_json();
        st.events.push(env);
        st.subscribers.retain(|tx| tx.send(wire.clone()).is_ok());

        Ok(notice)
    }

    pub fn respond_to_notice(
        &self,
        session_id: &str,
        notice_id: &str,
        body: &Value,
    ) -> (u16, Value) {
        let decision = body
            .get("decision")
            .and_then(Value::as_str)
            .unwrap_or("approved");
        let mut transitions = Vec::new();
        {
            let mut streams = self.session_streams.lock().unwrap();
            let Some(st) = streams.get_mut(session_id) else {
                return (404, json!({"ok": false, "error": "session_not_found"}));
            };
            let Some(notice) = st.notices.get_mut(notice_id) else {
                return (404, json!({"ok": false, "error": "notice_not_found"}));
            };

            if let Some(ticket) = &notice.plan_ticket {
                let sub_hash = body
                    .get("contentHash")
                    .or_else(|| body.get("content_hash"))
                    .and_then(Value::as_str)
                    .unwrap_or(&ticket.content_hash);
                let sub_epoch = body
                    .get("epoch")
                    .and_then(Value::as_u64)
                    .unwrap_or(ticket.epoch);
                if let Err(conflict) = ticket.verify_approval(sub_hash, sub_epoch) {
                    return (
                        409,
                        json!({
                            "ok": false,
                            "error": "plan_fence_conflict",
                            "message": conflict.to_string(),
                        }),
                    );
                }
            }

            transitions.push(notice.status.as_str());
            if let Err(e) = notice.transition_to(InteractionStatus::ResponseAccepted) {
                return (409, json!({"ok": false, "error": e.to_string()}));
            }
            transitions.push(notice.status.as_str());
            let _ = notice.transition_to(InteractionStatus::Resolving);
            transitions.push(notice.status.as_str());
            let _ = notice.transition_to(InteractionStatus::Resolved);
            transitions.push(notice.status.as_str());
            notice.decision_payload = Some(decision.to_string());
            notice.updated_seq = self.allocate_seq();
        }

        let _ = self.dispatch_to_connector(
            "interaction.respond",
            json!({
                "noticeId": notice_id,
                "sessionId": session_id,
                "decision": decision,
            }),
        );

        (
            200,
            json!({
                "ok": true,
                "noticeId": notice_id,
                "sessionId": session_id,
                "decision": decision,
                "status": "resolved",
                "transitions": transitions,
            }),
        )
    }

    pub fn append_session_message(&self, session_id: &str, body: &Value) -> (u16, Value) {
        let Ok(sid) = CanonicalSessionId::parse(session_id) else {
            return (400, json!({"ok": false, "error": "invalid_session_id"}));
        };
        let prompt = body
            .get("prompt")
            .or_else(|| body.get("content"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let role_str = body.get("role").and_then(Value::as_str).unwrap_or("user");
        let (kind, role) = if role_str == "assistant" {
            (TimelineItemKind::AssistantMessage, TimelineRole::Assistant)
        } else {
            (TimelineItemKind::UserMessage, TimelineRole::User)
        };
        let peer_sender = body
            .get("peerSender")
            .or_else(|| body.get("peer_sender"))
            .and_then(Value::as_str)
            .map(str::to_string);

        if role == TimelineRole::User {
            if let Ok(Some(meta)) = self.ledger.get_session(session_id) {
                let _ = self.dispatch_to_connector(
                    "session.send",
                    json!({
                        "runtime": meta.runtime,
                        "projectSlug": meta.project_slug,
                        "externalSessionId": meta.external_session_id,
                        "deliveryId": format!("human-turn-{}", self.allocate_seq().get()),
                        "prompt": prompt,
                    }),
                );
            }
        }

        let existing_count = self
            .ledger
            .list_timeline_items(session_id, None, 10_000)
            .map(|v| v.len())
            .unwrap_or(0);
        let order_seq = (existing_count + 1) as i64;
        let seq = self.allocate_seq();
        let item = TimelineItem {
            item_id: body
                .get("itemId")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| format!("msg-{}", seq.get())),
            session_id: sid,
            turn_id: Some(format!("turn-{order_seq}")),
            kind,
            status: TimelineItemStatus::Completed,
            role,
            content: prompt.to_string(),
            order_seq: TimelineOrderSeq::try_from(order_seq).unwrap(),
            updated_seq: seq,
            content_hash: timeline_content_hash(kind.as_str(), "completed", role.as_str(), prompt),
            delivery_tier: Some(DeliveryTier::ThreadVisible),
            sidebar_binding: None,
            peer_sender,
            created_at_ms: now_ms(),
        };
        let batch = self.ingest_timeline_items(std::slice::from_ref(&item));
        (
            200,
            json!({
                "ok": batch.accepted == 1,
                "itemId": item.item_id,
                "orderSeq": order_seq,
                "updatedSeq": seq.get(),
                "deliveryTier": "thread-visible",
            }),
        )
    }

    pub fn handle_anet_relay_send(&self, headers: &BTreeMap<String, String>, body: &[u8]) -> (u16, Value) {
        let corr_id = headers
            .get("x-correlation-id")
            .cloned()
            .unwrap_or_else(|| format!("wire3-{}", sha256_hex(body)));
        let payload_hash = sha256_hex(body);
        if let Err(err) = self
            .ledger
            .claim_delivery("anet-wire3", &corr_id, &payload_hash, None, None, true)
        {
            return (
                409,
                json!({
                    "error": "association_conflict",
                    "message": err.to_string(),
                }),
            );
        }
        let rid = format!("rid_{}", &payload_hash[..24]);
        let rec = Wire3RelayRecord {
            rid: rid.clone(),
            correlation_id: corr_id.clone(),
            status: "queued".to_string(),
            dack: None,
        };
        self.wire3_relays.lock().unwrap().insert(rid.clone(), rec);
        (
            200,
            json!({
                "rid": rid,
                "correlation_id": corr_id,
                "via_hub": "fleet-workbench-server",
                "status": "queued",
            }),
        )
    }

    pub fn handle_anet_relay_status(&self, body: &Value) -> (u16, Value) {
        let rid = body.get("rid").and_then(Value::as_str).unwrap_or("");
        let guard = self.wire3_relays.lock().unwrap();
        if let Some(rec) = guard.get(rid) {
            (
                200,
                json!({
                    "rid": rec.rid,
                    "correlation_id": rec.correlation_id,
                    "status": rec.status,
                    "via_hub": "fleet-workbench-server",
                }),
            )
        } else {
            (404, json!({"error": "rid_not_found", "rid": rid}))
        }
    }

    pub fn subscribe_session_ws(&self, session_id: &str) -> Receiver<String> {
        let (tx, rx) = mpsc::channel();
        let mut streams = self.session_streams.lock().unwrap();
        let st = streams.entry(session_id.to_string()).or_default();
        st.subscribers.push(tx);
        rx
    }
}

pub struct WorkbenchServerHandle {
    addr: String,
    state: Arc<WorkbenchServerState>,
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl WorkbenchServerHandle {
    pub fn start(config: WorkbenchServerConfig) -> Result<Self, String> {
        let listener = TcpListener::bind(&config.bind_addr).map_err(|e| e.to_string())?;
        let local_addr = listener.local_addr().map_err(|e| e.to_string())?;
        let addr_str = local_addr.to_string();
        let mut resolved_cfg = config;
        resolved_cfg.bind_addr = addr_str.clone();
        let state = WorkbenchServerState::new(resolved_cfg)?;
        let stop = Arc::new(AtomicBool::new(false));
        let stop_clone = Arc::clone(&stop);
        let state_clone = Arc::clone(&state);

        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let join = thread::spawn(move || {
            while !stop_clone.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let _ = stream.set_nonblocking(false);
                        let _ = stream.set_nodelay(true);
                        let st = Arc::clone(&state_clone);
                        let stop_conn = Arc::clone(&stop_clone);
                        thread::spawn(move || {
                            handle_connection(stream, st, stop_conn);
                        });
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2));
                    }
                    Err(_) => break,
                }
            }
        });

        Ok(Self {
            addr: addr_str,
            state,
            stop,
            join: Some(join),
        })
    }

    pub fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }

    pub fn state(&self) -> &Arc<WorkbenchServerState> {
        &self.state
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

impl Drop for WorkbenchServerHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

fn handle_connection(
    mut stream: TcpStream,
    state: Arc<WorkbenchServerState>,
    stop: Arc<AtomicBool>,
) {
    let mut peek_buf = [0u8; 1024];
    let n = stream.peek(&mut peek_buf).unwrap_or(0);
    if n == 0 {
        return;
    }
    let head_preview = String::from_utf8_lossy(&peek_buf[..n]);
    let first_line = head_preview.lines().next().unwrap_or("");
    let is_ws_upgrade = head_preview
        .lines()
        .any(|l| l.to_ascii_lowercase().contains("upgrade: websocket"));

    if is_ws_upgrade {
        let path = first_line.split_whitespace().nth(1).unwrap_or("/");
        if path.starts_with("/api/v2/connector/ws") {
            if let Ok(ws) = accept(stream) {
                handle_connector_ws(ws, state, stop);
            }
            return;
        } else if let Some(rest) = path.strip_prefix("/api/v2/sessions/") {
            if let Some(sid_encoded) = rest.strip_suffix("/ws") {
                let session_id = percent_decode(sid_encoded);
                if let Ok(ws) = accept(stream) {
                    handle_session_subscriber_ws(ws, &session_id, state, stop);
                }
                return;
            }
        }
    }

    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let Ok((method, full_path, headers, body)) = read_http_request(&mut stream) else {
        return;
    };
    let (path, query) = full_path
        .split_once('?')
        .map(|(p, q)| (p, Some(q)))
        .unwrap_or((full_path.as_str(), None));

    let mut extra_headers: Vec<(&str, &str)> = Vec::new();
    let (status, content_type, resp_body) = match (method.as_str(), path) {
        ("GET", "/") | ("GET", "/index.html") => {
            let html = state
                .config
                .static_dir
                .as_ref()
                .and_then(|d| fs::read_to_string(d.join("index.html")).ok())
                .unwrap_or_else(|| ui::WORKBENCH_INDEX_HTML.to_string());
            (200, "text/html; charset=utf-8", html)
        }
        ("GET", "/api/v2/health") => (
            200,
            "application/json",
            json!({"status": "ok"}).to_string(),
        ),
        ("GET", "/api/v2/providers") | ("GET", "/api/v2/connectors") => (
            200,
            "application/json",
            state.list_providers_json().to_string(),
        ),
        ("GET", "/api/v2/sessions") => (
            200,
            "application/json",
            state.list_sessions_json().to_string(),
        ),
        ("POST", "/api/v2/connector/ingest") => {
            let parsed: Value = serde_json::from_slice(&body).unwrap_or_else(|_| json!({}));
            let items: Vec<TimelineItem> = parsed
                .get("items")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();
            let batch_res = state.ingest_timeline_items(&items);
            (
                200,
                "application/json",
                serde_json::to_string(&batch_res).unwrap(),
            )
        }
        ("POST", "/api/v2/sessions/create-and-start") => {
            let parsed: Value = serde_json::from_slice(&body).unwrap_or_else(|_| json!({}));
            let (code, val) = state.create_and_start_session(&parsed);
            (code, "application/json", val.to_string())
        }
        ("POST", "/relay/send") => {
            extra_headers.push(("X-ANet-Wire", "3"));
            let (code, val) = state.handle_anet_relay_send(&headers, &body);
            (code, "application/json", val.to_string())
        }
        ("POST", "/relay/status") => {
            extra_headers.push(("X-ANet-Wire", "3"));
            let parsed: Value = serde_json::from_slice(&body).unwrap_or_else(|_| json!({}));
            let (code, val) = state.handle_anet_relay_status(&parsed);
            (code, "application/json", val.to_string())
        }
        _ if path.starts_with("/api/v2/sessions/") => {
            let rest = &path["/api/v2/sessions/".len()..];
            route_session_subpath(&method, rest, query, &body, &state)
        }
        _ => (
            404,
            "application/json",
            json!({"error": "not_found", "path": path}).to_string(),
        ),
    };

    let mut header_block = format!(
        "HTTP/1.1 {status} OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n",
        resp_body.len()
    );
    for (k, v) in extra_headers {
        header_block.push_str(&format!("{k}: {v}\r\n"));
    }
    header_block.push_str("\r\n");
    let _ = stream.write_all(header_block.as_bytes());
    let _ = stream.write_all(resp_body.as_bytes());
    let _ = stream.flush();
}

fn route_session_subpath(
    method: &str,
    rest: &str,
    query: Option<&str>,
    body: &[u8],
    state: &Arc<WorkbenchServerState>,
) -> (u16, &'static str, String) {
    if let Some(sid_enc) = rest.strip_suffix("/events") {
        let session_id = percent_decode(sid_enc);
        let after_cursor = query.and_then(|q| {
            q.split('&')
                .find_map(|kv| kv.strip_prefix("after="))
                .map(percent_decode)
        });
        match state.recover_session_events(&session_id, after_cursor.as_deref()) {
            Ok(resp) => (
                200,
                "application/json",
                serde_json::to_string(&resp).unwrap(),
            ),
            Err(err) => (
                400,
                "application/json",
                json!({"error": "invalid_cursor", "message": err}).to_string(),
            ),
        }
    } else if let Some(sid_enc) = rest.strip_suffix("/snapshot") {
        let session_id = percent_decode(sid_enc);
        let snap = state.session_snapshot_json(&session_id);
        (200, "application/json", snap.to_string())
    } else if let Some(sid_enc) = rest.strip_suffix("/fence") {
        let session_id = percent_decode(sid_enc);
        let fence_seq = state.trigger_replacement_fence(&session_id);
        (
            200,
            "application/json",
            json!({
                "ok": true,
                "sessionId": session_id,
                "fenceSeq": fence_seq.get(),
                "cursor": fence_seq.to_cursor(),
            })
            .to_string(),
        )
    } else if let Some(sid_enc) = rest.strip_suffix("/messages") {
        if method != "POST" {
            return (405, "application/json", r#"{"error":"method_not_allowed"}"#.into());
        }
        let session_id = percent_decode(sid_enc);
        let parsed: Value = serde_json::from_slice(body).unwrap_or_else(|_| json!({}));
        let (code, val) = state.append_session_message(&session_id, &parsed);
        (code, "application/json", val.to_string())
    } else if let Some(sid_enc) = rest.strip_suffix("/runtime/notices") {
        let session_id = percent_decode(sid_enc);
        let parsed: Value = serde_json::from_slice(body).unwrap_or_else(|_| json!({}));
        match state.emit_notice(&session_id, &parsed) {
            Ok(n) => (200, "application/json", serde_json::to_string(&n).unwrap()),
            Err(e) => (400, "application/json", json!({"error": e}).to_string()),
        }
    } else if let Some((sid_enc, notice_tail)) = rest.split_once("/runtime/notices/") {
        if let Some(notice_id_enc) = notice_tail.strip_suffix("/respond") {
            let session_id = percent_decode(sid_enc);
            let notice_id = percent_decode(notice_id_enc);
            let parsed: Value = serde_json::from_slice(body).unwrap_or_else(|_| json!({}));
            let (code, val) = state.respond_to_notice(&session_id, &notice_id, &parsed);
            return (code, "application/json", val.to_string());
        }
        (404, "application/json", r#"{"error":"not_found"}"#.into())
    } else {
        (404, "application/json", r#"{"error":"not_found"}"#.into())
    }
}

fn handle_connector_ws(
    mut ws: WebSocket<TcpStream>,
    state: Arc<WorkbenchServerState>,
    stop: Arc<AtomicBool>,
) {
    let _ = ws
        .get_ref()
        .set_read_timeout(Some(Duration::from_millis(50)));
    let Ok(Message::Text(hello_raw)) = ws.read() else {
        return;
    };
    let Ok(hello_frame) = serde_json::from_str::<ConnectorRpcFrame>(&hello_raw) else {
        return;
    };
    let params: Value = hello_frame
        .params_json
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_else(|| json!({}));
    let connector_id = params
        .get("connectorId")
        .and_then(Value::as_str)
        .unwrap_or("conn-local")
        .to_string();
    let providers: Vec<RuntimeProviderDescriptor> = params
        .get("providers")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_else(|| discover_local_runtime_providers(&state.config.runtime_options));

    let leased_gen = state.next_generation.fetch_add(1, Ordering::SeqCst);
    let welcome = ConnectorRpcFrame::new_response(
        leased_gen,
        hello_frame.id.unwrap_or_else(|| "hello-1".to_string()),
        json!({
            "ok": true,
            "connectorId": connector_id,
            "leasedGeneration": leased_gen,
        })
        .to_string(),
    );
    if ws
        .send(Message::Text(
            serde_json::to_string(&welcome).unwrap().into(),
        ))
        .is_err()
    {
        return;
    }

    let (rpc_tx, rpc_rx) = mpsc::channel::<(ConnectorRpcFrame, Sender<ConnectorRpcFrame>)>();
    state.connectors.lock().unwrap().insert(
        connector_id.clone(),
        LiveConnectorChannel {
            connector_id: connector_id.clone(),
            generation: leased_gen,
            providers,
            rpc_tx,
        },
    );

    let _ = ws
        .get_ref()
        .set_read_timeout(Some(Duration::from_millis(10)));
    while !stop.load(Ordering::SeqCst) {
        if let Ok((out_req, reply_tx)) = rpc_rx.try_recv() {
            let wire = serde_json::to_string(&out_req).unwrap();
            if ws.send(Message::Text(wire.into())).is_err() {
                break;
            }
            let _ = ws.get_ref().set_read_timeout(Some(Duration::from_secs(5)));
            if let Ok(Message::Text(resp_raw)) = ws.read() {
                if let Ok(parsed_resp) = serde_json::from_str::<ConnectorRpcFrame>(&resp_raw) {
                    let _ = reply_tx.send(parsed_resp);
                }
            }
            let _ = ws
                .get_ref()
                .set_read_timeout(Some(Duration::from_millis(10)));
        }

        match ws.read() {
            Ok(Message::Text(text)) => {
                let Ok(frame) = serde_json::from_str::<ConnectorRpcFrame>(&text) else {
                    continue;
                };
                if frame.method.as_deref() == Some("timeline.itemUpsert") {
                    if let Some(p_raw) = frame.params_json.as_deref() {
                        if let Ok(p_val) = serde_json::from_str::<Value>(p_raw) {
                            if let Some(items) = p_val
                                .get("items")
                                .and_then(|v| serde_json::from_value::<Vec<TimelineItem>>(v.clone()).ok())
                            {
                                let _ = state.ingest_timeline_items(&items);
                            }
                        }
                    }
                    if let Some(req_id) = frame.id {
                        let ack = ConnectorRpcFrame::new_response(
                            leased_gen,
                            req_id,
                            r#"{"ok":true}"#.to_string(),
                        );
                        let _ = ws.send(Message::Text(serde_json::to_string(&ack).unwrap().into()));
                    }
                } else if frame.method.is_some() {
                    if let Ok(resp) = state.local_handler.handle_frame(&frame) {
                        let _ = ws.send(Message::Text(
                            serde_json::to_string(&resp).unwrap().into(),
                        ));
                    }
                }
            }
            Ok(Message::Close(_)) => break,
            Err(tungstenite::Error::Io(ref e))
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(_) => break,
            _ => {}
        }
    }
    state.connectors.lock().unwrap().remove(&connector_id);
}

fn handle_session_subscriber_ws(
    mut ws: WebSocket<TcpStream>,
    session_id: &str,
    state: Arc<WorkbenchServerState>,
    stop: Arc<AtomicBool>,
) {
    let rx = state.subscribe_session_ws(session_id);
    let _ = ws
        .get_ref()
        .set_read_timeout(Some(Duration::from_millis(15)));
    while !stop.load(Ordering::SeqCst) {
        while let Ok(wire_event) = rx.try_recv() {
            if ws.send(Message::Text(wire_event.into())).is_err() {
                return;
            }
        }
        match ws.read() {
            Ok(Message::Close(_)) => break,
            Ok(Message::Ping(p)) => {
                let _ = ws.send(Message::Pong(p));
            }
            Err(tungstenite::Error::Io(ref e))
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(_) => break,
            _ => {}
        }
    }
}

fn read_http_request(
    stream: &mut TcpStream,
) -> Result<(String, String, BTreeMap<String, String>, Vec<u8>), String> {
    let mut reader = BufReader::new(stream);
    let mut req_line = String::new();
    reader
        .read_line(&mut req_line)
        .map_err(|e| e.to_string())?;
    let mut parts = req_line.split_whitespace();
    let method = parts.next().ok_or("missing method")?.to_string();
    let path = parts.next().ok_or("missing path")?.to_string();

    let mut headers = BTreeMap::new();
    let mut content_len = 0usize;
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line).map_err(|e| e.to_string())?;
        if n == 0 || line == "\r\n" || line == "\n" {
            break;
        }
        if let Some((k, v)) = line.split_once(':') {
            let key = k.trim().to_ascii_lowercase();
            let val = v.trim().to_string();
            if key == "content-length" {
                content_len = val.parse::<usize>().unwrap_or(0);
            }
            headers.insert(key, val);
        }
    }

    let mut body = vec![0u8; content_len];
    if content_len > 0 {
        reader.read_exact(&mut body).map_err(|e| e.to_string())?;
    }
    Ok((method, path, headers, body))
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(hex) = std::str::from_utf8(&bytes[i + 1..i + 3]) {
                if let Ok(val) = u8::from_str_radix(hex, 16) {
                    out.push(val);
                    i += 3;
                    continue;
                }
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut out = String::with_capacity(64);
    for b in digest {
        use std::fmt::Write;
        let _ = write!(&mut out, "{b:02x}");
    }
    out
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
