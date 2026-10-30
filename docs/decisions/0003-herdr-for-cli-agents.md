# ADR-0003：CLI 型 agent 一律经 Herdr

状态：CLI 通道仍采纳（2026-10-19）；外部 Agenvo→Codex 目标由 PR60 的 Desktop 空线程交接替代；dot 有原生平台任务工具及授权时优先原生通道，见[调用方路由](../caller-routing.md)。外部路线见[交接契约](../codex-desktop-handoff.md)。下述 Codex exec 记录只适用于明确选择 CLI 的历史/可选通道，不是默认派活或新机验收。

## 决定

Codex CLI、agy（Antigravity）等终端 agent 在 Herdr pane 里运行，由上游 `agenvo-herdr` 管理。
Herdr 以 headless 服务常驻（launchd 运行 `herdr server`）；用户需要看时在终端执行 `herdr` 接上。

## 理由

- 上游 connector 已支持 workspace、pane、读输出和事件，无需另写 connector 或调度器。
- Codex 派活使用官方 `codex exec`，进程退出即交还 writer，已观察到桌面版通过线程链接打开后显示完整历史、正常输入框，且没有 “This is open in another app” 占用横幅；常驻共享 app-server 会继续占用 writer。
- agy 的无头模式（`agy -p --output-format json`）输出 `conversation_id` 与结果，以 `--conversation` 续接；Antigravity 桌面版没有公开派活接口，2.25.0 也调不出 CLI 会话（`@conversation` 只搜桌面自己的索引，2026-10-22 实测）；加 `--new-project` 只会在桌面侧栏生成一个空项目（显示 No conversations yet），会话本身仍不可见、`@conversation` 搜不到（2026-10-22 用户实测），所以派活不要加 `--new-project`。agy 只作为 Herdr 里的 CLI agent 使用，不承诺桌面可见，也不为它写 connector。

## 约定

- 改代码的任务用 Herdr 原生 `worktree.create` 建 `~/Projects/.worktrees/<repo>/<任务名>`，只读任务用主 checkout；主 checkout 不用来开发。
- 每一轮在任务 workspace 里新开一个 tab，命令以 `echo $? > exit-N; exit` 结尾，tab 关闭（`pane_exited`、`pane.get` 返回 `pane_not_found`）就是这一轮结束；输入输出放 `~/Projects/_status/<任务名>/`。CLI 退出后 Herdr 立即释放该 pane 的 agent，所以不用 agent 状态判断结束（2026-10-23 实测）。
- Codex 每轮用 `codex exec`，追问用 `codex exec resume`（`-C`、`-s` 放在 `resume` 之前，每轮重复 `-s danger-full-access`、`-m` 与 `-c model_provider="magpie"`）；Codex connector 仅列出线程和读取历史。
- full access 参数由调用方传入：Codex 新建与续接每轮都用 `-s danger-full-access`；agy 无头命令每轮都用 `--dangerously-skip-permissions`，交互式 `agent.start` 在 `args` 里传同一参数。实例 `context` 记录实际配置，connector 不强制。
- 新建、跟进、续接、中断与桌面接手的完整步骤见[派活技能](https://github.com/semantic-craft/xw-stack/blob/main/skills/delegate-via-agenvo/SKILL.md)。
