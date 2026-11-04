# 跨仓库约定

本文件是各仓库共用的 agent 约定原文。其他仓库从 agent-fleet 仓库根的本文件同步。本仓的接入、维护和 CI 细则不在这里。

## 派活

派活正本是官方 Agent Network（ANet）Research。本机身份、`delegate` 和插件以 agent-fleet 仓库的 `docs/anet.md` 与官方文档为准：<https://docs.agentnetwork.org.cn>。

编码 agent 用官方 `anet agents wire` 与 `anet mcp` 接入（[接入编码 Agent](https://docs.agentnetwork.org.cn/docs/zh-cn/latest/howto/coding-agents.html)、[MCP](https://docs.agentnetwork.org.cn/docs/zh-cn/latest/howto/mcp.html)）。Herdr 只做 pane 宿主，不是派活入口，也不另造到 ANet 的桥。

舰队身份、任务账本和跨身份委派只走 ANet。要进已经打开的 Desktop 窗口，并且 cwd 等于 `~/Projects` 里的项目时，投递走 Reason：边界见 `docs/reason.md`，选用见 `skills/dispatch-desktop-reason`。同一件活只选一条，禁止同时发 ANet 任务和 Reason 进窗。Reason 签到另线，未签到不写成已通。体系落在专属的 `~/Projects`（少仓；主 A 可换 Grok、Dots 或 Cindy），叙事见 `docs/architecture.md`。

Hub 正本（2026-11-04 社长／现网）：自建 `https://anet.sonori.dev`（ANetHub 0.3.3）；本机五身份 Registered。官方 `hub.agentnetwork.org.cn` 已清仓，不作现行正本。

历史遗留，不要当作派活入口：agenvo、Hermes、LCU 自定义 MCP、旧 CommHub MCP／#90 门铃桥。**勿把自建 Hub URL 与旧 CommHub MCP 混称遗留。**

未点名时，派活交给 Codex。DSH 是同等能力的后备：须能列出、读输出、新建、追问、中断、看事件，任务在 DSH 桌面可见。Codex 不可用（额度或故障）时由 DSH 接上。不要把 DSH 退回到只能派任务的程度。

## 审查

审查、审稿、独立审默认 helios-codex。只有用户点名，或项目写明时，才用 agy 或 DSH。

## worktree 不遗留（2026-10-29 社长定）

任务在 worktree 里做，不改 `main` 的工作区。包括 subagent 的 `.claude/worktrees/agent-*`，以及 Herdr 的 `~/Projects/.worktrees/<仓库>/<任务>`。

完成后删除该 worktree 及其分支；专用 cargo target、`node_modules`、`.next` 一并清掉。删除前二选一：已经合并进 `main`；或尚未合并时，先把未提交改动提交到该分支，打 `archive/<worktree名>` tag，再 `git worktree remove` 与 `git branch -D`。编排 subagent 的会话负责收尾它派生的 worktree。确需保留的（待裁决、有冲突待解）在最终回复里写明路径和原因。
