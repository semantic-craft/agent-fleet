# Agent-Fleet Workbench Unification Handoff & Kickoff Prompt

## 1. Context scope and mandatory reading order

Any agent session picking up the Unified Rust Workbench implementation in `~/Projects/agent-fleet` must read the following files in exact order before creating a worktree or writing code:

1. **`AGENTS.md` and `lessons.md`** — Repository-wide invariants: work in an isolated git worktree (`~/Projects/.worktrees/agent-fleet/<slice>`), never leave worktrees behind after merge, strip `ELECTRON_RUN_AS_NODE` when launching Electron apps, and verify every fix on the real host surface.
2. **`DESIGN.md` and `crates/AGENTS.md`** — The 30-second routing table mapping tasks to `fleet-proto`, `fleet-daemon`, `fleet-hub`, and `fleet-cli`, plus the five canonical desktop hosts (`antigravity-desktop`, `openai-dots`, `deepseek-harness`, `cindy`, `zcode`) and `grokbot`.
3. **`docs/workbench-architecture.md`** — The first-principles unified Rust architecture that merges `sleep2agi/agent-network` (ANet Wire v3), `reason-machines/reason-network` (sidebar session mounting + `fchdir` containment), and `anywhere-labs/Agents-Anywhere` (`v2` Workbench protocol, Timeline sequencer, and Connector RPC) with zero Python/multi-runtime glue code.
4. **`docs/workbench-unification-plan.md`** — The 6-PR verifiable execution checklist (`PR-1` through `PR-6`) checked by `check-plan.mjs`.
5. **`docs/desktop-rea-dossier.md` and `skills/verify-fleet/SKILL.md`** — The native SQLite/IPC storage contracts for all five desktop hosts and the mandatory live verification driver (`fleet-cli doctor --gate`, `scripts/check`, `scripts/release`).

**Out of scope (do NOT read or touch unless explicitly instructed):**
- Do not read `PRIVATE`, `~/.anet/ids/*` private keys, or unrelated business repositories in `~/Projects`.
- Do not introduce Python (`uv`, `anywhere-cli`, `FastAPI`) subprocesses or wrapper scripts into `agent-fleet`.

---

## 2. Ready-to-paste Kickoff Prompt (for starting the meeting in `~/Projects/agent-fleet`)

Copy and paste the block below into your new session in `~/Projects/agent-fleet`:

```text
/poteto-mode

我们要在 `~/Projects/agent-fleet` 推进「三源合一」的纯 Rust 架构演进：将现有 `agent-fleet`（融合了 ANet Wire v3 与 Reason Network 的五大桌面端 + GrokBot 派活引擎）与开源项目 `anywhere-labs/Agents-Anywhere`（跨设备 Web/iOS/Android 工作台与实时 Timeline 看板）从第一性原理出发融合成单一 Rust 体系，彻底摒弃 Python `anywhere-cli` / FastAPI 等多 Runtime 胶水代码，并支持在杭州阿里云 ECS 自托管。

开工前请严格按顺序阅读以下上下文文件（不要跳读）：
1. `AGENTS.md` 与 `lessons.md`
2. `DESIGN.md` 与 `crates/AGENTS.md`
3. `docs/workbench-architecture.md`（核心领域模型、6 大冲突消解矩阵与端到端数据流）
4. `docs/workbench-unification-plan.md`（已通过 `check-plan.mjs` 校验的 `PR-1` 至 `PR-6` 可验证切片计划）
5. `docs/desktop-rea-dossier.md` 与 `skills/verify-fleet/SKILL.md`

核心架构约束（不可违反）：
1. 零多语言胶水：所有协议类型、Timeline 增量哈希（含 lone-surrogate `\udXXX` 转义）、SQLite WAL 统一账本（`fleet-ledger.sqlite`）、只读宿主历史投影（`HostTimelineObserver`）、Connector 出站 WS/HTTP Ingest 客户端（900 KiB 分界）以及自托管服务端（`fleet-workbench-server`）全部用 Rust 原生实现。
2. 严守原生数据库只读与写锁边界：观察五大桌面宿主历史时一律使用 `SQLITE_OPEN_READ_ONLY`，派活或远程接管通过宿主原生 RPC/CLI 执行，遇到活跃写锁返回 `NativeWriterActive`，绝不直接裸写运行中宿主的 SQLite WAL。
3. 严守项目沙箱边界：所有来自远程工作台的文件读写与终端命令必须经过 `fleet_proto::host::resolve_project_dir_in_root` 与 `NodeDaemon` 的 `libc::fchdir(fd)` 绑定，严格限制在 `~/Projects/<slug>` 内。
4. 证据分级与门禁：每个切片在独立 worktree（`~/Projects/.worktrees/agent-fleet/<branch>`）中开发，遵循先红后绿 TDD，跑通 `./scripts/check` 与 `fleet-cli doctor --gate` 并留存真实收据后才推进下一棒。

请先复述你对本计划的目标、核心数据结构与 `PR-1` 边界的理解，然后按照 `docs/workbench-unification-plan.md` 从 `PR-1`（`crates/fleet-proto` 的 `timeline.rs` 与 `workbench.rs` 领域建模及契约测试）开始执行。
```
