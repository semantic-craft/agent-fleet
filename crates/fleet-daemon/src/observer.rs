use crate::{
    antigravity::{list_conversation_summaries_readonly, AntigravityConversationSummary},
    host_adapter::HostAdapterError,
    ledger::{AdvisoryFileLock, FleetLedgerStore},
    tri_hosts::{
        decompress_dsh_two_frame_zstd, list_codex_projects_readonly, list_codex_threads_readonly,
        list_zcode_messages_with_conn, list_zcode_sessions_readonly,
        open_sqlite_readonly_preserve_mtime, CodexProjectRow,
    },
};
use fleet_proto::{
    sha256_hex, CanonicalSessionId, DeliveryTier, DesktopHostKind, SessionMeta,
    SidebarProjectBinding, TimelineItem, TimelineItemKind, TimelineItemStatus, TimelineOrderSeq,
    TimelineRole, UpdatedSeq,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    os::unix::io::AsRawFd,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct FleetMessageEnvelopeMeta {
    pub from: Option<String>,
    pub to: Option<String>,
    pub id: Option<String>,
    pub repo: Option<String>,
    pub reply_to: Option<String>,
    pub inner_body: String,
}

/// Extracts `<fleet-message from="..." to="..." id="..." repo="...">` metadata when present.
pub fn extract_fleet_message_metadata(text: &str) -> Option<FleetMessageEnvelopeMeta> {
    let open_start = text.find("<fleet-message")?;
    let after_prefix = &text[open_start + "<fleet-message".len()..];
    let tag_end_rel = after_prefix.find('>')?;
    let attr_slice = &after_prefix[..tag_end_rel];
    let body_start = open_start + "<fleet-message".len() + tag_end_rel + 1;
    let remaining = &text[body_start..];
    let inner_body = match remaining.find("</fleet-message>") {
        Some(close_rel) => remaining[..close_rel].trim().to_string(),
        None => remaining.trim().to_string(),
    };

    let from = extract_xml_attr(attr_slice, "from");
    let to = extract_xml_attr(attr_slice, "to");
    let id = extract_xml_attr(attr_slice, "id");
    let repo = extract_xml_attr(attr_slice, "repo");
    let reply_to = extract_xml_attr(attr_slice, "reply_to")
        .or_else(|| extract_xml_attr(attr_slice, "reply-to"));

    Some(FleetMessageEnvelopeMeta {
        from,
        to,
        id,
        repo,
        reply_to,
        inner_body,
    })
}

fn extract_xml_attr(attr_slice: &str, name: &str) -> Option<String> {
    let needle = format!("{name}=\"");
    let start = attr_slice.find(&needle)? + needle.len();
    let tail = &attr_slice[start..];
    let end = tail.find('"')?;
    let val = tail[..end].trim();
    (!val.is_empty()).then(|| val.to_string())
}

/// Guard holding an active native writer lock for a desktop host root or session.
#[derive(Debug)]
pub struct NativeWriterLockGuard {
    file: File,
    lock_path: PathBuf,
}

impl NativeWriterLockGuard {
    pub fn acquire(
        host_root: &Path,
        session_id: Option<&str>,
    ) -> Result<Self, HostAdapterError> {
        let lock_path = match session_id {
            Some(sid) if !sid.trim().is_empty() => {
                let locks_dir = host_root.join("locks");
                fs::create_dir_all(&locks_dir)?;
                locks_dir.join(format!("{sid}.writer.lock"))
            }
            _ => {
                fs::create_dir_all(host_root)?;
                host_root.join("writer.lock")
            }
        };
        let mut file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(true)
            .open(&lock_path)?;
        let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if rc != 0 {
            return Err(HostAdapterError::Io(std::io::Error::last_os_error()));
        }
        file.write_all(b"ACTIVE_WRITER\n")?;
        file.flush()?;
        Ok(Self { file, lock_path })
    }

    pub fn lock_path(&self) -> &Path {
        &self.lock_path
    }
}

impl Drop for NativeWriterLockGuard {
    fn drop(&mut self) {
        let _ = self.file.set_len(0);
        unsafe {
            let _ = libc::flock(self.file.as_raw_fd(), libc::LOCK_UN);
        }
        let _ = fs::remove_file(&self.lock_path);
    }
}

/// Checks whether a native desktop client holds an active writer lock on `host_root` or `session_id`.
/// Returns `Err(HostAdapterError::NativeWriterActive)` before any session takeover or turn mutation.
pub fn ensure_no_active_native_writer(
    host: DesktopHostKind,
    host_root: &Path,
    session_id: Option<&str>,
) -> Result<(), HostAdapterError> {
    let mut candidates = vec![host_root.join("writer.lock")];
    if let Some(sid) = session_id.filter(|s| !s.trim().is_empty()) {
        candidates.push(host_root.join("locks").join(format!("{sid}.writer.lock")));
        candidates.push(host_root.join("sessions").join(format!("{sid}.lock")));
        candidates.push(host_root.join("presence").join(format!("{sid}.lock")));
        candidates.push(host_root.join(format!("{sid}.writer.lock")));
    }
    for path in candidates {
        if !path.exists() {
            continue;
        }
        if let Ok(file) = OpenOptions::new().read(true).write(true).open(&path) {
            let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
            if rc != 0 {
                return Err(HostAdapterError::NativeWriterActive {
                    host,
                    detail: format!("active flock writer lock held on {}", path.display()),
                });
            }
            unsafe {
                let _ = libc::flock(file.as_raw_fd(), libc::LOCK_UN);
            }
        }
        if let Ok(raw) = fs::read_to_string(&path) {
            let trimmed = raw.trim();
            if trimmed.starts_with("ACTIVE_WRITER")
                || trimmed.contains("\"active\":true")
                || trimmed.contains("\"active\": true")
                || trimmed.contains("\"writer\":true")
                || trimmed.contains("\"writer\": true")
            {
                return Err(HostAdapterError::NativeWriterActive {
                    host,
                    detail: format!("active native writer sentinel at {}", path.display()),
                });
            }
        }
    }
    Ok(())
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct ObserverCheckpoint {
    pub file_size: u64,
    pub mtime_ns: i128,
    pub byte_offset: u64,
    pub next_order_seq: i32,
    pub meta_digest: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ObserverErrorRecord {
    pub host: String,
    pub session_id: String,
    pub path: PathBuf,
    pub message: String,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct ObserverScanReport {
    pub host: String,
    pub sessions_scanned: usize,
    pub sessions_skipped_unchanged: usize,
    pub turn_files_opened: usize,
    pub turn_bytes_read: u64,
    pub items_projected: usize,
    pub items_emitted: usize,
    pub emitted_items: Vec<TimelineItem>,
    pub errors: Vec<ObserverErrorRecord>,
}

pub trait HostTimelineObserver {
    fn host_name(&self) -> &str;
    fn connector_id(&self) -> &str;
    fn project_into_ledger(
        &mut self,
        ledger: &FleetLedgerStore,
    ) -> Result<ObserverScanReport, HostAdapterError>;
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct PersistedCheckpointBook {
    entries: BTreeMap<String, ObserverCheckpoint>,
}

fn load_checkpoint_book(
    state_root: &Path,
) -> Result<BTreeMap<String, ObserverCheckpoint>, HostAdapterError> {
    let lock_path = state_root.join("observer-checkpoints.lock");
    let _guard = AdvisoryFileLock::acquire_exclusive(&lock_path)?;
    let file_path = state_root.join("observer-checkpoints.json");
    if !file_path.exists() {
        return Ok(BTreeMap::new());
    }
    let bytes = fs::read(&file_path)?;
    if bytes.is_empty() {
        return Ok(BTreeMap::new());
    }
    let parsed: PersistedCheckpointBook = serde_json::from_slice(&bytes).unwrap_or_default();
    Ok(parsed.entries)
}

fn save_checkpoint_book(
    state_root: &Path,
    entries: &BTreeMap<String, ObserverCheckpoint>,
) -> Result<(), HostAdapterError> {
    let lock_path = state_root.join("observer-checkpoints.lock");
    let _guard = AdvisoryFileLock::acquire_exclusive(&lock_path)?;
    let file_path = state_root.join("observer-checkpoints.json");
    let tmp_path = file_path.with_extension(format!("json.tmp.{}", std::process::id()));
    let book = PersistedCheckpointBook {
        entries: entries.clone(),
    };
    let bytes = serde_json::to_vec(&book)
        .map_err(|e| HostAdapterError::Io(std::io::Error::other(e.to_string())))?;
    fs::write(&tmp_path, bytes)?;
    fs::rename(&tmp_path, &file_path)?;
    Ok(())
}

fn stat_size_and_mtime_ns(path: &Path) -> Option<(u64, i128)> {
    let meta = fs::metadata(path).ok()?;
    let mtime_ns = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as i128)
        .unwrap_or(0);
    Some((meta.len(), mtime_ns))
}

fn slug_from_path(path: &Path, fallback: &str) -> String {
    path.file_name()
        .and_then(|s| s.to_str())
        .map(sanitize_slug)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| fallback.to_string())
}

fn sanitize_slug(raw: &str) -> String {
    let mut out = String::new();
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
            out.push(ch);
        } else if (ch == ' ' || ch == '/') && !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let trimmed = out.trim_matches(|c| c == '-' || c == '.');
    if trimmed.is_empty() {
        "project".into()
    } else {
        trimmed.to_string()
    }
}

fn parse_jsonl_line_into_timeline_item(
    session_id: &CanonicalSessionId,
    external_session_id: &str,
    line_idx: usize,
    order_seq: i32,
    val: &Value,
    sidebar_binding: Option<&SidebarProjectBinding>,
) -> Option<TimelineItem> {
    let raw_type = val
        .get("type")
        .or_else(|| val.get("kind"))
        .or_else(|| val.get("event"))
        .and_then(Value::as_str)
        .unwrap_or("message");

    // Skip pure header/title metadata lines when they have no turn text or tool body
    if matches!(
        raw_type,
        "session/header" | "header" | "session_header" | "meta" | "session/title"
    ) && val.get("text").is_none()
        && val.get("content").is_none()
        && val.get("prompt").is_none()
    {
        return None;
    }

    let role_str = val
        .get("role")
        .or_else(|| val.pointer("/message/role"))
        .or_else(|| val.pointer("/data/role"))
        .or_else(|| val.get("source"))
        .and_then(Value::as_str)
        .unwrap_or("");

    let mut content_text = val
        .get("content")
        .or_else(|| val.get("text"))
        .or_else(|| val.get("prompt"))
        .or_else(|| val.get("output"))
        .or_else(|| val.pointer("/message/content"))
        .or_else(|| val.pointer("/message/text"))
        .or_else(|| val.pointer("/data/content"))
        .or_else(|| val.pointer("/data/text"))
        .and_then(|v| {
            if let Some(s) = v.as_str() {
                Some(s.to_string())
            } else if let Some(arr) = v.as_array() {
                let joined: Vec<String> = arr
                    .iter()
                    .filter_map(|part| {
                        part.get("text")
                            .or_else(|| part.get("content"))
                            .and_then(Value::as_str)
                            .map(str::to_string)
                    })
                    .collect();
                (!joined.is_empty()).then(|| joined.join("\n"))
            } else {
                Some(v.to_string())
            }
        })
        .unwrap_or_default();

    if content_text.is_empty() {
        if let Some(tool_name) = val
            .get("tool")
            .or_else(|| val.get("tool_name"))
            .or_else(|| val.pointer("/data/tool"))
            .and_then(Value::as_str)
        {
            content_text = format!("tool:{tool_name}");
        } else {
            return None;
        }
    }

    let fleet_meta = extract_fleet_message_metadata(&content_text);
    let (kind, role) = if fleet_meta.is_some() || raw_type == "peer_message" {
        (TimelineItemKind::PeerMessage, TimelineRole::User)
    } else if raw_type.contains("tool_result") || raw_type == "tool/result" {
        (TimelineItemKind::ToolResult, TimelineRole::Tool)
    } else if raw_type.contains("tool") || raw_type == "function_call" {
        (TimelineItemKind::ToolCall, TimelineRole::Tool)
    } else if raw_type.contains("reason") || raw_type.contains("thinking") {
        (TimelineItemKind::Reasoning, TimelineRole::Assistant)
    } else if matches!(
        role_str,
        "user" | "USER" | "USER_EXPLICIT" | "human" | "client"
    ) || raw_type.contains("user")
    {
        (TimelineItemKind::UserMessage, TimelineRole::User)
    } else if matches!(role_str, "tool" | "TOOL") {
        (TimelineItemKind::ToolResult, TimelineRole::Tool)
    } else if matches!(role_str, "system" | "SYSTEM") {
        (TimelineItemKind::SystemNotice, TimelineRole::System)
    } else {
        (TimelineItemKind::AssistantMessage, TimelineRole::Assistant)
    };

    let status = match val
        .get("status")
        .or_else(|| val.pointer("/data/status"))
        .and_then(Value::as_str)
    {
        Some("in_progress" | "running" | "streaming" | "pending" | "queued") => {
            TimelineItemStatus::InProgress
        }
        Some("failed" | "error") => TimelineItemStatus::Failed,
        Some("cancelled" | "canceled") => TimelineItemStatus::Cancelled,
        _ => TimelineItemStatus::Completed,
    };

    let item_id = val
        .get("item_id")
        .or_else(|| val.get("id"))
        .or_else(|| val.get("turn_id"))
        .or_else(|| val.get("turnId"))
        .or_else(|| val.pointer("/data/id"))
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .map(|s| format!("{external_session_id}:{s}"))
        .unwrap_or_else(|| format!("{external_session_id}:item-{}", line_idx + 1));

    let turn_id = val
        .get("turn_id")
        .or_else(|| val.get("turnId"))
        .or_else(|| val.get("runId"))
        .or_else(|| val.pointer("/data/turnId"))
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .map(str::to_string)
        .or_else(|| Some(format!("turn-{}", line_idx + 1)));

    let created_at_ms = val
        .get("created_at_ms")
        .or_else(|| val.get("timestamp_ms"))
        .or_else(|| val.get("time_created"))
        .or_else(|| val.pointer("/time/created"))
        .and_then(Value::as_i64)
        .unwrap_or(0);

    let seq = TimelineOrderSeq::try_from(i64::from(order_seq)).ok()?;
    let mut item = TimelineItem::new(
        item_id,
        session_id.clone(),
        turn_id,
        kind,
        status,
        role,
        content_text,
        seq,
        UpdatedSeq::new(0),
        created_at_ms,
    )
    .with_delivery_tier(DeliveryTier::ThreadVisible);

    if let Some(binding) = sidebar_binding {
        item = item.with_sidebar_binding(binding.clone());
    }
    if let Some(meta) = fleet_meta {
        if let Some(sender) = meta.from {
            item = item.with_peer_sender(sender);
        }
    } else if let Some(sender) = val.get("peer_sender").and_then(Value::as_str) {
        if !sender.trim().is_empty() {
            item = item.with_peer_sender(sender.to_string());
        }
    }

    Some(item)
}

// ============================================================================
// 1. OpenAI Dots (`openai-dots`: `state_5.sqlite` + rollout JSONL)
// ============================================================================

#[derive(Debug)]
pub struct OpenAiDotsTimelineObserver {
    connector_id: String,
    codex_root: PathBuf,
    checkpoints: BTreeMap<String, ObserverCheckpoint>,
    checkpoints_loaded: bool,
}

impl OpenAiDotsTimelineObserver {
    pub fn new(connector_id: impl Into<String>, codex_root: PathBuf) -> Self {
        Self {
            connector_id: connector_id.into(),
            codex_root,
            checkpoints: BTreeMap::new(),
            checkpoints_loaded: false,
        }
    }

    pub fn state_db_path(&self) -> PathBuf {
        DesktopHostKind::OpenaiDots
            .spec()
            .resolve_db_path(&self.codex_root, "state_5")
            .expect("openai-dots state_5 db path in HostSpec")
    }
}

impl HostTimelineObserver for OpenAiDotsTimelineObserver {
    fn host_name(&self) -> &str {
        DesktopHostKind::OpenaiDots.as_str()
    }

    fn connector_id(&self) -> &str {
        &self.connector_id
    }

    fn project_into_ledger(
        &mut self,
        ledger: &FleetLedgerStore,
    ) -> Result<ObserverScanReport, HostAdapterError> {
        if !self.checkpoints_loaded {
            self.checkpoints = load_checkpoint_book(ledger.state_root())?;
            self.checkpoints_loaded = true;
        }
        let spec = DesktopHostKind::OpenaiDots.spec();
        let mut report = ObserverScanReport {
            host: self.host_name().to_string(),
            ..ObserverScanReport::default()
        };
        let db_path = self.state_db_path();
        if !db_path.exists() {
            return Ok(report);
        }

        let projects = list_codex_projects_readonly(&db_path)?;
        let project_map: BTreeMap<String, CodexProjectRow> = projects
            .into_iter()
            .map(|p| (p.project_id.clone(), p))
            .collect();
        let threads = list_codex_threads_readonly(&db_path)?;

        let mut checkpoints_dirty = false;
        let mut pending_items = Vec::new();

        for thread in threads {
            report.sessions_scanned += 1;
            let cp_key = spec.checkpoint_key(&thread.thread_id);
            let rollout_path = if thread.rollout_path.trim().is_empty() {
                None
            } else {
                let p = PathBuf::from(&thread.rollout_path);
                if p.is_absolute() {
                    Some(p)
                } else {
                    Some(self.codex_root.join(p))
                }
            };

            let (file_size, mtime_ns) = rollout_path
                .as_deref()
                .and_then(stat_size_and_mtime_ns)
                .unwrap_or((0, 0));
            let meta_digest = sha256_hex(
                format!(
                    "{}\0{}\0{}\0{}",
                    thread.project_id,
                    thread.cwd.display(),
                    thread.title,
                    thread.preview
                )
                .as_bytes(),
            );

            if let Some(existing_cp) = self.checkpoints.get(&cp_key) {
                if spec.is_checkpoint_unchanged(
                    existing_cp.file_size,
                    existing_cp.mtime_ns,
                    existing_cp.byte_offset,
                    &existing_cp.meta_digest,
                    file_size,
                    mtime_ns,
                    &meta_digest,
                ) {
                    report.sessions_skipped_unchanged += 1;
                    continue;
                }
            }

            let Ok(canonical_sid) = CanonicalSessionId::derive(
                &self.connector_id,
                spec.id,
                &thread.thread_id,
            ) else {
                report.errors.push(ObserverErrorRecord {
                    host: self.host_name().into(),
                    session_id: thread.thread_id.clone(),
                    path: db_path.clone(),
                    message: "invalid thread_id for CanonicalSessionId".into(),
                });
                continue;
            };

            let project_dir = if !thread.cwd.as_os_str().is_empty() {
                thread.cwd.clone()
            } else if let Some(proj) = project_map.get(&thread.project_id) {
                proj.root_path.clone()
            } else {
                PathBuf::from("/fixture/Projects/default")
            };
            let slug = slug_from_path(&project_dir, "default");
            let project_id = if !thread.project_id.trim().is_empty() {
                thread.project_id.clone()
            } else {
                spec.default_project_id(&slug)
            };
            let proj_title = project_map
                .get(&thread.project_id)
                .map(|p| p.name.clone())
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| slug.clone());
            let ws_uri = format!("file://{}", project_dir.display());
            let binding = SidebarProjectBinding {
                host: DesktopHostKind::OpenaiDots,
                project_id,
                title: proj_title,
                project_dir,
                workspace_uris: vec![ws_uri],
            };

            let prev_cp = self.checkpoints.get(&cp_key).cloned().unwrap_or_default();
            let (mut byte_offset, mut next_order_seq) = spec.resume_cursor_window(
                prev_cp.file_size,
                prev_cp.byte_offset,
                prev_cp.next_order_seq,
                file_size,
            );

            let mut session_items = Vec::new();
            let mut session_failed = false;

            if let Some(ref rpath) = rollout_path {
                if rpath.exists() && file_size > byte_offset {
                    match File::open(rpath) {
                        Ok(mut file) => {
                            report.turn_files_opened += 1;
                            if file.seek(SeekFrom::Start(byte_offset)).is_ok() {
                                let mut buf = String::new();
                                match file.read_to_string(&mut buf) {
                                    Ok(bytes_read) => {
                                        report.turn_bytes_read += bytes_read as u64;
                                        for (idx, line) in buf.lines().enumerate() {
                                            let trimmed = line.trim();
                                            if trimmed.is_empty() {
                                                continue;
                                            }
                                            match serde_json::from_str::<Value>(trimmed) {
                                                Ok(val) => {
                                                    let line_ord =
                                                        (next_order_seq / 10).max(1) as usize - 1
                                                            + idx;
                                                    if let Some(item) =
                                                        parse_jsonl_line_into_timeline_item(
                                                            &canonical_sid,
                                                            &thread.thread_id,
                                                            line_ord,
                                                            next_order_seq,
                                                            &val,
                                                            Some(&binding),
                                                        )
                                                    {
                                                        next_order_seq =
                                                            next_order_seq.saturating_add(10);
                                                        session_items.push(item);
                                                    }
                                                }
                                                Err(err) => {
                                                    session_failed = true;
                                                    report.errors.push(ObserverErrorRecord {
                                                        host: self.host_name().into(),
                                                        session_id: thread.thread_id.clone(),
                                                        path: rpath.clone(),
                                                        message: format!(
                                                            "corrupt rollout JSONL: {err}"
                                                        ),
                                                    });
                                                    break;
                                                }
                                            }
                                        }
                                        if !session_failed {
                                            byte_offset = file_size;
                                        }
                                    }
                                    Err(err) => {
                                        session_failed = true;
                                        report.errors.push(ObserverErrorRecord {
                                            host: self.host_name().into(),
                                            session_id: thread.thread_id.clone(),
                                            path: rpath.clone(),
                                            message: format!("failed reading rollout file: {err}"),
                                        });
                                    }
                                }
                            }
                        }
                        Err(err) => {
                            session_failed = true;
                            report.errors.push(ObserverErrorRecord {
                                host: self.host_name().into(),
                                session_id: thread.thread_id.clone(),
                                path: rpath.clone(),
                                message: format!("failed opening rollout file: {err}"),
                            });
                        }
                    }
                }
            }

            if session_failed {
                continue;
            }

            if session_items.is_empty()
                && byte_offset == 0
                && !thread.preview.trim().is_empty()
            {
                let fleet_meta = extract_fleet_message_metadata(&thread.preview);
                let kind = if fleet_meta.is_some() {
                    TimelineItemKind::PeerMessage
                } else {
                    TimelineItemKind::UserMessage
                };
                if let Ok(seq) = TimelineOrderSeq::try_from(i64::from(next_order_seq)) {
                    let mut item = TimelineItem::new(
                        format!("{}:preview", thread.thread_id),
                        canonical_sid.clone(),
                        Some("turn-1".into()),
                        kind,
                        TimelineItemStatus::Completed,
                        TimelineRole::User,
                        thread.preview.clone(),
                        seq,
                        UpdatedSeq::new(0),
                        0,
                    )
                    .with_delivery_tier(DeliveryTier::ThreadVisible)
                    .with_sidebar_binding(binding.clone());
                    if let Some(sender) = fleet_meta.and_then(|m| m.from) {
                        item = item.with_peer_sender(sender);
                    }
                    next_order_seq = next_order_seq.saturating_add(10);
                    session_items.push(item);
                }
            }

            let session_title = if !thread.title.trim().is_empty() {
                thread.title.clone()
            } else if !thread.preview.trim().is_empty() {
                thread.preview.chars().take(64).collect()
            } else {
                thread.thread_id.clone()
            };
            let meta = SessionMeta {
                session_id: canonical_sid,
                connector_id: self.connector_id.clone(),
                runtime: DesktopHostKind::OpenaiDots.as_str().to_string(),
                external_session_id: thread.thread_id.clone(),
                project_slug: slug,
                title: session_title,
                latest_updated_seq: UpdatedSeq::new(0),
                delivery_tier: Some(DeliveryTier::ThreadVisible),
                sidebar_binding: Some(binding),
            };
            ledger.upsert_session(&meta)?;

            report.items_projected += session_items.len();
            pending_items.extend(session_items);

            self.checkpoints.insert(
                cp_key,
                ObserverCheckpoint {
                    file_size,
                    mtime_ns,
                    byte_offset,
                    next_order_seq,
                    meta_digest,
                },
            );
            checkpoints_dirty = true;
        }

        if !pending_items.is_empty() {
            let emitted = ledger.upsert_timeline_batch(&pending_items)?;
            report.items_emitted = emitted.len();
            report.emitted_items = emitted;
        }
        if checkpoints_dirty {
            save_checkpoint_book(ledger.state_root(), &self.checkpoints)?;
        }
        Ok(report)
    }
}

// ============================================================================
// 2. Antigravity Desktop (`antigravity-desktop`: `conversation_summaries.db` + cascade JSONL)
// ============================================================================

#[derive(Debug)]
pub struct AntigravityTimelineObserver {
    connector_id: String,
    antigravity_root: PathBuf,
    checkpoints: BTreeMap<String, ObserverCheckpoint>,
    checkpoints_loaded: bool,
}

impl AntigravityTimelineObserver {
    pub fn new(connector_id: impl Into<String>, antigravity_root: PathBuf) -> Self {
        Self {
            connector_id: connector_id.into(),
            antigravity_root,
            checkpoints: BTreeMap::new(),
            checkpoints_loaded: false,
        }
    }

    pub fn summaries_db_path(&self) -> PathBuf {
        DesktopHostKind::AntigravityDesktop
            .spec()
            .resolve_db_path(&self.antigravity_root, "conversation_summaries")
            .expect("antigravity conversation_summaries db path in HostSpec")
    }

    fn resolve_cascade_jsonl(&self, conversation_id: &str) -> Option<PathBuf> {
        DesktopHostKind::AntigravityDesktop
            .spec()
            .resolve_transcript_path(&self.antigravity_root, conversation_id)
    }
}

impl HostTimelineObserver for AntigravityTimelineObserver {
    fn host_name(&self) -> &str {
        DesktopHostKind::AntigravityDesktop.as_str()
    }

    fn connector_id(&self) -> &str {
        &self.connector_id
    }

    fn project_into_ledger(
        &mut self,
        ledger: &FleetLedgerStore,
    ) -> Result<ObserverScanReport, HostAdapterError> {
        if !self.checkpoints_loaded {
            self.checkpoints = load_checkpoint_book(ledger.state_root())?;
            self.checkpoints_loaded = true;
        }
        let spec = DesktopHostKind::AntigravityDesktop.spec();
        let mut report = ObserverScanReport {
            host: self.host_name().to_string(),
            ..ObserverScanReport::default()
        };
        let db_path = self.summaries_db_path();
        if !db_path.exists() {
            return Ok(report);
        }

        let summaries: Vec<AntigravityConversationSummary> =
            list_conversation_summaries_readonly(&db_path)?;
        let mut checkpoints_dirty = false;
        let mut pending_items = Vec::new();

        for summary in summaries {
            report.sessions_scanned += 1;
            let cp_key = spec.checkpoint_key(&summary.conversation_id);
            let cascade_path = self.resolve_cascade_jsonl(&summary.conversation_id);
            let (file_size, mtime_ns) = cascade_path
                .as_deref()
                .and_then(stat_size_and_mtime_ns)
                .unwrap_or((0, 0));
            let meta_digest = sha256_hex(
                format!(
                    "{}\0{}\0{}\0{}\0{:?}",
                    summary.project_id,
                    summary.title,
                    summary.preview,
                    summary.step_count,
                    summary.workspace_uris
                )
                .as_bytes(),
            );

            if let Some(existing_cp) = self.checkpoints.get(&cp_key) {
                if spec.is_checkpoint_unchanged(
                    existing_cp.file_size,
                    existing_cp.mtime_ns,
                    existing_cp.byte_offset,
                    &existing_cp.meta_digest,
                    file_size,
                    mtime_ns,
                    &meta_digest,
                ) {
                    report.sessions_skipped_unchanged += 1;
                    continue;
                }
            }

            let Ok(canonical_sid) = CanonicalSessionId::derive(
                &self.connector_id,
                spec.id,
                &summary.conversation_id,
            ) else {
                continue;
            };

            let primary_dir = summary
                .workspace_uris
                .first()
                .and_then(|u| u.strip_prefix("file://"))
                .map(PathBuf::from)
                .unwrap_or_else(|| spec.fallback_project_dir());
            let slug = slug_from_path(&primary_dir, spec.cursor.fallback_slug);
            let project_id = if !summary.project_id.trim().is_empty() {
                summary.project_id.clone()
            } else {
                spec.default_project_id(&slug)
            };
            let workspace_uris = if summary.workspace_uris.is_empty() {
                vec![format!("file://{}", primary_dir.display())]
            } else {
                summary.workspace_uris.clone()
            };
            let binding = SidebarProjectBinding {
                host: DesktopHostKind::AntigravityDesktop,
                project_id,
                title: if summary.title.trim().is_empty() {
                    slug.clone()
                } else {
                    summary.title.clone()
                },
                project_dir: primary_dir,
                workspace_uris,
            };

            let prev_cp = self.checkpoints.get(&cp_key).cloned().unwrap_or_default();
            let (mut byte_offset, mut next_order_seq) = spec.resume_cursor_window(
                prev_cp.file_size,
                prev_cp.byte_offset,
                prev_cp.next_order_seq,
                file_size,
            );

            let mut session_items = Vec::new();
            let mut session_failed = false;

            if let Some(ref cpath) = cascade_path {
                if file_size > byte_offset {
                    if let Ok(mut file) = File::open(cpath) {
                        report.turn_files_opened += 1;
                        if file.seek(SeekFrom::Start(byte_offset)).is_ok() {
                            let mut buf = String::new();
                            if let Ok(n) = file.read_to_string(&mut buf) {
                                report.turn_bytes_read += n as u64;
                                for (idx, line) in buf.lines().enumerate() {
                                    let trimmed = line.trim();
                                    if trimmed.is_empty() {
                                        continue;
                                    }
                                    match serde_json::from_str::<Value>(trimmed) {
                                        Ok(val) => {
                                            let line_ord =
                                                (next_order_seq / 10).max(1) as usize - 1 + idx;
                                            if let Some(item) = parse_jsonl_line_into_timeline_item(
                                                &canonical_sid,
                                                &summary.conversation_id,
                                                line_ord,
                                                next_order_seq,
                                                &val,
                                                Some(&binding),
                                            ) {
                                                next_order_seq = next_order_seq.saturating_add(10);
                                                session_items.push(item);
                                            }
                                        }
                                        Err(err) => {
                                            session_failed = true;
                                            report.errors.push(ObserverErrorRecord {
                                                host: self.host_name().into(),
                                                session_id: summary.conversation_id.clone(),
                                                path: cpath.clone(),
                                                message: format!("corrupt cascade JSONL: {err}"),
                                            });
                                            break;
                                        }
                                    }
                                }
                                if !session_failed {
                                    byte_offset = file_size;
                                }
                            }
                        }
                    }
                }
            }

            if session_failed {
                continue;
            }

            if session_items.is_empty() && byte_offset == 0 {
                let text = if !summary.preview.trim().is_empty() {
                    summary.preview.clone()
                } else {
                    summary.title.clone()
                };
                if !text.trim().is_empty() {
                    let fleet_meta = extract_fleet_message_metadata(&text);
                    let kind = if fleet_meta.is_some() {
                        TimelineItemKind::PeerMessage
                    } else {
                        TimelineItemKind::AssistantMessage
                    };
                    if let Ok(seq) = TimelineOrderSeq::try_from(i64::from(next_order_seq)) {
                        let mut item = TimelineItem::new(
                            format!("{}:summary", summary.conversation_id),
                            canonical_sid.clone(),
                            Some(format!("turn-{}", summary.step_count.max(1))),
                            kind,
                            TimelineItemStatus::Completed,
                            TimelineRole::Assistant,
                            text,
                            seq,
                            UpdatedSeq::new(0),
                            0,
                        )
                        .with_delivery_tier(DeliveryTier::ThreadVisible)
                        .with_sidebar_binding(binding.clone());
                        if let Some(sender) = fleet_meta.and_then(|m| m.from) {
                            item = item.with_peer_sender(sender);
                        }
                        next_order_seq = next_order_seq.saturating_add(10);
                        session_items.push(item);
                    }
                }
            }

            let meta = SessionMeta {
                session_id: canonical_sid,
                connector_id: self.connector_id.clone(),
                runtime: DesktopHostKind::AntigravityDesktop.as_str().to_string(),
                external_session_id: summary.conversation_id.clone(),
                project_slug: slug,
                title: if summary.title.trim().is_empty() {
                    summary.conversation_id.clone()
                } else {
                    summary.title.clone()
                },
                latest_updated_seq: UpdatedSeq::new(0),
                delivery_tier: Some(DeliveryTier::ThreadVisible),
                sidebar_binding: Some(binding),
            };
            ledger.upsert_session(&meta)?;

            report.items_projected += session_items.len();
            pending_items.extend(session_items);

            self.checkpoints.insert(
                cp_key,
                ObserverCheckpoint {
                    file_size,
                    mtime_ns,
                    byte_offset,
                    next_order_seq,
                    meta_digest,
                },
            );
            checkpoints_dirty = true;
        }

        if !pending_items.is_empty() {
            let emitted = ledger.upsert_timeline_batch(&pending_items)?;
            report.items_emitted = emitted.len();
            report.emitted_items = emitted;
        }
        if checkpoints_dirty {
            save_checkpoint_book(ledger.state_root(), &self.checkpoints)?;
        }
        Ok(report)
    }
}

// ============================================================================
// 3. DeepSeek Harness (`deepseek-harness`: `workspace.json` + two-frame `session.v4.jsonl.zstd`)
// ============================================================================

#[derive(Debug)]
pub struct DeepSeekHarnessTimelineObserver {
    connector_id: String,
    dsh_root: PathBuf,
    checkpoints: BTreeMap<String, ObserverCheckpoint>,
    checkpoints_loaded: bool,
}

impl DeepSeekHarnessTimelineObserver {
    pub fn new(connector_id: impl Into<String>, dsh_root: PathBuf) -> Self {
        Self {
            connector_id: connector_id.into(),
            dsh_root,
            checkpoints: BTreeMap::new(),
            checkpoints_loaded: false,
        }
    }
}

impl HostTimelineObserver for DeepSeekHarnessTimelineObserver {
    fn host_name(&self) -> &str {
        DesktopHostKind::DeepseekHarness.as_str()
    }

    fn connector_id(&self) -> &str {
        &self.connector_id
    }

    fn project_into_ledger(
        &mut self,
        ledger: &FleetLedgerStore,
    ) -> Result<ObserverScanReport, HostAdapterError> {
        if !self.checkpoints_loaded {
            self.checkpoints = load_checkpoint_book(ledger.state_root())?;
            self.checkpoints_loaded = true;
        }
        let spec = DesktopHostKind::DeepseekHarness.spec();
        let mut report = ObserverScanReport {
            host: self.host_name().to_string(),
            ..ObserverScanReport::default()
        };

        let ws_path = spec
            .resolve_aux_path_in_root(&self.dsh_root, "workspace_storage")
            .expect("dsh workspace_storage path in HostSpec");
        let mut session_to_ws: BTreeMap<String, (String, PathBuf, String)> = BTreeMap::new();
        let mut path_to_ws: BTreeMap<PathBuf, (String, String)> = BTreeMap::new();
        if ws_path.exists() {
            if let Ok(ws_doc) =
                serde_json::from_slice::<Value>(&fs::read(&ws_path).unwrap_or_default())
            {
                if let Some(workspaces) = ws_doc
                    .pointer("/tables/workspaces")
                    .and_then(Value::as_object)
                {
                    for (ws_id, entry) in workspaces {
                        let p = PathBuf::from(
                            entry
                                .get("path")
                                .and_then(Value::as_str)
                                .unwrap_or_default(),
                        );
                        let t = entry
                            .get("title")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string();
                        path_to_ws.insert(p.clone(), (ws_id.clone(), t.clone()));
                        if let Some(sids) = entry.get("sessionIds").and_then(Value::as_array) {
                            for sid in sids.iter().filter_map(Value::as_str) {
                                session_to_ws
                                    .insert(sid.to_string(), (ws_id.clone(), p.clone(), t.clone()));
                            }
                        }
                    }
                }
            }
        }

        // Discover archives from `<dsh_root>/sessions/--*--/<session_id>/session.v4.jsonl.zstd`
        let sessions_dir = spec
            .resolve_aux_path_in_root(&self.dsh_root, "sessions_dir")
            .expect("dsh sessions_dir path in HostSpec");
        let session_cache_dir = spec
            .resolve_aux_path_in_root(&self.dsh_root, "session_cache_dir")
            .expect("dsh session_cache_dir path in HostSpec");
        let mut archive_entries: Vec<(String, PathBuf)> = Vec::new();
        if sessions_dir.is_dir() {
            if let Ok(buckets) = fs::read_dir(&sessions_dir) {
                for bucket in buckets.flatten() {
                    let bpath = bucket.path();
                    if !bpath.is_dir() {
                        continue;
                    }
                    if let Ok(sdirs) = fs::read_dir(&bpath) {
                        for sdir in sdirs.flatten() {
                            let spath = sdir.path();
                            let archive = spath.join("session.v4.jsonl.zstd");
                            if archive.is_file() {
                                if let Some(sid) =
                                    spath.file_name().and_then(|n| n.to_str()).map(str::to_string)
                                {
                                    archive_entries.push((sid, archive));
                                }
                            }
                        }
                    }
                }
            }
        }
        archive_entries.sort_by(|a, b| a.0.cmp(&b.0));

        let mut checkpoints_dirty = false;
        let mut pending_items = Vec::new();

        for (session_id, archive_path) in archive_entries {
            report.sessions_scanned += 1;
            let cp_key = spec.checkpoint_key(&session_id);
            let (file_size, mtime_ns) =
                stat_size_and_mtime_ns(&archive_path).unwrap_or((0, 0));

            if let Some(existing_cp) = self.checkpoints.get(&cp_key) {
                if spec.is_checkpoint_unchanged(
                    existing_cp.file_size,
                    existing_cp.mtime_ns,
                    existing_cp.byte_offset,
                    &existing_cp.meta_digest,
                    file_size,
                    mtime_ns,
                    "",
                ) {
                    report.sessions_skipped_unchanged += 1;
                    continue;
                }
            }

            report.turn_files_opened += 1;
            report.turn_bytes_read += file_size;
            let (header_opt, events) = match decompress_dsh_two_frame_zstd(&archive_path) {
                Ok(pair) => pair,
                Err(err) => {
                    report.errors.push(ObserverErrorRecord {
                        host: self.host_name().into(),
                        session_id: session_id.clone(),
                        path: archive_path.clone(),
                        message: err.to_string(),
                    });
                    continue;
                }
            };

            let Ok(canonical_sid) = CanonicalSessionId::derive(
                &self.connector_id,
                spec.id,
                &session_id,
            ) else {
                continue;
            };

            let projcache_path = session_cache_dir.join(format!("{session_id}.json"));
            let projcache_doc: Option<Value> = if projcache_path.is_file() {
                fs::read(&projcache_path)
                    .ok()
                    .and_then(|b| serde_json::from_slice(&b).ok())
            } else {
                None
            };

            let cwd_from_meta = projcache_doc
                .as_ref()
                .and_then(|d| {
                    d.pointer("/record/identity/cwd")
                        .or_else(|| d.pointer("/record/rows/workingDirectory/val"))
                        .and_then(Value::as_str)
                })
                .or_else(|| {
                    header_opt.as_ref().and_then(|h| {
                        h.get("cwd")
                            .or_else(|| h.pointer("/data/cwd"))
                            .and_then(Value::as_str)
                    })
                })
                .map(PathBuf::from);

            let (project_id, project_dir, ws_title) =
                if let Some((wid, wdir, wtitle)) = session_to_ws.get(&session_id) {
                    (wid.clone(), wdir.clone(), wtitle.clone())
                } else if let Some(ref cdir) = cwd_from_meta {
                    if let Some((wid, wtitle)) = path_to_ws.get(cdir) {
                        (wid.clone(), cdir.clone(), wtitle.clone())
                    } else {
                        let slug = slug_from_path(cdir, spec.cursor.fallback_slug);
                        (
                            spec.default_project_id(&slug),
                            cdir.clone(),
                            slug,
                        )
                    }
                } else {
                    let fallback = spec.fallback_project_dir();
                    (
                        spec.default_project_id(spec.cursor.fallback_slug),
                        fallback,
                        spec.cursor.fallback_slug.into(),
                    )
                };

            let slug = slug_from_path(&project_dir, spec.cursor.fallback_slug);
            let binding = SidebarProjectBinding {
                host: DesktopHostKind::DeepseekHarness,
                project_id,
                title: if ws_title.trim().is_empty() {
                    slug.clone()
                } else {
                    ws_title
                },
                project_dir: project_dir.clone(),
                workspace_uris: vec![format!("file://{}", project_dir.display())],
            };

            let mut session_title = projcache_doc
                .as_ref()
                .and_then(|d| d.pointer("/record/rows/title/val").and_then(Value::as_str))
                .unwrap_or_default()
                .to_string();

            let mut session_items = Vec::new();
            let mut order_seq: i32 = 10;
            for (idx, ev) in events.iter().enumerate() {
                if ev.get("type").and_then(Value::as_str) == Some("session/title") {
                    if let Some(t) = ev
                        .pointer("/data/title")
                        .or_else(|| ev.get("title"))
                        .and_then(Value::as_str)
                    {
                        if session_title.trim().is_empty() {
                            session_title = t.to_string();
                        }
                    }
                }
                if let Some(item) = parse_jsonl_line_into_timeline_item(
                    &canonical_sid,
                    &session_id,
                    idx,
                    order_seq,
                    ev,
                    Some(&binding),
                ) {
                    order_seq = order_seq.saturating_add(10);
                    session_items.push(item);
                }
            }

            if session_title.trim().is_empty() {
                session_title = session_id.clone();
            }

            let meta = SessionMeta {
                session_id: canonical_sid,
                connector_id: self.connector_id.clone(),
                runtime: DesktopHostKind::DeepseekHarness.as_str().to_string(),
                external_session_id: session_id.clone(),
                project_slug: slug,
                title: session_title,
                latest_updated_seq: UpdatedSeq::new(0),
                delivery_tier: Some(DeliveryTier::ThreadVisible),
                sidebar_binding: Some(binding),
            };
            ledger.upsert_session(&meta)?;

            report.items_projected += session_items.len();
            pending_items.extend(session_items);

            self.checkpoints.insert(
                cp_key,
                ObserverCheckpoint {
                    file_size,
                    mtime_ns,
                    byte_offset: file_size,
                    next_order_seq: order_seq,
                    meta_digest: String::new(),
                },
            );
            checkpoints_dirty = true;
        }

        if !pending_items.is_empty() {
            let emitted = ledger.upsert_timeline_batch(&pending_items)?;
            report.items_emitted = emitted.len();
            report.emitted_items = emitted;
        }
        if checkpoints_dirty {
            save_checkpoint_book(ledger.state_root(), &self.checkpoints)?;
        }
        Ok(report)
    }
}

// ============================================================================
// 4. Cindy (`cindy`: `<cindy_root>/transcripts/*.jsonl`)
// ============================================================================

#[derive(Debug)]
pub struct CindyTimelineObserver {
    connector_id: String,
    cindy_root: PathBuf,
    checkpoints: BTreeMap<String, ObserverCheckpoint>,
    checkpoints_loaded: bool,
}

impl CindyTimelineObserver {
    pub fn new(connector_id: impl Into<String>, cindy_root: PathBuf) -> Self {
        Self {
            connector_id: connector_id.into(),
            cindy_root,
            checkpoints: BTreeMap::new(),
            checkpoints_loaded: false,
        }
    }

    pub fn transcripts_dir(&self) -> PathBuf {
        DesktopHostKind::Cindy
            .spec()
            .resolve_aux_path_in_root(&self.cindy_root, "transcripts_dir")
            .expect("cindy transcripts_dir path in HostSpec")
    }
}

impl HostTimelineObserver for CindyTimelineObserver {
    fn host_name(&self) -> &str {
        DesktopHostKind::Cindy.as_str()
    }

    fn connector_id(&self) -> &str {
        &self.connector_id
    }

    fn project_into_ledger(
        &mut self,
        ledger: &FleetLedgerStore,
    ) -> Result<ObserverScanReport, HostAdapterError> {
        if !self.checkpoints_loaded {
            self.checkpoints = load_checkpoint_book(ledger.state_root())?;
            self.checkpoints_loaded = true;
        }
        let spec = DesktopHostKind::Cindy.spec();
        let mut report = ObserverScanReport {
            host: self.host_name().to_string(),
            ..ObserverScanReport::default()
        };
        let tdir = self.transcripts_dir();
        if !tdir.is_dir() {
            return Ok(report);
        }

        let mut files: Vec<PathBuf> = fs::read_dir(&tdir)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("jsonl"))
            .collect();
        files.sort();

        let mut checkpoints_dirty = false;
        let mut pending_items = Vec::new();

        for path in files {
            let Some(session_key) = path
                .file_stem()
                .and_then(|s| s.to_str())
                .filter(|s| !s.is_empty())
                .map(str::to_string)
            else {
                continue;
            };
            report.sessions_scanned += 1;
            let cp_key = spec.checkpoint_key(&session_key);
            let (file_size, mtime_ns) = stat_size_and_mtime_ns(&path).unwrap_or((0, 0));

            if let Some(existing_cp) = self.checkpoints.get(&cp_key) {
                if spec.is_checkpoint_unchanged(
                    existing_cp.file_size,
                    existing_cp.mtime_ns,
                    existing_cp.byte_offset,
                    &existing_cp.meta_digest,
                    file_size,
                    mtime_ns,
                    "",
                ) {
                    report.sessions_skipped_unchanged += 1;
                    continue;
                }
            }

            let Ok(canonical_sid) = CanonicalSessionId::derive(
                &self.connector_id,
                spec.id,
                &session_key,
            ) else {
                continue;
            };

            let prev_cp = self.checkpoints.get(&cp_key).cloned().unwrap_or_default();
            let (mut byte_offset, mut next_order_seq) = spec.resume_cursor_window(
                prev_cp.file_size,
                prev_cp.byte_offset,
                prev_cp.next_order_seq,
                file_size,
            );

            let mut file = match File::open(&path) {
                Ok(f) => f,
                Err(err) => {
                    report.errors.push(ObserverErrorRecord {
                        host: self.host_name().into(),
                        session_id: session_key,
                        path: path.clone(),
                        message: err.to_string(),
                    });
                    continue;
                }
            };
            report.turn_files_opened += 1;
            if file.seek(SeekFrom::Start(byte_offset)).is_err() {
                continue;
            }
            let mut buf = String::new();
            let bytes_read = match file.read_to_string(&mut buf) {
                Ok(n) => n,
                Err(err) => {
                    report.errors.push(ObserverErrorRecord {
                        host: self.host_name().into(),
                        session_id: session_key,
                        path: path.clone(),
                        message: err.to_string(),
                    });
                    continue;
                }
            };
            report.turn_bytes_read += bytes_read as u64;

            let mut session_items = Vec::new();
            let mut session_failed = false;
            let mut discovered_cwd: Option<PathBuf> = None;
            let mut discovered_project_id: Option<String> = None;
            let mut discovered_title: Option<String> = None;

            for (idx, line) in buf.lines().enumerate() {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                match serde_json::from_str::<Value>(trimmed) {
                    Ok(val) => {
                        if discovered_cwd.is_none() {
                            if let Some(c) = val.get("cwd").and_then(Value::as_str) {
                                if !c.trim().is_empty() {
                                    discovered_cwd = Some(PathBuf::from(c));
                                }
                            }
                        }
                        if discovered_project_id.is_none() {
                            if let Some(pid) = val
                                .get("projectId")
                                .or_else(|| val.get("project_id"))
                                .and_then(Value::as_str)
                            {
                                if !pid.trim().is_empty() {
                                    discovered_project_id = Some(pid.to_string());
                                }
                            }
                        }
                        if discovered_title.is_none() {
                            if let Some(t) = val.get("title").and_then(Value::as_str) {
                                if !t.trim().is_empty() {
                                    discovered_title = Some(t.to_string());
                                }
                            }
                        }

                        let project_dir = discovered_cwd
                            .clone()
                            .unwrap_or_else(|| spec.fallback_project_dir());
                        let slug = slug_from_path(&project_dir, spec.cursor.fallback_slug);
                        let binding = SidebarProjectBinding {
                            host: DesktopHostKind::Cindy,
                            project_id: discovered_project_id
                                .clone()
                                .unwrap_or_else(|| spec.default_project_id(&slug)),
                            title: discovered_title.clone().unwrap_or_else(|| slug.clone()),
                            project_dir: project_dir.clone(),
                            workspace_uris: vec![format!("file://{}", project_dir.display())],
                        };

                        let line_ord = (next_order_seq / 10).max(1) as usize - 1 + idx;
                        if let Some(item) = parse_jsonl_line_into_timeline_item(
                            &canonical_sid,
                            &session_key,
                            line_ord,
                            next_order_seq,
                            &val,
                            Some(&binding),
                        ) {
                            next_order_seq = next_order_seq.saturating_add(spec.cursor.order_seq_step);
                            session_items.push(item);
                        }
                    }
                    Err(err) => {
                        session_failed = true;
                        report.errors.push(ObserverErrorRecord {
                            host: self.host_name().into(),
                            session_id: session_key.clone(),
                            path: path.clone(),
                            message: format!("corrupt Cindy transcript JSONL: {err}"),
                        });
                        break;
                    }
                }
            }

            if session_failed {
                continue;
            }
            byte_offset = file_size;

            if byte_offset == file_size && prev_cp.byte_offset == 0 {
                let project_dir = discovered_cwd
                    .unwrap_or_else(|| spec.fallback_project_dir());
                let slug = slug_from_path(&project_dir, spec.cursor.fallback_slug);
                let binding = SidebarProjectBinding {
                    host: DesktopHostKind::Cindy,
                    project_id: discovered_project_id
                        .unwrap_or_else(|| spec.default_project_id(&slug)),
                    title: discovered_title.clone().unwrap_or_else(|| slug.clone()),
                    project_dir: project_dir.clone(),
                    workspace_uris: vec![format!("file://{}", project_dir.display())],
                };
                let meta = SessionMeta {
                    session_id: canonical_sid,
                    connector_id: self.connector_id.clone(),
                    runtime: spec.id.to_string(),
                    external_session_id: session_key.clone(),
                    project_slug: slug,
                    title: discovered_title.unwrap_or_else(|| session_key.clone()),
                    latest_updated_seq: UpdatedSeq::new(0),
                    delivery_tier: Some(DeliveryTier::ThreadVisible),
                    sidebar_binding: Some(binding),
                };
                ledger.upsert_session(&meta)?;
            }

            report.items_projected += session_items.len();
            pending_items.extend(session_items);

            self.checkpoints.insert(
                cp_key,
                ObserverCheckpoint {
                    file_size,
                    mtime_ns,
                    byte_offset,
                    next_order_seq,
                    meta_digest: String::new(),
                },
            );
            checkpoints_dirty = true;
        }

        if !pending_items.is_empty() {
            let emitted = ledger.upsert_timeline_batch(&pending_items)?;
            report.items_emitted = emitted.len();
            report.emitted_items = emitted;
        }
        if checkpoints_dirty {
            save_checkpoint_book(ledger.state_root(), &self.checkpoints)?;
        }
        Ok(report)
    }
}

// ============================================================================
// 5. ZCode (`zcode`: `v2/tasks-index.sqlite` + `cli/db/db.sqlite`)
// ============================================================================

#[derive(Debug)]
pub struct ZcodeTimelineObserver {
    connector_id: String,
    zcode_root: PathBuf,
    checkpoints: BTreeMap<String, ObserverCheckpoint>,
    checkpoints_loaded: bool,
}

impl ZcodeTimelineObserver {
    pub fn new(connector_id: impl Into<String>, zcode_root: PathBuf) -> Self {
        Self {
            connector_id: connector_id.into(),
            zcode_root,
            checkpoints: BTreeMap::new(),
            checkpoints_loaded: false,
        }
    }

    pub fn tasks_index_db_path(&self) -> PathBuf {
        DesktopHostKind::Zcode
            .spec()
            .resolve_db_path(&self.zcode_root, "tasks-index")
            .expect("zcode tasks-index db path in HostSpec")
    }

    pub fn cli_db_path(&self) -> PathBuf {
        DesktopHostKind::Zcode
            .spec()
            .resolve_db_path(&self.zcode_root, "cli-db")
            .expect("zcode cli-db path in HostSpec")
    }
}

impl HostTimelineObserver for ZcodeTimelineObserver {
    fn host_name(&self) -> &str {
        DesktopHostKind::Zcode.as_str()
    }

    fn connector_id(&self) -> &str {
        &self.connector_id
    }

    fn project_into_ledger(
        &mut self,
        ledger: &FleetLedgerStore,
    ) -> Result<ObserverScanReport, HostAdapterError> {
        if !self.checkpoints_loaded {
            self.checkpoints = load_checkpoint_book(ledger.state_root())?;
            self.checkpoints_loaded = true;
        }
        let spec = DesktopHostKind::Zcode.spec();
        let mut report = ObserverScanReport {
            host: self.host_name().to_string(),
            ..ObserverScanReport::default()
        };
        let tasks_db = self.tasks_index_db_path();
        let cli_db = self.cli_db_path();
        if !tasks_db.exists() {
            return Ok(report);
        }

        let sessions = list_zcode_sessions_readonly(&tasks_db, &cli_db)?;
        let cli_conn = if cli_db.exists() {
            open_sqlite_readonly_preserve_mtime(&cli_db).ok()
        } else {
            None
        };
        let mut checkpoints_dirty = false;
        let mut pending_items = Vec::new();

        for snap in sessions {
            report.sessions_scanned += 1;
            let cp_key = spec.checkpoint_key(&snap.session_id);
            let meta_digest = sha256_hex(
                format!(
                    "{}\0{}\0{}\0{}\0{}",
                    snap.project_id,
                    snap.title,
                    snap.cwd.display(),
                    snap.message_count,
                    snap.task_status
                )
                .as_bytes(),
            );

            if let Some(existing_cp) = self.checkpoints.get(&cp_key) {
                if spec.is_checkpoint_unchanged(
                    existing_cp.file_size,
                    existing_cp.mtime_ns,
                    existing_cp.byte_offset,
                    &existing_cp.meta_digest,
                    snap.message_count as u64,
                    0,
                    &meta_digest,
                ) {
                    report.sessions_skipped_unchanged += 1;
                    continue;
                }
            }

            let Ok(canonical_sid) = CanonicalSessionId::derive(
                &self.connector_id,
                spec.id,
                &snap.session_id,
            ) else {
                continue;
            };

            let project_dir = if !snap.cwd.as_os_str().is_empty() {
                snap.cwd.clone()
            } else {
                spec.fallback_project_dir()
            };
            let slug = slug_from_path(&project_dir, spec.cursor.fallback_slug);
            let project_id = if !snap.project_id.trim().is_empty() {
                snap.project_id.clone()
            } else {
                spec.default_project_id(&slug)
            };
            let binding = SidebarProjectBinding {
                host: DesktopHostKind::Zcode,
                project_id,
                title: if snap.title.trim().is_empty() {
                    slug.clone()
                } else {
                    snap.title.clone()
                },
                project_dir: project_dir.clone(),
                workspace_uris: vec![format!("file://{}", project_dir.display())],
            };

            let messages = match cli_conn
                .as_ref()
                .map(|c| list_zcode_messages_with_conn(c, &snap.session_id))
                .unwrap_or_else(|| Ok(Vec::new()))
            {
                Ok(m) => m,
                Err(err) => {
                    report.errors.push(ObserverErrorRecord {
                        host: self.host_name().into(),
                        session_id: snap.session_id.clone(),
                        path: cli_db.clone(),
                        message: err.to_string(),
                    });
                    continue;
                }
            };

            let mut session_items = Vec::new();
            let mut order_seq: i32 = 10;

            for (idx, msg) in messages.iter().enumerate() {
                let msg_val: Value =
                    serde_json::from_str(&msg.message_data).unwrap_or_else(|_| serde_json::json!({}));
                let mut text = msg_val
                    .get("text")
                    .or_else(|| msg_val.get("content"))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();

                if text.is_empty() {
                    let mut part_texts = Vec::new();
                    for raw_part in &msg.parts_data {
                        if let Ok(pval) = serde_json::from_str::<Value>(raw_part) {
                            if let Some(pt) = pval
                                .get("text")
                                .or_else(|| pval.get("content"))
                                .and_then(Value::as_str)
                            {
                                if !pt.is_empty() {
                                    part_texts.push(pt.to_string());
                                }
                            }
                        }
                    }
                    text = part_texts.join("\n");
                }
                if text.trim().is_empty() {
                    continue;
                }

                let role_str = msg_val
                    .get("role")
                    .and_then(Value::as_str)
                    .unwrap_or("user");
                let fleet_meta = extract_fleet_message_metadata(&text);
                let (kind, role) = if fleet_meta.is_some() {
                    (TimelineItemKind::PeerMessage, TimelineRole::User)
                } else if matches!(role_str, "assistant" | "agent" | "model") {
                    (TimelineItemKind::AssistantMessage, TimelineRole::Assistant)
                } else if role_str == "tool" {
                    (TimelineItemKind::ToolResult, TimelineRole::Tool)
                } else {
                    (TimelineItemKind::UserMessage, TimelineRole::User)
                };

                let turn_id = msg_val
                    .get("turn_id")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| format!("turn-{}", idx + 1));

                if let Ok(seq) = TimelineOrderSeq::try_from(i64::from(order_seq)) {
                    let mut item = TimelineItem::new(
                        format!("{}:{}", snap.session_id, msg.message_id),
                        canonical_sid.clone(),
                        Some(turn_id),
                        kind,
                        TimelineItemStatus::Completed,
                        role,
                        text,
                        seq,
                        UpdatedSeq::new(0),
                        msg.time_created,
                    )
                    .with_delivery_tier(DeliveryTier::ThreadVisible)
                    .with_sidebar_binding(binding.clone());
                    if let Some(sender) = fleet_meta.and_then(|m| m.from) {
                        item = item.with_peer_sender(sender);
                    }
                    order_seq = order_seq.saturating_add(10);
                    session_items.push(item);
                }
            }

            if session_items.is_empty() && !snap.title.trim().is_empty() {
                let fleet_meta = extract_fleet_message_metadata(&snap.title);
                if let Ok(seq) = TimelineOrderSeq::try_from(i64::from(order_seq)) {
                    let mut item = TimelineItem::new(
                        format!("{}:title", snap.session_id),
                        canonical_sid.clone(),
                        Some("turn-1".into()),
                        if fleet_meta.is_some() {
                            TimelineItemKind::PeerMessage
                        } else {
                            TimelineItemKind::UserMessage
                        },
                        TimelineItemStatus::Completed,
                        TimelineRole::User,
                        snap.title.clone(),
                        seq,
                        UpdatedSeq::new(0),
                        0,
                    )
                    .with_delivery_tier(DeliveryTier::ThreadVisible)
                    .with_sidebar_binding(binding.clone());
                    if let Some(sender) = fleet_meta.and_then(|m| m.from) {
                        item = item.with_peer_sender(sender);
                    }
                    order_seq = order_seq.saturating_add(10);
                    session_items.push(item);
                }
            }

            let meta = SessionMeta {
                session_id: canonical_sid,
                connector_id: self.connector_id.clone(),
                runtime: DesktopHostKind::Zcode.as_str().to_string(),
                external_session_id: snap.session_id.clone(),
                project_slug: slug,
                title: if snap.title.trim().is_empty() {
                    snap.session_id.clone()
                } else {
                    snap.title.clone()
                },
                latest_updated_seq: UpdatedSeq::new(0),
                delivery_tier: Some(DeliveryTier::ThreadVisible),
                sidebar_binding: Some(binding),
            };
            ledger.upsert_session(&meta)?;

            report.items_projected += session_items.len();
            pending_items.extend(session_items);

            self.checkpoints.insert(
                cp_key,
                ObserverCheckpoint {
                    file_size: snap.message_count as u64,
                    mtime_ns: 0,
                    byte_offset: snap.message_count as u64,
                    next_order_seq: order_seq,
                    meta_digest,
                },
            );
            checkpoints_dirty = true;
        }

        if !pending_items.is_empty() {
            let emitted = ledger.upsert_timeline_batch(&pending_items)?;
            report.items_emitted = emitted.len();
            report.emitted_items = emitted;
        }
        if checkpoints_dirty {
            save_checkpoint_book(ledger.state_root(), &self.checkpoints)?;
        }
        Ok(report)
    }
}

// ============================================================================
// 6. GrokBot (`grokbot`: peer mailbox messages projected into timeline items)
// ============================================================================

#[derive(Debug)]
pub struct GrokBotTimelineObserver {
    connector_id: String,
    checkpoints: BTreeMap<String, ObserverCheckpoint>,
    checkpoints_loaded: bool,
}

impl GrokBotTimelineObserver {
    pub fn new(connector_id: impl Into<String>) -> Self {
        Self {
            connector_id: connector_id.into(),
            checkpoints: BTreeMap::new(),
            checkpoints_loaded: false,
        }
    }
}

impl HostTimelineObserver for GrokBotTimelineObserver {
    fn host_name(&self) -> &str {
        DesktopHostKind::ALL_SPECS[5].id
    }

    fn connector_id(&self) -> &str {
        &self.connector_id
    }

    fn project_into_ledger(
        &mut self,
        ledger: &FleetLedgerStore,
    ) -> Result<ObserverScanReport, HostAdapterError> {
        if !self.checkpoints_loaded {
            self.checkpoints = load_checkpoint_book(ledger.state_root())?;
            self.checkpoints_loaded = true;
        }
        let spec = &DesktopHostKind::ALL_SPECS[5];
        let mut report = ObserverScanReport {
            host: spec.id.into(),
            ..ObserverScanReport::default()
        };
        let all_messages = ledger.list_all_messages()?;
        let mut grouped: BTreeMap<String, Vec<fleet_proto::InboxMessage>> = BTreeMap::new();
        for msg in all_messages {
            let touches_grokbot =
                msg.from.starts_with(spec.id) || msg.to.starts_with(spec.id);
            if !touches_grokbot {
                continue;
            }
            let ext_sid = msg
                .session_id
                .clone()
                .unwrap_or_else(|| format!("{}-{}", spec.id, msg.id));
            grouped.entry(ext_sid).or_default().push(msg);
        }

        let mut checkpoints_dirty = false;
        let mut pending_items = Vec::new();

        for (ext_sid, msgs) in grouped {
            report.sessions_scanned += 1;
            let cp_key = spec.checkpoint_key(&ext_sid);
            let count = msgs.len() as u64;
            let delivered_mask: String = msgs
                .iter()
                .map(|m| if m.delivered { '1' } else { '0' })
                .collect();
            if let Some(existing_cp) = self.checkpoints.get(&cp_key) {
                if spec.is_checkpoint_unchanged(
                    existing_cp.file_size,
                    existing_cp.mtime_ns,
                    existing_cp.byte_offset,
                    &existing_cp.meta_digest,
                    count,
                    0,
                    &delivered_mask,
                ) {
                    report.sessions_skipped_unchanged += 1;
                    continue;
                }
            }

            let Ok(canonical_sid) =
                CanonicalSessionId::derive(&self.connector_id, spec.id, &ext_sid)
            else {
                continue;
            };
            let slug = msgs
                .first()
                .and_then(|m| m.project_slug.clone())
                .unwrap_or_else(|| spec.cursor.fallback_slug.into());
            let meta = SessionMeta {
                session_id: canonical_sid.clone(),
                connector_id: self.connector_id.clone(),
                runtime: spec.id.into(),
                external_session_id: ext_sid.clone(),
                project_slug: slug,
                title: format!("GrokBot {ext_sid}"),
                latest_updated_seq: UpdatedSeq::new(0),
                delivery_tier: Some(DeliveryTier::Accepted),
                sidebar_binding: None,
            };
            ledger.upsert_session(&meta)?;

            let mut order_seq: i32 = spec.cursor.initial_order_seq;
            for msg in &msgs {
                let tier = if msg.delivered {
                    DeliveryTier::Completed
                } else {
                    DeliveryTier::Accepted
                };
                if let Ok(seq) = TimelineOrderSeq::try_from(i64::from(order_seq)) {
                    let item = TimelineItem::new(
                        format!("{}:{}", spec.id, msg.id),
                        canonical_sid.clone(),
                        Some(msg.id.clone()),
                        TimelineItemKind::PeerMessage,
                        TimelineItemStatus::Completed,
                        TimelineRole::User,
                        msg.body.clone(),
                        seq,
                        UpdatedSeq::new(0),
                        (msg.created_at_unix as i64).saturating_mul(1000),
                    )
                    .with_delivery_tier(tier)
                    .with_peer_sender(msg.from.clone());
                    order_seq = order_seq.saturating_add(spec.cursor.order_seq_step);
                    report.items_projected += 1;
                    pending_items.push(item);
                }
            }

            self.checkpoints.insert(
                cp_key,
                ObserverCheckpoint {
                    file_size: count,
                    mtime_ns: 0,
                    byte_offset: count,
                    next_order_seq: order_seq,
                    meta_digest: delivered_mask,
                },
            );
            checkpoints_dirty = true;
        }

        if !pending_items.is_empty() {
            let emitted = ledger.upsert_timeline_batch(&pending_items)?;
            report.items_emitted = emitted.len();
            report.emitted_items = emitted;
        }
        if checkpoints_dirty {
            save_checkpoint_book(ledger.state_root(), &self.checkpoints)?;
        }
        Ok(report)
    }
}

/// Projects timeline items for a canonical desktop host root or `grokbot` into `ledger`.
pub fn project_host_timelines(
    ledger: &FleetLedgerStore,
    connector_id: &str,
    host: &str,
    host_root: Option<&Path>,
) -> Result<ObserverScanReport, HostAdapterError> {
    let trimmed = host.trim();
    let Some(spec) = DesktopHostKind::spec_by_id(trimmed) else {
        return Err(HostAdapterError::ExternalAgentProtocol(format!(
            "unsupported timeline observer host: {trimmed}"
        )));
    };
    match spec.desktop_kind {
        Some(DesktopHostKind::OpenaiDots) => {
            let root = host_root.ok_or(HostAdapterError::RuntimeNotConfigured(
                DesktopHostKind::OpenaiDots,
            ))?;
            OpenAiDotsTimelineObserver::new(connector_id, root.to_path_buf())
                .project_into_ledger(ledger)
        }
        Some(DesktopHostKind::AntigravityDesktop) => {
            let root = host_root.ok_or(HostAdapterError::RuntimeNotConfigured(
                DesktopHostKind::AntigravityDesktop,
            ))?;
            AntigravityTimelineObserver::new(connector_id, root.to_path_buf())
                .project_into_ledger(ledger)
        }
        Some(DesktopHostKind::DeepseekHarness) => {
            let root = host_root.ok_or(HostAdapterError::RuntimeNotConfigured(
                DesktopHostKind::DeepseekHarness,
            ))?;
            DeepSeekHarnessTimelineObserver::new(connector_id, root.to_path_buf())
                .project_into_ledger(ledger)
        }
        Some(DesktopHostKind::Cindy) => {
            let root = host_root.ok_or(HostAdapterError::RuntimeNotConfigured(
                DesktopHostKind::Cindy,
            ))?;
            CindyTimelineObserver::new(connector_id, root.to_path_buf())
                .project_into_ledger(ledger)
        }
        Some(DesktopHostKind::Zcode) => {
            let root = host_root.ok_or(HostAdapterError::RuntimeNotConfigured(
                DesktopHostKind::Zcode,
            ))?;
            ZcodeTimelineObserver::new(connector_id, root.to_path_buf())
                .project_into_ledger(ledger)
        }
        None => GrokBotTimelineObserver::new(connector_id).project_into_ledger(ledger),
    }
}
