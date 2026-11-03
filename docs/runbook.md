# Runbook

迁移与升级先读[兼容来源、模型选择保护及在途恢复门槛](migration-and-upgrades.md)；历史通过不代表未知新版本可用。

本机派活正本是官方 anet，见 [anet.md](anet.md)。CommHub MCP 与 agenvo fork 的安装、relay 部署和旧验收命令已从本仓库删除。历史档案：[ADR-0001](legacy/0001-agenvo-as-control-plane.md)、[ADR-0002](legacy/0002-minimal-agenvo-fork.md)。

dot→Codex 优先可用且已授权的原生平台任务通道。DSH、GrokBot 的历史外部路线在[归档](legacy/caller-routing.md)，不是现行入口。跨机先发现目标环境。

所有命令都在本仓库根目录执行。个人值从 overlay（`~/.config/agent-fleet/fleet.json`，或用 `AGENT_FLEET_CONFIG` 指定）读取。样例不再包含 relay 或 fork 字段。

故障时先看 [anet.md](anet.md)。旧 connector 诊断已归档：[runtime-recovery.md](legacy/runtime-recovery.md)。

## 0. 前提

- macOS，Node 24.13 以上。
- 原生运行时自行安装并登录：Codex（`codex` CLI，与 Codex Desktop 共用 `~/.codex`）、Herdr、DSH 桌面版。
- 本机派活用官方 anet CLI 与 `~/.anet` 身份，见 [anet.md](anet.md)。不需要为本仓库再部署一套 relay。

## Cloudflare CLI

与 Cloudflare 交互时用 `cf` CLI；项目若仍有 Wrangler 配置文件（wrangler.jsonc／wrangler.toml）且无 cloudflare.config.ts，则该项目继续用 Wrangler；有 Wrangler 配置时不要对该项目跑 cf dev／build／deploy，先 cf migrate。

旧 fork 上的 relay 部署步骤已删除。未确认目标项目已有 `cloudflare.config.ts` 前，不要改用 `cf` 部署那个项目。

## 1. 原生服务常驻

```sh
LABEL_PREFIX=dev.yourname node scripts/fleet.mjs launchd codex-app-server > ~/Library/LaunchAgents/dev.yourname.codex-app-server.plist
LABEL_PREFIX=dev.yourname node scripts/fleet.mjs launchd desktop-agents-autostart > ~/Library/LaunchAgents/dev.yourname.desktop-agents-autostart.plist
mkdir -p ~/Library/Logs/codex-app-server
for f in codex-app-server desktop-agents-autostart; do launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/dev.yourname.$f.plist; done
```

Herdr 只做 pane 宿主。本仓不再提供 `herdr-server` 的现行 launchd 模板；旧文件在 [legacy/herdr-server.plist](legacy/herdr-server.plist)，不要用它新装。本仓没有另一份现行 plist。Helios 上若仍加载该任务，或要改 LaunchAgent PATH，由外循环在本机处理。

DSH 桌面 socket 插件和 `fleet.mjs release dsh` 已退役，见 [legacy/dsh-adapter.md](legacy/dsh-adapter.md)。不要再装 `agent-fleet-dsh`。

## 2. 验收

本机派活验收是 `anet delegate --watch`，加上 [plugins/agent-network-cindy](../plugins/agent-network-cindy/README.md) 的 smoke（A／B 标记）。`fleet.mjs check` 不再探测 hub，调用它会失败并指向 [anet.md](anet.md)。

```sh
node scripts/fleet.mjs doctor              # 本机 launchd 服务与 connector 状态
```

网络迁移、桌面连续性和主动事件唤醒的历史记录在[归档](legacy/network-and-desktop-acceptance.md)。旧界面验收稿在 [prompts/acceptance-codex.md](../prompts/acceptance-codex.md)，不要执行。

## 发布与回滚

`fleet.mjs release` 已退役，调用会失败并指向 [legacy/dsh-adapter.md](legacy/dsh-adapter.md)。不要用它安装或回滚 DSH 插件。

## 日常操作

任务 worktree／分支收尾见根 [AGENTS.md](../AGENTS.md)「worktree 不遗留」。

| 要做什么 | 怎么做 |
| --- | --- |
| 改某个实例的 context | 改 overlay，执行 `contexts --apply`，重启对应 connector（`launchctl kickstart -k gui/$(id -u)/<label>`） |
| 看本机服务是否加载 | `node scripts/fleet.mjs doctor` |
| DSH 桌面插件 | 已退役，见 [legacy/dsh-adapter.md](legacy/dsh-adapter.md) |

会话级切模候选（未部署）的固定源码、离线测试与调用入口见 [DSH session-only](dsh-session-only.md)。能力缺失明确不支持，不把旧 `session/selectModel` 当作等价 fallback。connector 侧的旧补丁文件已从本仓库删除。

## 外部 Codex Desktop 交接

旧交接不是现行派活。全文在 [legacy/codex-desktop-handoff.md](legacy/codex-desktop-handoff.md)。
