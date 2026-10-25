# 架构

## 目标

1. 人只面对一个入口：手机上的 ChatGPT（主力是 dots），也可以是 Grok Bot。
2. 助手能看到每台电脑上的 agent 在做什么，并能新建任务、追加输入、读结果。
3. 任务在用户平时用的桌面 app 里可见（Codex Desktop、DSH；agy 会话只在 Herdr 终端可见，Antigravity 桌面版 2.25.0 调不出 CLI 会话），也可以在 Herdr 终端里运行（agy、codex CLI、dsh headless）。
4. 电脑不开入站端口；合盖、断网、关闭聊天都不会中断已经在跑的 agent。

## 调用方入口

先按[调用方路由](caller-routing.md)区分 dot 原生 Codex 任务、Agenvo→DSH／Herdr、DSH／GrokBot→Agenvo→Codex Desktop。原生任务工具依赖调用方与连接授权；外部 writer 阻断不影响 dot 日常 Codex 路线，也不能用 dot 工具替代外部需求。

## Agenvo 组件与职责

| 组件 | 运行在哪 | 职责 | 不负责 |
| --- | --- | --- | --- |
| 助手（ChatGPT / dots / Grok Bot） | 云端 | 理解目标、选实例、派活、读结果、订阅事件 | 执行代码 |
| Agenvo relay | Cloudflare Worker + Durable Object | MCP 端点、OAuth、设备配对与实例审批、把调用路由到 connector、转发 `runtime.changed` 事件 | 存储任务、调度、端到端加密 |
| connector（每种运行时一个进程） | 每台 Mac，launchd | 主动外连 relay；把 `search`/`execute` 翻成原生调用；附带实例 `context` | 启动或停止原生服务 |
| 原生服务 | 每台 Mac | 真正跑 agent：`codex app-server`、`herdr server`、DSH 桌面版（插件在 Unix socket 上转发其原生端点） | 远程访问控制 |
| 桌面 app | 每台 Mac | 让用户看到并接手会话 | — |

## 一次派活的路径

1. 用户对 dot 说"让 DSH 在 koe 里跑一下测试"。
2. 助手调 `search`，拿到实例列表、在线状态和每个实例的 `context`（写着项目目录、默认权限、适合干什么）。
3. 助手调 `execute`，依次 `call({deviceId, instanceId: "dsh-laptop"}, "workspace/create", {request: {path: ".../koe"}})`、`session/create`、`session/prompt`。改代码的任务先经同机 Herdr 实例 `worktree.create` 建任务目录，`path` 用任务目录。
4. relay 经 WSS 把调用交给笔记本上的 `agenvo-dsh`，后者经 Unix socket 调 DSH 桌面版 Host 的原生端点。
5. DSH 桌面在左栏该项目下建出可见会话并开始执行；connector 立即返回 session id。
6. 轮次结束时 connector 转发原生 `turn/end`（`runtime.changed`），dot 被唤醒后调 `session/follow` 读结果，再决定下一步。

**外部 Codex Desktop**（DSH／GrokBot 经 Agenvo 的明确需求，尚未验通）：

0. **创建前门槛**：缺目标组合原生交接及后续 agent 提交的完整 canary 证据时，报告 `blocked_native_writer_release`，不创建线程；connector 重启不释放独立 native writer。
1. discovery 选定 Codex deviceId/instanceId，brief 与 cwd 均在该目标机器，回读确认可用；`thread/start` 建空线程 → `threadId`。
2. 按已验证路径完成原生 ownership 交接，Desktop 接手；见[交接契约](codex-desktop-handoff.md)。
3. 在目标机器打开 `codex://threads/<threadId>?prompt=…`，确认 Desktop 可见。
4. 经 agenvo 对同一线程 `turn/start` 提交；Desktop 可见与 turn 结果分别验收。

**Claude Code、agy、dsh headless** 在 Herdr 里跑：

1. **任务目录**：改代码用 Herdr `worktree.create` 建 `~/Projects/.worktrees/<repo>/<任务名>`（分支 `agent/<任务名>`，基于 `origin/main`），只读用主 checkout；合并后 `worktree.remove`。
2. **一轮一个 tab**：`tab.create` + `pane.run` 写 brief、跑 CLI（`claude -p`、`agy -p`、`dsh --profile headless`），以 `echo $? > exit-N; exit` 收尾；结束看 `pane_exited` 或 `pane.get` → `pane_not_found`。
3. **续接**：同一任务目录新开 tab（`claude --resume`、`agy --conversation`、`dsh --session-id`）。

要在桌面**项目下**自动开跑：派 DSH（`workspace/create`、`session/create`、`session/prompt`、`session/follow`）。完整步骤见[派活技能](../skills/delegate-via-agenvo/SKILL.md)。

connector 与 relay 之间是会断开重连的长连接（2026-10-23 实测：relay 主机走机场线路时 31.5 分钟断 30 次，网关规则改走直连后 31.6 分钟断 1 次，见[网络与桌面验收](network-and-desktop-acceptance.md#重连可观察性)）。断线时进行中的调用返回 `unknown`、事件不补发，所以派活方以原生状态和 `_status/<任务名>/` 里的文件为准，事件只用来提早醒来。

## 信任边界

- 访问控制有三层：relay 管理员密钥、设备配对（核对指纹）、MCP 客户端 OAuth 授权。执行权限默认 full access。
- relay 运营方（Cloudflare）能看到流量，不是端到端加密。高度敏感的项目不要经它派活。
- 同一 macOS 用户下 full access 能触及该用户能访问的一切；这是有意的取舍，见 [ADR-0004](decisions/0004-desktop-strict-and-full-access.md)。

