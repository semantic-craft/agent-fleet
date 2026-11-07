use crate::{DeliveryTier, ProjectPathError, SidebarProjectBinding};
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use std::{fmt, ops::Deref};

const SHA256_K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
    0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
    0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
    0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
    0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
    0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
    0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
    0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
    0xc67178f2,
];

#[inline]
fn sha256_compress(state: &mut [u32; 8], block: &[u8; 64]) {
    let mut w = [0u32; 64];
    for i in 0..16 {
        let j = i * 4;
        w[i] = u32::from_be_bytes([block[j], block[j + 1], block[j + 2], block[j + 3]]);
    }
    for i in 16..64 {
        let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
        let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
        w[i] = w[i - 16]
            .wrapping_add(s0)
            .wrapping_add(w[i - 7])
            .wrapping_add(s1);
    }

    let mut a = state[0];
    let mut b = state[1];
    let mut c = state[2];
    let mut d = state[3];
    let mut e = state[4];
    let mut f = state[5];
    let mut g = state[6];
    let mut h = state[7];

    for i in 0..64 {
        let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let ch = (e & f) ^ ((!e) & g);
        let temp1 = h
            .wrapping_add(s1)
            .wrapping_add(ch)
            .wrapping_add(SHA256_K[i])
            .wrapping_add(w[i]);
        let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let maj = (a & b) ^ (a & c) ^ (b & c);
        let temp2 = s0.wrapping_add(maj);

        h = g;
        g = f;
        f = e;
        e = d.wrapping_add(temp1);
        d = c;
        c = b;
        b = a;
        a = temp1.wrapping_add(temp2);
    }

    state[0] = state[0].wrapping_add(a);
    state[1] = state[1].wrapping_add(b);
    state[2] = state[2].wrapping_add(c);
    state[3] = state[3].wrapping_add(d);
    state[4] = state[4].wrapping_add(e);
    state[5] = state[5].wrapping_add(f);
    state[6] = state[6].wrapping_add(g);
    state[7] = state[7].wrapping_add(h);
}

pub fn sha256_digest(data: &[u8]) -> [u8; 32] {
    let mut state: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut chunks = data.chunks_exact(64);
    for chunk in &mut chunks {
        let block: &[u8; 64] = chunk.try_into().expect("64-byte chunk");
        sha256_compress(&mut state, block);
    }
    let rem = chunks.remainder();
    let mut tail = [0u8; 128];
    tail[..rem.len()].copy_from_slice(rem);
    tail[rem.len()] = 0x80;
    let bit_len = (data.len() as u64) * 8;
    if rem.len() < 56 {
        tail[56..64].copy_from_slice(&bit_len.to_be_bytes());
        let block: &[u8; 64] = tail[..64].try_into().expect("64-byte block");
        sha256_compress(&mut state, block);
    } else {
        tail[120..128].copy_from_slice(&bit_len.to_be_bytes());
        let b1: &[u8; 64] = tail[..64].try_into().expect("first 64-byte block");
        let b2: &[u8; 64] = tail[64..128].try_into().expect("second 64-byte block");
        sha256_compress(&mut state, b1);
        sha256_compress(&mut state, b2);
    }

    let mut out = [0u8; 32];
    for (i, word) in state.iter().enumerate() {
        out[i * 4..(i + 1) * 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

pub fn sha256_hex(data: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = sha256_digest(data);
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TimelineError {
    InvalidOrderSeq(InvalidOrderSeq),
    InvalidSessionId(String),
    InvalidContentHash(String),
}

impl fmt::Display for TimelineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidOrderSeq(err) => Display::fmt(err, f),
            Self::InvalidSessionId(msg) => write!(f, "invalid canonical session id: {msg}"),
            Self::InvalidContentHash(msg) => write!(f, "invalid timeline content hash: {msg}"),
        }
    }
}

use std::fmt::Display;
impl std::error::Error for TimelineError {}

impl From<InvalidOrderSeq> for TimelineError {
    fn from(value: InvalidOrderSeq) -> Self {
        Self::InvalidOrderSeq(value)
    }
}

/// Branded canonical session ID (`sess_{runtime}_{sha256(connector_id:runtime:external_session_id)[:24]}`).
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct CanonicalSessionId(String);

impl CanonicalSessionId {
    pub fn derive(
        connector_id: &str,
        runtime: &str,
        external_session_id: &str,
    ) -> Result<Self, TimelineError> {
        let conn = connector_id.trim();
        let rt = runtime.trim();
        let ext = external_session_id.trim();
        if conn.is_empty()
            || rt.is_empty()
            || ext.is_empty()
            || conn.chars().any(char::is_control)
            || rt.chars().any(char::is_control)
            || ext.chars().any(char::is_control)
        {
            return Err(TimelineError::InvalidSessionId(
                "connector_id, runtime, and external_session_id must be nonempty and free of control chars".into(),
            ));
        }
        let normalized_rt: String = rt
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                    c.to_ascii_lowercase()
                } else {
                    '_'
                }
            })
            .collect();
        let composite = format!("{conn}:{normalized_rt}:{ext}");
        let hex = sha256_hex(composite.as_bytes());
        Ok(Self(format!("sess_{normalized_rt}_{}", &hex[..24])))
    }

    pub fn parse(raw: &str) -> Result<Self, TimelineError> {
        let trimmed = raw.trim();
        if !trimmed.starts_with("sess_")
            || trimmed.len() < 12
            || trimmed.len() > 256
            || trimmed.chars().any(char::is_control)
            || trimmed.chars().any(char::is_whitespace)
        {
            return Err(TimelineError::InvalidSessionId(raw.to_string()));
        }
        Ok(Self(trimmed.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Deref for CanonicalSessionId {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<str> for CanonicalSessionId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CanonicalSessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Deterministically derives `sess_{runtime}_{sha256(connector_id:runtime:external_session_id)[:24]}`.
pub fn stable_runtime_session_id(
    connector_id: &str,
    runtime: &str,
    external_session_id: &str,
) -> Result<CanonicalSessionId, TimelineError> {
    CanonicalSessionId::derive(connector_id, runtime, external_session_id)
}

/// Deterministically derives `item_{sha256(session_id:turn_id:position:kind)[:24]}`.
pub fn turn_position_item_id(
    session_id: &str,
    turn_id: &str,
    position: i32,
    kind: TimelineItemKind,
) -> String {
    let composite = format!(
        "{}:{}:{}:{}",
        session_id.trim(),
        turn_id.trim(),
        position,
        kind.as_str()
    );
    let hex = sha256_hex(composite.as_bytes());
    format!("item_{}", &hex[..24])
}

/// Error returned when a timeline `order_seq` value falls outside the signed 32-bit integer range.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidOrderSeq(pub i64);

impl InvalidOrderSeq {
    pub const fn code(&self) -> &'static str {
        "invalid_order_seq"
    }
}

impl fmt::Display for InvalidOrderSeq {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid_order_seq: {} is outside 32-bit signed range {}..={}",
            self.0,
            TimelineOrderSeq::MIN,
            TimelineOrderSeq::MAX
        )
    }
}

impl std::error::Error for InvalidOrderSeq {}

/// Signed 32-bit order sequence number bounded strictly to `-2_147_483_648..=2_147_483_647`.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TimelineOrderSeq(i32);

impl TimelineOrderSeq {
    pub const MIN: i64 = -2_147_483_648;
    pub const MAX: i64 = 2_147_483_647;

    pub const fn new(value: i32) -> Self {
        Self(value)
    }

    pub const fn get(self) -> i32 {
        self.0
    }

    pub const fn as_i64(self) -> i64 {
        self.0 as i64
    }
}

impl From<i32> for TimelineOrderSeq {
    fn from(value: i32) -> Self {
        Self(value)
    }
}

impl From<TimelineOrderSeq> for i32 {
    fn from(value: TimelineOrderSeq) -> Self {
        value.0
    }
}

impl From<TimelineOrderSeq> for i64 {
    fn from(value: TimelineOrderSeq) -> Self {
        value.0 as i64
    }
}

impl TryFrom<i64> for TimelineOrderSeq {
    type Error = InvalidOrderSeq;

    fn try_from(value: i64) -> Result<Self, Self::Error> {
        if (Self::MIN..=Self::MAX).contains(&value) {
            Ok(Self(value as i32))
        } else {
            Err(InvalidOrderSeq(value))
        }
    }
}

impl TryFrom<isize> for TimelineOrderSeq {
    type Error = InvalidOrderSeq;

    fn try_from(value: isize) -> Result<Self, Self::Error> {
        Self::try_from(value as i64)
    }
}

impl Serialize for TimelineOrderSeq {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_i32(self.0)
    }
}

impl<'de> Deserialize<'de> for TimelineOrderSeq {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = i64::deserialize(deserializer)?;
        Self::try_from(raw).map_err(de::Error::custom)
    }
}

/// Server-assigned or ledger-assigned monotonic sequence number (`i64`).
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct UpdatedSeq(pub i64);

impl UpdatedSeq {
    pub const fn new(value: i64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> i64 {
        self.0
    }

    pub const fn next(self) -> Self {
        Self(self.0.saturating_add(1))
    }

    pub fn to_cursor(self) -> String {
        format!("seq:{}", self.0)
    }

    pub fn parse_cursor(cursor: &str) -> Option<Self> {
        let num = cursor.trim().strip_prefix("seq:")?;
        num.parse::<i64>().ok().filter(|&v| v >= 0).map(Self)
    }
}

impl fmt::Display for UpdatedSeq {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TimelineItemKind {
    UserMessage,
    AssistantMessage,
    Reasoning,
    ToolCall,
    ToolResult,
    PeerMessage,
    FileDiff,
    SystemNotice,
}

impl TimelineItemKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UserMessage => "user_message",
            Self::AssistantMessage => "assistant_message",
            Self::Reasoning => "reasoning",
            Self::ToolCall => "tool_call",
            Self::ToolResult => "tool_result",
            Self::PeerMessage => "peer_message",
            Self::FileDiff => "file_diff",
            Self::SystemNotice => "system_notice",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim() {
            "user_message" => Some(Self::UserMessage),
            "assistant_message" => Some(Self::AssistantMessage),
            "reasoning" => Some(Self::Reasoning),
            "tool_call" => Some(Self::ToolCall),
            "tool_result" => Some(Self::ToolResult),
            "peer_message" => Some(Self::PeerMessage),
            "file_diff" => Some(Self::FileDiff),
            "system_notice" => Some(Self::SystemNotice),
            _ => None,
        }
    }
}

impl fmt::Display for TimelineItemKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TimelineItemStatus {
    InProgress,
    Completed,
    Failed,
    Cancelled,
}

impl TimelineItemStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim() {
            "in_progress" | "inProgress" => Some(Self::InProgress),
            "completed" => Some(Self::Completed),
            "failed" => Some(Self::Failed),
            "cancelled" | "canceled" => Some(Self::Cancelled),
            _ => None,
        }
    }
}

impl fmt::Display for TimelineItemStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TimelineRole {
    User,
    Assistant,
    System,
    Tool,
    Peer,
}

impl TimelineRole {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::System => "system",
            Self::Tool => "tool",
            Self::Peer => "peer",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim() {
            "user" => Some(Self::User),
            "assistant" => Some(Self::Assistant),
            "system" => Some(Self::System),
            "tool" => Some(Self::Tool),
            "peer" => Some(Self::Peer),
            _ => None,
        }
    }
}

impl fmt::Display for TimelineRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Deterministic content digest (`sha256:<64-hex>`) computed over `{type, status, role, content}`
/// with lone UTF-16 surrogates escaped as `\udxxx`.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct TimelineContentHash(String);

impl TimelineContentHash {
    pub fn parse(raw: &str) -> Result<Self, TimelineError> {
        let trimmed = raw.trim();
        let Some(hex) = trimmed.strip_prefix("sha256:") else {
            return Err(TimelineError::InvalidContentHash(raw.to_string()));
        };
        if hex.len() != 64 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(TimelineError::InvalidContentHash(raw.to_string()));
        }
        Ok(Self(format!("sha256:{}", hex.to_ascii_lowercase())))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Deref for TimelineContentHash {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl fmt::Display for TimelineContentHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

fn parse_u16_hex4(bytes: &[u8]) -> Option<u16> {
    if bytes.len() < 4 {
        return None;
    }
    let mut val = 0u16;
    for &b in &bytes[..4] {
        let digit = match b {
            b'0'..=b'9' => b - b'0',
            b'a'..=b'f' => b - b'a' + 10,
            b'A'..=b'F' => b - b'A' + 10,
            _ => return None,
        };
        val = (val << 4) | (digit as u16);
    }
    Some(val)
}

/// Converts a Rust `&str` (which may also contain `\uD800`..`\uDFFF` surrogate escapes) into a
/// UTF-16 code unit sequence so lone surrogates and surrogate pairs are normalized identically.
pub fn str_to_utf16_with_surrogate_escapes(input: &str) -> Vec<u16> {
    let bytes = input.as_bytes();
    let mut units = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\'
            && i + 6 <= bytes.len()
            && (bytes[i + 1] == b'u' || bytes[i + 1] == b'U')
        {
            if let Some(code_unit) = parse_u16_hex4(&bytes[i + 2..i + 6]) {
                if (0xD800..=0xDFFF).contains(&code_unit) {
                    units.push(code_unit);
                    i += 6;
                    continue;
                }
            }
        }
        let ch = input[i..].chars().next().expect("valid utf-8 char boundary");
        let mut buf = [0u16; 2];
        for &mut u in ch.encode_utf16(&mut buf) {
            units.push(u);
        }
        i += ch.len_utf8();
    }
    units
}

/// Writes a UTF-16 slice as a JSON string literal body (without surrounding quotes), escaping
/// standard JSON control characters and lone UTF-16 surrogates (`0xD800..=0xDFFF`) as `\udxxx`.
pub fn push_escaped_utf16_json_body(out: &mut String, units: &[u16]) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut i = 0;
    while i < units.len() {
        let u = units[i];
        if (0xD800..=0xDBFF).contains(&u) {
            if i + 1 < units.len() && (0xDC00..=0xDFFF).contains(&units[i + 1]) {
                let low = units[i + 1];
                let scalar =
                    0x10000 + (((u as u32 - 0xD800) << 10) | (low as u32 - 0xDC00));
                if let Some(ch) = char::from_u32(scalar) {
                    out.push(ch);
                    i += 2;
                    continue;
                }
            }
            out.push_str("\\u");
            out.push(HEX[((u >> 12) & 0x0f) as usize] as char);
            out.push(HEX[((u >> 8) & 0x0f) as usize] as char);
            out.push(HEX[((u >> 4) & 0x0f) as usize] as char);
            out.push(HEX[(u & 0x0f) as usize] as char);
            i += 1;
            continue;
        }
        if (0xDC00..=0xDFFF).contains(&u) {
            out.push_str("\\u");
            out.push(HEX[((u >> 12) & 0x0f) as usize] as char);
            out.push(HEX[((u >> 8) & 0x0f) as usize] as char);
            out.push(HEX[((u >> 4) & 0x0f) as usize] as char);
            out.push(HEX[(u & 0x0f) as usize] as char);
            i += 1;
            continue;
        }
        match u {
            0x0022 => out.push_str("\\\""),
            0x005C => out.push_str("\\\\"),
            0x0008 => out.push_str("\\b"),
            0x000C => out.push_str("\\f"),
            0x000A => out.push_str("\\n"),
            0x000D => out.push_str("\\r"),
            0x0009 => out.push_str("\\t"),
            0x0000..=0x001F => {
                out.push_str("\\u00");
                out.push(HEX[((u >> 4) & 0x0f) as usize] as char);
                out.push(HEX[(u & 0x0f) as usize] as char);
            }
            _ => {
                if let Some(ch) = char::from_u32(u as u32) {
                    out.push(ch);
                }
            }
        }
        i += 1;
    }
}

/// Formats the canonical JSON payload `{"type":...,"status":...,"role":...,"content":...}` with
/// lone UTF-16 surrogates escaped as `\udxxx`.
pub fn canonical_timeline_content_json_utf16(
    item_type: &str,
    status: &str,
    role: &str,
    content_utf16: &[u16],
) -> String {
    let mut json = String::with_capacity(64 + content_utf16.len() * 2);
    json.push_str("{\"type\":\"");
    push_escaped_utf16_json_body(&mut json, &str_to_utf16_with_surrogate_escapes(item_type));
    json.push_str("\",\"status\":\"");
    push_escaped_utf16_json_body(&mut json, &str_to_utf16_with_surrogate_escapes(status));
    json.push_str("\",\"role\":\"");
    push_escaped_utf16_json_body(&mut json, &str_to_utf16_with_surrogate_escapes(role));
    json.push_str("\",\"content\":\"");
    push_escaped_utf16_json_body(&mut json, content_utf16);
    json.push_str("\"}");
    json
}

/// Computes `sha256:<hex>` over `{"type":...,"status":...,"role":...,"content":...}` for a UTF-16 slice.
pub fn timeline_content_hash_utf16(
    item_type: &str,
    status: &str,
    role: &str,
    content_utf16: &[u16],
) -> TimelineContentHash {
    let canonical =
        canonical_timeline_content_json_utf16(item_type, status, role, content_utf16);
    TimelineContentHash(format!("sha256:{}", sha256_hex(canonical.as_bytes())))
}

/// Computes `sha256:<hex>` over `{"type":...,"status":...,"role":...,"content":...}` with
/// lone-surrogate `\udxxx` escaping matching Agents-Anywhere `contracts/protocol/1.1`.
pub fn timeline_content_hash(
    item_type: &str,
    status: &str,
    role: &str,
    content: &str,
) -> TimelineContentHash {
    if !content.as_bytes().contains(&b'\\') {
        let mut json = String::with_capacity(64 + content.len());
        json.push_str("{\"type\":\"");
        push_ascii_or_utf8_json_escaped(&mut json, item_type);
        json.push_str("\",\"status\":\"");
        push_ascii_or_utf8_json_escaped(&mut json, status);
        json.push_str("\",\"role\":\"");
        push_ascii_or_utf8_json_escaped(&mut json, role);
        json.push_str("\",\"content\":\"");
        push_ascii_or_utf8_json_escaped(&mut json, content);
        json.push_str("\"}");
        return TimelineContentHash(format!("sha256:{}", sha256_hex(json.as_bytes())));
    }
    let units = str_to_utf16_with_surrogate_escapes(content);
    timeline_content_hash_utf16(item_type, status, role, &units)
}

#[inline]
pub(crate) fn push_ascii_or_utf8_json_escaped(out: &mut String, s: &str) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\x08' => out.push_str("\\b"),
            '\x0c' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) <= 0x1F => {
                let u = c as u8;
                out.push_str("\\u00");
                out.push(HEX[(u >> 4) as usize] as char);
                out.push(HEX[(u & 0x0f) as usize] as char);
            }
            c => out.push(c),
        }
    }
}

/// Atomic conversation timeline record under `contracts/protocol/1.1`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TimelineItem {
    #[serde(rename = "itemId", alias = "item_id")]
    pub item_id: String,
    #[serde(rename = "sessionId", alias = "session_id")]
    pub session_id: CanonicalSessionId,
    #[serde(
        rename = "turnId",
        alias = "turn_id",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub turn_id: Option<String>,
    #[serde(rename = "type", alias = "kind")]
    pub kind: TimelineItemKind,
    pub status: TimelineItemStatus,
    pub role: TimelineRole,
    pub content: String,
    #[serde(rename = "orderSeq", alias = "order_seq")]
    pub order_seq: TimelineOrderSeq,
    #[serde(rename = "updatedSeq", alias = "updated_seq")]
    pub updated_seq: UpdatedSeq,
    #[serde(rename = "contentHash", alias = "content_hash")]
    pub content_hash: TimelineContentHash,
    #[serde(
        rename = "deliveryTier",
        alias = "delivery_tier",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub delivery_tier: Option<DeliveryTier>,
    #[serde(
        rename = "sidebarBinding",
        alias = "sidebar_binding",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub sidebar_binding: Option<SidebarProjectBinding>,
    #[serde(
        rename = "peerSender",
        alias = "peer_sender",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub peer_sender: Option<String>,
    #[serde(rename = "createdAtMs", alias = "created_at_ms", default)]
    pub created_at_ms: i64,
}

impl TimelineItem {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        item_id: impl Into<String>,
        session_id: CanonicalSessionId,
        turn_id: Option<String>,
        kind: TimelineItemKind,
        status: TimelineItemStatus,
        role: TimelineRole,
        content: impl Into<String>,
        order_seq: TimelineOrderSeq,
        updated_seq: UpdatedSeq,
        created_at_ms: i64,
    ) -> Self {
        let content_str = content.into();
        let content_hash =
            timeline_content_hash(kind.as_str(), status.as_str(), role.as_str(), &content_str);
        Self {
            item_id: item_id.into(),
            session_id,
            turn_id,
            kind,
            status,
            role,
            content: content_str,
            order_seq,
            updated_seq,
            content_hash,
            delivery_tier: None,
            sidebar_binding: None,
            peer_sender: None,
            created_at_ms,
        }
    }

    pub fn with_delivery_tier(mut self, tier: DeliveryTier) -> Self {
        self.delivery_tier = Some(tier);
        self
    }

    pub fn with_sidebar_binding(mut self, binding: SidebarProjectBinding) -> Self {
        self.sidebar_binding = Some(binding);
        self
    }

    pub fn with_peer_sender(mut self, sender: impl Into<String>) -> Self {
        self.peer_sender = Some(sender.into());
        self
    }

    pub fn recompute_content_hash(&self) -> TimelineContentHash {
        timeline_content_hash(
            self.kind.as_str(),
            self.status.as_str(),
            self.role.as_str(),
            &self.content,
        )
    }

    pub fn verify_content_hash(&self) -> bool {
        self.content_hash == self.recompute_content_hash()
    }

    /// Deterministic wire JSON serialization for `contracts/protocol/1.1`.
    pub fn to_json_string(&self) -> String {
        let mut out = String::with_capacity(256 + self.content.len());
        out.push_str("{\"itemId\":\"");
        push_ascii_or_utf8_json_escaped(&mut out, &self.item_id);
        out.push_str("\",\"sessionId\":\"");
        push_ascii_or_utf8_json_escaped(&mut out, self.session_id.as_str());
        if let Some(turn_id) = &self.turn_id {
            out.push_str("\",\"turnId\":\"");
            push_ascii_or_utf8_json_escaped(&mut out, turn_id);
        }
        out.push_str("\",\"type\":\"");
        out.push_str(self.kind.as_str());
        out.push_str("\",\"status\":\"");
        out.push_str(self.status.as_str());
        out.push_str("\",\"role\":\"");
        out.push_str(self.role.as_str());
        out.push_str("\",\"content\":\"");
        push_ascii_or_utf8_json_escaped(&mut out, &self.content);
        out.push_str("\",\"orderSeq\":");
        out.push_str(&self.order_seq.get().to_string());
        out.push_str(",\"updatedSeq\":");
        out.push_str(&self.updated_seq.get().to_string());
        out.push_str(",\"contentHash\":\"");
        push_ascii_or_utf8_json_escaped(&mut out, self.content_hash.as_str());
        out.push('"');
        if let Some(tier) = self.delivery_tier {
            out.push_str(",\"deliveryTier\":\"");
            out.push_str(tier.as_str());
            out.push('"');
        }
        if let Some(sender) = &self.peer_sender {
            out.push_str(",\"peerSender\":\"");
            push_ascii_or_utf8_json_escaped(&mut out, sender);
            out.push('"');
        }
        out.push_str(",\"createdAtMs\":");
        out.push_str(&self.created_at_ms.to_string());
        out.push('}');
        out
    }
}

/// Session metadata record under `contracts/protocol/1.1`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SessionMeta {
    #[serde(rename = "sessionId", alias = "session_id")]
    pub session_id: CanonicalSessionId,
    #[serde(rename = "connectorId", alias = "connector_id")]
    pub connector_id: String,
    pub runtime: String,
    #[serde(rename = "externalSessionId", alias = "external_session_id")]
    pub external_session_id: String,
    #[serde(rename = "projectSlug", alias = "project_slug")]
    pub project_slug: String,
    pub title: String,
    #[serde(rename = "latestUpdatedSeq", alias = "latest_updated_seq", default)]
    pub latest_updated_seq: UpdatedSeq,
    #[serde(
        rename = "deliveryTier",
        alias = "delivery_tier",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub delivery_tier: Option<DeliveryTier>,
    #[serde(
        rename = "sidebarBinding",
        alias = "sidebar_binding",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub sidebar_binding: Option<SidebarProjectBinding>,
}

impl SessionMeta {
    pub fn validate(&self) -> Result<(), ProjectPathError> {
        crate::validate_nonempty_id("connector_id", &self.connector_id)?;
        crate::validate_nonempty_id("runtime", &self.runtime)?;
        crate::validate_session_id(&self.external_session_id)?;
        crate::validate_project_slug(&self.project_slug)?;
        crate::validate_nonempty_id("title", &self.title)?;
        Ok(())
    }
}
