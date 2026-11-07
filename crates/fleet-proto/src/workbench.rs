use crate::{
    timeline::{push_ascii_or_utf8_json_escaped, CanonicalSessionId, TimelineError, UpdatedSeq},
    DesktopHostKind,
};
use serde::{Deserialize, Serialize};
use std::fmt;

pub const PROTOCOL_VERSION_V1_1: &str = "1.1";
pub const MAX_CONNECTOR_WS_FRAME_BYTES: usize = 900 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkbenchProtocolError {
    InvalidJson(String),
    UnsupportedVersion(String),
    InvalidField(String),
    InvalidCursor(String),
    InvalidTransition {
        from: InteractionStatus,
        to: InteractionStatus,
    },
    PlanFenceConflict(PlanFenceConflict),
    Timeline(TimelineError),
}

impl fmt::Display for WorkbenchProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidJson(msg) => write!(f, "invalid protocol json: {msg}"),
            Self::UnsupportedVersion(v) => write!(f, "unsupported workbench protocol version: {v}"),
            Self::InvalidField(field) => write!(f, "invalid or missing protocol field: {field}"),
            Self::InvalidCursor(cursor) => write!(f, "invalid sequence cursor: {cursor}"),
            Self::InvalidTransition { from, to } => {
                write!(f, "invalid interaction status transition: {from} -> {to}")
            }
            Self::PlanFenceConflict(err) => fmt::Display::fmt(err, f),
            Self::Timeline(err) => fmt::Display::fmt(err, f),
        }
    }
}

impl std::error::Error for WorkbenchProtocolError {}

impl From<TimelineError> for WorkbenchProtocolError {
    fn from(value: TimelineError) -> Self {
        Self::Timeline(value)
    }
}

impl From<PlanFenceConflict> for WorkbenchProtocolError {
    fn from(value: PlanFenceConflict) -> Self {
        Self::PlanFenceConflict(value)
    }
}

/// Event envelope pushed over `/api/v2/sessions/{id}/ws` and returned by `/api/v2/sessions/{id}/events`
/// per `contracts/protocol/1.1`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProtocolEventEnvelope {
    #[serde(rename = "protocolVersion", alias = "protocol_version")]
    pub protocol_version: String,
    #[serde(rename = "eventId", alias = "event_id")]
    pub event_id: String,
    #[serde(rename = "type", alias = "event_type")]
    pub event_type: String,
    #[serde(rename = "sessionId", alias = "session_id")]
    pub session_id: CanonicalSessionId,
    #[serde(rename = "updatedSeq", alias = "updated_seq")]
    pub updated_seq: UpdatedSeq,
    pub cursor: String,
    #[serde(rename = "timestampMs", alias = "timestamp_ms")]
    pub timestamp_ms: i64,
    #[serde(rename = "dataJson", alias = "data_json", default)]
    pub data_json: String,
}

impl ProtocolEventEnvelope {
    pub fn new(
        event_id: impl Into<String>,
        event_type: impl Into<String>,
        session_id: CanonicalSessionId,
        updated_seq: UpdatedSeq,
        timestamp_ms: i64,
        data_json: impl Into<String>,
    ) -> Self {
        let data = data_json.into();
        Self {
            protocol_version: PROTOCOL_VERSION_V1_1.to_string(),
            event_id: event_id.into(),
            event_type: event_type.into(),
            session_id,
            updated_seq,
            cursor: updated_seq.to_cursor(),
            timestamp_ms,
            data_json: if data.trim().is_empty() {
                "{}".to_string()
            } else {
                data
            },
        }
    }

    /// Durable `timeline.item_*` events deduplicate by `eventId`, whereas live capability and
    /// runtime state transitions (`runtime.capability.updated`, `runtime.state.updated`) must
    /// never be dropped by `eventId` deduplication so `A -> B -> A` transitions pass through.
    pub fn deduplicates_by_event_id(&self) -> bool {
        if matches!(
            self.event_type.as_str(),
            "runtime.capability.updated" | "runtime.state.updated"
        ) {
            return false;
        }
        self.event_type.starts_with("timeline.item_")
    }

    /// Deterministic wire JSON representation matching `contracts/protocol/1.1`.
    pub fn to_wire_json(&self) -> String {
        let data_raw = if self.data_json.trim().is_empty() {
            "{}"
        } else {
            self.data_json.trim()
        };
        let mut out = String::with_capacity(192 + data_raw.len());
        out.push_str("{\"protocolVersion\":\"");
        push_ascii_or_utf8_json_escaped(&mut out, &self.protocol_version);
        out.push_str("\",\"eventId\":\"");
        push_ascii_or_utf8_json_escaped(&mut out, &self.event_id);
        out.push_str("\",\"type\":\"");
        push_ascii_or_utf8_json_escaped(&mut out, &self.event_type);
        out.push_str("\",\"sessionId\":\"");
        push_ascii_or_utf8_json_escaped(&mut out, self.session_id.as_str());
        out.push_str("\",\"updatedSeq\":");
        out.push_str(&self.updated_seq.get().to_string());
        out.push_str(",\"cursor\":\"");
        push_ascii_or_utf8_json_escaped(&mut out, &self.cursor);
        out.push_str("\",\"timestampMs\":");
        out.push_str(&self.timestamp_ms.to_string());
        out.push_str(",\"data\":");
        out.push_str(data_raw);
        out.push('}');
        out
    }

    /// Parses a `contracts/protocol/1.1` JSON envelope string without losing byte-for-byte
    /// fidelity on the nested `"data"` payload.
    pub fn from_wire_json(raw: &str) -> Result<Self, WorkbenchProtocolError> {
        let trimmed = raw.trim();
        if !trimmed.starts_with('{') || !trimmed.ends_with('}') {
            return Err(WorkbenchProtocolError::InvalidJson(
                "expected top-level JSON object".into(),
            ));
        }
        let protocol_version = extract_json_string_field(trimmed, "protocolVersion")
            .ok_or_else(|| WorkbenchProtocolError::InvalidField("protocolVersion".into()))?;
        if protocol_version != PROTOCOL_VERSION_V1_1 {
            return Err(WorkbenchProtocolError::UnsupportedVersion(protocol_version));
        }
        let event_id = extract_json_string_field(trimmed, "eventId")
            .ok_or_else(|| WorkbenchProtocolError::InvalidField("eventId".into()))?;
        if event_id.is_empty() {
            return Err(WorkbenchProtocolError::InvalidField("eventId".into()));
        }
        let event_type = extract_json_string_field(trimmed, "type")
            .ok_or_else(|| WorkbenchProtocolError::InvalidField("type".into()))?;
        if event_type.is_empty() {
            return Err(WorkbenchProtocolError::InvalidField("type".into()));
        }
        let session_raw = extract_json_string_field(trimmed, "sessionId")
            .ok_or_else(|| WorkbenchProtocolError::InvalidField("sessionId".into()))?;
        let session_id = CanonicalSessionId::parse(&session_raw)?;
        let updated_seq_val = extract_json_i64_field(trimmed, "updatedSeq")
            .ok_or_else(|| WorkbenchProtocolError::InvalidField("updatedSeq".into()))?;
        let updated_seq = UpdatedSeq::new(updated_seq_val);
        let cursor = extract_json_string_field(trimmed, "cursor")
            .ok_or_else(|| WorkbenchProtocolError::InvalidField("cursor".into()))?;
        if UpdatedSeq::parse_cursor(&cursor) != Some(updated_seq) {
            return Err(WorkbenchProtocolError::InvalidCursor(cursor));
        }
        let timestamp_ms = extract_json_i64_field(trimmed, "timestampMs")
            .ok_or_else(|| WorkbenchProtocolError::InvalidField("timestampMs".into()))?;
        let data_json = extract_json_raw_value_field(trimmed, "data")
            .ok_or_else(|| WorkbenchProtocolError::InvalidField("data".into()))?;
        Ok(Self {
            protocol_version,
            event_id,
            event_type,
            session_id,
            updated_seq,
            cursor,
            timestamp_ms,
            data_json,
        })
    }
}

fn find_top_level_key(json_obj: &str, key: &str) -> Option<usize> {
    let bytes = json_obj.as_bytes();
    let needle = format!("\"{key}\"");
    let needle_bytes = needle.as_bytes();
    let mut depth = 0i32;
    let mut in_str = false;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if in_str {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == b'"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        match b {
            b'{' | b'[' => depth += 1,
            b'}' | b']' => depth -= 1,
            b'"' => {
                if depth == 1 && bytes[i..].starts_with(needle_bytes) {
                    let mut after = i + needle_bytes.len();
                    while after < bytes.len() && bytes[after].is_ascii_whitespace() {
                        after += 1;
                    }
                    if after < bytes.len() && bytes[after] == b':' {
                        return Some(after + 1);
                    }
                }
                in_str = true;
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn extract_json_string_field(json_obj: &str, key: &str) -> Option<String> {
    let mut pos = find_top_level_key(json_obj, key)?;
    let bytes = json_obj.as_bytes();
    while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
        pos += 1;
    }
    if pos >= bytes.len() || bytes[pos] != b'"' {
        return None;
    }
    pos += 1;
    let mut out = String::new();
    while pos < bytes.len() {
        let b = bytes[pos];
        if b == b'"' {
            return Some(out);
        }
        if b == b'\\' {
            pos += 1;
            if pos >= bytes.len() {
                return None;
            }
            match bytes[pos] {
                b'"' => out.push('"'),
                b'\\' => out.push('\\'),
                b'/' => out.push('/'),
                b'b' => out.push('\x08'),
                b'f' => out.push('\x0c'),
                b'n' => out.push('\n'),
                b'r' => out.push('\r'),
                b't' => out.push('\t'),
                _ => return None,
            }
            pos += 1;
            continue;
        }
        let ch = json_obj[pos..].chars().next()?;
        out.push(ch);
        pos += ch.len_utf8();
    }
    None
}

fn extract_json_i64_field(json_obj: &str, key: &str) -> Option<i64> {
    let raw = extract_json_raw_value_field(json_obj, key)?;
    raw.parse::<i64>().ok()
}

fn extract_json_raw_value_field(json_obj: &str, key: &str) -> Option<String> {
    let mut pos = find_top_level_key(json_obj, key)?;
    let bytes = json_obj.as_bytes();
    while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
        pos += 1;
    }
    if pos >= bytes.len() {
        return None;
    }
    let start = pos;
    let mut depth = 0i32;
    let mut in_str = false;
    while pos < bytes.len() {
        let b = bytes[pos];
        if in_str {
            if b == b'\\' {
                pos += 2;
                continue;
            }
            if b == b'"' {
                in_str = false;
            }
            pos += 1;
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'{' | b'[' => depth += 1,
            b'}' | b']' => {
                if depth == 0 {
                    break;
                }
                depth -= 1;
                if depth == 0 {
                    pos += 1;
                    break;
                }
            }
            b',' if depth == 0 => break,
            _ => {}
        }
        pos += 1;
    }
    let slice = json_obj[start..pos].trim();
    if slice.is_empty() {
        None
    } else {
        Some(slice.to_string())
    }
}

/// Typed capability flags computed per host and session state (`contracts/runtime-control/2.1`).
/// Clients render interactive buttons strictly from `EffectiveCapabilities`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EffectiveCapabilities {
    #[serde(rename = "session.send_message", alias = "session_send_message")]
    pub session_send_message: bool,
    #[serde(rename = "session.interrupt", alias = "session_interrupt")]
    pub session_interrupt: bool,
    #[serde(
        rename = "session.interaction.approval",
        alias = "session_interaction_approval"
    )]
    pub session_interaction_approval: bool,
    #[serde(rename = "workspace.read_file", alias = "workspace_read_file")]
    pub workspace_read_file: bool,
    #[serde(rename = "workspace.terminal", alias = "workspace_terminal")]
    pub workspace_terminal: bool,
    #[serde(
        rename = "dispatch_channel",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub dispatch_channel: Option<String>,
}

impl EffectiveCapabilities {
    pub fn for_desktop_host(host: DesktopHostKind, has_local_channel: bool) -> Self {
        if host == DesktopHostKind::Cindy && !has_local_channel {
            return Self {
                session_send_message: false,
                session_interrupt: false,
                session_interaction_approval: false,
                workspace_read_file: true,
                workspace_terminal: false,
                dispatch_channel: Some("anet".to_string()),
            };
        }
        Self {
            session_send_message: has_local_channel,
            session_interrupt: has_local_channel,
            session_interaction_approval: has_local_channel,
            workspace_read_file: true,
            workspace_terminal: has_local_channel,
            dispatch_channel: Some(if has_local_channel {
                "local".to_string()
            } else {
                "offline".to_string()
            }),
        }
    }

    pub fn for_grokbot() -> Self {
        Self {
            session_send_message: true,
            session_interrupt: false,
            session_interaction_approval: true,
            workspace_read_file: true,
            workspace_terminal: false,
            dispatch_channel: Some("mailbox".to_string()),
        }
    }
}

/// Interaction lifecycle state machine (`Open -> ResponseAccepted -> Resolving -> Resolved`).
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InteractionStatus {
    Open,
    ResponseAccepted,
    Resolving,
    Resolved,
}

impl InteractionStatus {
    pub const ALL: [Self; 4] = [
        Self::Open,
        Self::ResponseAccepted,
        Self::Resolving,
        Self::Resolved,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::ResponseAccepted => "response_accepted",
            Self::Resolving => "resolving",
            Self::Resolved => "resolved",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim() {
            "open" => Some(Self::Open),
            "response_accepted" | "responseAccepted" => Some(Self::ResponseAccepted),
            "resolving" => Some(Self::Resolving),
            "resolved" => Some(Self::Resolved),
            _ => None,
        }
    }

    /// Returns `true` strictly for valid forward transitions along
    /// `Open -> ResponseAccepted -> Resolving -> Resolved`.
    pub const fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Open, Self::ResponseAccepted)
                | (Self::ResponseAccepted, Self::Resolving)
                | (Self::Resolving, Self::Resolved)
        )
    }
}

impl fmt::Display for InteractionStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionNoticeKind {
    InputRequest,
    ToolApproval,
    PlanReview,
}

impl SessionNoticeKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InputRequest => "input_request",
            Self::ToolApproval => "tool_approval",
            Self::PlanReview => "plan_review",
        }
    }
}

/// Interactive notice card emitted on a session for tool approvals, input questions, or plan reviews.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SessionNotice {
    #[serde(rename = "noticeId", alias = "notice_id")]
    pub notice_id: String,
    #[serde(rename = "sessionId", alias = "session_id")]
    pub session_id: CanonicalSessionId,
    pub kind: SessionNoticeKind,
    pub status: InteractionStatus,
    pub title: String,
    pub prompt: String,
    #[serde(
        rename = "planTicket",
        alias = "plan_ticket",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub plan_ticket: Option<PlanReviewTicket>,
    #[serde(
        rename = "decisionPayload",
        alias = "decision_payload",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub decision_payload: Option<String>,
    #[serde(rename = "updatedSeq", alias = "updated_seq", default)]
    pub updated_seq: UpdatedSeq,
}

impl SessionNotice {
    pub fn transition_to(&mut self, next: InteractionStatus) -> Result<(), WorkbenchProtocolError> {
        if !self.status.can_transition_to(next) {
            return Err(WorkbenchProtocolError::InvalidTransition {
                from: self.status,
                to: next,
            });
        }
        self.status = next;
        Ok(())
    }
}

/// Error returned when a `PlanReviewTicket` approval does not match the current `content_hash` or `epoch`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PlanFenceConflict;

impl fmt::Display for PlanFenceConflict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("plan_fence_conflict: plan content_hash or epoch changed since ticket issuance")
    }
}

impl std::error::Error for PlanFenceConflict {}

/// Optimistic concurrency ticket guarding plan review approvals (`contracts/runtime-workflows/1.2`).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PlanReviewTicket {
    #[serde(rename = "reviewId", alias = "review_id")]
    pub review_id: String,
    #[serde(rename = "planId", alias = "plan_id")]
    pub plan_id: String,
    #[serde(rename = "planRevision", alias = "plan_revision")]
    pub plan_revision: u32,
    #[serde(rename = "contentHash", alias = "content_hash")]
    pub content_hash: String,
    pub epoch: u64,
}

impl PlanReviewTicket {
    /// Fails closed with `Err(PlanFenceConflict)` if `content_hash` or `epoch` differs by even 1 byte.
    pub fn verify_approval(
        &self,
        submitted_content_hash: &str,
        submitted_epoch: u64,
    ) -> Result<(), PlanFenceConflict> {
        if self.content_hash.is_empty()
            || submitted_content_hash.is_empty()
            || self.content_hash.as_bytes() != submitted_content_hash.as_bytes()
            || self.epoch != submitted_epoch
        {
            return Err(PlanFenceConflict);
        }
        Ok(())
    }

    pub fn verify_revision_and_approval(
        &self,
        submitted_revision: u32,
        submitted_content_hash: &str,
        submitted_epoch: u64,
    ) -> Result<(), PlanFenceConflict> {
        if self.plan_revision != submitted_revision {
            return Err(PlanFenceConflict);
        }
        self.verify_approval(submitted_content_hash, submitted_epoch)
    }
}

/// Bidirectional JSON-RPC frame exchanged on `WS /api/v2/connector/ws`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorRpcFrame {
    pub jsonrpc: String,
    #[serde(default)]
    pub generation: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    #[serde(
        rename = "paramsJson",
        alias = "params_json",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub params_json: Option<String>,
    #[serde(
        rename = "resultJson",
        alias = "result_json",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub result_json: Option<String>,
    #[serde(
        rename = "errorCode",
        alias = "error_code",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub error_code: Option<String>,
    #[serde(
        rename = "errorMessage",
        alias = "error_message",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub error_message: Option<String>,
}

impl ConnectorRpcFrame {
    pub fn new_request(
        generation: u64,
        id: impl Into<String>,
        method: impl Into<String>,
        params_json: impl Into<String>,
    ) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            generation,
            id: Some(id.into()),
            method: Some(method.into()),
            params_json: Some(params_json.into()),
            result_json: None,
            error_code: None,
            error_message: None,
        }
    }

    pub fn new_notification(
        generation: u64,
        method: impl Into<String>,
        params_json: impl Into<String>,
    ) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            generation,
            id: None,
            method: Some(method.into()),
            params_json: Some(params_json.into()),
            result_json: None,
            error_code: None,
            error_message: None,
        }
    }

    pub fn new_response(
        generation: u64,
        id: impl Into<String>,
        result_json: impl Into<String>,
    ) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            generation,
            id: Some(id.into()),
            method: None,
            params_json: None,
            result_json: Some(result_json.into()),
            error_code: None,
            error_message: None,
        }
    }

    pub fn new_error(
        generation: u64,
        id: impl Into<String>,
        error_code: impl Into<String>,
        error_message: impl Into<String>,
    ) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            generation,
            id: Some(id.into()),
            method: None,
            params_json: None,
            result_json: None,
            error_code: Some(error_code.into()),
            error_message: Some(error_message.into()),
        }
    }

    pub fn is_critical_lane(&self) -> bool {
        matches!(
            self.method.as_deref(),
            Some("interaction.respond" | "session.interrupt")
        )
    }

    pub fn estimated_wire_bytes(&self) -> usize {
        64 + self.id.as_ref().map_or(0, String::len)
            + self.method.as_ref().map_or(0, String::len)
            + self.params_json.as_ref().map_or(0, String::len)
            + self.result_json.as_ref().map_or(0, String::len)
            + self.error_code.as_ref().map_or(0, String::len)
            + self.error_message.as_ref().map_or(0, String::len)
    }

    pub fn requires_http_ingest_spillover(&self) -> bool {
        self.estimated_wire_bytes() > MAX_CONNECTOR_WS_FRAME_BYTES
    }
}

/// Per-item rejection entry returned inside `POST /api/v2/connector/ingest` HTTP 200 responses.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct IngestRejection {
    pub index: usize,
    pub code: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Partial-batch result returned by `POST /api/v2/connector/ingest`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct IngestBatchResult {
    pub accepted: usize,
    #[serde(default)]
    pub rejected: Vec<IngestRejection>,
}

/// Response returned by `GET /api/v2/sessions/{sessionId}/events?after=seq:N`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProtocolEventRecoveryResponse {
    pub events: Vec<ProtocolEventEnvelope>,
    #[serde(rename = "nextCursor", alias = "next_cursor")]
    pub next_cursor: String,
    #[serde(rename = "snapshotRequired", alias = "snapshot_required")]
    pub snapshot_required: bool,
}
