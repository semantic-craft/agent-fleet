# ADR-0003：CLI 型 agent 一律经 Herdr

状态：已采纳（2026-10-19）

## 决定

Claude Code、Codex CLI、agy（Antigravity）等终端 agent 在 Herdr pane 里运行，由上游 `agenvo-herdr` 管理。
Herdr 以 headless 服务常驻（launchd 运行 `herdr server`）；用户需要看时在终端执行 `herdr` 接上。

## 理由

- 上游 connector 已支持 workspace、pane、读输出和事件，无需另写 connector 或调度器。
- Codex 派活使用官方 `codex exec`，进程退出即交还 writer，已观察到桌面版通过线程链接打开后显示完整历史、正常输入框，且没有 “This is open in another app” 占用横幅；常驻共享 app-server 会继续占用 writer。
- Claude Code 的无头模式输出结构化 JSON 结果，以 session id 续接；请求经 `ANTHROPIC_BASE_URL` 指向的本机 magpie。完整命令只在派活技能里维护。
- agy 的无头模式（`agy -p --output-format json`）输出 `conversation_id` 与结果，以 `--conversation` 续接；Antigravity 桌面版没有公开派活接口，但能用 `@conversation` 调出 CLI 会话，所以不需要为它写 connector。

## 约定

- Codex 用 `workspace.create` + `pane.run`，追问用 `codex exec resume`（`-C`、`-s` 放在 `resume` 之前，每轮重复 `-s danger-full-access`、`-m` 与 `-c model_provider="magpie"`）；Codex connector 仅列出线程和读取历史。
- Claude Code 按派活技能中的完整无头示例新建与续接，每轮显式传 `--model deepseek/deepseek-flash`（可选第二模型 `--model minimax-code/MiniMax-M3.1-Flash`，同样经 magpie），`ANTHROPIC_BASE_URL` 须指向 magpie。不使用交互式 `agent.start kind claude`。
- full access 参数由调用方传入：Codex 新建与续接每轮都用 `-s danger-full-access`；Claude Code 新建与 `--resume` 每轮都用 `--dangerously-skip-permissions`；agy 无头命令每轮都用 `--dangerously-skip-permissions`，交互式 `agent.start` 在 `args` 里传同一参数。实例 `context` 记录实际配置，connector 不强制。
- 新建、跟进、续接、中断与桌面接手的完整步骤见[派活技能](../../skills/delegate-via-agenvo/SKILL.md)。
