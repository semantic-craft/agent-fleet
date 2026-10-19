# 架构

## 目标

1. 人只面对一个入口：手机上的 ChatGPT（主力是 dots），也可以是 Grok Bot。
2. 助手能看到每台电脑上的 agent 在做什么，并能新建任务、追加输入、读结果。
3. 任务在用户平时用的桌面 app 里可见（Codex Desktop、DSH、ZCode），也可以在 Herdr 终端里运行（claude、codex CLI、agy）。
4. 电脑不开入站端口；合盖、断网、关闭聊天都不会中断已经在跑的 agent。

## 组件与职责

| 组件 | 运行在哪 | 职责 | 不负责 |
| --- | --- | --- | --- |
| 助手（ChatGPT / dots / Grok Bot） | 云端 | 理解目标、选实例、派活、读结果、订阅事件 | 执行代码 |
| Agenvo relay | Cloudflare Worker + Durable Object | MCP 端点、OAuth、设备配对与实例审批、把调用路由到 connector、转发 `runtime.changed` 事件 | 存储任务、调度、端到端加密 |
| connector（每种运行时一个进程） | 每台 Mac，launchd | 主动外连 relay；把 `search`/`execute` 翻成原生调用；附带实例 `context` | 启动或停止原生服务 |
| 原生服务 | 每台 Mac | 真正跑 agent：`codex app-server`、`herdr server`、DSH 桌面版（插件在 Unix socket 上转发其原生端点）、ZCode 接收端 | 远程访问控制 |
| 桌面 app | 每台 Mac | 让用户看到并接手会话 | — |

## 一次派活的路径

1. 用户对 dot 说"让 ZCode 在 koe 里跑一下真机测试"。
2. 助手调 `search`，拿到实例列表、在线状态和每个实例的 `context`（写着项目目录、默认权限、适合干什么）。
3. 助手调 `execute`，里面是 `call({deviceId, instanceId: "zcode-laptop"}, "jobs.create", {project: "koe", text: ...})`。
4. relay 经 WSS 把调用交给笔记本上的 `agenvo-dispatch`，后者向 ZCode 接收端 `POST /v1/jobs`，带 `source: "agenvo"`、`runner: "app"`、`mode: "yolo"`。
5. ZCode 桌面在左栏建出可见会话并开始执行；connector 立即返回 job id。
6. connector 每 5 秒轮询自己派出的 job；完成时发出 `runtime.changed`（`nativeType: "job.finished"`），dot 被唤醒后调 `jobs.get` 读结果，再决定下一步。

Codex 和 Herdr 的路径相同，只是原生方法不同（`thread/start` + `turn/start`；`agent.start` + `agent.prompt`），而且可以在同一个 thread 或 pane 里继续追加输入。DSH 也一样：`agenvo-dsh` 调 DSH 桌面版自己的 `session/list`、`session/create`、`session/prompt`、`session/follow` 等，能列出并续写用户手动开的会话，事件是原生的 `turn/start`、`turn/end`。只有 ZCode 的接收端只能新建会话，后续输入要在桌面 app 里手动接着聊。

## 信任边界

- 访问控制有三层：relay 管理员密钥、设备配对（核对指纹）、MCP 客户端 OAuth 授权。执行权限默认 full access。
- relay 运营方（Cloudflare）能看到流量，不是端到端加密。高度敏感的项目不要经它派活。
- 同一 macOS 用户下 full access 能触及该用户能访问的一切；这是有意的取舍，见 [ADR-0004](decisions/0004-desktop-strict-and-full-access.md)。

## 与旧机制的关系

| 旧机制 | 去向 |
| --- | --- |
| Yobiru/tailmsg、dot-relay、agent-interop 的 dot 网关与门铃、各种 Grok↔X bridge | 被 Agenvo 取代，只保留一条通路（[ADR-0006](decisions/0006-coexistence-and-retirement.md)） |
| `hub_run`（lease、锁定 head、独立评审） | 远程入口退役，代码冻结；需要审计的任务走 agent-collab 的 PR 评审 |
| agent-collab（Issue/CLAIM/非作者评审合并） | 保留，是治理规则而不是传输方式 |
| reason-net（agent 会话间互发消息） | 冻结；本架构不需要横向消息 |
