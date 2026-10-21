# ADR-0003：CLI 型 agent 一律经 Herdr

状态：已采纳（2026-10-19）

Codex app-server 的 writer 交接采用 [ADR-0007](0007-codex-owned-runtime.md) 所述、用户已批准的生命周期托管特例；交互式 Codex CLI 的一般使用仍遵循本决策。

## 决定
claude、codex CLI、agy（Antigravity）等终端 agent 不单独写 connector，而是在 Herdr pane 里运行，由上游 `agenvo-herdr` 管理。
Herdr 以 headless 服务常驻（launchd 运行 `herdr server`）；用户需要看时在终端执行 `herdr` 接上。

## 理由
- Herdr 内置这些 agent 的识别清单（`herdr server agent-manifests` 能列出 agy 等），上游 connector 已支持 `agent.start`、`agent.prompt`、读输出和事件。
- 这是唯一能对 DSH/ZCode 以外的 agent 远程追加输入的路径。

## 约定
- full access 参数由调用方在 `agent.start` 的 `args` 里传，例如 `agy --dangerously-skip-permissions`、`codex --dangerously-bypass-approvals-and-sandbox`；写进实例 `context`，connector 不强制。
