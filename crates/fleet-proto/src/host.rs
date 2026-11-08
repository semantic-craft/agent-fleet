use crate::DeliveryTier;
use serde::{Deserialize, Serialize};
use std::{
    fmt,
    path::{Component, Path, PathBuf},
    str::FromStr,
};

/// Canonical five desktop hosts supported by the fleet engine.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DesktopHostKind {
    OpenaiDots,
    Cindy,
    DeepseekHarness,
    AntigravityDesktop,
    Zcode,
}

/// Primary or fallback task/turn dispatch transport channel for a host.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum HostDispatchChannel {
    DesktopGateway,
    CliProcess,
    AnetBridge,
    PeerMailbox,
}

impl HostDispatchChannel {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DesktopGateway => "desktop-gateway",
            Self::CliProcess => "cli-process",
            Self::AnetBridge => "anet",
            Self::PeerMailbox => "peer-mailbox",
        }
    }
}

/// Pinned SQLite table and column contract for a host database label.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HostTableExpectation {
    pub db: &'static str,
    pub table: &'static str,
    pub columns: &'static [&'static str],
}

/// Declarative storage paths and environment variable overrides for a host.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HostStorageSpec {
    pub root_env_var: Option<&'static str>,
    pub default_root_rel_home: &'static str,
    pub bin_env_var: Option<&'static str>,
    pub candidate_bins: &'static [&'static str],
    pub socket_env_var: Option<&'static str>,
    pub candidate_sockets_rel_root: &'static [&'static str],
    pub candidate_sockets_rel_home: &'static [&'static str],
    pub sqlite_dbs: &'static [(&'static str, &'static str)],
    pub transcript_rel_templates: &'static [&'static str],
    pub auxiliary_paths_rel_root: &'static [(&'static str, &'static str)],
    pub auxiliary_paths_rel_home: &'static [(&'static str, &'static str)],
    pub wiring_configs_rel_home: &'static [&'static str],
}

/// Incremental readback cursor discipline used by timeline observers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HostCursorMode {
    SeekableJsonlByteOffset { requires_meta_digest: bool },
    ArchiveSizeAndMtime,
    SqliteMessageCountAndDigest,
    MailboxCountAndDeliveredMask,
}

/// Declarative readback cursor and fallback binding parameters for a host.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HostCursorSpec {
    pub mode: HostCursorMode,
    pub fallback_project_dir: &'static str,
    pub fallback_slug: &'static str,
    pub initial_order_seq: i32,
    pub order_seq_step: i32,
}

/// Declarative static specification for one fleet host (`DesktopHostKind` or `grokbot`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HostSpec {
    pub id: &'static str,
    pub desktop_kind: Option<DesktopHostKind>,
    pub desktop_target: &'static str,
    pub aliases: &'static [&'static str],
    pub bundle_id: &'static str,
    pub app_display_name: &'static str,
    pub process_grep_pattern: &'static str,
    pub primary_channel: HostDispatchChannel,
    pub fallback_channel: Option<HostDispatchChannel>,
    pub storage: HostStorageSpec,
    pub sqlite_tables: &'static [HostTableExpectation],
    pub cursor: HostCursorSpec,
}

impl HostSpec {
    pub fn matches_alias(&self, raw: &str) -> bool {
        let trimmed = raw.trim();
        self.id.eq_ignore_ascii_case(trimmed)
            || self.desktop_target.eq_ignore_ascii_case(trimmed)
            || self
                .aliases
                .iter()
                .any(|alias| alias.eq_ignore_ascii_case(trimmed))
    }

    pub fn resolve_root_dir(&self, home: &Path) -> PathBuf {
        if let Some(env_key) = self.storage.root_env_var {
            if let Ok(val) = std::env::var(env_key) {
                let trimmed = val.trim();
                if !trimmed.is_empty() {
                    return PathBuf::from(trimmed);
                }
            }
        }
        home.join(self.storage.default_root_rel_home)
    }

    pub fn resolve_bin_path(&self, home: &Path) -> Option<PathBuf> {
        if let Some(env_key) = self.storage.bin_env_var {
            if let Ok(val) = std::env::var(env_key) {
                let p = PathBuf::from(val.trim());
                if p.is_file() {
                    return Some(p);
                }
            }
        }
        for candidate in self.storage.candidate_bins {
            let p = if candidate.starts_with('/') {
                PathBuf::from(candidate)
            } else {
                home.join(candidate)
            };
            if p.is_file() {
                return Some(p);
            }
        }
        None
    }

    pub fn resolve_socket_path(&self, home: &Path, root: &Path) -> Option<PathBuf> {
        if let Some(env_key) = self.storage.socket_env_var {
            if let Ok(val) = std::env::var(env_key) {
                let p = PathBuf::from(val.trim());
                if p.exists() {
                    return Some(p);
                }
            }
        }
        for rel in self.storage.candidate_sockets_rel_root {
            let p = root.join(rel);
            if p.exists() {
                return Some(p);
            }
        }
        for rel in self.storage.candidate_sockets_rel_home {
            let p = home.join(rel);
            if p.exists() {
                return Some(p);
            }
        }
        None
    }

    pub fn resolve_db_path(&self, root: &Path, label: &str) -> Option<PathBuf> {
        self.storage
            .sqlite_dbs
            .iter()
            .find(|(db_label, _)| *db_label == label)
            .map(|(_, rel)| root.join(rel))
    }

    pub fn resolve_aux_path_in_root(&self, root: &Path, label: &str) -> Option<PathBuf> {
        self.storage
            .auxiliary_paths_rel_root
            .iter()
            .find(|(aux_label, _)| *aux_label == label)
            .map(|(_, rel)| root.join(rel))
    }

    pub fn resolve_aux_path_in_home(&self, home: &Path, label: &str) -> Option<PathBuf> {
        self.storage
            .auxiliary_paths_rel_home
            .iter()
            .find(|(aux_label, _)| *aux_label == label)
            .map(|(_, rel)| home.join(rel))
    }

    pub fn resolve_transcript_path(&self, root: &Path, session_id: &str) -> Option<PathBuf> {
        for tmpl in self.storage.transcript_rel_templates {
            let candidate = root.join(tmpl.replace("{session_id}", session_id));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        None
    }

    pub fn resolve_wiring_config_paths(&self, config_home: &Path) -> Vec<PathBuf> {
        self.storage
            .wiring_configs_rel_home
            .iter()
            .map(|rel| config_home.join(rel))
            .collect()
    }

    pub fn checkpoint_key(&self, session_id: &str) -> String {
        format!("{}:{session_id}", self.id)
    }

    pub fn default_project_id(&self, slug: &str) -> String {
        format!("proj-{}-{slug}", self.id)
    }

    pub fn fallback_project_dir(&self) -> PathBuf {
        PathBuf::from(self.cursor.fallback_project_dir)
    }

    pub fn is_checkpoint_unchanged(
        &self,
        existing_size: u64,
        existing_mtime_ns: i128,
        existing_offset: u64,
        existing_digest: &str,
        current_size: u64,
        current_mtime_ns: i128,
        current_digest: &str,
    ) -> bool {
        match self.cursor.mode {
            HostCursorMode::SeekableJsonlByteOffset {
                requires_meta_digest,
            } => {
                existing_size == current_size
                    && existing_mtime_ns == current_mtime_ns
                    && existing_offset == current_size
                    && (!requires_meta_digest || existing_digest == current_digest)
            }
            HostCursorMode::ArchiveSizeAndMtime => {
                existing_size == current_size && existing_mtime_ns == current_mtime_ns
            }
            HostCursorMode::SqliteMessageCountAndDigest
            | HostCursorMode::MailboxCountAndDeliveredMask => {
                existing_offset == current_size && existing_digest == current_digest
            }
        }
    }

    pub fn resume_cursor_window(
        &self,
        existing_size: u64,
        existing_offset: u64,
        existing_next_seq: i32,
        current_size: u64,
    ) -> (u64, i32) {
        let byte_offset = if current_size >= existing_offset && existing_size > 0 {
            existing_offset
        } else {
            0
        };
        let next_order_seq = if byte_offset > 0 && existing_next_seq > 0 {
            existing_next_seq
        } else {
            self.cursor.initial_order_seq
        };
        (byte_offset, next_order_seq)
    }
}

impl DesktopHostKind {
    pub const ALL: [Self; 5] = [
        Self::OpenaiDots,
        Self::Cindy,
        Self::DeepseekHarness,
        Self::AntigravityDesktop,
        Self::Zcode,
    ];

    pub const ALL_SPECS: [HostSpec; 6] = [
        HostSpec {
            id: "openai-dots",
            desktop_kind: Some(Self::OpenaiDots),
            desktop_target: "codex",
            aliases: &["openai dots", "codex", "codex-desktop"],
            bundle_id: "com.openai.codex",
            app_display_name: "ChatGPT",
            process_grep_pattern: "Codex.app|app-server",
            primary_channel: HostDispatchChannel::DesktopGateway,
            fallback_channel: None,
            storage: HostStorageSpec {
                root_env_var: Some("CODEX_HOME"),
                default_root_rel_home: ".codex",
                bin_env_var: None,
                candidate_bins: &[],
                socket_env_var: Some("CODEX_APP_SERVER_SOCKET"),
                candidate_sockets_rel_root: &[
                    "app-server.sock",
                    "app-server-control/app-server-control.sock",
                    "ipc/ipc.sock",
                    "app-server-daemon/daemon-updater.sock",
                ],
                candidate_sockets_rel_home: &[],
                sqlite_dbs: &[("state_5", "state_5.sqlite")],
                transcript_rel_templates: &[],
                auxiliary_paths_rel_root: &[],
                auxiliary_paths_rel_home: &[],
                wiring_configs_rel_home: &[".codex/config.toml", ".codex/hooks.json"],
            },
            sqlite_tables: &[
                HostTableExpectation {
                    db: "state_5",
                    table: "threads",
                    columns: &["id", "cwd", "title", "project_id"],
                },
                HostTableExpectation {
                    db: "state_5",
                    table: "projects",
                    columns: &["id"],
                },
                HostTableExpectation {
                    db: "state_5",
                    table: "project_roots",
                    columns: &["project_id", "path"],
                },
            ],
            cursor: HostCursorSpec {
                mode: HostCursorMode::SeekableJsonlByteOffset {
                    requires_meta_digest: true,
                },
                fallback_project_dir: "/fixture/Projects/codex",
                fallback_slug: "codex",
                initial_order_seq: 10,
                order_seq_step: 10,
            },
        },
        HostSpec {
            id: "cindy",
            desktop_kind: Some(Self::Cindy),
            desktop_target: "cindy",
            aliases: &["cindy-desktop"],
            bundle_id: "com.xd.cindy",
            app_display_name: "Cindy",
            process_grep_pattern: "Cindy.app|cindy",
            primary_channel: HostDispatchChannel::DesktopGateway,
            fallback_channel: Some(HostDispatchChannel::AnetBridge),
            storage: HostStorageSpec {
                root_env_var: Some("CINDY_HOME"),
                default_root_rel_home: ".cindy",
                bin_env_var: None,
                candidate_bins: &[],
                socket_env_var: Some("CINDY_GATEWAY_SOCKET"),
                candidate_sockets_rel_root: &["cindy-gateway.sock", "gateway.sock"],
                candidate_sockets_rel_home: &[
                    "Library/Application Support/Cindy/gateway.sock",
                    "Library/Application Support/CindyGlobal/gateway.sock",
                ],
                sqlite_dbs: &[],
                transcript_rel_templates: &["transcripts/{session_id}.jsonl", "sessions/{session_id}.jsonl"],
                auxiliary_paths_rel_root: &[
                    ("transcripts_dir", "transcripts"),
                    ("sessions_dir", "sessions"),
                ],
                auxiliary_paths_rel_home: &[
                    (
                        "anet_bridge_dir",
                        "Library/Application Support/CindyGlobal/agent-network-cindy",
                    ),
                    (
                        "anet_bridge_config",
                        "Library/Application Support/CindyGlobal/agent-network-cindy/config.json",
                    ),
                ],
                wiring_configs_rel_home: &[".cindy/mcp.json"],
            },
            sqlite_tables: &[],
            cursor: HostCursorSpec {
                mode: HostCursorMode::SeekableJsonlByteOffset {
                    requires_meta_digest: false,
                },
                fallback_project_dir: "/fixture/Projects/cindy",
                fallback_slug: "cindy",
                initial_order_seq: 10,
                order_seq_step: 10,
            },
        },
        HostSpec {
            id: "deepseek-harness",
            desktop_kind: Some(Self::DeepseekHarness),
            desktop_target: "dsh",
            aliases: &["deepseek harness", "dsh"],
            bundle_id: "com.deepseek.dsh",
            app_display_name: "DeepSeek Harness",
            process_grep_pattern: "DeepSeek Harness|bin/dsh",
            primary_channel: HostDispatchChannel::CliProcess,
            fallback_channel: None,
            storage: HostStorageSpec {
                root_env_var: Some("DSH_HOME"),
                default_root_rel_home: ".dsh",
                bin_env_var: Some("DSH_BIN"),
                candidate_bins: &[
                    ".local/bin/dsh",
                    "bin/dsh",
                    "/Applications/DeepSeek Harness.app/Contents/Resources/runtime/cli/bin/dsh",
                ],
                socket_env_var: None,
                candidate_sockets_rel_root: &[],
                candidate_sockets_rel_home: &[],
                sqlite_dbs: &[],
                transcript_rel_templates: &["sessions/{session_id}/turn_0000.txt"],
                auxiliary_paths_rel_root: &[
                    ("workspace_storage", "storages/workspace.json"),
                    ("session_cache_dir", "storages/session_projcache/sessions"),
                    ("sessions_dir", "sessions"),
                ],
                auxiliary_paths_rel_home: &[],
                wiring_configs_rel_home: &[".dsh/cordis.patch.yml"],
            },
            sqlite_tables: &[],
            cursor: HostCursorSpec {
                mode: HostCursorMode::ArchiveSizeAndMtime,
                fallback_project_dir: "/fixture/Projects/dsh",
                fallback_slug: "dsh",
                initial_order_seq: 10,
                order_seq_step: 10,
            },
        },
        HostSpec {
            id: "antigravity-desktop",
            desktop_kind: Some(Self::AntigravityDesktop),
            desktop_target: "agy",
            aliases: &["antigravity desktop", "agy", "antigravity"],
            bundle_id: "com.google.antigravity",
            app_display_name: "Antigravity",
            process_grep_pattern: "Antigravity.app|bin/agy",
            primary_channel: HostDispatchChannel::CliProcess,
            fallback_channel: None,
            storage: HostStorageSpec {
                root_env_var: Some("ANTIGRAVITY_DATA_DIR"),
                default_root_rel_home: ".gemini/antigravity",
                bin_env_var: Some("AGY_BIN"),
                candidate_bins: &[
                    ".local/bin/agy",
                    "/Applications/Antigravity.app/Contents/Resources/app/bin/agy",
                    ".local/libexec/agy/agy",
                ],
                socket_env_var: None,
                candidate_sockets_rel_root: &[],
                candidate_sockets_rel_home: &[],
                sqlite_dbs: &[("conversation_summaries", "conversation_summaries.db")],
                transcript_rel_templates: &[
                    "cascades/{session_id}.jsonl",
                    "brain/{session_id}/.system_generated/logs/transcript.jsonl",
                ],
                auxiliary_paths_rel_root: &[("daemon_dir", "daemon")],
                auxiliary_paths_rel_home: &[],
                wiring_configs_rel_home: &[
                    ".gemini/config/mcp_config.json",
                    ".gemini/config/hooks.json",
                ],
            },
            sqlite_tables: &[HostTableExpectation {
                db: "conversation_summaries",
                table: "conversation_summaries",
                columns: &[
                    "conversation_id",
                    "title",
                    "preview",
                    "step_count",
                    "last_modified_time",
                    "workspace_uris",
                    "status",
                    "project_id",
                ],
            }],
            cursor: HostCursorSpec {
                mode: HostCursorMode::SeekableJsonlByteOffset {
                    requires_meta_digest: true,
                },
                fallback_project_dir: "/fixture/Projects/antigravity",
                fallback_slug: "antigravity",
                initial_order_seq: 10,
                order_seq_step: 10,
            },
        },
        HostSpec {
            id: "zcode",
            desktop_kind: Some(Self::Zcode),
            desktop_target: "zcode",
            aliases: &["zcode-desktop"],
            bundle_id: "dev.zcode.app",
            app_display_name: "ZCode",
            process_grep_pattern: "ZCode.app|bin/zcode",
            primary_channel: HostDispatchChannel::CliProcess,
            fallback_channel: None,
            storage: HostStorageSpec {
                root_env_var: Some("ZCODE_HOME"),
                default_root_rel_home: ".zcode",
                bin_env_var: Some("ZCODE_BIN"),
                candidate_bins: &[
                    "bin/zcode",
                    ".local/bin/zcode",
                    "/Applications/ZCode.app/Contents/Resources/cli/zcode",
                ],
                socket_env_var: None,
                candidate_sockets_rel_root: &[],
                candidate_sockets_rel_home: &[],
                sqlite_dbs: &[
                    ("tasks-index", "v2/tasks-index.sqlite"),
                    ("cli-db", "cli/db/db.sqlite"),
                ],
                transcript_rel_templates: &[],
                auxiliary_paths_rel_root: &[],
                auxiliary_paths_rel_home: &[],
                wiring_configs_rel_home: &[".zcode/cli/config.json"],
            },
            sqlite_tables: &[
                HostTableExpectation {
                    db: "tasks-index",
                    table: "tasks",
                    columns: &[
                        "workspace_key",
                        "workspace_path",
                        "task_id",
                        "title",
                        "title_overridden",
                        "project_id",
                        "task_status",
                        "provider",
                        "mode",
                        "meta_json",
                        "host_id",
                        "root_path",
                        "cli_title",
                        "cli_title_source",
                        "cli_created_at",
                        "cli_updated_at",
                        "cli_last_outcome",
                        "cli_deleted",
                    ],
                },
                HostTableExpectation {
                    db: "tasks-index",
                    table: "projects",
                    columns: &["project_id", "host_id", "name"],
                },
                HostTableExpectation {
                    db: "tasks-index",
                    table: "project_roots",
                    columns: &["project_id", "root_path"],
                },
                HostTableExpectation {
                    db: "tasks-index",
                    table: "hosts",
                    columns: &["host_id", "kind"],
                },
                HostTableExpectation {
                    db: "cli-db",
                    table: "session",
                    columns: &["id", "project_id", "slug", "directory", "title"],
                },
                HostTableExpectation {
                    db: "cli-db",
                    table: "message",
                    columns: &["id", "session_id", "data", "sequence"],
                },
                HostTableExpectation {
                    db: "cli-db",
                    table: "part",
                    columns: &["id", "message_id", "session_id", "data"],
                },
            ],
            cursor: HostCursorSpec {
                mode: HostCursorMode::SqliteMessageCountAndDigest,
                fallback_project_dir: "/fixture/Projects/zcode",
                fallback_slug: "zcode",
                initial_order_seq: 10,
                order_seq_step: 10,
            },
        },
        HostSpec {
            id: "grokbot",
            desktop_kind: None,
            desktop_target: "grokbot",
            aliases: &["grok", "grok-bot", "grok_bot", "grok-cli"],
            bundle_id: GROKBOT_BUNDLE_ID,
            app_display_name: GROKBOT_APP_DISPLAY_NAME,
            process_grep_pattern: "grok",
            primary_channel: HostDispatchChannel::PeerMailbox,
            fallback_channel: None,
            storage: HostStorageSpec {
                root_env_var: None,
                default_root_rel_home: ".grok",
                bin_env_var: None,
                candidate_bins: &[".local/bin/grok"],
                socket_env_var: None,
                candidate_sockets_rel_root: &[],
                candidate_sockets_rel_home: &[],
                sqlite_dbs: &[],
                transcript_rel_templates: &[],
                auxiliary_paths_rel_root: &[],
                auxiliary_paths_rel_home: &[],
                wiring_configs_rel_home: &[".grok/config.toml", ".grok/hooks/agent-fleet.json"],
            },
            sqlite_tables: &[],
            cursor: HostCursorSpec {
                mode: HostCursorMode::MailboxCountAndDeliveredMask,
                fallback_project_dir: "/fixture/Projects/grokbot",
                fallback_slug: "grokbot",
                initial_order_seq: 10,
                order_seq_step: 10,
            },
        },
    ];

    pub const fn spec(self) -> &'static HostSpec {
        match self {
            Self::OpenaiDots => &Self::ALL_SPECS[0],
            Self::Cindy => &Self::ALL_SPECS[1],
            Self::DeepseekHarness => &Self::ALL_SPECS[2],
            Self::AntigravityDesktop => &Self::ALL_SPECS[3],
            Self::Zcode => &Self::ALL_SPECS[4],
        }
    }

    pub fn spec_by_id(raw: &str) -> Option<&'static HostSpec> {
        Self::ALL_SPECS.iter().find(|spec| spec.matches_alias(raw))
    }

    pub fn as_str(&self) -> &'static str {
        self.spec().id
    }

    pub fn bundle_id(&self) -> &'static str {
        self.spec().bundle_id
    }

    pub fn app_display_name(&self) -> &'static str {
        self.spec().app_display_name
    }

    pub fn parse(value: &str) -> Result<Self, ProjectPathError> {
        Self::spec_by_id(value)
            .and_then(|spec| spec.desktop_kind)
            .ok_or_else(|| ProjectPathError::InvalidHost(value.to_string()))
    }
}

impl fmt::Display for DesktopHostKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProjectPathError {
    InvalidRoot(PathBuf),
    InvalidSlug(String),
    PathTraversal(String),
    OutsideProjectsRoot { root: PathBuf, candidate: PathBuf },
    CacheOrHiddenSegment(String),
    InvalidHost(String),
    InvalidIdentifier(String),
}

impl fmt::Display for ProjectPathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRoot(root) => {
                write!(f, "projects root must be an absolute path: {}", root.display())
            }
            Self::InvalidSlug(slug) => write!(
                f,
                "project slug must be a single safe segment ([A-Za-z0-9._-]): {slug}"
            ),
            Self::PathTraversal(input) => {
                write!(f, "path traversal or multi-segment path rejected: {input}")
            }
            Self::OutsideProjectsRoot { root, candidate } => write!(
                f,
                "project path {} escapes allowed root {}",
                candidate.display(),
                root.display()
            ),
            Self::CacheOrHiddenSegment(seg) => {
                write!(f, "hidden or cache segment rejected in project path: {seg}")
            }
            Self::InvalidHost(host) => write!(f, "unsupported desktop host kind: {host}"),
            Self::InvalidIdentifier(reason) => write!(f, "invalid lifecycle identifier: {reason}"),
        }
    }
}

impl std::error::Error for ProjectPathError {}

/// Validates a single project slug (`~/Projects/<slug>`) and rejects path traversal,
/// hidden/cache segments, control chars, or multi-segment relative/absolute paths.
pub fn validate_project_slug(slug: &str) -> Result<&str, ProjectPathError> {
    if slug.is_empty() || slug.len() > 128 || slug.trim() != slug {
        return Err(ProjectPathError::InvalidSlug(slug.to_string()));
    }
    if slug == "."
        || slug == ".."
        || slug.contains('/')
        || slug.contains('\\')
        || slug.contains("..")
        || slug.chars().any(char::is_control)
    {
        return Err(ProjectPathError::PathTraversal(slug.to_string()));
    }
    if slug.starts_with('.') || slug.starts_with('-') {
        return Err(ProjectPathError::CacheOrHiddenSegment(slug.to_string()));
    }
    let lowered = slug.to_ascii_lowercase();
    if matches!(
        lowered.as_str(),
        ".cache" | "caches" | ".anet" | "private" | "_private" | "node_modules" | "target"
    ) {
        return Err(ProjectPathError::CacheOrHiddenSegment(slug.to_string()));
    }
    if !slug
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    {
        return Err(ProjectPathError::InvalidSlug(slug.to_string()));
    }
    Ok(slug)
}

/// Validates that `projects_root` is an absolute path without parent traversal or cache components,
/// and returns the resolved child path `<projects_root>/<slug>`.
pub fn resolve_project_dir_in_root(
    projects_root: &Path,
    slug: &str,
) -> Result<PathBuf, ProjectPathError> {
    if !projects_root.is_absolute() {
        return Err(ProjectPathError::InvalidRoot(projects_root.to_path_buf()));
    }
    for component in projects_root.components() {
        match component {
            Component::ParentDir | Component::CurDir => {
                return Err(ProjectPathError::PathTraversal(
                    projects_root.display().to_string(),
                ));
            }
            Component::Normal(part) => {
                let s = part.to_string_lossy();
                if matches!(s.as_ref(), ".cache" | ".anet" | "PRIVATE") {
                    return Err(ProjectPathError::CacheOrHiddenSegment(s.into_owned()));
                }
            }
            _ => {}
        }
    }
    if std::fs::symlink_metadata(projects_root)
        .is_ok_and(|meta| meta.file_type().is_symlink())
    {
        return Err(ProjectPathError::OutsideProjectsRoot {
            root: projects_root.to_path_buf(),
            candidate: projects_root.to_path_buf(),
        });
    }
    let validated_slug = validate_project_slug(slug)?;
    let candidate = projects_root.join(validated_slug);
    if candidate.parent() != Some(projects_root) || !candidate.starts_with(projects_root) {
        return Err(ProjectPathError::OutsideProjectsRoot {
            root: projects_root.to_path_buf(),
            candidate,
        });
    }
    if std::fs::symlink_metadata(&candidate).is_ok_and(|meta| meta.file_type().is_symlink()) {
        return Err(ProjectPathError::OutsideProjectsRoot {
            root: projects_root.to_path_buf(),
            candidate,
        });
    }
    Ok(candidate)
}

/// Validates a relative path inside `<projects_root>/<slug>` for remote workbench file access,
/// rejecting traversal (`..`), absolute paths, hidden credential/cache segments (`.anet`, `.cache`,
/// `.ssh`, `PRIVATE`), and symlinks on disk.
pub fn resolve_workbench_file_path(
    projects_root: &Path,
    slug: &str,
    relative_path: &str,
) -> Result<PathBuf, ProjectPathError> {
    let project_dir = resolve_project_dir_in_root(projects_root, slug)?;
    let trimmed = relative_path.trim();
    if trimmed.is_empty()
        || trimmed.starts_with('/')
        || trimmed.starts_with('\\')
        || trimmed.contains('\\')
        || trimmed.chars().any(char::is_control)
    {
        return Err(ProjectPathError::PathTraversal(relative_path.to_string()));
    }
    let rel = Path::new(trimmed);
    let mut current = project_dir.clone();
    for component in rel.components() {
        match component {
            Component::Normal(part) => {
                let s = part.to_string_lossy();
                if s.is_empty() || s == "." || s == ".." {
                    return Err(ProjectPathError::PathTraversal(relative_path.to_string()));
                }
                let lowered = s.to_ascii_lowercase();
                if matches!(
                    lowered.as_str(),
                    ".cache" | "caches" | ".anet" | ".ssh" | "private" | "_private" | ".git"
                ) {
                    return Err(ProjectPathError::CacheOrHiddenSegment(s.into_owned()));
                }
                current.push(part);
                if std::fs::symlink_metadata(&current)
                    .is_ok_and(|meta| meta.file_type().is_symlink())
                {
                    return Err(ProjectPathError::OutsideProjectsRoot {
                        root: project_dir,
                        candidate: current,
                    });
                }
            }
            _ => return Err(ProjectPathError::PathTraversal(relative_path.to_string())),
        }
    }
    if !current.starts_with(&project_dir) || current == project_dir {
        return Err(ProjectPathError::OutsideProjectsRoot {
            root: project_dir,
            candidate: current,
        });
    }
    Ok(current)
}

pub fn validate_nonempty_id(field: &str, value: &str) -> Result<(), ProjectPathError> {
    if value.is_empty()
        || value.len() > 256
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        return Err(ProjectPathError::InvalidIdentifier(format!(
            "{field} must be nonempty, trimmed, and free of control characters"
        )));
    }
    Ok(())
}

/// Verified project directory inside the configured `projects_root`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectDirRecord {
    pub slug: String,
    pub path: PathBuf,
    pub workspace_uri: String,
    pub created: bool,
}

/// Sidebar project binding registered in a desktop host (`projects` / `project_roots` / `workspace_uris`).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SidebarProjectBinding {
    pub host: DesktopHostKind,
    pub project_id: String,
    pub title: String,
    pub project_dir: PathBuf,
    pub workspace_uris: Vec<String>,
}

/// Request to create a new sidebar-visible session under a registered sidebar project.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CreateSessionRequest {
    pub delivery_id: String,
    pub project_id: String,
    pub project_dir: PathBuf,
    pub title: String,
    pub prompt: String,
    pub model: Option<String>,
}

/// Request to deliver a follow-up turn to an existing session in the same sidebar project.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DeliverTurnRequest {
    pub delivery_id: String,
    pub project_id: String,
    pub session_id: String,
    pub project_dir: PathBuf,
    pub prompt: String,
    pub model: Option<String>,
}

/// Receipt proving session creation or follow-up turn delivery bound to a sidebar project.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HostSessionReceipt {
    pub host: DesktopHostKind,
    pub delivery_id: String,
    pub project_id: String,
    #[serde(alias = "conversation_id")]
    pub session_id: String,
    pub turn_id: String,
    pub project_dir: PathBuf,
    pub workspace_uris: Vec<String>,
    pub tier: DeliveryTier,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

impl HostSessionReceipt {
    /// Antigravity Desktop and unified callers refer to `session_id` as `conversation_id`.
    pub fn conversation_id(&self) -> &str {
        &self.session_id
    }
}

/// Manifest describing a pluggable third-party or open-source agent registered in `agents.d/<id>.json`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalAgentManifest {
    pub id: String,
    pub display_name: String,
    pub command: PathBuf,
    #[serde(default)]
    pub args_template: Vec<String>,
    #[serde(default)]
    pub resume_args_template: Vec<String>,
    #[serde(default)]
    pub description: String,
}

impl ExternalAgentManifest {
    pub fn validate(&self) -> Result<(), ProjectPathError> {
        validate_project_slug(&self.id)?;
        if DesktopHostKind::parse(&self.id).is_ok() {
            return Err(ProjectPathError::InvalidIdentifier(format!(
                "external agent id '{}' collides with a canonical desktop host",
                self.id
            )));
        }
        validate_nonempty_id("display_name", &self.display_name)?;
        if !self.command.is_absolute() {
            return Err(ProjectPathError::InvalidRoot(self.command.clone()));
        }
        for arg in self
            .args_template
            .iter()
            .chain(self.resume_args_template.iter())
        {
            if arg.chars().any(char::is_control) {
                return Err(ProjectPathError::InvalidIdentifier(
                    "external agent args_template must not contain control characters".into(),
                ));
            }
        }
        Ok(())
    }
}

pub const GROKBOT_BUNDLE_ID: &str = "com.anysphere.sand";
pub const GROKBOT_APP_DISPLAY_NAME: &str = "Grok Bot";

/// Validates a session or conversation fragment (`#<session_id>`) in a peer address.
pub fn validate_session_id(session_id: &str) -> Result<&str, ProjectPathError> {
    if session_id.is_empty()
        || session_id.len() > 256
        || session_id.trim() != session_id
        || session_id.chars().any(char::is_control)
        || session_id.chars().any(char::is_whitespace)
        || session_id.contains('/')
        || session_id.contains('\\')
        || session_id.contains('#')
    {
        return Err(ProjectPathError::InvalidIdentifier(format!(
            "session_id must be nonempty, trimmed, <= 256 chars, and free of whitespace, '/', '\\', or '#': {session_id}"
        )));
    }
    Ok(session_id)
}

/// Canonical peer address supporting `<agent>`, `<agent>/<slug>`, `<agent>#<session_id>`,
/// and `<agent>/<slug>#<session_id>`.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PeerAddress {
    pub agent: String,
    pub slug: Option<String>,
    pub session_id: Option<String>,
}

impl PeerAddress {
    pub fn parse(raw: &str) -> Result<Self, ProjectPathError> {
        let trimmed = raw.trim();
        validate_nonempty_id("peer_id", trimmed)?;
        let (target_part, session_opt) = match trimmed.split_once('#') {
            Some((left, right)) => {
                let sid = validate_session_id(right.trim())?.to_string();
                (left.trim(), Some(sid))
            }
            None => (trimmed, None),
        };
        let (agent, slug) = if let Some((agent_part, slug_part)) = target_part.split_once('/') {
            let norm_agent = normalize_single_agent_id(agent_part)?;
            let norm_slug = validate_project_slug(slug_part.trim())?.to_string();
            (norm_agent, Some(norm_slug))
        } else {
            let norm_agent = normalize_single_agent_id(target_part)?;
            (norm_agent, None)
        };
        Ok(Self {
            agent,
            slug,
            session_id: session_opt,
        })
    }

    pub fn is_session_scoped(&self) -> bool {
        self.session_id.is_some()
    }

    pub fn with_slug(mut self, slug: impl Into<String>) -> Result<Self, ProjectPathError> {
        let s = slug.into();
        let validated = validate_project_slug(s.trim())?.to_string();
        self.slug = Some(validated);
        Ok(self)
    }

    pub fn with_session_id(
        mut self,
        session_id: impl Into<String>,
    ) -> Result<Self, ProjectPathError> {
        let s = session_id.into();
        let validated = validate_session_id(s.trim())?.to_string();
        self.session_id = Some(validated);
        Ok(self)
    }

    pub fn without_session(&self) -> Self {
        Self {
            agent: self.agent.clone(),
            slug: self.slug.clone(),
            session_id: None,
        }
    }
}

impl FromStr for PeerAddress {
    type Err = ProjectPathError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl fmt::Display for PeerAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.agent)?;
        if let Some(slug) = &self.slug {
            write!(f, "/{slug}")?;
        }
        if let Some(sid) = &self.session_id {
            write!(f, "#{sid}")?;
        }
        Ok(())
    }
}

/// Normalizes a peer or caller identifier (`<agent>`, `<agent>/<slug>`, or `<agent>/<slug>#<session_id>`).
pub fn normalize_peer_id(raw: &str) -> Result<String, ProjectPathError> {
    PeerAddress::parse(raw).map(|a| a.to_string())
}

fn normalize_single_agent_id(raw: &str) -> Result<String, ProjectPathError> {
    let trimmed = raw.trim();
    if let Some(spec) = DesktopHostKind::spec_by_id(trimmed) {
        return Ok(spec.id.to_string());
    }
    validate_project_slug(trimmed)?;
    Ok(trimmed.to_string())
}

/// Small text file attachment carried alongside a peer message (modeled on Reason Network's inbox attachments).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MessageAttachment {
    pub relative_path: String,
    pub size_bytes: usize,
    pub saved_path: Option<PathBuf>,
}

/// Bidirectional peer message record persisted in the engine's mailbox store (`<state_root>/mailboxes.json`).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InboxMessage {
    pub id: String,
    pub from: String,
    pub to: String,
    pub project_slug: Option<String>,
    pub session_id: Option<String>,
    pub reply_to: Option<String>,
    pub body: String,
    #[serde(default)]
    pub files: Vec<MessageAttachment>,
    #[serde(default = "default_true")]
    pub wake: bool,
    #[serde(default)]
    pub notify: bool,
    pub created_at_unix: u64,
    #[serde(default)]
    pub delivered: bool,
}

fn default_true() -> bool {
    true
}

impl InboxMessage {
    pub fn sender_address(&self) -> Result<PeerAddress, ProjectPathError> {
        PeerAddress::parse(&self.from)
    }

    pub fn recipient_address(&self) -> Result<PeerAddress, ProjectPathError> {
        PeerAddress::parse(&self.to)
    }

    pub fn target_session_id(&self) -> Option<String> {
        self.recipient_address()
            .ok()
            .and_then(|a| a.session_id)
            .or_else(|| self.session_id.clone())
    }

    pub fn matches_recipient(&self, query: &PeerAddress) -> bool {
        let Ok(msg_to) = PeerAddress::parse(&self.to) else {
            return false;
        };
        if msg_to.agent != query.agent {
            return false;
        }
        if let Some(q_slug) = query.slug.as_deref() {
            let effective_slug = msg_to.slug.as_deref().or(self.project_slug.as_deref());
            if effective_slug != Some(q_slug) {
                return false;
            }
        }
        if let Some(target_sid) = msg_to.session_id.as_deref() {
            query.session_id.as_deref() == Some(target_sid)
        } else {
            match query.session_id.as_deref() {
                None => true,
                Some(q_sid) => self.session_id.as_deref() == Some(q_sid),
            }
        }
    }

    pub fn matches_sender(&self, query: &PeerAddress) -> bool {
        let Ok(msg_from) = PeerAddress::parse(&self.from) else {
            return false;
        };
        if msg_from.agent != query.agent {
            return false;
        }
        if let Some(q_slug) = query.slug.as_deref() {
            let effective_slug = msg_from.slug.as_deref().or(self.project_slug.as_deref());
            if effective_slug != Some(q_slug) {
                return false;
            }
        }
        match query.session_id.as_deref() {
            Some(q_sid) => msg_from.session_id.as_deref() == Some(q_sid),
            None => msg_from.session_id.is_none(),
        }
    }

    /// Renders the message as a structured peer input block (`<fleet-message ...>`) so the receiving
    /// desktop agent or GrokBot knows the sender identity, project repo, message ID, and how to reply.
    pub fn format_peer_frame(&self) -> String {
        let repo_attr = self
            .project_slug
            .as_deref()
            .map(|s| format!(" repo=\"{s}\""))
            .unwrap_or_default();
        let reply_attr = self
            .reply_to
            .as_deref()
            .map(|r| format!(" reply_to=\"{r}\""))
            .unwrap_or_default();
        let mut out = format!(
            "<fleet-message from=\"{}\" to=\"{}\" id=\"{}\"{}{}>\n{}\n</fleet-message>\nFrom peer agent \"{}\". Reply with the agent-fleet send_message tool to \"{}\" with reply_to=\"{}\".",
            self.from,
            self.to,
            self.id,
            repo_attr,
            reply_attr,
            self.body.trim(),
            self.from,
            self.from,
            self.id
        );
        if !self.files.is_empty() {
            out.push_str("\nAttached files:");
            for f in &self.files {
                if let Some(saved) = &f.saved_path {
                    out.push_str(&format!(
                        "\n- {} ({} bytes) -> {}",
                        f.relative_path,
                        f.size_bytes,
                        saved.display()
                    ));
                } else {
                    out.push_str(&format!("\n- {} ({} bytes)", f.relative_path, f.size_bytes));
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_host_kind_parses_all_five_canonical_hosts() {
        for (input, expected) in [
            ("openai-dots", DesktopHostKind::OpenaiDots),
            ("openai dots", DesktopHostKind::OpenaiDots),
            ("codex", DesktopHostKind::OpenaiDots),
            ("cindy", DesktopHostKind::Cindy),
            ("cindy-desktop", DesktopHostKind::Cindy),
            ("deepseek-harness", DesktopHostKind::DeepseekHarness),
            ("dsh", DesktopHostKind::DeepseekHarness),
            ("antigravity-desktop", DesktopHostKind::AntigravityDesktop),
            ("antigravity desktop", DesktopHostKind::AntigravityDesktop),
            ("agy", DesktopHostKind::AntigravityDesktop),
            ("zcode", DesktopHostKind::Zcode),
            ("zcode-desktop", DesktopHostKind::Zcode),
        ] {
            assert_eq!(DesktopHostKind::parse(input).unwrap(), expected);
        }
        assert_eq!(DesktopHostKind::ALL.len(), 5);
        assert_eq!(DesktopHostKind::ALL_SPECS.len(), 6);
        assert!(DesktopHostKind::parse("unknown-host").is_err());
        let grok = DesktopHostKind::spec_by_id("grok-bot").unwrap();
        assert_eq!(grok.id, "grokbot");
        assert_eq!(grok.desktop_kind, None);
        assert_eq!(grok.primary_channel, HostDispatchChannel::PeerMailbox);
        for host in DesktopHostKind::ALL {
            let spec = host.spec();
            assert_eq!(spec.desktop_kind, Some(host));
            assert_eq!(DesktopHostKind::spec_by_id(spec.id), Some(spec));
            assert_eq!(DesktopHostKind::spec_by_id(spec.desktop_target), Some(spec));
        }
    }

    #[test]
    fn external_agent_manifest_validates_id_and_rejects_canonical_collisions() {
        let valid = ExternalAgentManifest {
            id: "opencode-agent".into(),
            display_name: "OpenCode Agent".into(),
            command: PathBuf::from("/usr/local/bin/opencode"),
            args_template: vec!["--cwd".into(), "{cwd}".into(), "-p".into(), "{prompt}".into()],
            resume_args_template: vec![
                "--cwd".into(),
                "{cwd}".into(),
                "--session".into(),
                "{session_id}".into(),
                "-p".into(),
                "{prompt}".into(),
            ],
            description: "Open-source terminal coding agent".into(),
        };
        assert!(valid.validate().is_ok());

        let collision = ExternalAgentManifest {
            id: "zcode".into(),
            ..valid.clone()
        };
        assert!(collision.validate().is_err());

        let relative_cmd = ExternalAgentManifest {
            command: PathBuf::from("opencode"),
            ..valid
        };
        assert!(relative_cmd.validate().is_err());
    }

    #[test]
    fn validate_project_slug_rejects_traversal_hidden_and_out_of_bounds_names() {
        for valid in ["agent-fleet", "demo_project", "slice.i.2026"] {
            assert_eq!(validate_project_slug(valid).unwrap(), valid);
        }
        for invalid in [
            "",
            ".",
            "..",
            "../escape",
            "a/b",
            "a\\b",
            "/abs/path",
            ".hidden",
            "-flag",
            ".cache",
            ".anet",
            "PRIVATE",
            "bad space",
            "bad\nctrl",
        ] {
            assert!(
                validate_project_slug(invalid).is_err(),
                "expected invalid slug rejection for {invalid:?}"
            );
        }
    }

    #[test]
    fn resolve_project_dir_in_root_enforces_absolute_root_and_single_child() {
        let root = Path::new("/tmp/fleet-projects");
        let resolved = resolve_project_dir_in_root(root, "alpha-1").unwrap();
        assert_eq!(resolved, PathBuf::from("/tmp/fleet-projects/alpha-1"));

        assert!(resolve_project_dir_in_root(Path::new("relative/root"), "alpha").is_err());
        assert!(resolve_project_dir_in_root(Path::new("/tmp/../escape"), "alpha").is_err());
        assert!(resolve_project_dir_in_root(Path::new("/tmp/.cache/projects"), "alpha").is_err());
        assert!(resolve_project_dir_in_root(root, "../outside").is_err());
    }

    #[test]
    fn peer_address_parses_normalizes_and_validates_session_fragments() {
        let a1 = PeerAddress::parse("agy/agent-fleet#conv-123").unwrap();
        assert_eq!(a1.agent, "antigravity-desktop");
        assert_eq!(a1.slug.as_deref(), Some("agent-fleet"));
        assert_eq!(a1.session_id.as_deref(), Some("conv-123"));
        assert!(a1.is_session_scoped());
        assert_eq!(a1.to_string(), "antigravity-desktop/agent-fleet#conv-123");
        assert_eq!(
            a1.without_session().to_string(),
            "antigravity-desktop/agent-fleet"
        );
        assert_eq!(
            normalize_peer_id("agy/agent-fleet#conv-123").unwrap(),
            "antigravity-desktop/agent-fleet#conv-123"
        );

        let a2 = PeerAddress::parse("dsh#sess-99").unwrap();
        assert_eq!(a2.agent, "deepseek-harness");
        assert_eq!(a2.slug, None);
        assert_eq!(a2.session_id.as_deref(), Some("sess-99"));
        assert_eq!(a2.to_string(), "deepseek-harness#sess-99");

        for invalid in [
            "agy/agent-fleet#",
            "agy/agent-fleet#bad/slash",
            "agy/agent-fleet#bad#hash",
            "agy/agent-fleet#bad space",
            "agy/agent-fleet#bad\nctrl",
        ] {
            assert!(
                PeerAddress::parse(invalid).is_err(),
                "expected invalid PeerAddress for {invalid:?}"
            );
        }
    }

    #[test]
    fn inbox_message_format_and_session_isolation_matching() {
        let msg = InboxMessage {
            id: "msg-1".into(),
            from: "antigravity-desktop/agent-fleet#conv-123".into(),
            to: "deepseek-harness/agent-fleet".into(),
            project_slug: Some("agent-fleet".into()),
            session_id: Some("dsh-sess-1".into()),
            reply_to: None,
            body: "Inspect Slice X.".into(),
            files: Vec::new(),
            wake: true,
            notify: false,
            created_at_unix: 1000,
            delivered: false,
        };
        let framed = msg.format_peer_frame();
        assert!(framed.contains(
            "<fleet-message from=\"antigravity-desktop/agent-fleet#conv-123\" to=\"deepseek-harness/agent-fleet\" id=\"msg-1\" repo=\"agent-fleet\">"
        ));
        assert!(framed.contains(
            "Reply with the agent-fleet send_message tool to \"antigravity-desktop/agent-fleet#conv-123\" with reply_to=\"msg-1\"."
        ));

        assert!(msg.matches_recipient(&PeerAddress::parse("deepseek-harness").unwrap()));
        assert!(
            msg.matches_recipient(&PeerAddress::parse("deepseek-harness/agent-fleet").unwrap())
        );
        assert!(msg.matches_recipient(
            &PeerAddress::parse("deepseek-harness/agent-fleet#dsh-sess-1").unwrap()
        ));
        assert!(!msg.matches_recipient(
            &PeerAddress::parse("deepseek-harness/agent-fleet#dsh-sess-2").unwrap()
        ));

        let reply = InboxMessage {
            id: "msg-2".into(),
            from: "deepseek-harness/agent-fleet#dsh-sess-1".into(),
            to: "antigravity-desktop/agent-fleet#conv-123".into(),
            project_slug: Some("agent-fleet".into()),
            session_id: Some("conv-123".into()),
            reply_to: Some("msg-1".into()),
            body: "Slice X complete.".into(),
            files: Vec::new(),
            wake: true,
            notify: false,
            created_at_unix: 1001,
            delivered: false,
        };
        assert!(!reply.matches_recipient(&PeerAddress::parse("antigravity-desktop").unwrap()));
        assert!(!reply.matches_recipient(
            &PeerAddress::parse("antigravity-desktop/agent-fleet").unwrap()
        ));
        assert!(!reply.matches_recipient(
            &PeerAddress::parse("antigravity-desktop/agent-fleet#conv-other").unwrap()
        ));
        assert!(reply.matches_recipient(
            &PeerAddress::parse("antigravity-desktop/agent-fleet#conv-123").unwrap()
        ));

        assert!(reply.matches_sender(
            &PeerAddress::parse("deepseek-harness/agent-fleet#dsh-sess-1").unwrap()
        ));
        assert!(!reply.matches_sender(
            &PeerAddress::parse("deepseek-harness/agent-fleet").unwrap()
        ));
        assert!(!reply.matches_sender(
            &PeerAddress::parse("deepseek-harness/agent-fleet#dsh-sess-2").unwrap()
        ));
    }
}
