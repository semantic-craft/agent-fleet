use crate::{
    read_cindy_transcript, AntigravityConversationSummary, AntigravityDesktopAdapter,
    AntigravityRuntimeConfig, CindyAdapter, CindyRuntimeConfig, CindySessionSnapshot,
    CodexProjectRow, CodexThreadRow, DeepSeekHarnessAdapter, DesktopHostAdapter, DshRuntimeConfig,
    DshSessionSnapshot, HostAdapterError, OpenAiDotsAdapter, OpenAiDotsRuntimeConfig, ZcodeAdapter,
    ZcodeProjectRow, ZcodeRuntimeConfig, ZcodeSessionSnapshot,
};
use fleet_proto::{
    validate_brief, validate_nonempty_id, CreateSessionRequest, DeliverTurnRequest,
    DesktopHostKind, HostSessionReceipt, ProjectDirRecord, ProjectPathError, SidebarProjectBinding,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Input specification for a `/poteto-mode` (`pstack-relay`) cross-agent self-development slice
/// delivered through `fleet-cli` to one of the four canonical desktop hosts.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PotetoRelayTask {
    pub slice_id: String,
    pub slug: String,
    pub title: String,
    pub goal: String,
    pub done_means: String,
    pub proof_to_show: String,
    pub prior_evidence: String,
    pub followup_prompt: String,
    pub model: Option<String>,
}

impl PotetoRelayTask {
    /// Renders the canonical `/poteto-mode` (`pstack-relay`) 5-element prompt and enforces
    /// `validate_brief` (no `MODEL_WRONG` gates or worker model self-attestation).
    pub fn render_initial_prompt(&self) -> Result<String, HostAdapterError> {
        validate_nonempty_id("slice_id", &self.slice_id)?;
        validate_nonempty_id("title", &self.title)?;
        if self.goal.trim().is_empty() {
            return Err(ProjectPathError::InvalidIdentifier(
                "goal must be nonempty".into(),
            )
            .into());
        }
        if self.done_means.trim().is_empty() {
            return Err(ProjectPathError::InvalidIdentifier(
                "done_means must be nonempty".into(),
            )
            .into());
        }
        let proof = if self.proof_to_show.trim().is_empty() {
            "real command output and artifact receipt from scripts/check (prove-it-works, not 'it compiles')"
        } else {
            self.proof_to_show.trim()
        };
        let prior = if self.prior_evidence.trim().is_empty() {
            "docs/pstack-relay.md;.scratch/pstack-relay/decisions.tsv"
        } else {
            self.prior_evidence.trim()
        };
        let prompt = format!(
            "/poteto-mode\n\
             new task. Take over this repo from `docs/pstack-relay.md` and `.scratch/pstack-relay/decisions.tsv` \
             (treat the prior trail as authoritative; do not redo finished slices).\n\
             - What you already know: {prior}\n\
             - Goal (**Slice {slice}: {title}**): {goal}\n\
             - Done means (falsifiable check): {done}\n\
             - Proof to show: {proof}\n\
             - Real constraints:\n\
               1. Restate the goal and smallest change in plain words, then proceed immediately (`never-block-on-the-human`).\n\
               2. Settle technical forks with running code or isolated prototypes.\n\
               3. Record decisions in `.scratch/pstack-relay/decisions.tsv` (`show-me-your-work`).",
            prior = prior,
            slice = self.slice_id.trim(),
            title = self.title.trim(),
            goal = self.goal.trim(),
            done = self.done_means.trim(),
            proof = proof,
        );
        validate_brief(&prompt)?;
        Ok(prompt)
    }

    pub fn validate_followup_prompt(&self) -> Result<String, HostAdapterError> {
        let text = self.followup_prompt.trim();
        if text.is_empty() {
            return Err(ProjectPathError::InvalidIdentifier(
                "followup_prompt must be nonempty".into(),
            )
            .into());
        }
        validate_brief(text)?;
        Ok(text.to_string())
    }
}

/// Host-specific store verification evidence proving sidebar project binding, directory creation,
/// and same-session multi-turn visibility.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "store_kind", rename_all = "kebab-case")]
pub enum HostStoreVerificationEvidence {
    OpenaiDots {
        project_row: CodexProjectRow,
        thread_row: CodexThreadRow,
        deep_link: String,
        turn_ids: Vec<String>,
    },
    Cindy {
        session_snapshot: CindySessionSnapshot,
        transcript_entry_count: usize,
        turn_ids: Vec<String>,
    },
    DeepseekHarness {
        session_snapshot: DshSessionSnapshot,
        turn_ids: Vec<String>,
    },
    AntigravityDesktop {
        summary: AntigravityConversationSummary,
        hub_http_port: Option<u16>,
        turn_ids: Vec<String>,
    },
    Zcode {
        project_row: ZcodeProjectRow,
        session_snapshot: ZcodeSessionSnapshot,
        turn_ids: Vec<String>,
    },
    LocalState {
        persisted_turn_count: usize,
        turn_ids: Vec<String>,
    },
}

/// End-to-end receipt proving that a desktop host created `<projects_root>/<slug>`, registered the
/// project in the desktop sidebar, started a `/poteto-mode` (`pstack-relay`) session, and delivered
/// a follow-up verification turn on the same session.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HostRelayClosureReceipt {
    pub host: DesktopHostKind,
    pub slice_id: String,
    pub rendered_initial_prompt: String,
    pub followup_prompt: String,
    pub project_dir: ProjectDirRecord,
    pub sidebar_project: SidebarProjectBinding,
    pub initial_session: HostSessionReceipt,
    pub followup_turn: HostSessionReceipt,
    pub store_evidence: HostStoreVerificationEvidence,
}

/// Optional runtime configuration for any of the five canonical desktop hosts.
#[derive(Clone, Debug, Default)]
pub struct HostRuntimeOptions {
    pub openai_dots: Option<OpenAiDotsRuntimeConfig>,
    pub cindy: Option<CindyRuntimeConfig>,
    pub dsh: Option<DshRuntimeConfig>,
    pub antigravity: Option<AntigravityRuntimeConfig>,
    pub zcode: Option<ZcodeRuntimeConfig>,
}

/// Executes the `/poteto-mode` (`pstack-relay`) self-development lifecycle on the requested
/// desktop host (`openai-dots`, `cindy`, `deepseek-harness`, `antigravity-desktop`, or `zcode`),
/// returning a verified `HostRelayClosureReceipt`.
pub fn run_host_poteto_relay(
    host: DesktopHostKind,
    projects_root: PathBuf,
    state_root: PathBuf,
    task: &PotetoRelayTask,
    runtimes: &HostRuntimeOptions,
) -> Result<HostRelayClosureReceipt, HostAdapterError> {
    let initial_prompt = task.render_initial_prompt()?;
    let followup_prompt = task.validate_followup_prompt()?;
    let create_delivery_id = format!(
        "relay-{}-{}-{}-create",
        host.as_str(),
        task.slice_id.to_ascii_lowercase(),
        task.slug
    );
    let followup_delivery_id = format!(
        "relay-{}-{}-{}-followup",
        host.as_str(),
        task.slice_id.to_ascii_lowercase(),
        task.slug
    );

    match host {
        DesktopHostKind::OpenaiDots => {
            let mut adapter = match runtimes.openai_dots.clone() {
                Some(rt) => OpenAiDotsAdapter::with_runtime(projects_root, state_root, rt)?,
                None => OpenAiDotsAdapter::new(projects_root, state_root)?,
            };
            let project_dir = adapter.ensure_project_dir(&task.slug)?;
            let sidebar_project =
                adapter.ensure_sidebar_project(&task.slug, Some(&task.title))?;
            let create_req = CreateSessionRequest {
                delivery_id: create_delivery_id,
                project_id: sidebar_project.project_id.clone(),
                project_dir: sidebar_project.project_dir.clone(),
                title: task.title.clone(),
                prompt: initial_prompt.clone(),
                model: task.model.clone(),
            };
            let created = adapter.create_dots_session(&create_req)?;
            let followup_req = DeliverTurnRequest {
                delivery_id: followup_delivery_id,
                project_id: sidebar_project.project_id.clone(),
                session_id: created.session_id.clone(),
                project_dir: sidebar_project.project_dir.clone(),
                prompt: followup_prompt.clone(),
                model: task.model.clone(),
            };
            let followup = adapter.deliver_dots_turn(&followup_req)?;
            let turn_ids = vec![created.turn_id.clone(), followup.turn_id.clone()];
            let store_evidence = if runtimes.openai_dots.is_some() {
                let project_row = adapter
                    .read_project_row(&sidebar_project.project_id)?
                    .ok_or_else(|| {
                        HostAdapterError::OpenAiDotsProtocol(format!(
                            "missing project {} in state_5.sqlite during relay verification",
                            sidebar_project.project_id
                        ))
                    })?;
                let thread_row = adapter
                    .read_thread_row(&followup.thread_id)?
                    .ok_or_else(|| {
                        HostAdapterError::OpenAiDotsProtocol(format!(
                            "missing thread {} in state_5.sqlite during relay verification",
                            followup.thread_id
                        ))
                    })?;
                HostStoreVerificationEvidence::OpenaiDots {
                    project_row,
                    thread_row,
                    deep_link: followup.deep_link.clone(),
                    turn_ids,
                }
            } else {
                let persisted = adapter
                    .session(&followup.session_id)
                    .map(|s| s.turns.len())
                    .unwrap_or(0);
                HostStoreVerificationEvidence::LocalState {
                    persisted_turn_count: persisted,
                    turn_ids,
                }
            };
            Ok(HostRelayClosureReceipt {
                host,
                slice_id: task.slice_id.clone(),
                rendered_initial_prompt: initial_prompt,
                followup_prompt,
                project_dir,
                sidebar_project,
                initial_session: created.into(),
                followup_turn: followup.into(),
                store_evidence,
            })
        }
        DesktopHostKind::Cindy => {
            let mut adapter = match runtimes.cindy.clone() {
                Some(rt) => CindyAdapter::with_runtime(projects_root, state_root, rt)?,
                None => CindyAdapter::new(projects_root, state_root)?,
            };
            let project_dir = adapter.ensure_project_dir(&task.slug)?;
            let sidebar_project =
                adapter.ensure_sidebar_project(&task.slug, Some(&task.title))?;
            let create_req = CreateSessionRequest {
                delivery_id: create_delivery_id,
                project_id: sidebar_project.project_id.clone(),
                project_dir: sidebar_project.project_dir.clone(),
                title: task.title.clone(),
                prompt: initial_prompt.clone(),
                model: task.model.clone(),
            };
            let created = adapter.create_cindy_session(&create_req)?;
            let followup_req = DeliverTurnRequest {
                delivery_id: followup_delivery_id,
                project_id: sidebar_project.project_id.clone(),
                session_id: created.session_id.clone(),
                project_dir: sidebar_project.project_dir.clone(),
                prompt: followup_prompt.clone(),
                model: task.model.clone(),
            };
            let followup = adapter.deliver_cindy_turn(&followup_req)?;
            let turn_ids = vec![created.turn_id.clone(), followup.turn_id.clone()];
            let store_evidence = if runtimes.cindy.is_some() {
                let session_snapshot = adapter.read_cindy_session(&followup.session_key)?;
                let entries = read_cindy_transcript(&session_snapshot.transcript_path)?;
                HostStoreVerificationEvidence::Cindy {
                    session_snapshot,
                    transcript_entry_count: entries.len(),
                    turn_ids,
                }
            } else {
                let persisted = adapter
                    .session(&followup.session_id)
                    .map(|s| s.turns.len())
                    .unwrap_or(0);
                HostStoreVerificationEvidence::LocalState {
                    persisted_turn_count: persisted,
                    turn_ids,
                }
            };
            Ok(HostRelayClosureReceipt {
                host,
                slice_id: task.slice_id.clone(),
                rendered_initial_prompt: initial_prompt,
                followup_prompt,
                project_dir,
                sidebar_project,
                initial_session: created.into(),
                followup_turn: followup.into(),
                store_evidence,
            })
        }
        DesktopHostKind::DeepseekHarness => {
            let mut adapter = match runtimes.dsh.clone() {
                Some(rt) => DeepSeekHarnessAdapter::with_runtime(projects_root, state_root, rt)?,
                None => DeepSeekHarnessAdapter::new(projects_root, state_root)?,
            };
            let project_dir = adapter.ensure_project_dir(&task.slug)?;
            let sidebar_project =
                adapter.ensure_sidebar_project(&task.slug, Some(&task.title))?;
            let create_req = CreateSessionRequest {
                delivery_id: create_delivery_id,
                project_id: sidebar_project.project_id.clone(),
                project_dir: sidebar_project.project_dir.clone(),
                title: task.title.clone(),
                prompt: initial_prompt.clone(),
                model: task.model.clone(),
            };
            let created = adapter.create_dsh_session(&create_req)?;
            let followup_req = DeliverTurnRequest {
                delivery_id: followup_delivery_id,
                project_id: sidebar_project.project_id.clone(),
                session_id: created.session_id.clone(),
                project_dir: sidebar_project.project_dir.clone(),
                prompt: followup_prompt.clone(),
                model: task.model.clone(),
            };
            let followup = adapter.deliver_dsh_turn(&followup_req)?;
            let turn_ids = vec![created.turn_id.clone(), followup.turn_id.clone()];
            let store_evidence = if runtimes.dsh.is_some() {
                let session_snapshot = adapter.read_dsh_session(&followup.session_id)?;
                HostStoreVerificationEvidence::DeepseekHarness {
                    session_snapshot,
                    turn_ids,
                }
            } else {
                let persisted = adapter
                    .session(&followup.session_id)
                    .map(|s| s.turns.len())
                    .unwrap_or(0);
                HostStoreVerificationEvidence::LocalState {
                    persisted_turn_count: persisted,
                    turn_ids,
                }
            };
            Ok(HostRelayClosureReceipt {
                host,
                slice_id: task.slice_id.clone(),
                rendered_initial_prompt: initial_prompt,
                followup_prompt,
                project_dir,
                sidebar_project,
                initial_session: created.into(),
                followup_turn: followup.into(),
                store_evidence,
            })
        }
        DesktopHostKind::AntigravityDesktop => {
            let mut adapter = match runtimes.antigravity.clone() {
                Some(rt) => {
                    AntigravityDesktopAdapter::with_runtime(projects_root, state_root, rt)?
                }
                None => AntigravityDesktopAdapter::new(projects_root, state_root)?,
            };
            let project_dir = adapter.ensure_project_dir(&task.slug)?;
            let sidebar_project =
                adapter.ensure_sidebar_project(&task.slug, Some(&task.title))?;
            let create_req = CreateSessionRequest {
                delivery_id: create_delivery_id,
                project_id: sidebar_project.project_id.clone(),
                project_dir: sidebar_project.project_dir.clone(),
                title: task.title.clone(),
                prompt: initial_prompt.clone(),
                model: task.model.clone(),
            };
            let created = adapter.create_antigravity_session(&create_req)?;
            let followup_req = DeliverTurnRequest {
                delivery_id: followup_delivery_id,
                project_id: sidebar_project.project_id.clone(),
                session_id: created.conversation_id.clone(),
                project_dir: sidebar_project.project_dir.clone(),
                prompt: followup_prompt.clone(),
                model: task.model.clone(),
            };
            let followup = adapter.deliver_antigravity_turn(&followup_req)?;
            let turn_ids = vec![created.turn_id.clone(), followup.turn_id.clone()];
            let store_evidence = if runtimes.antigravity.is_some() {
                let summary = adapter
                    .read_conversation_summary(&followup.conversation_id)?
                    .ok_or_else(|| {
                        HostAdapterError::AntigravityProtocol(format!(
                            "missing conversation {} in conversation_summaries.db during relay verification",
                            followup.conversation_id
                        ))
                    })?;
                HostStoreVerificationEvidence::AntigravityDesktop {
                    summary,
                    hub_http_port: followup.hub_http_port,
                    turn_ids,
                }
            } else {
                let persisted = adapter
                    .session(&followup.session_id)
                    .map(|s| s.turns.len())
                    .unwrap_or(0);
                HostStoreVerificationEvidence::LocalState {
                    persisted_turn_count: persisted,
                    turn_ids,
                }
            };
            Ok(HostRelayClosureReceipt {
                host,
                slice_id: task.slice_id.clone(),
                rendered_initial_prompt: initial_prompt,
                followup_prompt,
                project_dir,
                sidebar_project,
                initial_session: created.into(),
                followup_turn: followup.into(),
                store_evidence,
            })
        }
        DesktopHostKind::Zcode => {
            let mut adapter = match runtimes.zcode.clone() {
                Some(rt) => ZcodeAdapter::with_runtime(projects_root, state_root, rt)?,
                None => ZcodeAdapter::new(projects_root, state_root)?,
            };
            let project_dir = adapter.ensure_project_dir(&task.slug)?;
            let sidebar_project =
                adapter.ensure_sidebar_project(&task.slug, Some(&task.title))?;
            let create_req = CreateSessionRequest {
                delivery_id: create_delivery_id,
                project_id: sidebar_project.project_id.clone(),
                project_dir: sidebar_project.project_dir.clone(),
                title: task.title.clone(),
                prompt: initial_prompt.clone(),
                model: task.model.clone(),
            };
            let created = adapter.create_zcode_session(&create_req)?;
            let followup_req = DeliverTurnRequest {
                delivery_id: followup_delivery_id,
                project_id: sidebar_project.project_id.clone(),
                session_id: created.session_id.clone(),
                project_dir: sidebar_project.project_dir.clone(),
                prompt: followup_prompt.clone(),
                model: task.model.clone(),
            };
            let followup = adapter.deliver_zcode_turn(&followup_req)?;
            let turn_ids = vec![created.turn_id.clone(), followup.turn_id.clone()];
            let store_evidence = if runtimes.zcode.is_some() {
                let project_row = adapter
                    .read_project_row(&sidebar_project.project_id)?
                    .ok_or_else(|| {
                        HostAdapterError::ZcodeProtocol(format!(
                            "missing project {} in tasks-index.sqlite during relay verification",
                            sidebar_project.project_id
                        ))
                    })?;
                let session_snapshot = adapter
                    .read_zcode_session(&followup.session_id)?
                    .ok_or_else(|| {
                        HostAdapterError::ZcodeProtocol(format!(
                            "missing session {} in ZCode SQLite stores during relay verification",
                            followup.session_id
                        ))
                    })?;
                HostStoreVerificationEvidence::Zcode {
                    project_row,
                    session_snapshot,
                    turn_ids,
                }
            } else {
                let persisted = adapter
                    .session(&followup.session_id)
                    .map(|s| s.turns.len())
                    .unwrap_or(0);
                HostStoreVerificationEvidence::LocalState {
                    persisted_turn_count: persisted,
                    turn_ids,
                }
            };
            Ok(HostRelayClosureReceipt {
                host,
                slice_id: task.slice_id.clone(),
                rendered_initial_prompt: initial_prompt,
                followup_prompt,
                project_dir,
                sidebar_project,
                initial_session: created.into(),
                followup_turn: followup.into(),
                store_evidence,
            })
        }
    }
}
