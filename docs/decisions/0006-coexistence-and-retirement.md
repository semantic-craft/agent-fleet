# ADR-0006：只保留一条远程通路，其余全部退役

状态：已采纳（2026-10-19）。这一版取代同日的两个早期版本，它们分别是"稳定一周后退役"和"Grok 迁出前保留 Yobiru"。

## 决定

所有远程派活只走 Agenvo。不为任何次要入口保留第二条传输链路：保留第二条的维护成本，高于失去那个入口的代价。

| 旧组件 | 去向 | 时机 |
| --- | --- | --- |
| dot 网关、门铃、long-poll、tunnel（agent-interop 的 `dot/`），`dot-relay`，reason-net | 退役 | ChatGPT 里加上 Agenvo 连接器，并且验收通过的当天 |
| Yobiru/tailmsg 及其适配器、部署副本；`agent-bridges` 和三个 Grok 专用桥 | 退役 | 先在 grok.com 上把 relay 加为自定义连接器：连上了就当天退役；连不上，就放弃让 Grok 派活，同样当天退役。不为 Grok 保留 Yobiru |
| `hub_run` 的远程入口 | 退役，2026-10-19 随 agent-interop 删除 | 与 dot 网关一起退役。它只经 DOT 旧网关调用 |
| `bot-handbook`、`grokbot-box-services` | 归档 | Grok 那台机器停用后 |
| agent-collab 协议 | 2026-10-19 删除 | 认领协议与评审工具只服务于 Grok Bot 与 hub_run 时代；其中现役的 `lcu-dsh/` 已带历史拆成 `semantic-craft/lcu-dsh` |

## Grok 的接入事实（2026-10-19 核实）

- Grok 在 grok.com/connectors 支持自定义远程 MCP 加 OAuth。它优先用 URL 形式的 client 身份（CIMD，`https://grok.com/oauth/mcp-client.json`）。
- grok.com 的 Cloudflare 防护会拦截 Worker 发出的请求，返回 403 challenge 页。所以即使在 relay 上打开 CIMD，relay 也取不到这份元数据。因此不打开 CIMD，让 Grok 走动态注册（relay 默认支持）。
- 如果 Grok 不接受动态注册，按上表直接放弃 Grok 派活，不写绕过方案。
- 2026-10-19 实测：Grok 走动态注册成功，授权页显示 client Grok、回调 grok.com；search 与 ZCode 派活都通过。

## 各批次清单（主力 Mac，2026-10-19 核对）

触发条件满足的当天，按下表逐批执行退役步骤。部署副本与状态目录不在 `~/Projects`，与仓库目录一起删除（2026-10-19 起不再归档，见下方执行记录）。

| 批次 | 触发 | launchd label | 部署副本与状态目录 | `~/Projects` 目录 |
| --- | --- | --- | --- | --- |
| A：dot 旧通路 | ChatGPT 连接器验收通过 | `dev.yobiru.dot.longpoll`、`dev.yobiru.dot.bridge`、`dev.yobiru.dot.tunnel`、`dev.yobiru.hub.runner`、`local.agent-infra.health-observation`（只监控这批服务，不同批停掉会持续报警）、`com.reasonmachines.network.*`（含 `.probe`） | `~/.local/share/agent-infra-lessons/` 的 `releases/` 与 `rollback-*`（同目录的 `handbook-*` 随 `bot-handbook` 归档）、`~/.local/share/dot-relay/`、`~/.local/share/agent-infra-health/`、`~/.dot-longpoll/`、`~/.config/dot-hub/`、`~/.reason-net/` | `dot-relay`；agent-interop 的四个 checkout 不移动（`hub_run` 代码冻结、保留） |
| B：Yobiru 与 Grok 桥 | grok.com 添加 relay 连接器的尝试结束（成败都算） | `dev.tailmsg`、`dev.tailmsg.codex-bridge`、`dev.tailmsg.agy-e2e`、`dev.codex.bridge`、`dev.dsh.bridge`、`dev.zcode.bridge` | `~/.tailmsg/` | `agent-bridges`、`codex-task-bridge`、`deepseek-grok-bridge`、`hermes-grokbot-api-bridge`、`tailmsg-adapters`、`dispatch-deploy-semantic-craft` |

不在清单里、必须保留的：`dev.semantic-craft.dsh-grokbot-dispatch` 和 `dev.semantic-craft.zcode-dispatch` 是接收端（名字带 grokbot 也不退役）；`io.agenvo.connector.*` 与 `dev.xw.*` 属于本架构。第二台 Mac 上的 Grok 桥未核对，执行 B 批时在那台机器上按同样方法查 launchd。

## 执行记录

2026-10-19：ChatGPT 连接器验收通过，Grok 经动态注册接入 relay 成功（报告在 overlay 外的验收目录）。当天执行 A、B 两批：两台 Mac 的服务已停、plist 在 `_retired/`；目录与部署副本在 `~/Projects/_archive/dot-path-20261019/`、`yobiru-grok-20261019/`（第二台 Mac 的部署副本在它自己的 `_archive/legacy-transport-20261019/`）；GitHub 上 dot-relay、deepseek-grok-bridge、hermes-grokbot-api-bridge、tailmsg-adapters 已归档（grokbot-to-codex 此前已归档）。剩下 `bot-handbook`、`grokbot-box-services` 按表在 Grok 机器停用后归档。

2026-10-19（续）：Hermes 已不用，`hermes-dsh-dispatch` 退役。第二台 Mac 的 DSH 桌面版改装 dsh-grokbot-dispatch（`/v1/jobs` 只绑本机回环），该机 DSH 实例改附着 `~/.local/state/dsh-grokbot-dispatch/native.sock` 并在 relay 重新批准，`fleet.mjs check --dispatch --instance <该实例>` 通过；那台的 checkout 与状态目录移到它自己的 `_archive/hermes-dsh-dispatch-20261019/`，Hermes 侧 key 已删；GitHub 仓库已归档。

2026-10-19（再续）：用户决定退役路线一律删除、不留归档（“这些都是过去探索的错误路线”）。两台 Mac 上 `_archive/` 里的 dot-path、yobiru-grok、agent-fleet、hermes-dsh-dispatch、legacy-transport 目录，`agent-interop-private` checkout 及其 worktree，`~/.local/share/{dot-direct-project,agent-infra-lessons,zcode-dispatch}`、`~/.local/state/zcode-dispatch/dot-worktrees`、`_retired/` 里对应的 plist 全部删除。ZCode 接收端部署改由 `fleet.mjs release zcode` 负责。

2026-10-19（再续二）：`agent-collab` 删除（本地、`~/.local/state/dot-hub/` 里的 Dot 任务工作副本、GitHub 仓库）；`lcu-dsh/` 先带提交历史拆成独立仓库 `semantic-craft/lcu-dsh`（`~/Projects/lcu-dsh`），线上 `~/.local/share/lcu-dsh/config.json` 的 `workspaceRoot` 改指向它并用线上 release 的加载器校验通过。GitHub 上 agent-interop-private、dot-relay、deepseek-grok-bridge、hermes-grokbot-api-bridge、tailmsg-adapters、grokbot-to-codex、hermes-dsh-dispatch、agent-dispatch-mesh、agent-scripts、pplx-zcode-handshake 已永久删除。

## 退役步骤

1. 在 [landscape](../landscape.md) 把状态改成"退役中"。
2. 用 `launchctl bootout` 停掉服务，plist 移进 `~/Library/LaunchAgents/_retired/`。
3. 跑一次 `fleet.mjs check`。
4. 删除本地目录、部署副本、状态目录和 `_retired/` 里的 plist（先确认没有未推送的有用工作），并把这些仓库从每日维护任务里摘掉。不留 `_archive` 副本。
5. 归档对应的 GitHub 仓库。
