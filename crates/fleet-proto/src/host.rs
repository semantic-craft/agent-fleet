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

impl DesktopHostKind {
    pub const ALL: [Self; 5] = [
        Self::OpenaiDots,
        Self::Cindy,
        Self::DeepseekHarness,
        Self::AntigravityDesktop,
        Self::Zcode,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::OpenaiDots => "openai-dots",
            Self::Cindy => "cindy",
            Self::DeepseekHarness => "deepseek-harness",
            Self::AntigravityDesktop => "antigravity-desktop",
            Self::Zcode => "zcode",
        }
    }

    pub fn bundle_id(&self) -> &'static str {
        match self {
            Self::OpenaiDots => "com.openai.codex",
            Self::Cindy => "com.xd.cindy",
            Self::DeepseekHarness => "com.deepseek.dsh",
            Self::AntigravityDesktop => "com.google.antigravity",
            Self::Zcode => "dev.zcode.app",
        }
    }

    pub fn app_display_name(&self) -> &'static str {
        match self {
            Self::OpenaiDots => "ChatGPT",
            Self::Cindy => "Cindy",
            Self::DeepseekHarness => "DeepSeek Harness",
            Self::AntigravityDesktop => "Antigravity",
            Self::Zcode => "ZCode",
        }
    }

    pub fn parse(value: &str) -> Result<Self, ProjectPathError> {
        match value.trim() {
            "openai-dots" | "openai dots" | "codex" | "codex-desktop" => Ok(Self::OpenaiDots),
            "cindy" | "cindy-desktop" => Ok(Self::Cindy),
            "deepseek-harness" | "deepseek harness" | "dsh" => Ok(Self::DeepseekHarness),
            "antigravity-desktop" | "antigravity desktop" | "agy" | "antigravity" => {
                Ok(Self::AntigravityDesktop)
            }
            "zcode" | "zcode-desktop" => Ok(Self::Zcode),
            _ => Err(ProjectPathError::InvalidHost(value.to_string())),
        }
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
    if let Ok(desktop) = DesktopHostKind::parse(trimmed) {
        return Ok(desktop.as_str().to_string());
    }
    match trimmed.to_ascii_lowercase().as_str() {
        "grok" | "grokbot" | "grok-bot" | "grok_bot" | "grok-cli" => Ok("grokbot".to_string()),
        _ => {
            validate_project_slug(trimmed)?;
            Ok(trimmed.to_string())
        }
    }
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
        assert!(DesktopHostKind::parse("unknown-host").is_err());
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
