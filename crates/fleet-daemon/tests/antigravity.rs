#![cfg(unix)]

use fleet_daemon::{
    discover_antigravity_hubs, read_conversation_summary, AntigravityDesktopAdapter,
    AntigravityRuntimeConfig, DesktopHostAdapter, HostAdapterError,
};
use fleet_proto::{CreateSessionRequest, DeliverTurnRequest, DeliveryTier, DesktopHostKind};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);

const CONVERSATION_SUMMARIES_DDL: &str = "\
CREATE TABLE `conversation_summaries` (\
`conversation_id` text,\
`title` text NOT NULL DEFAULT \"\",\
`preview` text NOT NULL DEFAULT \"\",\
`step_count` integer NOT NULL DEFAULT 0,\
`last_modified_time` datetime NOT NULL,\
`workspace_uris` text NOT NULL,\
`status` text NOT NULL DEFAULT \"\",\
`source` text NOT NULL DEFAULT \"\",\
`project_id` text NOT NULL DEFAULT \"\",\
`agent_name` text NOT NULL DEFAULT \"\",\
`parent_conversation_id` text NOT NULL DEFAULT \"\",\
`nesting_depth` integer NOT NULL DEFAULT 0,\
`battle_id` text NOT NULL DEFAULT \"\",\
`winning_conversation_id` text NOT NULL DEFAULT \"\",\
`not_fully_idle` numeric NOT NULL DEFAULT false,\
`killed` numeric NOT NULL DEFAULT false,\
`last_user_input_time` datetime NOT NULL,\
`last_user_input_step_index` integer NOT NULL DEFAULT -1,\
`app_data_dir` text NOT NULL DEFAULT \"\",\
`raw_summary` blob,\
`group_id` text NOT NULL DEFAULT \"\",\
PRIMARY KEY (`conversation_id`));\
CREATE INDEX `idx_conversation_summaries_last_user_input_time` ON `conversation_summaries`(`last_user_input_time`);\
CREATE INDEX `idx_conversation_summaries_last_modified_time` ON `conversation_summaries`(`last_modified_time`);\
";

struct AntigravityFixture {
    root: PathBuf,
    projects_root: PathBuf,
    state_root: PathBuf,
    antigravity_root: PathBuf,
    daemon_dir: PathBuf,
    db_path: PathBuf,
    agy_bin: PathBuf,
    invocations_log: PathBuf,
}

impl AntigravityFixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "fleet-antigravity-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let projects_root = root.join("Projects");
        let state_root = root.join("state");
        let antigravity_root = root.join(".gemini/antigravity");
        let daemon_dir = antigravity_root.join("daemon");
        let presence_dir = antigravity_root.join("presence");
        fs::create_dir_all(&projects_root).unwrap();
        fs::create_dir_all(&state_root).unwrap();
        fs::create_dir_all(&daemon_dir).unwrap();
        fs::create_dir_all(&presence_dir).unwrap();

        let db_path = antigravity_root.join("conversation_summaries.db");
        let status = Command::new("/usr/bin/sqlite3")
            .arg(&db_path)
            .arg(CONVERSATION_SUMMARIES_DDL)
            .status()
            .unwrap();
        assert!(status.success(), "failed to initialize conversation_summaries.db");

        // Seed an unscoped CLI conversation row ("default-cli-project") to ensure our adapter
        // never confuses unscoped rows with sidebar-bound conversations.
        let seed_sql = "\
INSERT INTO conversation_summaries \
(conversation_id, title, preview, step_count, last_modified_time, workspace_uris, status, source, project_id, last_user_input_time) \
VALUES ('cli-unscoped-0001', '', 'unscoped cli prompt', 1, '2026-11-05T00:00:00Z', '[]', 'CASCADE_RUN_STATUS_IDLE', 'CORTEX_TRAJECTORY_SOURCE_CLI', 'default-cli-project', '2026-11-05T00:00:00Z');";
        assert!(Command::new("/usr/bin/sqlite3")
            .arg(&db_path)
            .arg(seed_sql)
            .status()
            .unwrap()
            .success());

        // Write hub_52019.json matching ~/.gemini/antigravity/daemon/hub_<port>.json schema.
        let hub_json = serde_json::json!({
            "pid": 42420,
            "httpsPort": 52018,
            "httpPort": 52019,
            "lsVersion": "1.28.0-antigravity",
            "csrfToken": "secret-csrf-never-in-receipt-998877"
        });
        fs::write(
            daemon_dir.join("hub_52019.json"),
            serde_json::to_vec_pretty(&hub_json).unwrap(),
        )
        .unwrap();

        let invocations_log = root.join("agy_invocations.jsonl");
        let agy_bin = root.join("mock_agy.py");
        write_mock_agy_script(&agy_bin, &invocations_log);

        Self {
            root,
            projects_root,
            state_root,
            antigravity_root,
            daemon_dir,
            db_path,
            agy_bin,
            invocations_log,
        }
    }

    fn runtime_config(&self) -> AntigravityRuntimeConfig {
        AntigravityRuntimeConfig::new(self.antigravity_root.clone(), self.agy_bin.clone())
    }
}

impl Drop for AntigravityFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn write_mock_agy_script(script_path: &Path, invocations_log: &Path) {
    let script = format!(
        r#"#!/usr/bin/env python3
import hashlib
import json
import os
import sqlite3
import sys
from pathlib import Path

log_path = Path({log_path:?})
db_path = Path(os.environ["ANTIGRAVITY_SUMMARIES_DB"])
args = sys.argv[1:]

record = {{
    "cwd": os.getcwd(),
    "args": args,
    "hub_http_port": os.environ.get("ANTIGRAVITY_HUB_HTTP_PORT"),
}}
with log_path.open("a", encoding="utf-8") as f:
    f.write(json.dumps(record) + "\n")

new_project = "--new-project" in args
project_arg = None
conversation_arg = None
add_dirs = []
prompt_arg = None
model_arg = None

i = 0
while i < len(args):
    a = args[i]
    if a == "--project" and i + 1 < len(args):
        project_arg = args[i + 1]
        i += 2
    elif a == "--conversation" and i + 1 < len(args):
        conversation_arg = args[i + 1]
        i += 2
    elif a == "--add-dir" and i + 1 < len(args):
        add_dirs.append(args[i + 1])
        i += 2
    elif a in ("-p", "--print", "--prompt") and i + 1 < len(args):
        prompt_arg = args[i + 1]
        i += 2
    elif a == "--model" and i + 1 < len(args):
        model_arg = args[i + 1]
        i += 2
    else:
        i += 1

if new_project and prompt_arg is None:
    digest = hashlib.sha256(f"{{project_arg}}:{{add_dirs}}".encode()).hexdigest()[:12]
    project_id = f"agy-proj-{{digest}}"
    print(json.dumps({{
        "project_id": project_id,
        "title": project_arg or "",
        "workspace_uris": [f"file://{{Path(d).resolve()}}" for d in add_dirs],
    }}))
    sys.exit(0)

conn = sqlite3.connect(str(db_path))
cur = conn.cursor()

# Simulate real Antigravity CLI behavior: if --project is omitted or set to
# force-unscoped-bucket, it falls into "default-cli-project".
effective_project_id = project_arg if project_arg else "default-cli-project"
if os.environ.get("MOCK_AGY_FORCE_DEFAULT_BUCKET") == "1":
    effective_project_id = "default-cli-project"

workspace_uris = json.dumps([f"file://{{Path(d).resolve()}}" for d in add_dirs])

if conversation_arg is None:
    digest = hashlib.sha256(f"{{effective_project_id}}:{{prompt_arg}}:{{len(add_dirs)}}".encode()).hexdigest()[:16]
    conv_id = f"conv-{{digest}}"
    turn_id = f"turn-{{digest}}-1"
    cur.execute(
        """
        INSERT OR REPLACE INTO conversation_summaries
        (conversation_id, title, preview, step_count, last_modified_time, workspace_uris,
         status, source, project_id, last_user_input_time, last_user_input_step_index, app_data_dir)
        VALUES (?, ?, ?, ?, '2026-11-05T10:30:00Z', ?, 'CASCADE_RUN_STATUS_IDLE', '', ?, '2026-11-05T10:30:00Z', 0, 'antigravity')
        """,
        (conv_id, prompt_arg[:32] if prompt_arg else "", prompt_arg or "", 2, workspace_uris, effective_project_id),
    )
    conn.commit()
    conn.close()
    print(json.dumps({{
        "conversation_id": conv_id,
        "turn_id": turn_id,
        "project_id": effective_project_id,
    }}))
else:
    cur.execute(
        "SELECT step_count, project_id, workspace_uris FROM conversation_summaries WHERE conversation_id = ?",
        (conversation_arg,),
    )
    row = cur.fetchone()
    if row is None:
        conn.close()
        sys.stderr.write(f"unknown conversation: {{conversation_arg}}\n")
        sys.exit(2)
    next_steps = int(row[0]) + 2
    cur.execute(
        """
        UPDATE conversation_summaries
        SET preview = ?, step_count = ?, last_modified_time = '2026-11-05T10:31:00Z',
            workspace_uris = ?, project_id = ?
        WHERE conversation_id = ?
        """,
        (prompt_arg or "", next_steps, workspace_uris, effective_project_id, conversation_arg),
    )
    conn.commit()
    conn.close()
    turn_id = f"turn-{{conversation_arg}}-{{next_steps}}"
    print(json.dumps({{
        "conversation_id": conversation_arg,
        "turn_id": turn_id,
        "project_id": effective_project_id,
    }}))
"#,
        log_path = invocations_log.display()
    );
    fs::write(script_path, script).unwrap();
    let mut perms = fs::metadata(script_path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(script_path, perms).unwrap();
}

#[test]
fn antigravity_discovers_hub_and_redacts_csrf_token_from_debug_and_receipts() {
    let f = AntigravityFixture::new();
    let hubs = discover_antigravity_hubs(&f.daemon_dir).unwrap();
    assert_eq!(hubs.len(), 1);
    let hub = &hubs[0];
    assert_eq!(hub.pid, 42420);
    assert_eq!(hub.https_port, 52018);
    assert_eq!(hub.http_port, 52019);
    assert_eq!(hub.ls_version, "1.28.0-antigravity");
    assert_eq!(
        hub.csrf_token_in_memory(),
        "secret-csrf-never-in-receipt-998877"
    );
    let debug_repr = format!("{hub:?}");
    assert!(
        !debug_repr.contains("secret-csrf-never-in-receipt-998877"),
        "CSRF token must never leak into Debug output: {debug_repr}"
    );
    assert!(debug_repr.contains("<redacted>"));
}

#[test]
fn antigravity_creates_project_delivers_and_resumes_conversation_verified_in_summaries_db() {
    let f = AntigravityFixture::new();
    let mut adapter = AntigravityDesktopAdapter::with_runtime(
        f.projects_root.clone(),
        f.state_root.clone(),
        f.runtime_config(),
    )
    .unwrap();

    // 1. Discover Hub daemon
    let hubs = adapter.discover_hubs().unwrap();
    assert_eq!(hubs.len(), 1);
    assert_eq!(hubs[0].http_port, 52019);

    // 2. Create project directory & register sidebar project via `agy --new-project --project ... --add-dir ...`
    let binding = adapter
        .ensure_sidebar_project("self-dev-agy", Some("Antigravity Self Dev"))
        .unwrap();
    assert_eq!(binding.host, DesktopHostKind::AntigravityDesktop);
    assert!(binding.project_dir.is_dir());
    assert_eq!(binding.project_dir, f.projects_root.join("self-dev-agy"));
    assert!(
        binding.project_id.starts_with("agy-proj-"),
        "expected project_id from agy --new-project, got {}",
        binding.project_id
    );
    assert_ne!(binding.project_id, "default-cli-project");
    assert_ne!(binding.project_id, "outside-of-project");
    let expected_uri = format!("file://{}", binding.project_dir.display());
    assert_eq!(binding.workspace_uris, vec![expected_uri.clone()]);

    // 3. Create sidebar-visible conversation and verify `conversation_summaries.db` readback
    let create_req = CreateSessionRequest {
        delivery_id: "agy-deliv-create-1".into(),
        project_id: binding.project_id.clone(),
        project_dir: binding.project_dir.clone(),
        title: "Slice J Antigravity Session".into(),
        prompt: "Create initial module and verify workspace binding.".into(),
        model: Some("gemini-3-pro".into()),
    };
    let created = adapter.create_antigravity_session(&create_req).unwrap();
    assert_eq!(created.host, DesktopHostKind::AntigravityDesktop);
    assert_eq!(created.project_id, binding.project_id);
    assert!(!created.conversation_id.is_empty());
    assert_eq!(created.session_id, created.conversation_id);
    assert_eq!(created.project_dir, binding.project_dir);
    assert_eq!(created.workspace_uris, vec![expected_uri.clone()]);
    assert_eq!(created.hub_http_port, Some(52019));
    assert_eq!(created.tier, DeliveryTier::ThreadVisible);
    assert_eq!(created.step_count, 2);

    // Verify receipt JSON contains both conversation_id and workspace_uris and no CSRF token
    let receipt_json = serde_json::to_string(&created).unwrap();
    assert!(receipt_json.contains("\"conversation_id\""));
    assert!(receipt_json.contains("\"workspace_uris\""));
    assert!(!receipt_json.contains("secret-csrf-never-in-receipt-998877"));

    // Read directly from conversation_summaries.db and verify row binding
    let db_row = read_conversation_summary(
        Path::new("/usr/bin/sqlite3"),
        &f.db_path,
        &created.conversation_id,
    )
    .unwrap()
    .expect("conversation row must exist in conversation_summaries.db");
    assert_eq!(db_row.project_id, binding.project_id);
    assert_ne!(db_row.project_id, "default-cli-project");
    assert_eq!(db_row.workspace_uris, vec![expected_uri.clone()]);
    assert_eq!(
        db_row.preview,
        "Create initial module and verify workspace binding."
    );

    // 4. Resume same conversation via `agy --conversation <id> --project <id> --add-dir <dir>`
    let followup_req = DeliverTurnRequest {
        delivery_id: "agy-deliv-turn-2".into(),
        project_id: binding.project_id.clone(),
        session_id: created.conversation_id.clone(),
        project_dir: binding.project_dir.clone(),
        prompt: "Run follow-up verification in the same Antigravity conversation.".into(),
        model: None,
    };
    let followup = adapter.deliver_antigravity_turn(&followup_req).unwrap();
    assert_eq!(followup.conversation_id, created.conversation_id);
    assert_eq!(followup.project_id, binding.project_id);
    assert_eq!(followup.workspace_uris, vec![expected_uri.clone()]);
    assert_eq!(followup.step_count, 4);
    assert_ne!(followup.turn_id, created.turn_id);

    let db_row_after = adapter
        .read_conversation_summary(&created.conversation_id)
        .unwrap()
        .unwrap();
    assert_eq!(db_row_after.step_count, 4);
    assert_eq!(db_row_after.project_id, binding.project_id);
    assert_eq!(
        db_row_after.preview,
        "Run follow-up verification in the same Antigravity conversation."
    );

    // 5. Inspect logged `agy` CLI invocations to confirm exact flags and working directory
    let log_lines: Vec<serde_json::Value> = fs::read_to_string(&f.invocations_log)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(log_lines.len(), 3);
    assert!(log_lines[0]["args"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v == "--new-project"));
    assert!(log_lines[1]["args"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v == "--project"));
    assert!(log_lines[2]["args"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v == "--conversation"));

    println!(
        "ANTIGRAVITY_VERIFIED project_id={} conversation_id={} workspace_uris={:?} steps={}",
        followup.project_id, followup.conversation_id, followup.workspace_uris, followup.step_count
    );
}

#[test]
fn antigravity_rejects_unscoped_default_cli_bucket_in_summaries_db() {
    let f = AntigravityFixture::new();
    // Rewrite mock_agy.py to simulate a buggy/unscoped invocation that writes "default-cli-project"
    // into conversation_summaries.db despite being asked for a project.
    let broken_script = r#"#!/usr/bin/env python3
import json, os, sqlite3, sys
from pathlib import Path

args = sys.argv[1:]
if "--new-project" in args and "-p" not in args:
    print(json.dumps({"project_id": "agy-proj-valid-999"}))
    sys.exit(0)

db_path = Path(os.environ["ANTIGRAVITY_SUMMARIES_DB"])
conn = sqlite3.connect(str(db_path))
cur = conn.cursor()
conv_id = "conv-fell-into-default-bucket"
cur.execute(
    """
    INSERT OR REPLACE INTO conversation_summaries
    (conversation_id, title, preview, step_count, last_modified_time, workspace_uris,
     status, source, project_id, last_user_input_time)
    VALUES (?, 'unscoped', 'prompt', 1, '2026-11-05T10:35:00Z', '[]',
            'CASCADE_RUN_STATUS_IDLE', 'CORTEX_TRAJECTORY_SOURCE_CLI', 'default-cli-project', '2026-11-05T10:35:00Z')
    """,
    (conv_id,),
)
conn.commit()
conn.close()
print(json.dumps({"conversation_id": conv_id, "turn_id": "turn-bad-1"}))
"#;
    fs::write(&f.agy_bin, broken_script).unwrap();

    let mut adapter = AntigravityDesktopAdapter::with_runtime(
        f.projects_root.clone(),
        f.state_root.clone(),
        f.runtime_config(),
    )
    .unwrap();
    let binding = adapter
        .ensure_sidebar_project("bucket-guard", Some("Bucket Guard"))
        .unwrap();

    let err = adapter
        .create_antigravity_session(&CreateSessionRequest {
            delivery_id: "deliv-bucket-guard".into(),
            project_id: binding.project_id.clone(),
            project_dir: binding.project_dir.clone(),
            title: "Should Reject Default Bucket".into(),
            prompt: "Verify that falling into default-cli-project is caught.".into(),
            model: None,
        })
        .unwrap_err();
    assert!(
        matches!(err, HostAdapterError::ForbiddenProjectId(ref id) if id == "default-cli-project"),
        "expected ForbiddenProjectId(default-cli-project), got {err:?}"
    );
    assert!(
        adapter
            .existing_session("conv-fell-into-default-bucket")
            .is_none(),
        "rejected unscoped session must not be committed to adapter state"
    );
}

#[test]
fn select_matching_antigravity_project_prefers_exact_name_or_basename_match() {
    use fleet_daemon::select_matching_antigravity_project;
    use serde_json::json;

    let workspace_uri = "file:///workspace/Projects/agent-fleet";
    let projects = vec![
        json!({
            "id": "d706d778-76e8-49fe-af53-2685e78b7a84",
            "name": "antigravity-conversation-friction-audit",
            "projectResources": {
                "resources": [{ "gitFolder": { "folderUri": workspace_uri } }]
            }
        }),
        json!({
            "id": "0fce9b2d-91fc-4b89-8117-692dcb3d8749",
            "name": "antigravity-routine-hygiene-audit",
            "projectResources": {
                "resources": [{ "gitFolder": { "folderUri": workspace_uri } }]
            }
        }),
        json!({
            "id": "2f9dc567-62d4-4d4e-9764-877e9d6f694c",
            "name": "agent-fleet",
            "projectResources": {
                "resources": [{ "gitFolder": { "folderUri": workspace_uri } }]
            }
        }),
    ];

    let selected = select_matching_antigravity_project(
        &projects,
        workspace_uri,
        "agent-fleet",
        "agent-fleet",
    );
    assert_eq!(
        selected.as_deref(),
        Some("2f9dc567-62d4-4d4e-9764-877e9d6f694c")
    );

    let fallback = select_matching_antigravity_project(
        &projects[..2],
        workspace_uri,
        "agent-fleet",
        "agent-fleet",
    );
    assert_eq!(
        fallback.as_deref(),
        Some("d706d778-76e8-49fe-af53-2685e78b7a84")
    );
}
