use crate::{DirectoryError, ExecutionError, NodeDaemon, SessionDirectory};
use fleet_proto::{DeliveryTier, DesktopHostKind, SessionRecord, SessionSelector};
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fmt, fs,
    os::unix::{fs::MetadataExt, net::UnixStream},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tungstenite::{client::client_with_config, protocol::WebSocketConfig, Message, WebSocket};

#[derive(Debug)]
pub enum DesktopError {
    Selection(ExecutionError),
    Directory(DirectoryError),
    Invalid(String),
    Rejected(Value),
    SendRejected(Value),
    Unavailable(String),
    Unknown(String),
}
impl fmt::Display for DesktopError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Selection(e) => e.fmt(f),
            Self::Directory(e) => e.fmt(f),
            Self::Invalid(e) => write!(f, "desktop target rejected: {e}"),
            Self::Rejected(e) => write!(f, "desktop request rejected: {e}"),
            Self::SendRejected(e) => write!(f, "desktop delivery rejected: {e}"),
            Self::Unavailable(e) => write!(f, "desktop unavailable: {e}"),
            Self::Unknown(e) => write!(
                f,
                "desktop outcome unknown; read back before any new delivery: {e}"
            ),
        }
    }
}
impl DesktopError {
    pub fn tier(&self) -> DeliveryTier {
        match self {
            Self::SendRejected(_) => DeliveryTier::Failed,
            _ => DeliveryTier::Unknown,
        }
    }
}
impl std::error::Error for DesktopError {}
impl From<ExecutionError> for DesktopError {
    fn from(e: ExecutionError) -> Self {
        Self::Selection(e)
    }
}
impl From<DirectoryError> for DesktopError {
    fn from(e: DirectoryError) -> Self {
        Self::Directory(e)
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct DesktopReceipt {
    delivery_id: String,
    session: fleet_proto::SessionId,
    cwd: PathBuf,
    turn_id: String,
    tier: DeliveryTier,
    #[serde(skip)]
    connection_id: u64,
}

impl DesktopReceipt {
    pub fn delivery_id(&self) -> &str {
        &self.delivery_id
    }
    pub fn session(&self) -> &fleet_proto::SessionId {
        &self.session
    }
    pub fn cwd(&self) -> &Path {
        &self.cwd
    }
    pub fn turn_id(&self) -> &str {
        &self.turn_id
    }

    pub fn tier(&self) -> DeliveryTier {
        self.tier
    }

    /// A completed native turn does not imply a visible Desktop thread.
    pub fn confirms(&self, claim: DeliveryTier) -> bool {
        self.tier == claim
    }
}

enum Connection {
    Open(WebSocket<UnixStream>),
    Lost,
}

/// One connection, one existing thread. Send acknowledges acceptance; observe reads turn state.
/// There is no reconnect or resend path; callers must retain unknown outcomes across restarts.
pub struct DesktopDelivery {
    connection: Connection,
    connection_id: u64,
    timeout: Duration,
    next_id: u64,
    events: VecDeque<Value>,
    event_bytes: usize,
    previous_observation: Option<Value>,
    session: SessionRecord,
    directory: PathBuf,
    selector: SessionSelector,
    daemon: NodeDaemon,
    cwd_identity: (u64, u64),
    claimed_deliveries: BTreeSet<String>,
    accepted_turns: BTreeSet<String>,
    delivery_turns: BTreeMap<String, String>,
}

impl DesktopDelivery {
    pub fn connect(
        socket: &Path,
        directory: PathBuf,
        selector: SessionSelector,
        cache_roots: Vec<PathBuf>,
        timeout: Duration,
    ) -> Result<Self, DesktopError> {
        if !socket.is_absolute() || timeout.is_zero() || timeout > Duration::from_secs(60) {
            return Err(DesktopError::Invalid(
                "absolute socket and timeout in (0, 60s] required".into(),
            ));
        }
        let now = now()?;
        let session = SessionDirectory::discover(&directory, now)?
            .resolve(&selector, now)?
            .clone();
        let spec = DesktopHostKind::OpenaiDots.spec();
        if session.id.desktop != spec.desktop_target
            || spec.primary_channel.as_str() != "desktop-gateway"
        {
            return Err(DesktopError::Invalid(format!(
                "this adapter requires desktop={}",
                spec.desktop_target
            )));
        }
        let mut daemon = NodeDaemon::new(directory.clone(), cache_roots)?;
        daemon.select(&selector)?;
        let metadata = fs::metadata(&session.project_cwd).map_err(unavailable)?;
        let stream = UnixStream::connect(socket).map_err(unavailable)?;
        stream
            .set_read_timeout(Some(timeout))
            .map_err(unavailable)?;
        stream
            .set_write_timeout(Some(timeout))
            .map_err(unavailable)?;
        let config = WebSocketConfig::default()
            .max_message_size(Some(4 * 1024 * 1024))
            .max_frame_size(Some(4 * 1024 * 1024));
        let (ws, _) =
            client_with_config("ws://localhost/", stream, Some(config)).map_err(unavailable)?;
        static NEXT_CONNECTION: AtomicU64 = AtomicU64::new(1);
        let mut delivery = Self {
            connection_id: NEXT_CONNECTION.fetch_add(1, Ordering::Relaxed),
            connection: Connection::Open(ws),
            timeout,
            next_id: 0,
            events: VecDeque::new(),
            event_bytes: 0,
            previous_observation: None,
            session,
            directory,
            selector,
            daemon,
            cwd_identity: (metadata.dev(), metadata.ino()),
            claimed_deliveries: BTreeSet::new(),
            accepted_turns: BTreeSet::new(),
            delivery_turns: BTreeMap::new(),
        };
        delivery.request(
            "initialize",
            json!({
                "clientInfo":{"name":"fleet-desktop-delivery","version":env!("CARGO_PKG_VERSION")},
                "capabilities":{"experimentalApi":true}
            }),
            false,
        )?;
        delivery.write(json!({"method":"initialized"}), false)?;
        delivery.validate()?;
        Ok(delivery)
    }

    pub fn session(&self) -> &SessionRecord {
        &self.session
    }

    pub fn send(&mut self, delivery_id: &str, text: &str) -> Result<DesktopReceipt, DesktopError> {
        self.send_with_model(delivery_id, text, None)
    }

    /// The dispatcher supplies model configuration separately from the task brief.
    pub fn send_with_model(
        &mut self,
        delivery_id: &str,
        text: &str,
        model: Option<&str>,
    ) -> Result<DesktopReceipt, DesktopError> {
        if model.is_some_and(|model| {
            model.trim().is_empty() || model.len() > 256 || model.chars().any(char::is_control)
        }) {
            return Err(DesktopError::Invalid(
                "nonempty model id (max 256 bytes) required".into(),
            ));
        }
        if delivery_id.is_empty()
            || delivery_id.len() > 256
            || delivery_id.chars().any(char::is_control)
            || text.trim().is_empty()
            || text.len() > 64 * 1024
        {
            return Err(DesktopError::Invalid(
                "nonempty delivery id and message (max 64 KiB) required".into(),
            ));
        }
        fleet_proto::validate_brief(text).map_err(|e| DesktopError::Invalid(e.to_string()))?;
        self.validate()?;
        if !self.claimed_deliveries.insert(delivery_id.into()) {
            return Err(DesktopError::Invalid(
                "delivery id already attempted; read back instead".into(),
            ));
        }
        let mut params = json!({
            "threadId":self.session.id.thread_id,
            "clientUserMessageId":delivery_id,
            "input":[{"type":"text","text":text,"text_elements":[]}]
        });
        if let Some(model) = model {
            params["model"] = json!(model);
        }
        let response = self.request("turn/start", params, true)?;
        let Some(turn_id) = response
            .pointer("/turn/id")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        else {
            return Err(self.lose("accepted response omitted turn id", true));
        };
        if !self.accepted_turns.insert(turn_id.into()) {
            return Err(self.lose("accepted response reused an existing turn id", true));
        }
        self.delivery_turns
            .insert(delivery_id.into(), turn_id.into());
        Ok(DesktopReceipt {
            delivery_id: delivery_id.into(),
            session: self.session.id.clone(),
            cwd: self.session.project_cwd.clone(),
            turn_id: turn_id.into(),
            tier: DeliveryTier::Accepted,
            connection_id: self.connection_id,
        })
    }

    /// Read the exact accepted turn. Neither loaded-list nor rollout text proves UI visibility.
    pub fn observe(&mut self, receipt: &DesktopReceipt) -> Result<DesktopReceipt, DesktopError> {
        if receipt.connection_id != self.connection_id
            || receipt.session != self.session.id
            || receipt.cwd != self.session.project_cwd
            || self.delivery_turns.get(&receipt.delivery_id) != Some(&receipt.turn_id)
        {
            return Err(DesktopError::Invalid(
                "receipt does not match accepted delivery".into(),
            ));
        }
        self.validate_local()?;
        let thread = self.read()?;
        let turns: Vec<_> = thread["turns"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|turn| turn["id"].as_str() == Some(receipt.turn_id.as_str()))
            .collect();
        let tier = if turns.len() == 1 {
            match turns[0]["status"].as_str() {
                Some("inProgress") => DeliveryTier::Running,
                Some("completed") => DeliveryTier::Completed,
                Some("failed" | "interrupted") => DeliveryTier::Failed,
                _ => DeliveryTier::Unknown,
            }
        } else {
            DeliveryTier::Unknown
        };
        let mut observed = receipt.clone();
        observed.tier = tier;
        Ok(observed)
    }

    pub fn read(&mut self) -> Result<Value, DesktopError> {
        let result = self.request(
            "thread/read",
            json!({
                "threadId":self.session.id.thread_id,"includeTurns":false
            }),
            false,
        )?;
        self.check_thread(&result)?;
        let mut thread = result["thread"].clone();
        let mut turns = Vec::new();
        let mut history_bytes = 0;
        let mut cursor = Value::Null;
        let mut seen = BTreeSet::new();
        loop {
            let page = self.request(
                "thread/turns/list",
                json!({
                    "threadId":self.session.id.thread_id,"itemsView":"full",
                    "sortDirection":"asc","limit":100,"cursor":cursor
                }),
                false,
            )?;
            let Some(data) = page["data"].as_array() else {
                return Err(self.lose("invalid turn page", false));
            };
            history_bytes += page.to_string().len();
            turns.extend(data.iter().cloned());
            if history_bytes > 16 * 1024 * 1024 || turns.len() > 10_000 {
                return Err(self.lose("thread history exceeds read limit", false));
            }
            match page.get("nextCursor") {
                None | Some(Value::Null) => break,
                Some(Value::String(next))
                    if !next.is_empty() && seen.insert(next.clone()) && seen.len() <= 100 =>
                {
                    cursor = json!(next)
                }
                _ => return Err(self.lose("invalid turn pagination", false)),
            }
        }
        thread["turns"] = json!(turns);
        Ok(thread)
    }

    pub fn interrupt(&mut self, turn_id: &str) -> Result<(), DesktopError> {
        if !self.accepted_turns.contains(turn_id) {
            return Err(DesktopError::Invalid(
                "interrupt requires a turn accepted by this connection".into(),
            ));
        }
        self.validate()?;
        let thread = self.read()?;
        let active = thread["turns"].as_array().is_some_and(|turns| {
            turns
                .iter()
                .any(|t| t["id"].as_str() == Some(turn_id) && t["status"] == "inProgress")
        });
        if !active {
            return Err(DesktopError::Invalid("accepted turn is not active".into()));
        }
        self.validate_local()?;
        self.request(
            "turn/interrupt",
            json!({"threadId":self.session.id.thread_id,"turnId":turn_id}),
            true,
        )?;
        Ok(())
    }

    /// Reports changed thread snapshots as `source: poll`, including the initial snapshot.
    /// Native notifications are opportunistic and labelled `source: native`; this connection
    /// does not acquire a subscription or resume a thread owned by the Desktop.
    pub fn events(&mut self) -> Result<Vec<Value>, DesktopError> {
        let thread = self.read()?;
        self.event_bytes = 0;
        let mut events: Vec<Value> = self
            .events
            .drain(..)
            .map(|mut event| {
                event["source"] = json!("native");
                event
            })
            .collect();
        if self.previous_observation.as_ref() != Some(&thread) {
            events.push(json!({
                "method":"fleet/thread/observed","source":"poll",
                "params":{"threadId":self.session.id.thread_id,"thread":thread}
            }));
            self.previous_observation = Some(thread);
        }
        Ok(events)
    }

    fn validate_local(&mut self) -> Result<(), DesktopError> {
        self.daemon.select(&self.selector)?;
        let now = now()?;
        if SessionDirectory::discover(&self.directory, now)?.resolve(&self.selector, now)?
            != &self.session
        {
            return Err(DesktopError::Invalid("session record changed".into()));
        }
        let metadata = fs::metadata(&self.session.project_cwd).map_err(unavailable)?;
        if (metadata.dev(), metadata.ino()) != self.cwd_identity {
            return Err(DesktopError::Invalid("project directory replaced".into()));
        }
        Ok(())
    }

    fn validate(&mut self) -> Result<(), DesktopError> {
        self.validate_local()?;
        let mut cursor = Value::Null;
        let mut seen = BTreeSet::new();
        let mut loaded = false;
        loop {
            let page = self.request(
                "thread/loaded/list",
                json!({"cursor":cursor,"limit":100}),
                false,
            )?;
            let Some(ids) = page["data"].as_array() else {
                return Err(self.lose("invalid loaded-thread page", false));
            };
            for id in ids {
                let Some(id) = id.as_str() else {
                    return Err(self.lose("invalid loaded-thread id", false));
                };
                loaded |= id == self.session.id.thread_id;
            }
            match page.get("nextCursor") {
                None | Some(Value::Null) => break,
                Some(Value::String(next))
                    if !next.is_empty() && seen.insert(next.clone()) && seen.len() <= 100 =>
                {
                    cursor = json!(next)
                }
                _ => return Err(self.lose("invalid loaded-thread pagination", false)),
            }
        }
        if !loaded {
            return Err(DesktopError::Invalid(
                "thread is not loaded; no thread was created or resumed".into(),
            ));
        }
        let result = self.request(
            "thread/read",
            json!({"threadId":self.session.id.thread_id,"includeTurns":false}),
            false,
        )?;
        self.check_thread(&result)?;
        if !matches!(
            result
                .pointer("/thread/status/type")
                .and_then(Value::as_str),
            Some("idle" | "active")
        ) {
            return Err(DesktopError::Invalid("thread is not available".into()));
        }
        self.validate_local()
    }

    fn check_thread(&mut self, response: &Value) -> Result<(), DesktopError> {
        let thread = &response["thread"];
        if thread["id"].as_str() != Some(self.session.id.thread_id.as_str())
            || thread["cwd"].as_str().map(Path::new) != Some(self.session.project_cwd.as_path())
        {
            return Err(self.lose("native thread identity or cwd mismatch", false));
        }
        Ok(())
    }

    fn write(&mut self, value: Value, mutation: bool) -> Result<(), DesktopError> {
        let Connection::Open(ws) = &mut self.connection else {
            return Err(DesktopError::Unavailable(
                "connection lost; no automatic reconnect or resend".into(),
            ));
        };
        ws.send(Message::Text(value.to_string().into()))
            .map_err(|e| self.lose(&e.to_string(), mutation))
    }

    fn request(
        &mut self,
        method: &str,
        params: Value,
        mutation: bool,
    ) -> Result<Value, DesktopError> {
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or_else(|| DesktopError::Invalid("request id exhausted".into()))?;
        let id = self.next_id;
        let deadline = Instant::now() + self.timeout;
        self.write(json!({"id":id,"method":method,"params":params}), mutation)?;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(self.lose("request timed out", mutation));
            }
            let Connection::Open(ws) = &mut self.connection else {
                return Err(DesktopError::Unavailable("connection lost".into()));
            };
            ws.get_mut()
                .set_read_timeout(Some(remaining))
                .map_err(unavailable)?;
            let message = ws.read().map_err(|e| self.lose(&e.to_string(), mutation))?;
            let value: Value = match message {
                Message::Text(text) => {
                    serde_json::from_str(&text).map_err(|_| self.lose("invalid JSON", mutation))?
                }
                Message::Ping(_) | Message::Pong(_) => continue,
                _ => return Err(self.lose("connection closed or unexpected frame", mutation)),
            };
            if let Some(response_id) = value.get("id") {
                if response_id != &json!(id) {
                    return Err(self.lose("RPC response id mismatch", mutation));
                }
                if let Some(error) = value.get("error") {
                    return Err(if method == "turn/start" {
                        DesktopError::SendRejected(error.clone())
                    } else {
                        DesktopError::Rejected(error.clone())
                    });
                }
                return value
                    .get("result")
                    .cloned()
                    .ok_or_else(|| self.lose("RPC response missing result", mutation));
            }
            let params = &value["params"];
            let thread_id = params
                .get("threadId")
                .or_else(|| params.pointer("/thread/id"))
                .and_then(Value::as_str);
            if thread_id == Some(self.session.id.thread_id.as_str()) && value["method"].is_string()
            {
                self.event_bytes += value.to_string().len();
                if self.events.len() >= 1024 || self.event_bytes > 4 * 1024 * 1024 {
                    return Err(self.lose("thread event buffer full", mutation));
                }
                self.events.push_back(value);
            }
        }
    }

    fn lose(&mut self, reason: &str, mutation: bool) -> DesktopError {
        self.connection = Connection::Lost;
        if mutation {
            DesktopError::Unknown(reason.into())
        } else {
            DesktopError::Unavailable(reason.into())
        }
    }
}

fn now() -> Result<u64, DesktopError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .map_err(unavailable)
}
fn unavailable(e: impl fmt::Display) -> DesktopError {
    DesktopError::Unavailable(e.to_string())
}
