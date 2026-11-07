# Unify agent-fleet and Agents-Anywhere in Rust

Unify `agent-fleet` (`sleep2agi/agent-network` + `reason-machines/reason-network`) with `anywhere-labs/Agents-Anywhere` (`v2` workbench protocol) into one pure-Rust coordination, timeline, and self-hosted workbench engine, eliminating the Python connector and Python FastAPI server.
End users and peer agents gain a single Rust runtime on the workstation (`fleet-daemon` + `fleet-cli`) and a single Rust server binary (`fleet-workbench-server`) for Alibaba Cloud self-hosting that serves the unmodified `web-next` / iOS / Android workbench clients while preserving five-desktop sidebar mounting (`DeliveryTier::ThreadVisible`), `fchdir` workspace containment, and `<fleet-message>` peer dispatch.
The program enforces one rule across every PR, namely zero multi-runtime glue code (`python3` or `uv` subprocesses) in the connector or server data path, and executes `PR-1`, `PR-2`, `PR-3`, `PR-4`, `PR-5`, and `PR-6` in order.

## How to read this

One box is one unit of work. Every box names the evidence that checks it. A nested box is a sub-step of the box above it. Check a box only when its evidence exists, a file, a log line, a screenshot, a test run, or a SHA. The body is a how-to. The appendices explain and record.

The program runs `skills/poteto-mode/playbooks/autopilot-stack.md` from the installed plugin. The operator merges every PR after reviewing its verdict, and `PR-5` and `PR-6` stop at merge-ready for the operator's review gate.

Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

## Program checklist

### Arm the program

- [ ] State the protocol and this plan to the operator, then stop. Start execution only on the operator's explicit go.
- [ ] On the operator's go, write this exact text into `notes.md` in the program's store under `~/.claude/orchestrate/agent-fleet-workbench/` and restate it in your todolist. "Plan path `docs/workbench-unification-plan.md`, PR ids `PR-1` -> `PR-2` -> `PR-3` -> `PR-4` -> `PR-5` -> `PR-6`, verification rule `Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.`, the operator merges each PR bottom-up, and the done condition is all six PRs verified with zero Python runtime glue."
- [ ] Read these from the installed plugin at program start. Re-read them at every tick.
  - [ ] `skills/poteto-mode/playbooks/autopilot-stack.md`
  - [ ] `skills/swarm/SKILL.md`
  - [ ] `skills/verify-fleet/SKILL.md`
  - [ ] `skills/poteto-mode/playbooks/opening-a-pr.md`
  - [ ] `skills/principle-redesign-from-first-principles/SKILL.md`
  - [ ] `skills/principle-model-the-domain/SKILL.md`
  - [ ] `skills/principle-boundary-discipline/SKILL.md`
- [ ] Arm the hourly audit tick as a real `/loop` in dynamic mode, which schedules its own wake-up rather than blocking on a sleep. Never leave the cadence to memory.
- [ ] Use this tick prompt, verbatim. "Re-read the execution playbook from the installed plugin and the objective in the program store's `notes.md`. Audit the operation against both and fix drift in this tick. Probe every active lane and judge progress by side effects only. Stand down a stuck lane and dispatch its replacement now. Then post a short status message to the operator in chat only when the audit found a tracked change that no earlier status message reported, such as a PR opened, a code-ready head, a round launched or closed, a verdict, a merge, a stuck agent and the action taken, a blocker added or cleared, or a decision only the operator can make. Name every such change and nothing else. Do not repeat a table, the merged list, or an unchanged blocker. If the audit found none, end the turn with no reply text. Either way, log this tick's row in your decision trail. The row names the items reported, or none."
- [ ] On the operator's hold or stand-down, send every owner a zero-writes order at once.

### Spawn owners

- [ ] Spawn one owner per PR with the full lifecycle the execution playbook names.
- [ ] Follow this dependency graph. Start dependent work only after its parent merges, or base it on the parent branch when the execution playbook stacks.
  - [ ] `PR-1` is first and branches from `main`.
  - [ ] `PR-2` after `PR-1`.
  - [ ] `PR-3` after `PR-2`.
  - [ ] `PR-4` after `PR-3`.
  - [ ] `PR-5` after `PR-4`.
  - [ ] `PR-6` after `PR-5`.
- [ ] Hold the file boundaries. `PR-1` touches `crates/fleet-proto/**`. `PR-2` touches `crates/fleet-daemon/src/ledger.rs`, `crates/fleet-daemon/src/host_adapter.rs`, and `crates/fleet-daemon/tests/**`. `PR-3` touches `crates/fleet-daemon/src/observer.rs`, `crates/fleet-daemon/src/antigravity.rs`, `crates/fleet-daemon/src/tri_hosts.rs`, and `crates/fleet-daemon/tests/**`. `PR-4` touches `crates/fleet-daemon/src/connector_ws.rs`, `crates/fleet-cli/src/main.rs`, and `crates/fleet-daemon/tests/**`. `PR-5` touches `crates/fleet-workbench-server/**`, `Cargo.toml`, and `docker/**`. `PR-6` touches `crates/fleet-cli/src/main.rs`, `docs/**`, `DESIGN.md`, and `README.md`.
- [ ] Hold the review gate. `PR-5` and `PR-6` change an interaction. They wait for the operator's review in chat with screenshots and a video before merge.

### PR mechanics, for every PR

- [ ] Resolve the forge once. Default to `gh`; if `command -v origin` succeeds and Origin can resolve the repository, use `origin pr` for every PR operation. Record any fallback to `gh`. Never require `gt`.
- [ ] Open the PR ready, never draft, per **Opening a PR**. Use the run's built-in PR tool when it has one, else `origin pr create --status open --base <base-branch>` or `gh pr create --base <base-branch>` according to the resolved forge. A stack child targets its parent branch.
- [ ] Run the repo's lint and typecheck once before the PR-facing push. Push with hooks on.
- [ ] Run `/deslop` before each commit and `/no-comments` before review.
- [ ] Triage every review-bot and security-reviewer comment per `../references/bugbot-triage.md`.
- [ ] Before the code-ready report and babysit, and again after merge prep, record the base and head SHAs prepared by the topology owner under the execution playbook.

### Verdict and merge, for every PR

- [ ] A full swarm runs once, at the code-ready head. Another full swarm runs only for new behavior, a redesign, or a large new diff. A rebase, a merge-conflict resolution, or a minor fix gets a single verifier at the new head, and a wording-only change gets none. Run each full swarm per `skills/swarm/SKILL.md`. One gates lane. The ten live lanes from the PR's **Verify, live** block. The perf lane from its **Verify, perf** block. Two or more audit lanes, each with its own focus, that read the diff and the receipts and distrust the PR body. The root audits the receipts in the merge-ready report before the verdict.
- [ ] Judge every finding by the execution playbook's blocking rule. The verdict is clean when none blocks. A failure on trunk behavior in a lane marked `Regression lane against trunk.` is recorded against trunk and does not block the PR. A new head gets a fresh verdict, except for results that stay valid under the patch-id rule in `playbooks/shipping.md`.
- [ ] The root appends each verified PR to the base-branch stack and the operator lands the contiguous verified run bottom-up under the patch-id rule from `playbooks/shipping.md`.

### Boot recipe, for every live lane

Each live lane runs in its own worktree at the PR head. Drive through the skill path or exact commands recorded in this boot recipe.

- [ ] `git fetch origin <head-branch> && git checkout <head SHA>`.
- [ ] Run `./scripts/check` and `cargo build --workspace` to build `fleet-cli` and any server binary in the lane's isolated target dir. Wait for ready.
- [ ] Deliver input only through `skills/verify-fleet/SKILL.md` commands (`fleet-cli doctor --gate`, `fleet-cli send`, `fleet-cli mcp`, and loopback HTTP/WebSocket probes). Read-only diagnostics are `fleet-cli who`, `fleet-cli inbox`, and `sqlite3 -readonly`.
- [ ] Save every screenshot to `/tmp/swarm-<pr-id>/worker-<n>/<slug>.png` and return the paths with the report.

## Model unified workbench and timeline domain in fleet-proto (PR-1)

**Depends on.** None.

**Files.**

- [ ] Create `crates/fleet-proto/src/timeline.rs`.
- [ ] Create `crates/fleet-proto/src/workbench.rs`.
- [ ] Edit `crates/fleet-proto/src/lib.rs`.
- [ ] Edit `crates/fleet-proto/src/delivery.rs`.
- [ ] Edit `crates/fleet-proto/src/host.rs`.

**Build.**

- [ ] Define `TimelineItem`, `TimelineItemKind`, `TimelineOrderSeq` (32-bit signed integer bounded to `-2_147_483_648..=2_147_483_647`), `UpdatedSeq` (`i64`), and `timeline_content_hash` (with lone-surrogate `\udXXX` escaping) in `crates/fleet-proto/src/timeline.rs`.
- [ ] Define `stable_runtime_session_id` (`sess_{runtime}_{sha256(connector_id:runtime:external_session_id)[:24]}`) and `turn_position_item_id` in `crates/fleet-proto/src/timeline.rs` so live events and replayed desktop histories produce identical item IDs.
- [ ] Define `ProtocolEventEnvelope`, `EffectiveCapabilities`, `SessionNotice`, `InteractionStatus` (`Open`, `ResponseAccepted`, `Resolving`, `Resolved`), `PlanReviewTicket` (`review_id`, `plan_id`, `plan_revision`, `content_hash`, `epoch`), and `ConnectorRpcFrame` in `crates/fleet-proto/src/workbench.rs`.
- [ ] Embed `DeliveryTier` and `SidebarProjectBinding` as optional typed fields inside `TimelineItem` and `SessionMeta` so remote workbench clients see proof of desktop sidebar visibility (`ThreadVisible`) alongside standard Agents-Anywhere fields.

**You see.**

- [ ] `cargo test -p fleet-proto` prints `test result: ok` with deterministic SHA-256 content hashes matching Agents-Anywhere `contracts/protocol/1.1` fixtures and rejecting out-of-bounds `order_seq` values.

**Verify, unit.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] Add `crates/fleet-proto/tests/workbench_contracts.rs` testing `timeline_content_hash` surrogate escaping, `stable_runtime_session_id`, `InteractionStatus` state transitions, `PlanReviewTicket` epoch fencing, and `DeliveryTier` lattice projection. Run `cargo test -p fleet-proto --test workbench_contracts`.

**Verify, live.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked. Ten lanes on the configured `swarm workers` model at the PR head, per the boot recipe.

- [ ] Lane 1. Regression lane against trunk. Run `./scripts/check` and `fleet-cli doctor --gate` at trunk and head. Save `pr1-lane1-regression.png`. Pass when both trunk and head exit `0` with identical `schema_status` across all five desktop hosts.
- [ ] Lane 2. Run `fleet-cli skill` and inspect the serialized JSON schema output for `DeliveryTier` and `PeerAddress`. Save `pr1-lane2-skill.png`. Pass when the CLI exits `0` and lists all six canonical peers.
- [ ] Lane 3. Run `cargo test -p fleet-proto -- --nocapture` and render the output in terminal. Save `pr1-lane3-proto-unit.png`. Pass when `0` tests fail and `timeline_content_hash` matches known vector `sha256:`.
- [ ] Lane 4. Validate `ProtocolEventEnvelope` JSON serialization against `contracts/protocol/1.1` sample payloads via `cargo test -p fleet-proto envelope_wire_roundtrip`. Save `pr1-lane4-envelope.png`. Pass when roundtrip JSON equals input byte-for-byte.
- [ ] Lane 5. Test `TimelineOrderSeq::try_from(i64::MAX)` rejection in `fleet-proto`. Save `pr1-lane5-order-seq.png`. Pass when `InvalidOrderSeq` is returned without panic.
- [ ] Lane 6. Test `InteractionStatus::can_transition_to` across all 16 state pairs. Save `pr1-lane6-interactions.png`. Pass when only valid transitions (`Open -> ResponseAccepted -> Resolving -> Resolved`) return `true`.
- [ ] Lane 7. Test `PlanReviewTicket::verify_approval` when `content_hash` or `epoch` mutates by 1 byte. Save `pr1-lane7-plan-fence.png`. Pass when mismatched hash or epoch returns `Err(PlanFenceConflict)`.
- [ ] Lane 8. Test `resolve_project_dir_in_root` against symlink and `..` traversal inputs for workbench file requests. Save `pr1-lane8-path-boundary.png`. Pass when traversal and cache paths are rejected.
- [ ] Lane 9. Test `validate_brief` on workbench `create-and-start` prompt payloads containing `MODEL_WRONG`. Save `pr1-lane9-brief-policy.png`. Pass when `BriefPolicyError` is returned.
- [ ] Lane 10. Run `node scripts/guard.mjs private && node scripts/guard.mjs landscape`. Save `pr1-lane10-guards.png`. Pass when both guards print `OK` and exit `0`.

**Verify, perf.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] Metric. Wall-clock latency to hash and serialize 10,000 `TimelineItem` structs in `fleet-proto` plus `./scripts/check` total workspace compile time.
- [ ] Probe. Run `cargo test -p fleet-proto --release bench_timeline_hash_10k -- --nocapture` interleaved at trunk (hashing 10,000 `InboxMessage` frames) and head (hashing 10,000 `TimelineItem` structs).
- [ ] Baseline. Record the trunk 10,000-message framing duration in milliseconds first.
- [ ] Rule. Head 10,000-item content-hash and JSON serialization must complete in under `50 ms` total and add no more than `5%` to `cargo check --workspace`.

**Review gate.** None. PR-1 is not review-gated.

**Merge.**

- [ ] Root's clean verdict at the exact head SHA.
- [ ] Bugbot triage done.
- [ ] Base and verdict are current under the execution playbook and the patch-id rule in `playbooks/shipping.md`.
- [ ] The root appends `PR-1` to the base-branch stack and the operator lands it bottom-up.

## Replace flat JSON mailbox with SQLite WAL unified ledger in fleet-daemon (PR-2)

**Depends on.** PR-1.

**Files.**

- [ ] Create `crates/fleet-daemon/src/ledger.rs`.
- [ ] Edit `crates/fleet-daemon/src/lib.rs`.
- [ ] Edit `crates/fleet-daemon/src/host_adapter.rs`.
- [ ] Create `crates/fleet-daemon/tests/ledger_wal.rs`.

**Build.**

- [ ] Implement `FleetLedgerStore` in `crates/fleet-daemon/src/ledger.rs` backed by `rusqlite` in `WAL` mode (`<state_root>/fleet-ledger.sqlite`) with tables `sessions`, `timeline_items`, `inbox_messages`, `notices`, `sync_fingerprints`, and `correlation_claims`.
- [ ] Implement automatic, idempotent one-time import from legacy `<state_root>/mailboxes.json` and `<host>-state.json` into `fleet-ledger.sqlite` on first open, preserving `FleetMailboxStore` public trait signatures so `fleet-cli mcp` and `send` callers require zero flag changes.
- [ ] Implement monotonic `updated_seq` allocation and per-item `content_hash` fingerprint comparison inside `FleetLedgerStore::upsert_timeline_item` so unchanged items are skipped and modified items emit a sequenced delta.

**You see.**

- [ ] Running `fleet-cli inbox --for grokbot` and `fleet-cli who` reads and writes `<state_root>/fleet-ledger.sqlite` in WAL mode with zero whole-file JSON rewrites and zero lost messages under 16 concurrent writer threads.

**Verify, unit.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] Add `crates/fleet-daemon/tests/ledger_wal.rs` covering legacy `mailboxes.json` migration, 16-thread concurrent `enqueue` and `upsert_timeline_item`, `DuplicateDelivery` rejection, and fingerprint deduplication. Run `cargo test -p fleet-daemon --test ledger_wal`.

**Verify, live.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked. Ten lanes on the configured `swarm workers` model at the PR head, per the boot recipe.

- [ ] Lane 1. Regression lane against trunk. Run `./scripts/check` and `cargo test -p fleet-cli poteto_relay_four_hosts` at trunk and head. Save `pr2-lane1-regression.png`. Pass when all four-host relay tests pass on both trunk and head.
- [ ] Lane 2. Seed a temporary `<state_root>/mailboxes.json` with 5 unread messages, run `fleet-cli inbox --for grokbot --state-root <dir>`, and inspect `<dir>/fleet-ledger.sqlite`. Save `pr2-lane2-migration.png`. Pass when all 5 messages are returned once and `sqlite3 <dir>/fleet-ledger.sqlite "PRAGMA journal_mode;"` prints `wal`.
- [ ] Lane 3. Spawn 8 parallel `fleet-cli send` processes targeting an isolated fixture host sharing one `--state-root`. Save `pr2-lane3-concurrent-send.png`. Pass when all 8 exits are `0` and `sqlite3` counts exactly `8` distinct delivery claims.
- [ ] Lane 4. Re-send an identical `delivery_id` with mutated prompt text to the same `--state-root`. Save `pr2-lane4-duplicate-reject.png`. Pass when `fleet-cli` exits non-zero with `DuplicateDelivery`.
- [ ] Lane 5. Upsert 100 timeline items into `FleetLedgerStore`, then re-upsert the same 100 items with 3 modified items. Save `pr2-lane5-delta-sync.png`. Pass when the delta batch contains exactly the `3` modified items with strictly increasing `updated_seq`.
- [ ] Lane 6. Test `fleet-cli mcp` over stdio (`initialize`, `list_team`, `send_message`, `check_inbox`, `message_status`) backed by `fleet-ledger.sqlite`. Save `pr2-lane6-mcp-stdio.png`. Pass when all 5 JSON-RPC responses return valid results.
- [ ] Lane 7. Simulate crash recovery by leaving an uncommitted reader transaction open while another process writes timeline items. Save `pr2-lane7-wal-isolation.png`. Pass when neither reader nor writer blocks or corrupts the database (`PRAGMA integrity_check` prints `ok`).
- [ ] Lane 8. Verify file permissions on `<state_root>/fleet-ledger.sqlite`. Save `pr2-lane8-permissions.png`. Pass when mode is `0600` on Unix.
- [ ] Lane 9. Run `fleet-cli doctor --gate` on the host machine. Save `pr2-lane9-doctor-gate.png`. Pass when exit code is `0`.
- [ ] Lane 10. Run `npm test` in repository root. Save `pr2-lane10-npm-test.png`. Pass when all Node tests and privacy guards pass.

**Verify, perf.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] Metric. P99 latency of `enqueue` + `upsert_timeline_item` when the store already holds 10,000 messages/items.
- [ ] Probe. Run `cargo test -p fleet-daemon --release bench_store_10k_append -- --nocapture` comparing trunk (`FleetMailboxStore` rewriting a 10,000-entry `mailboxes.json`) against head (`FleetLedgerStore` indexed WAL insert).
- [ ] Baseline. Record trunk P99 latency to append the 10,001st message into `mailboxes.json` first.
- [ ] Rule. Head P99 append latency at 10,000 existing rows must be under `5 ms` and at least `5x` faster than trunk's full-JSON rewrite.

**Review gate.** None. PR-2 is not review-gated.

**Merge.**

- [ ] Root's clean verdict at the exact head SHA.
- [ ] Bugbot triage done.
- [ ] Base and verdict are current under the execution playbook and the patch-id rule in `playbooks/shipping.md`.
- [ ] The root appends `PR-2` to the base-branch stack and the operator lands it bottom-up.

## Project native desktop host histories into unified timeline items in fleet-daemon (PR-3)

**Depends on.** PR-2.

**Files.**

- [ ] Create `crates/fleet-daemon/src/observer.rs`.
- [ ] Edit `crates/fleet-daemon/src/antigravity.rs`.
- [ ] Edit `crates/fleet-daemon/src/tri_hosts.rs`.
- [ ] Edit `crates/fleet-daemon/src/host_adapter.rs`.
- [ ] Create `crates/fleet-daemon/tests/timeline_observer.rs`.

**Build.**

- [ ] Implement `HostTimelineObserver` trait and concrete read-only projectors in `crates/fleet-daemon/src/observer.rs` for all five canonical desktop hosts (`antigravity-desktop`, `openai-dots`, `deepseek-harness`, `cindy`, `zcode`) plus `grokbot`.
- [ ] Implement checkpointed incremental history scanning for `openai-dots` (`state_5.sqlite` + rollout JSONL), `antigravity-desktop` (`conversation_summaries.db` + cascade turns), `deepseek-harness` (`workspace.json` + two-frame `session.v4.jsonl.zstd`), `cindy` (`~/.cindy/transcripts/*.jsonl`), and `zcode` (`v2/tasks-index.sqlite` + `cli/db/db.sqlite`), mapping native turns, tool calls, and `<fleet-message>` blocks into `fleet_proto::timeline::TimelineItem` records stored in `FleetLedgerStore`.
- [ ] Enforce read-only native database access (`SQLITE_OPEN_READ_ONLY`) during observation and check active writer locks before any session takeover so `fleet-daemon` never corrupts a running desktop app's WAL or silently displaces an active human writer.

**You see.**

- [ ] `fleet-cli hosts sync-timeline --host zcode` (and `openai-dots`, `deepseek-harness`, `antigravity-desktop`, `cindy`) projects native desktop turns into `FleetLedgerStore` with deterministic `item_id`, `content_hash`, and `DeliveryTier::ThreadVisible`, omitting unchanged items on repeat scans.

**Verify, unit.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] Add `crates/fleet-daemon/tests/timeline_observer.rs` with isolated fixtures for all five desktop hosts and `grokbot`, verifying incremental checkpointing, zstd two-frame decoding for DSH, `<fleet-message>` metadata extraction, and writer-lock conflict reporting. Run `cargo test -p fleet-daemon --test timeline_observer`.

**Verify, live.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked. Ten lanes on the configured `swarm workers` model at the PR head, per the boot recipe.

- [ ] Lane 1. Regression lane against trunk. Run `./scripts/check` at trunk and head. Save `pr3-lane1-regression.png`. Pass when all workspace tests and Node checks exit `0`.
- [ ] Lane 2. Project a fixture `openai-dots` `state_5.sqlite` + rollout JSONL twice in a row. Save `pr3-lane2-dots-observer.png`. Pass when scan 1 emits `N > 0` items and scan 2 emits `0` items without reading unchanged turn bodies.
- [ ] Lane 3. Project a fixture `deepseek-harness` `session.v4.jsonl.zstd` two-frame archive into `FleetLedgerStore`. Save `pr3-lane3-dsh-observer.png`. Pass when both frames decompress cleanly and emit ordered `TimelineItem` rows.
- [ ] Lane 4. Project a fixture `zcode` `tasks-index.sqlite` + `cli/db/db.sqlite` session containing a `<fleet-message from="grokbot">` block. Save `pr3-lane4-zcode-observer.png`. Pass when the projected item carries `DeliveryTier::ThreadVisible` and preserves the peer sender metadata.
- [ ] Lane 5. Project a fixture `antigravity-desktop` `conversation_summaries.db` session. Save `pr3-lane5-agy-observer.png`. Pass when `project_id` and `workspace_uris` map to the canonical `SidebarProjectBinding`.
- [ ] Lane 6. Project a fixture `cindy` transcript JSONL file and append 2 new lines. Save `pr3-lane6-cindy-observer.png`. Pass when the incremental scan emits only the `2` new items.
- [ ] Lane 7. Simulate an active native writer lock on `openai-dots` during a remote turn takeover attempt. Save `pr3-lane7-writer-conflict.png`. Pass when the adapter returns `Err(NativeWriterActive)` before mutating the session.
- [ ] Lane 8. Verify that a failing scan on one corrupt session file does not block timeline projection for healthy sessions in the same run. Save `pr3-lane8-fault-isolation.png`. Pass when healthy sessions advance their checkpoints and the corrupt session logs a bounded error.
- [ ] Lane 9. Run read-only timeline projection against the live local `~/.zcode` database into a temporary `--state-root`. Save `pr3-lane9-live-zcode-read.png`. Pass when real ZCode tasks project into `fleet-ledger.sqlite` without modifying `~/.zcode` mtimes.
- [ ] Lane 10. Run `fleet-cli doctor --gate`. Save `pr3-lane10-doctor-gate.png`. Pass when exit code is `0`.

**Verify, perf.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] Metric. Duration of a no-change incremental scan across 50 sessions containing 5,000 total turns.
- [ ] Probe. Run `cargo test -p fleet-daemon --release bench_incremental_scan_50_sessions -- --nocapture` at head (and compare against full uncheckpointed JSONL parsing baseline).
- [ ] Baseline. Record the full uncheckpointed parse time for 50 sessions / 5,000 turns first.
- [ ] Rule. Checkpointed no-change rescan of 50 sessions must complete in under `25 ms` and emit `0` duplicate timeline writes.

**Review gate.** None. PR-3 is not review-gated.

**Merge.**

- [ ] Root's clean verdict at the exact head SHA.
- [ ] Bugbot triage done.
- [ ] Base and verdict are current under the execution playbook and the patch-id rule in `playbooks/shipping.md`.
- [ ] The root appends `PR-3` to the base-branch stack and the operator lands it bottom-up.

## Implement outbound Rust Connector RPC and ingest client in fleet-daemon (PR-4)

**Depends on.** PR-3.

**Files.**

- [ ] Create `crates/fleet-daemon/src/connector_ws.rs`.
- [ ] Create `crates/fleet-daemon/src/sandbox_fs.rs`.
- [ ] Edit `crates/fleet-daemon/src/lib.rs`.
- [ ] Edit `crates/fleet-cli/src/main.rs`.
- [ ] Create `crates/fleet-daemon/tests/connector_protocol.rs`.

**Build.**

- [ ] Implement `ConnectorClient` in `crates/fleet-daemon/src/connector_ws.rs` speaking the Agents-Anywhere v2 Connector protocol (`WS /api/v2/connector/ws` for bidirectional JSON-RPC `runtime.*`, `session.*`, `workspace.*`, `interaction.respond`, and `timeline.itemUpsert` under `900 KiB`; automatic fallback to chunked `POST /api/v2/connector/ingest` when batches exceed `900 KiB` or WS reconnects) with generation fencing and 8-standard / 2-critical concurrency semaphores.
- [ ] Implement `SandboxFsHandler` in `crates/fleet-daemon/src/sandbox_fs.rs` handling remote workbench file browse/read/write and command execution requests strictly gated by `fleet_proto::host::resolve_project_dir_in_root` and `NodeDaemon`'s `libc::fchdir` descriptor pinning so remote workbench clients can never escape `~/Projects/<slug>`.
- [ ] Add `fleet-cli connector start --server-url <url> --connector-id <id> --connector-token <token>` in `crates/fleet-cli/src/main.rs` to run the pure-Rust outbound connector daemon without Python or `uv`.

**You see.**

- [ ] `fleet-cli connector start` connects over WebSocket to a v2 Server endpoint, registers all 5 desktop hosts + `grokbot` as Runtime Providers, streams `timeline.itemUpsert` deltas under `900 KiB`, falls back to `/api/v2/connector/ingest` for large snapshots, and routes incoming `session.create` / `session.send` RPCs into `DesktopHostAdapter`.

**Verify, unit.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] Add `crates/fleet-daemon/tests/connector_protocol.rs` testing `900 KiB` WS-to-HTTP-ingest automatic spillover, partial rejection isolation (`{ accepted, rejected }`), generation fencing, `resolve_project_dir_in_root` sandbox enforcement on remote file RPCs, and `interaction.respond` routing. Run `cargo test -p fleet-daemon --test connector_protocol`.

**Verify, live.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked. Ten lanes on the configured `swarm workers` model at the PR head, per the boot recipe.

- [ ] Lane 1. Regression lane against trunk. Run `./scripts/check` at trunk and head. Save `pr4-lane1-regression.png`. Pass when all existing tests pass on both trunk and head.
- [ ] Lane 2. Start an isolated loopback mock v2 Server and connect `fleet-cli connector start`. Save `pr4-lane2-ws-handshake.png`. Pass when the connector authenticates, claims the lease, and reports all 6 local peers (`5` desktop hosts + `grokbot`) in `runtime.discover`.
- [ ] Lane 3. Emit a `1.2 MiB` timeline snapshot batch through `ConnectorClient`. Save `pr4-lane3-ingest-spillover.png`. Pass when `ConnectorClient` automatically routes the batch to `POST /api/v2/connector/ingest` instead of dropping the WebSocket connection.
- [ ] Lane 4. Return a partial rejection `{ accepted: 9, rejected: [{ index: 4, code: "invalid_order_seq" }] }` from the mock ingest endpoint. Save `pr4-lane4-partial-reject.png`. Pass when the 9 accepted items advance their sync fingerprints in `fleet-ledger.sqlite` and item 4 is isolated without crashing the loop.
- [ ] Lane 5. Send a `session.create` RPC from the mock server targeting `zcode/demo-app`. Save `pr4-lane5-rpc-create.png`. Pass when `ZcodeAdapter` creates `~/Projects/demo-app` in the fixture root, mounts the sidebar project, and returns `ok: true` with `DeliveryTier::ThreadVisible`.
- [ ] Lane 6. Send a `workspace.readFile` RPC with `path: "../../.ssh/id_rsa"` and `path: ".anet/identities"`. Save `pr4-lane6-sandbox-escape.png`. Pass when `SandboxFsHandler` rejects both requests with `ProjectDirBoundaryError` and reads zero bytes.
- [ ] Lane 7. Send 12 concurrent standard RPCs and 2 critical `interaction.respond` RPCs over the connector channel. Save `pr4-lane7-semaphores.png`. Pass when the 2 critical RPCs complete via the critical semaphore without starving behind the 8-slot standard semaphore.
- [ ] Lane 8. Drop the mock WebSocket server mid-stream and restart it with a new generation ID. Save `pr4-lane8-reconnect-fence.png`. Pass when `ConnectorClient` reconnects, fences stale generation replies, and flushes pending outbox items via HTTP ingest.
- [ ] Lane 9. Verify zero Python processes are spawned by running `pgrep -fl "anywhere-cli|python"` on the connector child tree during execution. Save `pr4-lane9-zero-python.png`. Pass when the connector process tree contains only the Rust `fleet-cli` binary.
- [ ] Lane 10. Run `fleet-cli doctor --gate` and `npm test`. Save `pr4-lane10-doctor-gate.png`. Pass when both exit `0`.

**Verify, perf.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] Metric. End-to-end loopback RPC dispatch + timeline delta ingest latency and resident memory (RSS) of the connector daemon.
- [ ] Probe. Run `cargo test -p fleet-daemon --release bench_connector_rpc_roundtrip -- --nocapture` measuring 1,000 WebSocket JSON-RPC roundtrips and `/bin/ps -o rss= -p <pid>`.
- [ ] Baseline. Record the baseline loopback WebSocket roundtrip time from `DesktopDelivery` on trunk first.
- [ ] Rule. P99 loopback connector RPC roundtrip must be under `3 ms` and connector daemon RSS must remain under `35 MiB` (compared to `>120 MiB` for a Python + Node runtime stack).

**Review gate.** None. PR-4 is not review-gated.

**Merge.**

- [ ] Root's clean verdict at the exact head SHA.
- [ ] Bugbot triage done.
- [ ] Base and verdict are current under the execution playbook and the patch-id rule in `playbooks/shipping.md`.
- [ ] The root appends `PR-4` to the base-branch stack and the operator lands it bottom-up.

## Build self-hosted Rust workbench server crate for Alibaba Cloud deployment (PR-5)

**Depends on.** PR-4.

**Files.**

- [ ] Create `crates/fleet-workbench-server/Cargo.toml`.
- [ ] Create `crates/fleet-workbench-server/src/main.rs`.
- [ ] Create `crates/fleet-workbench-server/src/lib.rs`.
- [ ] Create `crates/fleet-workbench-server/src/ui.rs`.
- [ ] Create `docker/Dockerfile.workbench`.
- [ ] Create `docker/docker-compose.aliyun.yml`.
- [ ] Edit `Cargo.toml`.

**Build.**

- [ ] Implement `fleet-workbench-server` in Rust (`axum` + `tokio` + `rusqlite` WAL / `sqlx` Postgres) exposing both the Agents-Anywhere `/api/v2/*` REST/WebSocket surface (`/api/v2/health`, `/api/v2/connector/ws`, `/api/v2/connector/ingest`, `/api/v2/sessions/create-and-start`, `/api/v2/sessions/{id}/ws`, `/api/v2/sessions/{id}/events`, `/api/v2/sessions/{id}/snapshot`, `/api/v2/sessions/{id}/runtime/notices/{noticeId}/respond`) and the ANet Wire v3 `/relay/send` + `/relay/status` surface on one server binary, while serving either the prebuilt `web-next` static bundle via `--static-dir <path>` or the built-in self-contained workbench UI in `crates/fleet-workbench-server/src/ui.rs` from the same origin.
- [ ] Implement the in-memory `TimelineWriteBuffer` + monotonic `updatedSeq` allocator + per-session FIFO notification lanes in Rust so a single-node Alibaba Cloud ECS instance runs with zero Redis dependency (using embedded WAL durability) while remaining wire-compatible with `web-next`, iOS, and Android clients.
- [ ] Create `docker/Dockerfile.workbench` (multi-stage build compiling `web-next` from `anywhere-labs/Agents-Anywhere` into `/srv/workbench/static` and `fleet-workbench-server` into `/usr/local/bin/fleet-workbench-server`) and `docker/docker-compose.aliyun.yml` (with S3/Aliyun OSS attachment backend config `AGENT_SERVER_FILES_BACKEND=s3`) ready to drop into `~/Projects/vps-ops/docker/stacks/agents-anywhere/`.

**You see.**

- [ ] Starting `fleet-workbench-server` on `http://127.0.0.1:5174` serves `GET /api/v2/health` with `{"status":"ok"}`, accepts a `fleet-cli connector start` connection, renders the workbench web UI in a browser, and streams live timeline updates over `/api/v2/sessions/{id}/ws`.

**Verify, unit.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] Add `crates/fleet-workbench-server/tests/api_v2_contract.rs` testing `create-and-start` admission rollback on `ok: false`, monotonic `updatedSeq` allocation, `/events?after=seq:N` gap recovery (`snapshotRequired`), interaction state transitions, and ANet Wire v3 `/relay/send` coexistence. Run `cargo test -p fleet-workbench-server`.

**Verify, live.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked. Ten lanes on the configured `swarm workers` model at the PR head, per the boot recipe.

- [ ] Lane 1. Regression lane against trunk. Run `./scripts/check` at trunk and head. Save `pr5-lane1-regression.png`. Pass when all workspace tests and Node checks exit `0`.
- [ ] Lane 2. Start `fleet-workbench-server` and probe `GET /api/v2/health`. Save `pr5-lane2-health.png`. Pass when HTTP 200 returns `{"status":"ok"}`.
- [ ] Lane 3. Connect `fleet-cli connector start` to `fleet-workbench-server` and open the workbench UI in headless browser at `http://127.0.0.1:5174`. Save `pr5-lane3-workbench-home.png`. Pass when the online connector and all 5 desktop host providers + `grokbot` render in the UI.
- [ ] Lane 4. Trigger `POST /api/v2/sessions/create-and-start` for `zcode/demo-workbench` from the workbench UI. Save `pr5-lane4-create-session.png`. Pass when the session is created, mounted in ZCode's sidebar store (`DeliveryTier::ThreadVisible`), and rendered in the web timeline.
- [ ] Lane 5. Stream 20 rapid timeline item updates from the connector and observe the browser WebSocket client (`33 ms` coalescing buffer). Save `pr5-lane5-ws-stream.png`. Pass when `updatedSeq` advances monotonically without gaps or duplicate items.
- [ ] Lane 6. Disconnect the browser WebSocket, emit 5 timeline updates on the connector, and reconnect calling `GET /api/v2/sessions/{id}/events?after=seq:N`. Save `pr5-lane6-event-recovery.png`. Pass when all 5 missed events recover with `snapshotRequired: false`.
- [ ] Lane 7. Trigger a session replacement fence and call `/events?after=seq:1`. Save `pr5-lane7-snapshot-required.png`. Pass when `/events` returns `snapshotRequired: true` and `GET /snapshot` reconciles the full timeline.
- [ ] Lane 8. Emit an `input_request` notice from the connector, render the approval card in the browser, and submit an approval via `POST /runtime/notices/{id}/respond`. Save `pr5-lane8-interaction-approve.png`. Pass when notice status transitions `open -> response_accepted -> resolved` and the connector receives `interaction.respond`.
- [ ] Lane 9. Send an ANet Wire v3 `POST /relay/send` and `POST /relay/status` request to the same `fleet-workbench-server` instance. Save `pr5-lane9-anet-wire3.png`. Pass when the server records the correlation claim and returns a valid Wire v3 receipt.
- [ ] Lane 10. Validate `docker compose -f docker/docker-compose.aliyun.yml config`. Save `pr5-lane10-docker-config.png`. Pass when `docker compose config` exits `0` with valid volume and OSS environment bindings.

**Verify, perf.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] Metric. Server throughput (timeline upserts/sec), P99 WebSocket fan-out latency, and idle container RSS.
- [ ] Probe. Run `cargo test -p fleet-workbench-server --release bench_timeline_fanout_10k -- --nocapture` pushing 10,000 timeline upserts through `/api/v2/connector/ws` to an active session subscriber.
- [ ] Baseline. Record `fleet-hub` loopback HTTP relay throughput on trunk first, and set absolute budgets for the new `/api/v2` workbench fan-out path.
- [ ] Rule. `fleet-workbench-server` must sustain `>= 5,000` timeline upserts/sec with P99 WebSocket push latency `< 10 ms` and idle RSS `< 50 MiB` (eliminating the `~500 MiB` Postgres + Redis + FastAPI baseline on Alibaba Cloud ECS).

**Review gate.** The operator reviews before merge.

- [ ] Copy lane 3, lane 4, and lane 8 screenshots into `~/Projects/_artifacts/agent-fleet/workbench-unification/PR-5-review-workbench.png`.
- [ ] Record a 30 to 60 second video of the change in a lane worktree showing a task dispatched from the Web Workbench landing in a local desktop host sidebar and streaming its reply back to the browser. Save it as `~/Projects/_artifacts/agent-fleet/workbench-unification/PR-5-review.mp4`.
- [ ] Post the screenshots and the video in chat. Stop at merge-ready. Wait for the operator's click.

**Merge.**

- [ ] Root's clean verdict at the exact head SHA.
- [ ] Bugbot triage done.
- [ ] Base and verdict are current under the execution playbook and the patch-id rule in `playbooks/shipping.md`.
- [ ] The root appends `PR-5` to the base-branch stack and the operator lands it bottom-up.

## Wire peer MCP dispatch into live workbench timeline and close end-to-end verification (PR-6)

**Depends on.** PR-5.

**Files.**

- [ ] Edit `crates/fleet-cli/src/main.rs`.
- [ ] Edit `DESIGN.md`.
- [ ] Edit `README.md`.
- [ ] Edit `docs/architecture.md`.
- [ ] Edit `docs/landscape.md`.

**Build.**

- [ ] Wire `dispatch_unified_message` and `handle_mcp_tool_call` (`send_message`, `check_inbox`, `list_team`, `message_status`) in `crates/fleet-cli/src/main.rs` so every peer-to-peer task dispatch (e.g. `grokbot -> openai-dots/my-repo` or `zcode -> antigravity-desktop/my-repo`) automatically emits sequenced `TimelineItem` records (`<fleet-message>` peer frame + `DeliveryTier` status badge) to the connected `fleet-workbench-server`.
- [ ] Add `fleet-cli doctor --workbench <url>` health and schema verification, and update `DESIGN.md`, `README.md`, `docs/architecture.md`, and `docs/landscape.md` to document the unified 3-pillar Rust architecture (`ANet` + `Reason` + `Agents-Anywhere Workbench`) and mark `alma-relay-ops` ready for retirement once Alibaba Cloud cutover is approved.

**You see.**

- [ ] When `grokbot` dispatches a task to `zcode/agent-fleet` via MCP `send_message`, the thread appears in ZCode Desktop's left sidebar (`DeliveryTier::ThreadVisible`) AND simultaneously streams live into the self-hosted Web Workbench timeline where the human operator can watch, approve tool calls, or reply from phone or browser.

**Verify, unit.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] Add `crates/fleet-cli/tests/unified_workbench_e2e.rs` testing bidirectional MCP peer dispatch (`grokbot <-> desktop host`) mirrored live onto `fleet-workbench-server` WebSocket subscribers. Run `cargo test -p fleet-cli --test unified_workbench_e2e`.

**Verify, live.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked. Ten lanes on the configured `swarm workers` model at the PR head, per the boot recipe.

- [ ] Lane 1. Regression lane against trunk. Run `./scripts/check` and `fleet-cli doctor --gate` at trunk and head. Save `pr6-lane1-regression.png`. Pass when all 13 check commands in `./scripts/check` exit `0`.
- [ ] Lane 2. Dispatch `grokbot -> zcode/agent-fleet` via `fleet-cli mcp` `send_message` while `fleet-workbench-server` and browser are running. Save `pr6-lane2-mcp-to-workbench.png`. Pass when the `<fleet-message>` prompt and `ThreadVisible` badge appear in both ZCode and the Web Workbench timeline.
- [ ] Lane 3. Reply from `zcode/agent-fleet -> grokbot` via `fleet-cli send --reply-to <id>`. Save `pr6-lane3-reply-to-grokbot.png`. Pass when `fleet-cli inbox --for grokbot` receives the reply and the Web Workbench timeline shows the reply in the same thread.
- [ ] Lane 4. Send a follow-up human message from the Web Workbench into the active peer session. Save `pr6-lane4-human-intervene.png`. Pass when `deliver_turn` appends the human turn to the same native desktop thread without creating a duplicate session.
- [ ] Lane 5. Run `fleet-cli doctor --workbench http://127.0.0.1:5174 --gate`. Save `pr6-lane5-doctor-workbench.png`. Pass when doctor verifies local 5-host schemas AND remote workbench `/api/v2/health` + connector lease status.
- [ ] Lane 6. Test `dispatch-herdr` with no active agent and with an active agent per `skills/verify-fleet/SKILL.md`. Save `pr6-lane6-herdr-gate.png`. Pass when inactive herdr fails closed without pane paste and active herdr reports `FLEET_TIER`.
- [ ] Lane 7. Test `dispatch-cindy-routing` per `skills/verify-fleet/SKILL.md`. Save `pr6-lane7-cindy-routing.png`. Pass when `cindy` reports honest `dispatch_channel: anet` status in both `doctor` and the Workbench provider catalog.
- [ ] Lane 8. Run `node scripts/guard.mjs private && node scripts/guard.mjs landscape`. Save `pr6-lane8-guards.png`. Pass when both guards print `OK` with zero personal overlay leaks.
- [ ] Lane 9. Run `python3 scripts/release --skip-check` and execute the installed `~/.local/bin/fleet-cli` MCP smoke test. Save `pr6-lane9-release-smoke.png`. Pass when installed binary SHA-256 matches the build output and MCP smoke passes.
- [ ] Lane 10. Inspect process tree and dependency graph (`cargo tree`) to confirm zero Python or external runtime wrappers exist. Save `pr6-lane10-cargo-tree.png`. Pass when the entire workspace compiles to static/native Rust binaries.

**Verify, perf.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] Metric. End-to-end latency from `fleet-cli send` invocation to both native SQLite `ThreadVisible` verification and Web Workbench WebSocket event emission.
- [ ] Probe. Run `cargo test -p fleet-cli --release bench_e2e_dispatch_and_workbench_stream -- --nocapture` interleaved with trunk's `poteto_relay_four_hosts` dispatch benchmark.
- [ ] Baseline. Record trunk's local dispatch + SQLite readback latency first.
- [ ] Rule. Adding live Workbench timeline streaming must add less than `10 ms` overhead to `fleet-cli send` local completion (`P99 overhead < 10 ms`).

**Review gate.** The operator reviews before merge.

- [ ] Copy lane 2, lane 3, and lane 4 screenshots into `~/Projects/_artifacts/agent-fleet/workbench-unification/PR-6-review-e2e.png`.
- [ ] Record a 30 to 60 second video of the change in a lane worktree demonstrating `grokbot` MCP dispatch, desktop sidebar mounting, live Web Workbench timeline streaming, and human intervention in one thread. Save it as `~/Projects/_artifacts/agent-fleet/workbench-unification/PR-6-review.mp4`.
- [ ] Post the screenshots and the video in chat. Stop at merge-ready. Wait for the operator's click.

**Merge.**

- [ ] Root's clean verdict at the exact head SHA.
- [ ] Bugbot triage done.
- [ ] Base and verdict are current under the execution playbook and the patch-id rule in `playbooks/shipping.md`.
- [ ] The root appends `PR-6` to the base-branch stack and the operator lands it bottom-up.

## Close the program

- [ ] Every box above is checked with its evidence.
- [ ] Reply to the operator with the report the execution playbook names.

## Appendix A. Prototype evidence

Architectural exploration ran two parallel read-only subagents across `~/Projects/agent-fleet/crates/` (`commit f50282e`) and upstream `anywhere-labs/Agents-Anywhere` (`main` at `2026-11-07T09:41:32Z`).
1. **Desktop host and transport compatibility proven on trunk (`f50282e`).** `crates/fleet-daemon/src/desktop.rs` already implements non-blocking WebSocket JSON-RPC over Unix sockets using `tungstenite`, and `crates/fleet-daemon/src/host_adapter.rs` + `tri_hosts.rs` + `antigravity.rs` already read and verify the native SQLite/JSON/zstd stores of all five desktop hosts (`state_5.sqlite`, `conversation_summaries.db`, `workspace.json` + `session.v4.jsonl.zstd`, Cindy transcripts, and ZCode `tasks-index.sqlite` + `cli/db/db.sqlite`).
2. **Protocol surface proven from upstream `contracts/`.** `anywhere-labs/Agents-Anywhere` separates its UI clients (`web-next`, `desktop-workbench`, `ios`, `android`) from its backend strictly through versioned JSON schemas (`contracts/protocol/1.1/manifest.json`, `contracts/runtime-workflows/1.2/manifest.json`, `contracts/runtime-control/2.1/manifest.json`). Replacing the Python Connector and FastAPI Server with Rust binaries that speak `/api/v2/*` and `/api/v2/connector/ws` requires zero forks of the mobile apps and allows serving the static `web-next` export directly from Rust.
3. **Unproven items reserved for execution lanes.** Live WebSocket coalescing under `web-next` 33 ms buffer against the Rust `fleet-workbench-server` (`PR-5` Lane 5) and Alibaba Cloud ECS OSS S3 attachment presigning (`PR-5` Lane 10) remain to be proven during slice execution.

## Appendix B. Alternatives rejected

1. **Wrapping Python `anywhere-cli` around `fleet-cli` as multi-runtime glue.** Running Python `uv run anywhere-cli` alongside Rust `fleet-daemon` would duplicate SQLite/JSONL polling, introduce two competing local state directories (`~/.agents-anywhere` vs `<state_root>/mailboxes.json`), risk writer-lock collisions on `codex app-server` and ZCode SQLite stores, and require maintaining both Python 3.12 and Rust toolchains on every workstation.
2. **Deploying upstream Python FastAPI + PostgreSQL 17 + Redis AOF stack unmodified on Alibaba Cloud ECS.** Upstream's server requires three containers (PostgreSQL 17, Redis with AOF `everysec` + `INFO server` ACLs, and Python FastAPI) designed for multi-tenant cloud hosting. For our self-hosted single-operator fleet on Hangzhou Alibaba Cloud ECS, a single Rust binary (`fleet-workbench-server`) with an in-memory sequence lease + embedded SQLite WAL (or optional Postgres) and Aliyun OSS attachment storage cuts memory footprint by `~90%` and unifies ANet Hub Wire v3 (`/relay/send`) with the `/api/v2` Workbench API in one process.
3. **Discarding `agent-fleet` and forking only `Agents-Anywhere`.** Upstream `Agents-Anywhere` has no concept of peer-to-peer agent delegation (`<fleet-message>`), no `DeliveryTier::ThreadVisible` sidebar proof, no `validate_brief` anti-self-attestation guard, and no adapters for `antigravity-desktop`, `cindy`, `zcode`, or `grokbot`.

## Appendix C. Risks

1. **Native Desktop Writer-Lock Contention (`PR-3`)**. If a remote workbench user sends a message to a Codex or Antigravity session while the desktop app holds an active turn lock, naive injection fails or corrupts state. The owner of `PR-3` watches `NativeWriterActive` detection and enforces read-only SQLite connections (`SQLITE_OPEN_READ_ONLY`) for all timeline observers.
2. **Cross-Language SHA-256 Content Hash Drift (`PR-1`)**. `web-next` and upstream contracts compute `timeline_content_hash` over `{type, status, role, content}` with explicit lone-surrogate escaping (`\udXXX`). The owner of `PR-1` watches surrogate test vectors in `workbench_contracts.rs` so Rust and TypeScript hashes never diverge.
3. **Web-Next Static Bundle Coupling (`PR-5`)**. The `web-next` UI has no pre-existing project-local verification driver in `agent-fleet`. The owner of `PR-5` drives the UI via headless browser / CDP alongside `skills/verify-fleet/SKILL.md` in the live lanes.

## Appendix D. Links and reading list

Every agent session executing a slice in this program must read these files in order before editing any code:
1. `AGENTS.md` and `lessons.md` (repository root rules, worktree cleanup mandate, and past failure modes).
2. `DESIGN.md` and `crates/AGENTS.md` (30-second crate routing table, five desktop host storage contracts, and core invariants).
3. `docs/workbench-architecture.md` (the first-principles domain model, conflict resolution matrix, and wire protocol specification for this unification).
4. `docs/desktop-rea-dossier.md` (exact SQLite schemas and IPC protocols for Antigravity, Codex, DSH, Cindy, and ZCode).
5. `skills/verify-fleet/SKILL.md` (mandatory live verification driver and honest receipt rules).

Run `skills/how/SKILL.md` before `PR-3` and `PR-5`. Run `skills/interrogate/SKILL.md` on `PR-4` (sandbox boundary) and `PR-5` (server wire contract) before shipping. Log every decision in `.scratch/pstack-relay/decisions.tsv` per `skills/show-me-your-work/SKILL.md`.
