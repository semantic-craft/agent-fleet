# agent-fleet

**agent-fleet 是基于 ANet 的自有舰队编排引擎仓，也是整套舰队编排引擎的唯一正本仓。**

| 仓库 | 职责 |
| --- | --- |
| Reason Network | 会话目录＋跨机会话总线引擎仓 |
| agenvo | 助手↔本机 runtime 的 MCP 中继／连接器引擎仓；旧控制面已退出 |
| ANet | 上游 Hub＋节点派活 runtime |
| agent-fleet | 基于 ANet 的自有舰队编排引擎仓：宿主插件、派活契约、workdir＋new-mac 脚本、验收门禁；不是经验笔记，也不再造 Hub |

Hub 正本是 **https://anet.sonori.dev（ANetHub 0.3.3）**。官方 `hub.agentnetwork.org.cn` 已清仓，不是现行正本。CommHub MCP／#90 门铃与 agenvo-as-control-plane 是遗留，不能与现行 Hub URL 混称。

体系落在专属的 `~/Projects`（少仓；主 A 可换 Grok、Dots 或 Cindy）。四层、外循环 orchestrator、内循环编排和 worker 互派见 [docs/architecture.md](docs/architecture.md)。PODAS 只是伞名，没有目录。

## 当前派活（本机）

舰队身份、任务账本和跨身份委派只走**官方 Agent Network（anet）0.3.9**。正本：[docs/anet.md](docs/anet.md)。官方文档只认 <https://docs.agentnetwork.org.cn>。

编码 agent 接入官方网络：`anet agents wire` 写入客户端条目，`anet mcp` 提供 stdio MCP（[接入编码 Agent](https://docs.agentnetwork.org.cn/docs/zh-cn/latest/howto/coding-agents.html)、[MCP](https://docs.agentnetwork.org.cn/docs/zh-cn/latest/howto/mcp.html)）。

Herdr 只做 pane 宿主，不是派活控制面。不另造 Herdr 与 ANet 之间的桥。

- 身份：`default`／`turing`／`cindy`／`agy`／`dsh`；inbound `closed`；只 allow 自有 AID。
- 派活：`env -u ANET_HOME ANET_DATA_DIR=$HOME/.anet ~/.local/bin/anet --id turing delegate <AID> "…" --watch`。
- Cindy 宿主插件：[plugins/agent-network-cindy](plugins/agent-network-cindy/README.md)（显示名 Cindy × Agent Network；工具 `send_to_peer`、`bridge_status`）。

不另造控制面层，不经 CommHub MCP、不经 #90 门铃桥做本机派活。那些是历史路径，见文末「遗留附录」。

dot 的原生平台任务与 anet 并列，不互相替代。DSH、GrokBot 的历史外部路线在 [docs/legacy/caller-routing.md](docs/legacy/caller-routing.md)。旧 Desktop 交接在 [docs/legacy/codex-desktop-handoff.md](docs/legacy/codex-desktop-handoff.md)，不是现行步骤。

Desktop 进窗（窗口里看得见、结果回得来、cwd 等于 `~/Projects` 里的项目）走 Reason，边界见 [docs/reason.md](docs/reason.md)。同一件活不要既走 ANet 又走 Reason。Reason 签到另线，本文不宣称已通。薄技能：[skills/dispatch-desktop-reason](skills/dispatch-desktop-reason/SKILL.md)。

迁移与升级仍先读[兼容来源、模型选择保护及在途恢复门槛](docs/migration-and-upgrades.md)。

## 组成（当前）

| 层 | 来源 | 本仓库做什么 |
| --- | --- | --- |
| A 规矩 | outer-loop-kit | 不复制分流长文和版本钉。体验与四层见 [docs/architecture.md](docs/architecture.md) |
| B 舰队 | 官方 anet 0.3.9（上游） | 不实现协议。[docs/anet.md](docs/anet.md)：身份、inbound、delegate、env 教训 |
| C 投递 | Reason（上游；签到另线） | 不实现、不宣称已通。边界 [docs/reason.md](docs/reason.md) |
| D 壳 | codex-host（上游） | 不实现、不 fork、不抢壳 |
| 自制派活插件 | 本仓库 [plugins/](plugins/README.md) | 一宿主一目录；Cindy 见 [agent-network-cindy](plugins/agent-network-cindy/README.md) |
| 跨仓库约定与教训 | 本仓库 | 根 [AGENTS.md](AGENTS.md)（其他仓库的同步源）、[lessons.md](lessons.md) |
| 机器清单／脚本（遗留部署仍可用） | 本仓库 | [scripts/fleet.mjs](scripts/fleet.mjs)、[templates/](templates/)、[prompts/](prompts/) |

通用派发契约：[docs/dispatch-contract.md](docs/dispatch-contract.md)。ANet 技能：[skills/delegate-via-anet](skills/delegate-via-anet/SKILL.md)。Desktop 进窗选用：[skills/dispatch-desktop-reason](skills/dispatch-desktop-reason/SKILL.md)。跨 Projects 来源与迁移：[docs/anet-alignment.md](docs/anet-alignment.md)。

## 从哪里开始

- 本机舰队与协议：[docs/anet.md](docs/anet.md)、官方 <https://docs.agentnetwork.org.cn>。
- Desktop 进窗边界（未签到，不宣称已通）：[docs/reason.md](docs/reason.md)。
- 判断 `~/Projects` 目录能不能动：[docs/landscape.md](docs/landscape.md)。
- Agent 接手：跨仓库约定是根 [AGENTS.md](AGENTS.md)。[docs/AGENTS.shared.md](docs/AGENTS.shared.md) 只指向它。本仓再读 [lessons.md](lessons.md)、[docs/architecture.md](docs/architecture.md)。
- 新 Mac 重建与旧 connector 运维：[docs/new-mac.md](docs/new-mac.md)、[docs/runbook.md](docs/runbook.md)、文末附录。
- 项目工作目录与门禁：[docs/workdir.md](docs/workdir.md)。

## 检查

本地验证入口：`scripts/check`（运行 `npm test`，不调用 live daemon）。

- [scripts/gate.sh](scripts/gate.sh)：只读 PR 合并门禁，`gate.sh <owner/repo> <pr> <sha>`；离线正文检查用 `gate.sh --check-body <文件>`。

```sh
npm install
git config core.hooksPath .githooks       # 一次性：提交前自动跑 guard
npm test                                  # 配置解析、context 渲染与私有值检查
node scripts/fleet.mjs doctor             # 本机 overlay 里列出的 launchd 服务是否已加载
```

License: Apache-2.0。


## 遗留附录：Agenvo 与 CommHub

> **历史材料，不是现行派活，不要按本节安装、验收或派活。** 派活只走官方 Agent Network（[docs/anet.md](docs/anet.md)）。CommHub MCP、#90 门铃桥、Agenvo relay 与 `delegate-via-agenvo` 都已退出。

档案（只读）：[docs/legacy/](docs/legacy/README.md)。ADR、旧路由、旧 Desktop 交接、旧验收稿和已删除的 DSH socket 插件都在那里。[docs/caller-routing.md](docs/caller-routing.md) 只留指针。`adapters/dsh` 与 `fleet.mjs release dsh` 已删除，理由见 [docs/legacy/dsh-adapter.md](docs/legacy/dsh-adapter.md)。
