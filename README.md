# agent-fleet

**agent-fleet 是多 agent 协作的唯一正本**：协议说明、教训、当前实现和自制派活插件（[plugins/](plugins/README.md)）都在这里。

## 当前派活（本机）

本机多 agent 派活只走**官方 Agent Network（anet）0.3.9**。正本：[docs/anet.md](docs/anet.md)。官方文档只认 <https://docs.agentnetwork.org.cn>。

编码 agent 接入官方网络：`anet agents wire` 写入客户端条目，`anet mcp` 提供 stdio MCP（[接入编码 Agent](https://docs.agentnetwork.org.cn/docs/zh-cn/latest/howto/coding-agents.html)、[MCP](https://docs.agentnetwork.org.cn/docs/zh-cn/latest/howto/mcp.html)）。

Herdr 只做 pane 宿主，不是派活控制面。不另造 Herdr 与 ANet 之间的桥。

- 身份：`default`／`turing`／`cindy`；inbound `closed`；只 allow 自有 AID。
- 派活：`env -u ANET_HOME ANET_DATA_DIR=$HOME/.anet ~/.local/bin/anet --id turing delegate <AID> "…" --watch`。
- Cindy 宿主插件：[plugins/agent-network-cindy](plugins/agent-network-cindy/README.md)（显示名 Cindy × Agent Network；工具 `send_to_peer`、`bridge_status`）。

不另造控制面层，不经 CommHub MCP、不经 `anet.sonori.dev`、不经 #90 门铃桥做本机派活。那些是历史路径，见文末「遗留附录」。

dot 的原生平台任务与 anet 并列，不互相替代。DSH、GrokBot 的历史外部路线见[调用方路由](docs/caller-routing.md)。交还 Desktop 时才涉及 `thread/unsubscribe`，见[交接契约](docs/codex-desktop-handoff.md)。

迁移与升级仍先读[兼容来源、模型选择保护及在途恢复门槛](docs/migration-and-upgrades.md)。

## 组成（当前）

| 层 | 来源 | 本仓库做什么 |
| --- | --- | --- |
| 协议与本机节点 | 官方 anet 0.3.9 | [docs/anet.md](docs/anet.md)：身份、inbound、delegate、env 教训 |
| 自制派活插件 | 本仓库 [plugins/](plugins/README.md) | 一宿主一目录；Cindy 见 [agent-network-cindy](plugins/agent-network-cindy/README.md) |
| 跨仓库约定与教训 | 本仓库 | 根 [AGENTS.md](AGENTS.md)（其他仓库的同步源）、[lessons.md](lessons.md) |
| 机器清单／脚本（遗留部署仍可用） | 本仓库 | [scripts/fleet.mjs](scripts/fleet.mjs)、[templates/](templates/)、[prompts/](prompts/) |

通用派发契约：[docs/dispatch-contract.md](docs/dispatch-contract.md)。技能正文：[skills/delegate-via-anet](skills/delegate-via-anet/SKILL.md)。跨 Projects 来源与迁移：[docs/anet-alignment.md](docs/anet-alignment.md)。

## 从哪里开始

- 本机派活与协议：[docs/anet.md](docs/anet.md)、官方 <https://docs.agentnetwork.org.cn>。
- 判断 `~/Projects` 目录能不能动：[docs/landscape.md](docs/landscape.md)。
- Agent 接手：跨仓库约定是根 [AGENTS.md](AGENTS.md)。[docs/AGENTS.shared.md](docs/AGENTS.shared.md) 只指向它。本仓再读 [lessons.md](lessons.md)、[docs/architecture.md](docs/architecture.md)。
- 新 Mac／旧 connector 运维（遗留）：[docs/new-mac.md](docs/new-mac.md)、[docs/runbook.md](docs/runbook.md)、文末附录。

## 检查

- [scripts/gate.sh](scripts/gate.sh)：只读 PR 合并门禁，`gate.sh <owner/repo> <pr> <sha>`；离线正文检查用 `gate.sh --check-body <文件>`。

```sh
npm install
git config core.hooksPath .githooks       # 一次性：提交前自动跑 guard
npm test                                  # 配置解析、context 渲染与私有值检查
node scripts/fleet.mjs doctor             # 本机 connector 与原生服务状态
node scripts/fleet.mjs release dsh        # 把已推送的提交装进固定版本的发布目录并链到 DSH 桌面 profile（--rollback 回退）
```

License: Apache-2.0。


## 遗留附录：Agenvo 与 CommHub

> **历史材料，不是现行派活，不要按本节安装、验收或派活。** 派活只走官方 Agent Network（[docs/anet.md](docs/anet.md)）。CommHub MCP、`anet.sonori.dev`、#90 门铃桥、Agenvo relay 与 `delegate-via-agenvo` 都已退出。

档案（只读）：[docs/legacy/](docs/legacy/README.md) 下的 ADR-0001（CommHub 曾为远程控制面）、ADR-0002、ADR-0006。旧叙述仍在 [docs/architecture.md](docs/architecture.md) 与 [docs/caller-routing.md](docs/caller-routing.md)，文首已标遗留。DSH 桌面插件 [adapters/dsh](adapters/dsh/README.md) 的退役另批，不是本附录的派活入口。
