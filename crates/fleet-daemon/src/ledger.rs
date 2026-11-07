use crate::host_adapter::{HostAdapterError, PersistedSessionRecord};
use fleet_proto::{
    sha256_hex, timeline_content_hash, validate_brief, validate_nonempty_id, validate_session_id,
    CanonicalSessionId, DeliveryTier, DesktopHostKind, EffectiveCapabilities, InboxMessage,
    InteractionStatus, PeerAddress, ProjectPathError, SessionMeta, SessionNotice,
    SidebarProjectBinding, TimelineContentHash, TimelineItem, TimelineItemKind, TimelineItemStatus,
    TimelineOrderSeq, TimelineRole, UpdatedSeq,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::{c_char, c_int, c_uchar, c_void, CStr, CString},
    fs::{self, File, OpenOptions},
    os::unix::io::AsRawFd,
    path::{Path, PathBuf},
    ptr,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

#[repr(C)]
pub(crate) struct Sqlite3Db {
    _private: [u8; 0],
}

#[repr(C)]
pub(crate) struct Sqlite3Stmt {
    _private: [u8; 0],
}

pub(crate) const SQLITE_OK: c_int = 0;
pub(crate) const SQLITE_BUSY: c_int = 5;
pub(crate) const SQLITE_LOCKED: c_int = 6;
pub(crate) const SQLITE_ROW: c_int = 100;
pub(crate) const SQLITE_DONE: c_int = 101;
pub(crate) const SQLITE_NULL: c_int = 5;

pub const SQLITE_OPEN_READONLY: c_int = 0x0000_0001;
pub const SQLITE_OPEN_READWRITE: c_int = 0x0000_0002;
pub const SQLITE_OPEN_CREATE: c_int = 0x0000_0004;
pub const SQLITE_OPEN_NOMUTEX: c_int = 0x0000_8000;

const SQLITE_TRANSIENT: isize = -1;

#[link(name = "sqlite3")]
unsafe extern "C" {
    fn sqlite3_open_v2(
        filename: *const c_char,
        pp_db: *mut *mut Sqlite3Db,
        flags: c_int,
        z_vfs: *const c_char,
    ) -> c_int;
    fn sqlite3_close_v2(db: *mut Sqlite3Db) -> c_int;
    fn sqlite3_errmsg(db: *mut Sqlite3Db) -> *const c_char;
    fn sqlite3_busy_timeout(db: *mut Sqlite3Db, ms: c_int) -> c_int;
    fn sqlite3_exec(
        db: *mut Sqlite3Db,
        sql: *const c_char,
        callback: Option<
            unsafe extern "C" fn(*mut c_void, c_int, *mut *mut c_char, *mut *mut c_char) -> c_int,
        >,
        arg: *mut c_void,
        errmsg: *mut *mut c_char,
    ) -> c_int;
    fn sqlite3_free(ptr: *mut c_void);
    fn sqlite3_prepare_v2(
        db: *mut Sqlite3Db,
        z_sql: *const c_char,
        n_byte: c_int,
        pp_stmt: *mut *mut Sqlite3Stmt,
        pz_tail: *mut *const c_char,
    ) -> c_int;
    fn sqlite3_finalize(p_stmt: *mut Sqlite3Stmt) -> c_int;
    fn sqlite3_step(p_stmt: *mut Sqlite3Stmt) -> c_int;
    fn sqlite3_bind_text(
        p_stmt: *mut Sqlite3Stmt,
        index: c_int,
        val: *const c_char,
        n_byte: c_int,
        destructor: isize,
    ) -> c_int;
    fn sqlite3_bind_int64(p_stmt: *mut Sqlite3Stmt, index: c_int, val: i64) -> c_int;
    fn sqlite3_bind_null(p_stmt: *mut Sqlite3Stmt, index: c_int) -> c_int;
    fn sqlite3_column_type(p_stmt: *mut Sqlite3Stmt, i_col: c_int) -> c_int;
    fn sqlite3_column_text(p_stmt: *mut Sqlite3Stmt, i_col: c_int) -> *const c_uchar;
    fn sqlite3_column_bytes(p_stmt: *mut Sqlite3Stmt, i_col: c_int) -> c_int;
    fn sqlite3_column_int64(p_stmt: *mut Sqlite3Stmt, i_col: c_int) -> i64;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SqliteError {
    Busy(String),
    Io(String),
    Query(String),
}

impl std::fmt::Display for SqliteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Busy(msg) => write!(f, "sqlite busy/locked: {msg}"),
            Self::Io(msg) => write!(f, "sqlite I/O error: {msg}"),
            Self::Query(msg) => write!(f, "sqlite query error: {msg}"),
        }
    }
}

impl std::error::Error for SqliteError {}

impl From<SqliteError> for HostAdapterError {
    fn from(value: SqliteError) -> Self {
        HostAdapterError::Io(std::io::Error::other(value.to_string()))
    }
}

pub(crate) struct SqliteConnection {
    raw: *mut Sqlite3Db,
}

unsafe impl Send for SqliteConnection {}

impl Drop for SqliteConnection {
    fn drop(&mut self) {
        if !self.raw.is_null() {
            unsafe {
                let _ = sqlite3_close_v2(self.raw);
            }
            self.raw = ptr::null_mut();
        }
    }
}

impl SqliteConnection {
    pub(crate) fn open_with_flags(
        path: &Path,
        flags: c_int,
        busy_timeout_ms: c_int,
    ) -> Result<Self, SqliteError> {
        let path_str = path.to_str().ok_or_else(|| {
            SqliteError::Io(format!("non-UTF-8 sqlite path: {}", path.display()))
        })?;
        let c_path = CString::new(path_str)
            .map_err(|e| SqliteError::Io(format!("invalid path CString: {e}")))?;
        let mut db: *mut Sqlite3Db = ptr::null_mut();
        let rc = unsafe { sqlite3_open_v2(c_path.as_ptr(), &mut db, flags, ptr::null()) };
        if rc != SQLITE_OK || db.is_null() {
            let msg = if !db.is_null() {
                let err = unsafe { Self::errmsg_raw(db) };
                unsafe {
                    let _ = sqlite3_close_v2(db);
                }
                err
            } else {
                format!("sqlite3_open_v2 failed with code {rc}")
            };
            if rc == SQLITE_BUSY || rc == SQLITE_LOCKED {
                return Err(SqliteError::Busy(msg));
            }
            return Err(SqliteError::Io(msg));
        }
        if busy_timeout_ms >= 0 {
            unsafe {
                let _ = sqlite3_busy_timeout(db, busy_timeout_ms);
            }
        }
        Ok(Self { raw: db })
    }

    unsafe fn errmsg_raw(db: *mut Sqlite3Db) -> String {
        if db.is_null() {
            return "null sqlite db pointer".into();
        }
        let ptr = unsafe { sqlite3_errmsg(db) };
        if ptr.is_null() {
            "unknown sqlite error".into()
        } else {
            unsafe { CStr::from_ptr(ptr).to_string_lossy().into_owned() }
        }
    }

    pub(crate) fn last_error(&self, rc: c_int) -> SqliteError {
        let msg = unsafe { Self::errmsg_raw(self.raw) };
        if rc == SQLITE_BUSY || rc == SQLITE_LOCKED {
            SqliteError::Busy(msg)
        } else {
            SqliteError::Query(format!("(code {rc}) {msg}"))
        }
    }

    pub(crate) fn exec_batch(&self, sql: &str) -> Result<(), SqliteError> {
        let c_sql = CString::new(sql)
            .map_err(|e| SqliteError::Query(format!("SQL contains interior NUL: {e}")))?;
        let mut errmsg: *mut c_char = ptr::null_mut();
        let rc = unsafe {
            sqlite3_exec(
                self.raw,
                c_sql.as_ptr(),
                None,
                ptr::null_mut(),
                &mut errmsg,
            )
        };
        if rc != SQLITE_OK {
            let text = if !errmsg.is_null() {
                let s = unsafe { CStr::from_ptr(errmsg).to_string_lossy().into_owned() };
                unsafe { sqlite3_free(errmsg as *mut c_void) };
                s
            } else {
                unsafe { Self::errmsg_raw(self.raw) }
            };
            if rc == SQLITE_BUSY || rc == SQLITE_LOCKED {
                return Err(SqliteError::Busy(text));
            }
            return Err(SqliteError::Query(text));
        }
        Ok(())
    }

    pub(crate) fn prepare<'a>(&'a self, sql: &str) -> Result<SqliteStmt<'a>, SqliteError> {
        let c_sql = CString::new(sql)
            .map_err(|e| SqliteError::Query(format!("SQL contains interior NUL: {e}")))?;
        let mut stmt: *mut Sqlite3Stmt = ptr::null_mut();
        let rc = unsafe {
            sqlite3_prepare_v2(
                self.raw,
                c_sql.as_ptr(),
                c_sql.as_bytes().len() as c_int,
                &mut stmt,
                ptr::null_mut(),
            )
        };
        if rc != SQLITE_OK || stmt.is_null() {
            return Err(self.last_error(rc));
        }
        Ok(SqliteStmt {
            conn: self,
            raw: stmt,
        })
    }
}

pub(crate) struct SqliteStmt<'a> {
    conn: &'a SqliteConnection,
    raw: *mut Sqlite3Stmt,
}

impl Drop for SqliteStmt<'_> {
    fn drop(&mut self) {
        if !self.raw.is_null() {
            unsafe {
                let _ = sqlite3_finalize(self.raw);
            }
            self.raw = ptr::null_mut();
        }
    }
}

impl SqliteStmt<'_> {
    pub(crate) fn bind_text(&mut self, index: c_int, value: &str) -> Result<(), SqliteError> {
        let rc = unsafe {
            sqlite3_bind_text(
                self.raw,
                index,
                value.as_ptr() as *const c_char,
                value.len() as c_int,
                SQLITE_TRANSIENT,
            )
        };
        if rc != SQLITE_OK {
            return Err(self.conn.last_error(rc));
        }
        Ok(())
    }

    pub(crate) fn bind_opt_text(
        &mut self,
        index: c_int,
        value: Option<&str>,
    ) -> Result<(), SqliteError> {
        match value {
            Some(v) => self.bind_text(index, v),
            None => self.bind_null(index),
        }
    }

    pub(crate) fn bind_i64(&mut self, index: c_int, value: i64) -> Result<(), SqliteError> {
        let rc = unsafe { sqlite3_bind_int64(self.raw, index, value) };
        if rc != SQLITE_OK {
            return Err(self.conn.last_error(rc));
        }
        Ok(())
    }

    pub(crate) fn bind_null(&mut self, index: c_int) -> Result<(), SqliteError> {
        let rc = unsafe { sqlite3_bind_null(self.raw, index) };
        if rc != SQLITE_OK {
            return Err(self.conn.last_error(rc));
        }
        Ok(())
    }

    pub(crate) fn step(&mut self) -> Result<bool, SqliteError> {
        let rc = unsafe { sqlite3_step(self.raw) };
        match rc {
            SQLITE_ROW => Ok(true),
            SQLITE_DONE => Ok(false),
            other => Err(self.conn.last_error(other)),
        }
    }

    pub(crate) fn column_is_null(&self, col: c_int) -> bool {
        unsafe { sqlite3_column_type(self.raw, col) == SQLITE_NULL }
    }

    pub(crate) fn column_text(&self, col: c_int) -> String {
        if self.column_is_null(col) {
            return String::new();
        }
        let ptr = unsafe { sqlite3_column_text(self.raw, col) };
        let len = unsafe { sqlite3_column_bytes(self.raw, col) };
        if ptr.is_null() || len <= 0 {
            return String::new();
        }
        let slice = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
        String::from_utf8_lossy(slice).into_owned()
    }

    pub(crate) fn column_opt_text(&self, col: c_int) -> Option<String> {
        if self.column_is_null(col) {
            None
        } else {
            Some(self.column_text(col))
        }
    }

    pub(crate) fn column_i64(&self, col: c_int) -> i64 {
        unsafe { sqlite3_column_int64(self.raw, col) }
    }
}

pub(crate) struct AdvisoryFileLock {
    file: File,
}

impl AdvisoryFileLock {
    pub(crate) fn acquire_exclusive(lock_path: &Path) -> Result<Self, HostAdapterError> {
        if let Some(parent) = lock_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(lock_path)?;
        let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
        if rc != 0 {
            return Err(HostAdapterError::Io(std::io::Error::last_os_error()));
        }
        Ok(Self { file })
    }
}

impl Drop for AdvisoryFileLock {
    fn drop(&mut self) {
        unsafe {
            let _ = libc::flock(self.file.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct LegacyPersistedMailboxState {
    #[serde(default)]
    messages: Vec<InboxMessage>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct LegacyPersistedHostState {
    #[serde(default)]
    projects: BTreeMap<String, SidebarProjectBinding>,
    #[serde(default)]
    slug_to_project_id: BTreeMap<String, String>,
    #[serde(default)]
    sessions: BTreeMap<String, PersistedSessionRecord>,
    #[serde(default)]
    claimed_deliveries: BTreeSet<String>,
}

const LEDGER_SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS ledger_meta (
    key TEXT PRIMARY KEY,
    int_value INTEGER NOT NULL DEFAULT 0,
    text_value TEXT
);

INSERT OR IGNORE INTO ledger_meta (key, int_value, text_value)
VALUES ('next_updated_seq', 0, NULL);

CREATE TABLE IF NOT EXISTS sessions (
    session_id TEXT PRIMARY KEY,
    connector_id TEXT NOT NULL,
    runtime TEXT NOT NULL,
    external_session_id TEXT NOT NULL,
    project_slug TEXT NOT NULL,
    title TEXT NOT NULL,
    latest_updated_seq INTEGER NOT NULL DEFAULT 0,
    delivery_tier TEXT,
    sidebar_binding_json TEXT,
    interaction_status TEXT NOT NULL DEFAULT 'idle',
    capabilities_json TEXT
);

CREATE TABLE IF NOT EXISTS timeline_items (
    item_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    turn_id TEXT,
    kind TEXT NOT NULL,
    status TEXT NOT NULL,
    role TEXT NOT NULL,
    content TEXT NOT NULL,
    order_seq INTEGER NOT NULL,
    updated_seq INTEGER NOT NULL,
    content_hash TEXT NOT NULL,
    delivery_tier TEXT,
    sidebar_binding_json TEXT,
    peer_sender TEXT,
    created_at_ms INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (session_id, item_id)
);

CREATE INDEX IF NOT EXISTS idx_timeline_session_updated
ON timeline_items (session_id, updated_seq);

CREATE INDEX IF NOT EXISTS idx_timeline_session_order
ON timeline_items (session_id, order_seq, item_id);

CREATE TABLE IF NOT EXISTS inbox_messages (
    id TEXT PRIMARY KEY,
    sender TEXT NOT NULL,
    recipient TEXT NOT NULL,
    reply_to TEXT,
    project_slug TEXT,
    session_id TEXT,
    title TEXT,
    body TEXT NOT NULL,
    files_json TEXT NOT NULL DEFAULT '[]',
    wake INTEGER NOT NULL DEFAULT 1,
    notify INTEGER NOT NULL DEFAULT 0,
    created_at_unix INTEGER NOT NULL,
    delivered INTEGER NOT NULL DEFAULT 0,
    seq INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_inbox_delivered_seq
ON inbox_messages (delivered, seq);

CREATE INDEX IF NOT EXISTS idx_inbox_session_seq
ON inbox_messages (session_id, seq);

CREATE TABLE IF NOT EXISTS notices (
    notice_id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    level TEXT NOT NULL,
    code TEXT NOT NULL,
    message TEXT NOT NULL,
    updated_seq INTEGER NOT NULL,
    created_at_ms INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_notices_session_seq
ON notices (session_id, updated_seq);

CREATE TABLE IF NOT EXISTS sync_fingerprints (
    session_id TEXT NOT NULL,
    item_id TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'completed',
    delivery_tier TEXT,
    updated_seq INTEGER NOT NULL,
    PRIMARY KEY (session_id, item_id)
);

CREATE TABLE IF NOT EXISTS correlation_claims (
    scope_key TEXT NOT NULL,
    delivery_id TEXT NOT NULL,
    payload_hash TEXT NOT NULL,
    session_id TEXT,
    turn_id TEXT,
    claimed_at_ms INTEGER NOT NULL,
    PRIMARY KEY (scope_key, delivery_id)
);
"#;

/// Unified SQLite WAL ledger (`<state_root>/fleet-ledger.sqlite`) replacing flat JSON mailbox
/// rewrites and coordinating monotonic `updated_seq`, `content_hash` fingerprints, and correlation claims.
#[derive(Clone)]
pub struct FleetLedgerStore {
    state_root: PathBuf,
    db_path: PathBuf,
    conn: Arc<Mutex<SqliteConnection>>,
}

impl std::fmt::Debug for FleetLedgerStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FleetLedgerStore")
            .field("state_root", &self.state_root)
            .field("db_path", &self.db_path)
            .finish()
    }
}

impl FleetLedgerStore {
    pub fn open(state_root: impl AsRef<Path>) -> Result<Self, HostAdapterError> {
        let state_root = state_root.as_ref().to_path_buf();
        if !state_root.is_absolute() {
            return Err(ProjectPathError::InvalidRoot(state_root).into());
        }
        fs::create_dir_all(&state_root)?;
        let db_path = state_root.join("fleet-ledger.sqlite");
        let lock_path = state_root.join("fleet-ledger.lock");
        let _guard = AdvisoryFileLock::acquire_exclusive(&lock_path)?;

        let conn = SqliteConnection::open_with_flags(
            &db_path,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE | SQLITE_OPEN_NOMUTEX,
            10_000,
        )?;
        Self::enforce_mode_0600(&db_path)?;

        conn.exec_batch(
            "PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL; PRAGMA foreign_keys = ON;",
        )?;
        conn.exec_batch(LEDGER_SCHEMA_SQL)?;
        Self::enforce_mode_0600(&db_path)?;

        let store = Self {
            state_root,
            db_path,
            conn: Arc::new(Mutex::new(conn)),
        };
        store.import_legacy_state_with_conn_locked()?;
        Ok(store)
    }

    #[cfg(unix)]
    fn enforce_mode_0600(path: &Path) -> Result<(), HostAdapterError> {
        use std::os::unix::fs::PermissionsExt;
        if path.exists() {
            fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    }

    #[cfg(not(unix))]
    fn enforce_mode_0600(_path: &Path) -> Result<(), HostAdapterError> {
        Ok(())
    }

    pub fn state_root(&self) -> &Path {
        &self.state_root
    }

    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    pub fn journal_mode(&self) -> Result<String, HostAdapterError> {
        let conn = self.conn.lock().expect("ledger mutex poisoned");
        let mut stmt = conn.prepare("PRAGMA journal_mode;")?;
        if stmt.step()? {
            Ok(stmt.column_text(0).to_ascii_lowercase())
        } else {
            Ok(String::new())
        }
    }

    pub fn integrity_check(&self) -> Result<String, HostAdapterError> {
        let conn = self.conn.lock().expect("ledger mutex poisoned");
        let mut stmt = conn.prepare("PRAGMA integrity_check;")?;
        if stmt.step()? {
            Ok(stmt.column_text(0).to_ascii_lowercase())
        } else {
            Ok(String::new())
        }
    }

    fn import_legacy_state_with_conn_locked(&self) -> Result<(), HostAdapterError> {
        let conn = self.conn.lock().expect("ledger mutex poisoned");
        Self::import_legacy_mailboxes_inner(&conn, &self.state_root)?;
        Self::import_legacy_host_states_inner(&conn, &self.state_root)?;
        Ok(())
    }

    fn meta_text(conn: &SqliteConnection, key: &str) -> Result<Option<String>, SqliteError> {
        let mut stmt = conn.prepare("SELECT text_value FROM ledger_meta WHERE key = ?1;")?;
        stmt.bind_text(1, key)?;
        if stmt.step()? {
            Ok(stmt.column_opt_text(0))
        } else {
            Ok(None)
        }
    }

    fn set_meta_text(conn: &SqliteConnection, key: &str, val: &str) -> Result<(), SqliteError> {
        let mut stmt = conn.prepare(
            "INSERT INTO ledger_meta (key, int_value, text_value) VALUES (?1, 0, ?2)
             ON CONFLICT(key) DO UPDATE SET text_value = excluded.text_value;",
        )?;
        stmt.bind_text(1, key)?;
        stmt.bind_text(2, val)?;
        let _ = stmt.step()?;
        Ok(())
    }

    fn allocate_updated_seq_in_tx(conn: &SqliteConnection) -> Result<UpdatedSeq, SqliteError> {
        let mut update = conn.prepare(
            "UPDATE ledger_meta SET int_value = int_value + 1 WHERE key = 'next_updated_seq';",
        )?;
        let _ = update.step()?;
        let mut select =
            conn.prepare("SELECT int_value FROM ledger_meta WHERE key = 'next_updated_seq';")?;
        if select.step()? {
            Ok(UpdatedSeq::new(select.column_i64(0)))
        } else {
            Ok(UpdatedSeq::new(1))
        }
    }

    fn import_legacy_mailboxes_inner(
        conn: &SqliteConnection,
        state_root: &Path,
    ) -> Result<(), HostAdapterError> {
        let mailbox_path = state_root.join("mailboxes.json");
        if !mailbox_path.exists() {
            return Ok(());
        }
        let bytes = fs::read(&mailbox_path)?;
        let digest = sha256_hex(&bytes);
        if Self::meta_text(conn, "mailboxes_json_digest")?.as_deref() == Some(digest.as_str()) {
            return Ok(());
        }
        let parsed: LegacyPersistedMailboxState =
            serde_json::from_slice(&bytes).map_err(|e| {
                HostAdapterError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("corrupt {}: {e}", mailbox_path.display()),
                ))
            })?;
        conn.exec_batch("BEGIN IMMEDIATE;")?;
        let result = (|| -> Result<(), HostAdapterError> {
            for msg in parsed.messages {
                let mut check =
                    conn.prepare("SELECT 1 FROM inbox_messages WHERE id = ?1 LIMIT 1;")?;
                check.bind_text(1, &msg.id)?;
                if check.step()? {
                    continue;
                }
                drop(check);
                let seq = Self::allocate_updated_seq_in_tx(conn)?;
                let files_json = serde_json::to_string(&msg.files).unwrap_or_else(|_| "[]".into());
                let mut ins = conn.prepare(
                    "INSERT INTO inbox_messages
                     (id, sender, recipient, reply_to, project_slug, session_id, title, body, files_json, wake, notify, created_at_unix, delivered, seq)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, ?7, ?8, ?9, ?10, ?11, ?12, ?13);",
                )?;
                ins.bind_text(1, &msg.id)?;
                ins.bind_text(2, &msg.from)?;
                ins.bind_text(3, &msg.to)?;
                ins.bind_opt_text(4, msg.reply_to.as_deref())?;
                ins.bind_opt_text(5, msg.project_slug.as_deref())?;
                ins.bind_opt_text(6, msg.session_id.as_deref())?;
                ins.bind_text(7, &msg.body)?;
                ins.bind_text(8, &files_json)?;
                ins.bind_i64(9, i64::from(msg.wake))?;
                ins.bind_i64(10, i64::from(msg.notify))?;
                ins.bind_i64(11, msg.created_at_unix as i64)?;
                ins.bind_i64(12, i64::from(msg.delivered))?;
                ins.bind_i64(13, seq.get())?;
                let _ = ins.step()?;

                let payload_hash =
                    sha256_hex(format!("{}\0{}\0{}", msg.from, msg.to, msg.body).as_bytes());
                let mut claim = conn.prepare(
                    "INSERT OR IGNORE INTO correlation_claims
                     (scope_key, delivery_id, payload_hash, session_id, turn_id, claimed_at_ms)
                     VALUES ('mailbox', ?1, ?2, ?3, NULL, ?4);",
                )?;
                claim.bind_text(1, &msg.id)?;
                claim.bind_text(2, &payload_hash)?;
                claim.bind_opt_text(3, msg.session_id.as_deref())?;
                claim.bind_i64(4, (msg.created_at_unix as i64).saturating_mul(1000))?;
                let _ = claim.step()?;
            }
            Self::set_meta_text(conn, "mailboxes_json_digest", &digest)?;
            Ok(())
        })();
        match result {
            Ok(()) => {
                conn.exec_batch("COMMIT;")?;
                Ok(())
            }
            Err(err) => {
                let _ = conn.exec_batch("ROLLBACK;");
                Err(err)
            }
        }
    }

    fn import_legacy_host_states_inner(
        conn: &SqliteConnection,
        state_root: &Path,
    ) -> Result<(), HostAdapterError> {
        for host in DesktopHostKind::ALL {
            let state_path = state_root.join(format!("{}-state.json", host.as_str()));
            if !state_path.exists() {
                continue;
            }
            let bytes = fs::read(&state_path)?;
            let meta_key = format!("host_state_digest:{}", host.as_str());
            let digest = sha256_hex(&bytes);
            if Self::meta_text(conn, &meta_key)?.as_deref() == Some(digest.as_str()) {
                continue;
            }
            let Ok(parsed) = serde_json::from_slice::<LegacyPersistedHostState>(&bytes) else {
                continue;
            };
            conn.exec_batch("BEGIN IMMEDIATE;")?;
            let res = (|| -> Result<(), HostAdapterError> {
                for delivery_id in &parsed.claimed_deliveries {
                    let mut claim = conn.prepare(
                        "INSERT OR IGNORE INTO correlation_claims
                         (scope_key, delivery_id, payload_hash, session_id, turn_id, claimed_at_ms)
                         VALUES (?1, ?2, ?3, NULL, NULL, 0);",
                    )?;
                    claim.bind_text(1, host.as_str())?;
                    claim.bind_text(2, delivery_id)?;
                    claim.bind_text(3, "legacy-state-claim")?;
                    let _ = claim.step()?;
                }
                for session in parsed.sessions.values() {
                    let slug = session
                        .project_dir
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("project");
                    let Ok(canonical_sid) = CanonicalSessionId::derive(
                        "local-connector",
                        host.as_str(),
                        &session.session_id,
                    ) else {
                        continue;
                    };
                    let binding = parsed.projects.get(&session.project_id).cloned().or_else(|| {
                        Some(SidebarProjectBinding {
                            host: session.host,
                            project_id: session.project_id.clone(),
                            title: session.title.clone(),
                            project_dir: session.project_dir.clone(),
                            workspace_uris: session.workspace_uris.clone(),
                        })
                    });
                    let binding_json = binding
                        .as_ref()
                        .and_then(|b| serde_json::to_string(b).ok());
                    let seq = Self::allocate_updated_seq_in_tx(conn)?;
                    let mut up = conn.prepare(
                        "INSERT OR IGNORE INTO sessions
                         (session_id, connector_id, runtime, external_session_id, project_slug, title, latest_updated_seq, delivery_tier, sidebar_binding_json, interaction_status, capabilities_json)
                         VALUES (?1, 'local-connector', ?2, ?3, ?4, ?5, ?6, 'accepted', ?7, 'idle', NULL);",
                    )?;
                    up.bind_text(1, canonical_sid.as_str())?;
                    up.bind_text(2, host.as_str())?;
                    up.bind_text(3, &session.session_id)?;
                    up.bind_text(4, slug)?;
                    up.bind_text(5, &session.title)?;
                    up.bind_i64(6, seq.get())?;
                    up.bind_opt_text(7, binding_json.as_deref())?;
                    let _ = up.step()?;

                    for turn in &session.turns {
                        let prompt_hash = sha256_hex(turn.prompt.as_bytes());
                        let mut claim = conn.prepare(
                            "INSERT OR REPLACE INTO correlation_claims
                             (scope_key, delivery_id, payload_hash, session_id, turn_id, claimed_at_ms)
                             VALUES (?1, ?2, ?3, ?4, ?5, 0);",
                        )?;
                        claim.bind_text(1, host.as_str())?;
                        claim.bind_text(2, &turn.delivery_id)?;
                        claim.bind_text(3, &prompt_hash)?;
                        claim.bind_text(4, &session.session_id)?;
                        claim.bind_text(5, &turn.turn_id)?;
                        let _ = claim.step()?;
                    }
                }
                Self::set_meta_text(conn, &meta_key, &digest)?;
                Ok(())
            })();
            match res {
                Ok(()) => conn.exec_batch("COMMIT;")?,
                Err(e) => {
                    let _ = conn.exec_batch("ROLLBACK;");
                    return Err(e);
                }
            }
        }
        Ok(())
    }

    /// Claims a delivery ID within `scope_key`.
    /// When `allow_idempotent_same_payload` is `true`, an identical `(scope_key, delivery_id, payload_hash)`
    /// returns `Ok(false)` (already claimed with identical payload); any conflicting `payload_hash` or
    /// repeat when `allow_idempotent_same_payload` is `false` returns `Err(HostAdapterError::DuplicateDelivery)`.
    pub fn claim_delivery(
        &self,
        scope_key: &str,
        delivery_id: &str,
        payload_hash: &str,
        session_id: Option<&str>,
        turn_id: Option<&str>,
        allow_idempotent_same_payload: bool,
    ) -> Result<bool, HostAdapterError> {
        validate_nonempty_id("scope_key", scope_key)?;
        validate_nonempty_id("delivery_id", delivery_id)?;
        let conn = self.conn.lock().expect("ledger mutex poisoned");
        conn.exec_batch("BEGIN IMMEDIATE;")?;
        let res = (|| -> Result<bool, HostAdapterError> {
            let mut sel = conn.prepare(
                "SELECT payload_hash FROM correlation_claims
                 WHERE scope_key = ?1 AND delivery_id = ?2 LIMIT 1;",
            )?;
            sel.bind_text(1, scope_key)?;
            sel.bind_text(2, delivery_id)?;
            if sel.step()? {
                let existing_hash = sel.column_text(0);
                if allow_idempotent_same_payload && existing_hash == payload_hash {
                    return Ok(false);
                }
                return Err(HostAdapterError::DuplicateDelivery(delivery_id.to_string()));
            }
            drop(sel);
            let mut ins = conn.prepare(
                "INSERT INTO correlation_claims
                 (scope_key, delivery_id, payload_hash, session_id, turn_id, claimed_at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6);",
            )?;
            ins.bind_text(1, scope_key)?;
            ins.bind_text(2, delivery_id)?;
            ins.bind_text(3, payload_hash)?;
            ins.bind_opt_text(4, session_id)?;
            ins.bind_opt_text(5, turn_id)?;
            ins.bind_i64(6, now_ms())?;
            let _ = ins.step()?;
            Ok(true)
        })();
        match res {
            Ok(claimed) => {
                conn.exec_batch("COMMIT;")?;
                Ok(claimed)
            }
            Err(err) => {
                let _ = conn.exec_batch("ROLLBACK;");
                Err(err)
            }
        }
    }

    pub fn count_correlation_claims(&self) -> Result<usize, HostAdapterError> {
        let conn = self.conn.lock().expect("ledger mutex poisoned");
        let mut stmt =
            conn.prepare("SELECT COUNT(DISTINCT delivery_id) FROM correlation_claims;")?;
        if stmt.step()? {
            Ok(stmt.column_i64(0).max(0) as usize)
        } else {
            Ok(0)
        }
    }

    pub fn enqueue(&self, mut message: InboxMessage) -> Result<InboxMessage, HostAdapterError> {
        validate_nonempty_id("message.id", &message.id)?;
        let from_addr = PeerAddress::parse(&message.from)?;
        let to_addr = PeerAddress::parse(&message.to)?;
        message.from = from_addr.to_string();
        message.to = to_addr.to_string();
        if message.project_slug.is_none() {
            message.project_slug = to_addr.slug.clone().or_else(|| from_addr.slug.clone());
        }
        if message.session_id.is_none() {
            message.session_id = to_addr.session_id.clone();
        }
        if let Some(ref sid) = message.session_id {
            validate_session_id(sid)?;
        }
        validate_brief(&message.body)?;

        let payload_hash = sha256_hex(
            format!("{}\0{}\0{}", message.from, message.to, message.body).as_bytes(),
        );
        let conn = self.conn.lock().expect("ledger mutex poisoned");
        Self::import_legacy_mailboxes_inner(&conn, &self.state_root)?;
        conn.exec_batch("BEGIN IMMEDIATE;")?;
        let res = (|| -> Result<InboxMessage, HostAdapterError> {
            if let Some(existing) = Self::get_message_in_tx(&conn, &message.id)? {
                if existing.from == message.from
                    && existing.to == message.to
                    && existing.body == message.body
                {
                    return Ok(existing);
                }
                return Err(HostAdapterError::DuplicateDelivery(message.id.clone()));
            }
            let seq = Self::allocate_updated_seq_in_tx(&conn)?;
            let files_json =
                serde_json::to_string(&message.files).unwrap_or_else(|_| "[]".into());
            let mut ins = conn.prepare(
                "INSERT INTO inbox_messages
                 (id, sender, recipient, reply_to, project_slug, session_id, title, body, files_json, wake, notify, created_at_unix, delivered, seq)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, ?7, ?8, ?9, ?10, ?11, ?12, ?13);",
            )?;
            ins.bind_text(1, &message.id)?;
            ins.bind_text(2, &message.from)?;
            ins.bind_text(3, &message.to)?;
            ins.bind_opt_text(4, message.reply_to.as_deref())?;
            ins.bind_opt_text(5, message.project_slug.as_deref())?;
            ins.bind_opt_text(6, message.session_id.as_deref())?;
            ins.bind_text(7, &message.body)?;
            ins.bind_text(8, &files_json)?;
            ins.bind_i64(9, i64::from(message.wake))?;
            ins.bind_i64(10, i64::from(message.notify))?;
            ins.bind_i64(11, message.created_at_unix as i64)?;
            ins.bind_i64(12, i64::from(message.delivered))?;
            ins.bind_i64(13, seq.get())?;
            let _ = ins.step()?;

            let mut claim = conn.prepare(
                "INSERT OR REPLACE INTO correlation_claims
                 (scope_key, delivery_id, payload_hash, session_id, turn_id, claimed_at_ms)
                 VALUES ('mailbox', ?1, ?2, ?3, NULL, ?4);",
            )?;
            claim.bind_text(1, &message.id)?;
            claim.bind_text(2, &payload_hash)?;
            claim.bind_opt_text(3, message.session_id.as_deref())?;
            claim.bind_i64(4, now_ms())?;
            let _ = claim.step()?;
            Ok(message)
        })();
        match res {
            Ok(saved) => {
                conn.exec_batch("COMMIT;")?;
                Ok(saved)
            }
            Err(err) => {
                let _ = conn.exec_batch("ROLLBACK;");
                Err(err)
            }
        }
    }

    fn row_to_inbox_message(stmt: &SqliteStmt<'_>) -> InboxMessage {
        let files_json = stmt.column_text(7);
        let files: Vec<fleet_proto::MessageAttachment> =
            serde_json::from_str(&files_json).unwrap_or_default();
        InboxMessage {
            id: stmt.column_text(0),
            from: stmt.column_text(1),
            to: stmt.column_text(2),
            reply_to: stmt.column_opt_text(3),
            project_slug: stmt.column_opt_text(4),
            session_id: stmt.column_opt_text(5),
            body: stmt.column_text(6),
            files,
            wake: stmt.column_i64(8) != 0,
            notify: stmt.column_i64(9) != 0,
            created_at_unix: stmt.column_i64(10).max(0) as u64,
            delivered: stmt.column_i64(11) != 0,
        }
    }

    fn get_message_in_tx(
        conn: &SqliteConnection,
        id: &str,
    ) -> Result<Option<InboxMessage>, SqliteError> {
        let mut stmt = conn.prepare(
            "SELECT id, sender, recipient, reply_to, project_slug, session_id, body, files_json, wake, notify, created_at_unix, delivered
             FROM inbox_messages WHERE id = ?1 LIMIT 1;",
        )?;
        stmt.bind_text(1, id)?;
        if stmt.step()? {
            Ok(Some(Self::row_to_inbox_message(&stmt)))
        } else {
            Ok(None)
        }
    }

    pub fn get_message(&self, id: &str) -> Result<Option<InboxMessage>, HostAdapterError> {
        let conn = self.conn.lock().expect("ledger mutex poisoned");
        Self::import_legacy_mailboxes_inner(&conn, &self.state_root)?;
        Ok(Self::get_message_in_tx(&conn, id)?)
    }

    pub fn check_inbox(
        &self,
        recipient: &str,
        mark_delivered: bool,
    ) -> Result<Vec<InboxMessage>, HostAdapterError> {
        let query = PeerAddress::parse(recipient)?;
        let conn = self.conn.lock().expect("ledger mutex poisoned");
        Self::import_legacy_mailboxes_inner(&conn, &self.state_root)?;
        if mark_delivered {
            conn.exec_batch("BEGIN IMMEDIATE;")?;
        }
        let res = (|| -> Result<Vec<InboxMessage>, HostAdapterError> {
            let mut stmt = conn.prepare(
                "SELECT id, sender, recipient, reply_to, project_slug, session_id, body, files_json, wake, notify, created_at_unix, delivered
                 FROM inbox_messages WHERE delivered = 0 ORDER BY seq ASC;",
            )?;
            let mut matched = Vec::new();
            while stmt.step()? {
                let mut msg = Self::row_to_inbox_message(&stmt);
                if msg.matches_recipient(&query) {
                    if mark_delivered {
                        msg.delivered = true;
                    }
                    matched.push(msg);
                }
            }
            drop(stmt);
            if mark_delivered && !matched.is_empty() {
                for msg in &matched {
                    let mut u =
                        conn.prepare("UPDATE inbox_messages SET delivered = 1 WHERE id = ?1;")?;
                    u.bind_text(1, &msg.id)?;
                    let _ = u.step()?;
                }
            }
            Ok(matched)
        })();
        if mark_delivered {
            match res {
                Ok(v) => {
                    conn.exec_batch("COMMIT;")?;
                    Ok(v)
                }
                Err(e) => {
                    let _ = conn.exec_batch("ROLLBACK;");
                    Err(e)
                }
            }
        } else {
            res
        }
    }

    pub fn check_outbox(&self, sender: &str) -> Result<Vec<InboxMessage>, HostAdapterError> {
        let query = PeerAddress::parse(sender)?;
        let conn = self.conn.lock().expect("ledger mutex poisoned");
        Self::import_legacy_mailboxes_inner(&conn, &self.state_root)?;
        let mut stmt = conn.prepare(
            "SELECT id, sender, recipient, reply_to, project_slug, session_id, body, files_json, wake, notify, created_at_unix, delivered
             FROM inbox_messages ORDER BY seq ASC;",
        )?;
        let mut out = Vec::new();
        while stmt.step()? {
            let msg = Self::row_to_inbox_message(&stmt);
            if msg.matches_sender(&query) {
                out.push(msg);
            }
        }
        Ok(out)
    }

    pub fn list_by_session(&self, session_id: &str) -> Result<Vec<InboxMessage>, HostAdapterError> {
        let sid = validate_session_id(session_id.trim())?;
        let conn = self.conn.lock().expect("ledger mutex poisoned");
        Self::import_legacy_mailboxes_inner(&conn, &self.state_root)?;
        let mut stmt = conn.prepare(
            "SELECT id, sender, recipient, reply_to, project_slug, session_id, body, files_json, wake, notify, created_at_unix, delivered
             FROM inbox_messages ORDER BY seq ASC;",
        )?;
        let mut out = Vec::new();
        while stmt.step()? {
            let msg = Self::row_to_inbox_message(&stmt);
            let matches = msg.session_id.as_deref() == Some(sid)
                || msg
                    .sender_address()
                    .ok()
                    .and_then(|a| a.session_id)
                    .as_deref()
                    == Some(sid)
                || msg
                    .recipient_address()
                    .ok()
                    .and_then(|a| a.session_id)
                    .as_deref()
                    == Some(sid);
            if matches {
                out.push(msg);
            }
        }
        Ok(out)
    }

    pub fn list_all_messages(&self) -> Result<Vec<InboxMessage>, HostAdapterError> {
        let conn = self.conn.lock().expect("ledger mutex poisoned");
        Self::import_legacy_mailboxes_inner(&conn, &self.state_root)?;
        let mut stmt = conn.prepare(
            "SELECT id, sender, recipient, reply_to, project_slug, session_id, body, files_json, wake, notify, created_at_unix, delivered
             FROM inbox_messages ORDER BY seq ASC;",
        )?;
        let mut out = Vec::new();
        while stmt.step()? {
            out.push(Self::row_to_inbox_message(&stmt));
        }
        Ok(out)
    }

    pub fn upsert_session(&self, meta: &SessionMeta) -> Result<UpdatedSeq, HostAdapterError> {
        meta.validate()?;
        let conn = self.conn.lock().expect("ledger mutex poisoned");
        conn.exec_batch("BEGIN IMMEDIATE;")?;
        let res = (|| -> Result<UpdatedSeq, HostAdapterError> {
            let seq = if meta.latest_updated_seq.get() > 0 {
                meta.latest_updated_seq
            } else {
                Self::allocate_updated_seq_in_tx(&conn)?
            };
            let tier_str = meta.delivery_tier.map(|t| t.as_str());
            let binding_json = meta
                .sidebar_binding
                .as_ref()
                .and_then(|b| serde_json::to_string(b).ok());
            let mut stmt = conn.prepare(
                "INSERT INTO sessions
                 (session_id, connector_id, runtime, external_session_id, project_slug, title, latest_updated_seq, delivery_tier, sidebar_binding_json, interaction_status, capabilities_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'idle', NULL)
                 ON CONFLICT(session_id) DO UPDATE SET
                    connector_id = excluded.connector_id,
                    runtime = excluded.runtime,
                    external_session_id = excluded.external_session_id,
                    project_slug = excluded.project_slug,
                    title = excluded.title,
                    latest_updated_seq = MAX(sessions.latest_updated_seq, excluded.latest_updated_seq),
                    delivery_tier = COALESCE(excluded.delivery_tier, sessions.delivery_tier),
                    sidebar_binding_json = COALESCE(excluded.sidebar_binding_json, sessions.sidebar_binding_json);",
            )?;
            stmt.bind_text(1, meta.session_id.as_str())?;
            stmt.bind_text(2, &meta.connector_id)?;
            stmt.bind_text(3, &meta.runtime)?;
            stmt.bind_text(4, &meta.external_session_id)?;
            stmt.bind_text(5, &meta.project_slug)?;
            stmt.bind_text(6, &meta.title)?;
            stmt.bind_i64(7, seq.get())?;
            stmt.bind_opt_text(8, tier_str)?;
            stmt.bind_opt_text(9, binding_json.as_deref())?;
            let _ = stmt.step()?;
            Ok(seq)
        })();
        match res {
            Ok(seq) => {
                conn.exec_batch("COMMIT;")?;
                Ok(seq)
            }
            Err(err) => {
                let _ = conn.exec_batch("ROLLBACK;");
                Err(err)
            }
        }
    }

    pub fn get_session(&self, session_id: &str) -> Result<Option<SessionMeta>, HostAdapterError> {
        let conn = self.conn.lock().expect("ledger mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT session_id, connector_id, runtime, external_session_id, project_slug, title, latest_updated_seq, delivery_tier, sidebar_binding_json
             FROM sessions WHERE session_id = ?1 LIMIT 1;",
        )?;
        stmt.bind_text(1, session_id)?;
        if !stmt.step()? {
            return Ok(None);
        }
        Ok(Some(Self::row_to_session_meta(&stmt)?))
    }

    pub fn list_sessions(&self) -> Result<Vec<SessionMeta>, HostAdapterError> {
        let conn = self.conn.lock().expect("ledger mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT session_id, connector_id, runtime, external_session_id, project_slug, title, latest_updated_seq, delivery_tier, sidebar_binding_json
             FROM sessions ORDER BY latest_updated_seq DESC, session_id ASC;",
        )?;
        let mut out = Vec::new();
        while stmt.step()? {
            out.push(Self::row_to_session_meta(&stmt)?);
        }
        Ok(out)
    }

    fn row_to_session_meta(stmt: &SqliteStmt<'_>) -> Result<SessionMeta, HostAdapterError> {
        let raw_sid = stmt.column_text(0);
        let session_id = CanonicalSessionId::parse(&raw_sid).map_err(|e| {
            HostAdapterError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                e.to_string(),
            ))
        })?;
        let delivery_tier = stmt
            .column_opt_text(7)
            .and_then(|s| DeliveryTier::parse(&s));
        let sidebar_binding = stmt
            .column_opt_text(8)
            .and_then(|s| serde_json::from_str::<SidebarProjectBinding>(&s).ok());
        Ok(SessionMeta {
            session_id,
            connector_id: stmt.column_text(1),
            runtime: stmt.column_text(2),
            external_session_id: stmt.column_text(3),
            project_slug: stmt.column_text(4),
            title: stmt.column_text(5),
            latest_updated_seq: UpdatedSeq::new(stmt.column_i64(6)),
            delivery_tier,
            sidebar_binding,
        })
    }

    /// Upserts a single `TimelineItem`, comparing its computed `content_hash` against `sync_fingerprints`.
    /// Returns `Ok(None)` when the item's `content_hash`, `status`, and `delivery_tier` are unchanged,
    /// or `Ok(Some(sequenced_item))` with a newly allocated monotonic `updated_seq` when inserted or modified.
    pub fn upsert_timeline_item(
        &self,
        item: &TimelineItem,
    ) -> Result<Option<TimelineItem>, HostAdapterError> {
        let mut batch = self.upsert_timeline_batch(std::slice::from_ref(item))?;
        Ok(batch.pop())
    }

    /// Upserts a batch of `TimelineItem`s in a single WAL transaction, returning only the inserted
    /// or modified items with their newly assigned strictly increasing `updated_seq`.
    pub fn upsert_timeline_batch(
        &self,
        items: &[TimelineItem],
    ) -> Result<Vec<TimelineItem>, HostAdapterError> {
        if items.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.conn.lock().expect("ledger mutex poisoned");
        conn.exec_batch("BEGIN IMMEDIATE;")?;
        let res = (|| -> Result<Vec<TimelineItem>, HostAdapterError> {
            let mut deltas = Vec::new();
            for item in items {
                validate_nonempty_id("item_id", &item.item_id)?;
                let computed_hash = timeline_content_hash(
                    item.kind.as_str(),
                    item.status.as_str(),
                    item.role.as_str(),
                    &item.content,
                );
                let tier_str = item.delivery_tier.map(|t| t.as_str());

                let mut fp_stmt = conn.prepare(
                    "SELECT content_hash, status, delivery_tier FROM sync_fingerprints
                     WHERE session_id = ?1 AND item_id = ?2 LIMIT 1;",
                )?;
                fp_stmt.bind_text(1, item.session_id.as_str())?;
                fp_stmt.bind_text(2, &item.item_id)?;
                if fp_stmt.step()? {
                    let prev_hash = fp_stmt.column_text(0);
                    let prev_status = fp_stmt.column_text(1);
                    let prev_tier = fp_stmt.column_opt_text(2);
                    if prev_hash == computed_hash.as_str()
                        && prev_status == item.status.as_str()
                        && prev_tier.as_deref() == tier_str
                    {
                        continue;
                    }
                }
                drop(fp_stmt);

                let seq = Self::allocate_updated_seq_in_tx(&conn)?;
                let binding_json = item
                    .sidebar_binding
                    .as_ref()
                    .and_then(|b| serde_json::to_string(b).ok());

                let mut up_item = conn.prepare(
                    "INSERT INTO timeline_items
                     (item_id, session_id, turn_id, kind, status, role, content, order_seq, updated_seq, content_hash, delivery_tier, sidebar_binding_json, peer_sender, created_at_ms)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
                     ON CONFLICT(session_id, item_id) DO UPDATE SET
                        turn_id = COALESCE(excluded.turn_id, timeline_items.turn_id),
                        kind = excluded.kind,
                        status = excluded.status,
                        role = excluded.role,
                        content = excluded.content,
                        order_seq = excluded.order_seq,
                        updated_seq = excluded.updated_seq,
                        content_hash = excluded.content_hash,
                        delivery_tier = COALESCE(excluded.delivery_tier, timeline_items.delivery_tier),
                        sidebar_binding_json = COALESCE(excluded.sidebar_binding_json, timeline_items.sidebar_binding_json),
                        peer_sender = COALESCE(excluded.peer_sender, timeline_items.peer_sender),
                        created_at_ms = CASE WHEN excluded.created_at_ms > 0 THEN excluded.created_at_ms ELSE timeline_items.created_at_ms END;",
                )?;
                up_item.bind_text(1, &item.item_id)?;
                up_item.bind_text(2, item.session_id.as_str())?;
                up_item.bind_opt_text(3, item.turn_id.as_deref())?;
                up_item.bind_text(4, item.kind.as_str())?;
                up_item.bind_text(5, item.status.as_str())?;
                up_item.bind_text(6, item.role.as_str())?;
                up_item.bind_text(7, &item.content)?;
                up_item.bind_i64(8, item.order_seq.as_i64())?;
                up_item.bind_i64(9, seq.get())?;
                up_item.bind_text(10, computed_hash.as_str())?;
                up_item.bind_opt_text(11, tier_str)?;
                up_item.bind_opt_text(12, binding_json.as_deref())?;
                up_item.bind_opt_text(13, item.peer_sender.as_deref())?;
                up_item.bind_i64(14, item.created_at_ms)?;
                let _ = up_item.step()?;

                let mut up_fp = conn.prepare(
                    "INSERT INTO sync_fingerprints
                     (session_id, item_id, content_hash, status, delivery_tier, updated_seq)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                     ON CONFLICT(session_id, item_id) DO UPDATE SET
                        content_hash = excluded.content_hash,
                        status = excluded.status,
                        delivery_tier = excluded.delivery_tier,
                        updated_seq = excluded.updated_seq;",
                )?;
                up_fp.bind_text(1, item.session_id.as_str())?;
                up_fp.bind_text(2, &item.item_id)?;
                up_fp.bind_text(3, computed_hash.as_str())?;
                up_fp.bind_text(4, item.status.as_str())?;
                up_fp.bind_opt_text(5, tier_str)?;
                up_fp.bind_i64(6, seq.get())?;
                let _ = up_fp.step()?;

                let mut up_sess = conn.prepare(
                    "UPDATE sessions SET
                        latest_updated_seq = MAX(latest_updated_seq, ?2),
                        delivery_tier = COALESCE(?3, delivery_tier),
                        sidebar_binding_json = COALESCE(?4, sidebar_binding_json)
                     WHERE session_id = ?1;",
                )?;
                up_sess.bind_text(1, item.session_id.as_str())?;
                up_sess.bind_i64(2, seq.get())?;
                up_sess.bind_opt_text(3, tier_str)?;
                up_sess.bind_opt_text(4, binding_json.as_deref())?;
                let _ = up_sess.step()?;

                let mut sequenced = item.clone();
                sequenced.updated_seq = seq;
                sequenced.content_hash = computed_hash;
                deltas.push(sequenced);
            }
            Ok(deltas)
        })();
        match res {
            Ok(deltas) => {
                conn.exec_batch("COMMIT;")?;
                Ok(deltas)
            }
            Err(err) => {
                let _ = conn.exec_batch("ROLLBACK;");
                Err(err)
            }
        }
    }

    pub fn list_timeline_items(
        &self,
        session_id: &str,
        after_seq: Option<UpdatedSeq>,
        limit: usize,
    ) -> Result<Vec<TimelineItem>, HostAdapterError> {
        let cap = limit.max(1).min(50_000) as i64;
        let conn = self.conn.lock().expect("ledger mutex poisoned");
        let mut out = Vec::new();
        if let Some(cursor) = after_seq {
            let mut stmt = conn.prepare(
                "SELECT item_id, session_id, turn_id, kind, status, role, content, order_seq, updated_seq, content_hash, delivery_tier, sidebar_binding_json, peer_sender, created_at_ms
                 FROM timeline_items
                 WHERE session_id = ?1 AND updated_seq > ?2
                 ORDER BY updated_seq ASC, order_seq ASC, item_id ASC
                 LIMIT ?3;",
            )?;
            stmt.bind_text(1, session_id)?;
            stmt.bind_i64(2, cursor.get())?;
            stmt.bind_i64(3, cap)?;
            while stmt.step()? {
                out.push(Self::row_to_timeline_item(&stmt)?);
            }
        } else {
            let mut stmt = conn.prepare(
                "SELECT item_id, session_id, turn_id, kind, status, role, content, order_seq, updated_seq, content_hash, delivery_tier, sidebar_binding_json, peer_sender, created_at_ms
                 FROM timeline_items
                 WHERE session_id = ?1
                 ORDER BY order_seq ASC, item_id ASC
                 LIMIT ?2;",
            )?;
            stmt.bind_text(1, session_id)?;
            stmt.bind_i64(2, cap)?;
            while stmt.step()? {
                out.push(Self::row_to_timeline_item(&stmt)?);
            }
        }
        Ok(out)
    }

    fn row_to_timeline_item(stmt: &SqliteStmt<'_>) -> Result<TimelineItem, HostAdapterError> {
        let item_id = stmt.column_text(0);
        let session_id = CanonicalSessionId::parse(&stmt.column_text(1)).map_err(|e| {
            HostAdapterError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                e.to_string(),
            ))
        })?;
        let turn_id = stmt.column_opt_text(2);
        let kind = TimelineItemKind::parse(&stmt.column_text(3)).ok_or_else(|| {
            HostAdapterError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "invalid timeline item kind in ledger",
            ))
        })?;
        let status = TimelineItemStatus::parse(&stmt.column_text(4)).ok_or_else(|| {
            HostAdapterError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "invalid timeline item status in ledger",
            ))
        })?;
        let role = TimelineRole::parse(&stmt.column_text(5)).ok_or_else(|| {
            HostAdapterError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "invalid timeline role in ledger",
            ))
        })?;
        let content = stmt.column_text(6);
        let order_seq = TimelineOrderSeq::try_from(stmt.column_i64(7)).map_err(|e| {
            HostAdapterError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                e.to_string(),
            ))
        })?;
        let updated_seq = UpdatedSeq::new(stmt.column_i64(8));
        let content_hash = TimelineContentHash::parse(&stmt.column_text(9)).map_err(|e| {
            HostAdapterError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                e.to_string(),
            ))
        })?;
        let delivery_tier = stmt
            .column_opt_text(10)
            .and_then(|s| DeliveryTier::parse(&s));
        let sidebar_binding = stmt
            .column_opt_text(11)
            .and_then(|s| serde_json::from_str::<SidebarProjectBinding>(&s).ok());
        let peer_sender = stmt.column_opt_text(12);
        let created_at_ms = stmt.column_i64(13);

        Ok(TimelineItem {
            item_id,
            session_id,
            turn_id,
            kind,
            status,
            role,
            content,
            order_seq,
            updated_seq,
            content_hash,
            delivery_tier,
            sidebar_binding,
            peer_sender,
            created_at_ms,
        })
    }

    pub fn append_notice(
        &self,
        notice: &SessionNotice,
    ) -> Result<SessionNotice, HostAdapterError> {
        validate_nonempty_id("notice_id", &notice.notice_id)?;
        let conn = self.conn.lock().expect("ledger mutex poisoned");
        conn.exec_batch("BEGIN IMMEDIATE;")?;
        let res = (|| -> Result<SessionNotice, HostAdapterError> {
            let seq = Self::allocate_updated_seq_in_tx(&conn)?;
            let mut stmt = conn.prepare(
                "INSERT OR REPLACE INTO notices
                 (notice_id, session_id, level, code, message, updated_seq, created_at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7);",
            )?;
            let mut stored = notice.clone();
            stored.updated_seq = seq;
            let payload_json = serde_json::to_string(&stored).map_err(|e| {
                HostAdapterError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    e.to_string(),
                ))
            })?;
            stmt.bind_text(1, &stored.notice_id)?;
            stmt.bind_text(2, stored.session_id.as_str())?;
            stmt.bind_text(3, stored.kind.as_str())?;
            stmt.bind_text(4, stored.status.as_str())?;
            stmt.bind_text(5, &payload_json)?;
            stmt.bind_i64(6, seq.get())?;
            stmt.bind_i64(7, now_ms())?;
            let _ = stmt.step()?;

            Ok(stored)
        })();
        match res {
            Ok(out) => {
                conn.exec_batch("COMMIT;")?;
                Ok(out)
            }
            Err(err) => {
                let _ = conn.exec_batch("ROLLBACK;");
                Err(err)
            }
        }
    }

    pub fn list_notices(&self, session_id: &str) -> Result<Vec<SessionNotice>, HostAdapterError> {
        let conn = self.conn.lock().expect("ledger mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT message FROM notices WHERE session_id = ?1 ORDER BY updated_seq ASC;",
        )?;
        stmt.bind_text(1, session_id)?;
        let mut out = Vec::new();
        while stmt.step()? {
            let payload_json = stmt.column_text(0);
            let notice: SessionNotice = serde_json::from_str(&payload_json).map_err(|e| {
                HostAdapterError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    e.to_string(),
                ))
            })?;
            out.push(notice);
        }
        Ok(out)
    }

    pub fn update_session_runtime_state(
        &self,
        session_id: &str,
        status: InteractionStatus,
        capabilities: Option<&EffectiveCapabilities>,
    ) -> Result<UpdatedSeq, HostAdapterError> {
        let conn = self.conn.lock().expect("ledger mutex poisoned");
        conn.exec_batch("BEGIN IMMEDIATE;")?;
        let res = (|| -> Result<UpdatedSeq, HostAdapterError> {
            let seq = Self::allocate_updated_seq_in_tx(&conn)?;
            let caps_json = capabilities.and_then(|c| serde_json::to_string(c).ok());
            let mut stmt = conn.prepare(
                "UPDATE sessions SET
                    interaction_status = ?2,
                    capabilities_json = COALESCE(?3, capabilities_json),
                    latest_updated_seq = MAX(latest_updated_seq, ?4)
                 WHERE session_id = ?1;",
            )?;
            stmt.bind_text(1, session_id)?;
            stmt.bind_text(2, status.as_str())?;
            stmt.bind_opt_text(3, caps_json.as_deref())?;
            stmt.bind_i64(4, seq.get())?;
            let _ = stmt.step()?;
            Ok(seq)
        })();
        match res {
            Ok(seq) => {
                conn.exec_batch("COMMIT;")?;
                Ok(seq)
            }
            Err(err) => {
                let _ = conn.exec_batch("ROLLBACK;");
                Err(err)
            }
        }
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
