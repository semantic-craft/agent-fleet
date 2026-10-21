# 架构

## 目标

1. 人只面对一个入口：手机上的 ChatGPT（主力是 dots），也可以是 Grok Bot。
2. 助手能看到每台电脑上的 agent 在做什么，并能新建任务、追加输入、读结果。
3. 任务在用户平时用的桌面 app 里可见（Codex Desktop、DSH），也可以在 Herdr 终端里运行（agy、codex CLI、dsh headless）。
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

1. 用户对 dot 说"让 DSH 在 koe 里跑一下测试"。
2. 助手调 `search`，拿到实例列表、在线状态和每个实例的 `context`（写着项目目录、默认权限、适合干什么）。
3. 助手调 `execute`，依次 `call({deviceId, instanceId: "dsh-laptop"}, "workspace/create", {request: {path: ".../koe"}})`、`session/create`、`session/prompt`。
4. relay 经 WSS 把调用交给笔记本上的 `agenvo-dsh`，后者经 Unix socket 调 DSH 桌面版 Host 的原生端点。
5. DSH 桌面在左栏该项目下建出可见会话并开始执行；connector 立即返回 session id。
6. 轮次结束时 connector 转发原生 `turn/end`（`runtime.changed`），dot 被唤醒后调 `session/follow` 读结果，再决定下一步。

Codex 派活经 Herdr 的 `workspace.create` + `pane.run` 运行官方 `codex exec`，追问用 `codex exec resume` 续接同一线程；进程退出后 writer 交还，用户通过 `codex://threads/<thread_id>` 在桌面版继续。Codex connector 只用 `thread/list`（`sourceKinds` 含 `exec`、`modelProviders: []`）和 `thread/read` 读取历史。Claude Code 同样在 Herdr pane 里运行，按派活技能中的完整示例新建与续接，请求经本机 magpie，每轮显式使用 `deepseek/deepseek-flash`（可选第二模型 `minimax-code/MiniMax-M3.1-Flash`，同样经 magpie）；agy 用 `agent.start` + `agent.prompt`。完整命令见[派活技能](../skills/delegate-via-agenvo/SKILL.md)。DSH 的 `agenvo-dsh` 调 DSH 桌面版自己的 `session/list`、`session/create`、`session/prompt`、`session/follow` 等，能列出并续写用户手动开的会话，事件是原生的 `turn/start`、`turn/end`。

## 信任边界

- 访问控制有三层：relay 管理员密钥、设备配对（核对指纹）、MCP 客户端 OAuth 授权。执行权限默认 full access。
- relay 运营方（Cloudflare）能看到流量，不是端到端加密。高度敏感的项目不要经它派活。
- 同一 macOS 用户下 full access 能触及该用户能访问的一切；这是有意的取舍，见 [ADR-0004](decisions/0004-desktop-strict-and-full-access.md)。

