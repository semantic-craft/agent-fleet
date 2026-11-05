# DESIGN.md: Hybrid Agent Network + Reason Network Engine

`agent-fleet` is an open-source Rust coordination engine that synthesizes two upstream architectures into one verifiable system:

1. **[sleep2agi/agent-network](https://github.com/sleep2agi/agent-network) (ANet)**: cryptographically correlated task ledgers, single-channel operation claims, anonymous status readback (`POST /relay/send` and `POST /relay/status` on Wire v3), and explicit delivery tiers.
2. **[reason-machines/reason-network](https://github.com/reason-machines/reason-network)**: live session directory discovery, `cwd`-follows-session process pinning, and direct injection into open desktop project threads so dispatched work appears under the left sidebar project (`~/Projects/<slug>`).

```
+-----------------------------------------------------------------------------------+
|                      Any Machine Running agent-fleet (`fleet-cli`)                |
|                                                                                   |
|   External Open-Source Agents          CLI / Automation        ANet Hub Relay     |
|   (Claude Code, OpenCode, Cursor, ...) (`who`, `send`, `relay`) (`fleet-hub`)     |
|                 |                               |                     |           |
|                 +---------------+---------------+---------------------+           |
|                                 |                                                 |
|                        [fleet-cli mcp / send]                                     |
|                                 |                                                 |
|        +------------------------+------------------------+                        |
|        |                        |                        |                        |
|        v                        v                        v                        |
|  +-----------+           +-------------+          +--------------+                |
|  |Project Dir|           |Host Adapter |          |External Agent|                |
|  |Boundary   |           |5 Desktop UIs|          |Registry      |                |
|  |~/Projects |           |Sidebar Sync |          |agents.d/*.json                |
|  +-----------+           +-------------+          +--------------+                |
|                                 |                                                 |
|     +-------------+-------------+-------------+-------------+                     |
|     |             |             |             |             |                     |
|     v             v             v             v             v                     |
| Antigravity   OpenAI Dots   DeepSeek      Cindy         ZCode                     |
| Desktop       (Codex App)   Harness       Desktop       Desktop                   |
+-----------------------------------------------------------------------------------+
```

## 1. Five Canonical Desktop Agents and Sidebar Storage Contracts

Every canonical desktop host implements the `DesktopHostAdapter` trait in `crates/fleet-daemon/src/host_adapter.rs`. Before any session is created or continued, the adapter validates `<projects_root>/<slug>` (rejecting path traversal, symlinks, hidden/cache directories, and out-of-bounds paths) and registers the project in the host's native sidebar store.

| Host Kind (`DesktopHostKind`) | Aliases | Sidebar Project Store | Session / Thread Store & Transport | Verification Readback |
| --- | --- | --- | --- | --- |
| `antigravity-desktop` | `agy`, `antigravity` | `~/.gemini/antigravity/daemon/hub_<port>.json` (`CreateProject`) | `StartCascade` + `SendUserCascadeMessage` (`source=""`, `desktop_body=true`) or `agy` CLI | `~/.gemini/antigravity/conversation_summaries.db` (`project_id` and `workspace_uris`) |
| `openai-dots` | `codex`, `codex-desktop` | `~/.codex/state_5.sqlite` (`projects` + `project_roots`) and `.codex-global-state.json` | `codex app-server` Unix WebSocket (`project/create`, `thread/start`, `thread/resume`, `turn/start`) | `state_5.sqlite` (`threads.project_id`, `cwd`, `archived = 0`, non-empty `preview`) |
| `deepseek-harness` | `dsh` | `~/.dsh/storages/workspace.json` (`tables.workspaces.<uuid>.sessionIds`) | `dsh --profile headless --json` + two-frame `session.v4.jsonl.zstd` + `session_projcache` | `workspace.json` + `session_projcache/sessions/<id>.json` + `session.v4.jsonl.zstd` |
| `cindy` | `cindy-desktop` | Cindy Gateway Unix WebSocket (`sessions.patch`) | `sessions.create` + `cindy_session_send` + `~/.cindy/transcripts/<sessionKey>.jsonl` | `sessions.get` + transcript JSONL entry count and `cwd` match |
| `zcode` | `zcode-desktop` | `~/.zcode/v2/tasks-index.sqlite` (`hosts`, `projects`, `project_roots`) | `zcode --cwd <dir> --surface desktop --json` + `tasks-index.sqlite` (`tasks`) + `cli/db/db.sqlite` (`session`, `message`) | `tasks-index.sqlite` (`tasks.project_id`, `root_path`, `archived = 0`) + `cli/db/db.sqlite` (`message_count >= 1`) |

## 2. Core Invariants

### 2.1 Project Root and `cwd`-Follows-Session Boundary
- Dispatched tasks never run in an arbitrary daemon working directory or a hidden cache directory (`.cache`, `.anet`, `PRIVATE`).
- `resolve_project_dir_in_root` enforces that `<projects_root>` is an absolute path and `<slug>` is a single safe path segment (`[A-Za-z0-9._-]`).
- `NodeDaemon` (`crates/fleet-daemon/src/execution.rs`) pins the verified project directory via an open directory file descriptor (`fchdir`) before spawning child processes.

### 2.2 Single-Channel Correlation and Delivery Tier Lattice
- Every dispatch requires a unique `delivery_id`. Re-submitting a claimed `delivery_id` on the same adapter or Hub journal is rejected (`DuplicateDelivery`).
- `DeliveryTier` (`crates/fleet-proto/src/delivery.rs`) separates transport acceptance from UI visibility and task completion:
  - `Accepted`: recorded in local state or Hub queue without live runtime store proof.
  - `ThreadVisible`: verified in the target desktop host's native sidebar store (`conversation_summaries.db`, `state_5.sqlite`, `workspace.json` + `session_projcache`, Cindy transcript, or ZCode `tasks-index.sqlite` + `cli/db/db.sqlite`).
  - `Running` / `Completed` / `Failed` / `Unknown`: derived from correlated native turn or Hub receipts, never fabricated from HTTP 200 alone.

### 2.3 Dispatcher Model Ownership (`BriefPolicy`)
- Model selection belongs to the dispatcher configuration (`--model` or host config such as Codex Magpie `zcode/GLM-5.3-Flash`).
- `validate_brief` (`crates/fleet-proto/src/brief.rs`) rejects prompts that ask a worker agent to self-attest its model identity or abort with `MODEL_WRONG`.

## 3. Open-Source Extensibility (`agents.d` and MCP)

### 3.1 Standard MCP Server (`fleet-cli mcp`)
Any MCP-capable client (Claude Code, Cursor, OpenCode, Gemini CLI, Zed, or custom orchestrators) can wire `fleet-cli mcp --projects-root <dir> --state-root <dir>` over stdio JSON-RPC 2.0. It exposes four tools:
- `list_team`: enumerates all five canonical desktop hosts, their sidebar projects and active sessions, live `SessionDirectory` entries, and registered external agents.
- `send_message`: dispatches a new task or same-session follow-up turn to `<agent>/<project-slug>` (`antigravity-desktop`, `openai-dots`, `deepseek-harness`, `cindy`, `zcode`, or any external agent ID).
- `message_status`: reads back the correlated session and turn history for a host and `session_id`.
- `register_agent`: registers a third-party open-source agent manifest in `<state_root>/agents.d/<id>.json`.

### 3.2 Pluggable External Agent Manifests (`agents.d/<id>.json`)
Third-party open-source agents can join the fleet without modifying Rust source code by registering an `ExternalAgentManifest`:

```json
{
  "id": "open-coder",
  "display_name": "OpenCoder CLI",
  "command": "/usr/local/bin/open-coder",
  "args_template": ["--cwd", "{cwd}", "--session", "{session_id}", "-p", "{prompt}"],
  "resume_args_template": ["--cwd", "{cwd}", "--resume", "{session_id}", "-p", "{prompt}"],
  "description": "Community open-source coding agent"
}
```

Template placeholders `{cwd}`, `{slug}`, `{session_id}`, `{delivery_id}`, `{title}`, and `{prompt}` are expanded against the verified `<projects_root>/<slug>` directory at dispatch time.
