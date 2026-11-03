# 架构

> **当前正本是官方 anet 0.3.9。** 身份、inbound、`delegate --watch` 与 Cindy 插件见 [anet.md](anet.md) 与 [plugins/agent-network-cindy](../plugins/agent-network-cindy/README.md)。CommHub MCP、#90 门铃桥与 agenvo fork 的安装步骤已从本仓库删除。历史档案：[ADR-0001](legacy/0001-agenvo-as-control-plane.md)、[ADR-0002](legacy/0002-minimal-agenvo-fork.md)。

## 目标

1. 人只面对一个入口：手机上的 ChatGPT（主力是 dots），也可以是 Grok Bot。
2. 助手能看到每台电脑上的 agent 在做什么，并能新建任务、追加输入、读结果。
3. 任务在用户平时用的桌面 app 里可见（Codex Desktop、DSH）。
4. 电脑不开入站端口；合盖、断网、关闭聊天都不会中断已经在跑的 agent。

## 当前路径

跨 ANet 身份委派只走官方 anet。不经 CommHub MCP、不经 `anet.sonori.dev`、不经 #90 门铃桥。做法见 [anet.md](anet.md)。编码 agent 用官方 `anet agents wire` 与 `anet mcp` 接入。Herdr 只做 pane 宿主，不是派活控制面。dot 若已有原生平台任务工具且电脑已授权，Codex 可走原生通道（平台执行环境，不是另一 ANet 控制面）。DSH、GrokBot 的历史外部路线见[调用方路由](caller-routing.md)。

桌面 DSH 插件仍是 [adapters/dsh](../adapters/dsh/README.md)，由 `fleet.mjs release dsh` 装进桌面 profile。它不是控制面。

## 历史外部路线（不再安装）

下列句子保留交接契约里的门槛，供旧文档交叉引用。不要按它们重新安装 fork 或门铃。

历史外部路线里，discovery 使用 deviceId／instanceId。DSH、GrokBot 的 Codex Desktop 可见性按[调用方路由](caller-routing.md)各自记录。dot 的原生平台任务不走 CommHub。

**Desktop 交互控制交还门槛**：独立同 host 执行基线不以退订能力为创建前置，Desktop 项保持未验；仅当完成交还时，目标 connector 支持 `thread/unsubscribe`，watchEvents 不收养其他 loaded 线程；否则 `blocked_native_writer_release`。同 host 的共享 app-server 与 Desktop 自带 backend 也必须交还 writer。connector 重启不释放独立 native writer。详见[交接契约](codex-desktop-handoff.md)。
