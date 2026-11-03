# 架构

> **当前正本是官方 anet 0.3.9。** 身份、inbound、`delegate --watch` 与 Cindy 插件见 [anet.md](anet.md) 与 [plugins/agent-network-cindy](../plugins/agent-network-cindy/README.md)。CommHub MCP、#90 门铃桥与 agenvo fork 的安装步骤已从本仓库删除。历史档案：[ADR-0001](legacy/0001-agenvo-as-control-plane.md)、[ADR-0002](legacy/0002-minimal-agenvo-fork.md)。

## 目标

1. 人只面对一个入口：手机上的 ChatGPT（主力是 dots），也可以是 Grok Bot。
2. 助手能看到每台电脑上的 agent 在做什么，并能新建任务、追加输入、读结果。
3. 任务在用户平时用的桌面 app 里可见（Codex Desktop、DSH）。
4. 电脑不开入站端口；合盖、断网、关闭聊天都不会中断已经在跑的 agent。

## 当前路径

跨 ANet 身份委派只走官方 anet。不经 CommHub MCP、不经 `anet.sonori.dev`、不经 #90 门铃桥。做法见 [anet.md](anet.md)。编码 agent 用官方 `anet agents wire` 与 `anet mcp` 接入。Herdr 只做 pane 宿主，不是派活控制面。dot 若已有原生平台任务工具且电脑已授权，Codex 可走原生通道（平台执行环境，不是另一 ANet 控制面）。DSH、GrokBot 的历史外部路线在[归档](legacy/caller-routing.md)，不是现行入口。

DSH 桌面 socket 插件已退役，见 [legacy/dsh-adapter.md](legacy/dsh-adapter.md)。它不是控制面。

## 历史外部路线（不再安装）

不要重新安装 fork 或门铃。历史 discovery 使用 deviceId／instanceId，全文在归档。dot 的原生平台任务不走 CommHub。旧 Desktop 交接见 [legacy/codex-desktop-handoff.md](legacy/codex-desktop-handoff.md)。
