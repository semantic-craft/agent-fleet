# Runbook

迁移与升级先读[兼容来源、模型选择保护及在途恢复门槛](migration-and-upgrades.md)；历史通过不代表未知新版本可用。

本机派活正本是官方 anet，见 [anet.md](anet.md)。CommHub MCP 与 agenvo fork 的安装、relay 部署和旧验收命令已从本仓库删除。历史档案：[ADR-0001](legacy/0001-agenvo-as-control-plane.md)、[ADR-0002](legacy/0002-minimal-agenvo-fork.md)。

dot→Codex 优先可用且已授权的原生平台任务通道。DSH、GrokBot 的 Codex Desktop 交接仍按[调用方路由](caller-routing.md)记载，阻断只作用于该外部路线。跨机先发现目标环境。

所有命令都在本仓库根目录执行。个人值从 overlay（`~/.config/agent-fleet/fleet.json`，或用 `AGENT_FLEET_CONFIG` 指定）读取。样例不再包含 relay 或 fork 字段。

故障恢复先读[运行时、控制面与回流诊断](runtime-recovery.md)。

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
LABEL_PREFIX=dev.yourname node scripts/fleet.mjs launchd herdr-server     > ~/Library/LaunchAgents/dev.yourname.herdr-server.plist
LABEL_PREFIX=dev.yourname node scripts/fleet.mjs launchd desktop-agents-autostart > ~/Library/LaunchAgents/dev.yourname.desktop-agents-autostart.plist
mkdir -p ~/Library/Logs/codex-app-server ~/Library/Logs/herdr-server
for f in codex-app-server herdr-server desktop-agents-autostart; do launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/dev.yourname.$f.plist; done
```

DSH：每台 Mac 跑 `node scripts/fleet.mjs release dsh --sha <reviewed SHA>`，把本仓库的 [adapters/dsh](../adapters/dsh/README.md) 插件（`agent-fleet-dsh`）链进桌面版的 `desktop` profile；它在 `~/.local/state/agent-fleet-dsh/native.sock`（0600）上转发桌面 Host 的原生会话接口。每次 `release dsh` 之后都要退出并重开 DeepSeek Harness，正在运行的桌面版不会自己加载新插件。

## 2. 验收

本机派活验收是 `anet delegate --watch`，加上 [plugins/agent-network-cindy](../plugins/agent-network-cindy/README.md) 的 smoke（A／B 标记）。`fleet.mjs check` 不再探测 hub，调用它会失败并指向 [anet.md](anet.md)。

```sh
node scripts/fleet.mjs doctor              # 本机 launchd 服务与 connector 状态
```

网络迁移、桌面连续性和主动事件唤醒的历史记录见[网络与桌面验收](network-and-desktop-acceptance.md)。界面级说明仍在 [prompts/acceptance-codex.md](../prompts/acceptance-codex.md)。

## 发布与回滚

DSH 插件不从 git checkout 直接链进桌面 profile。`release dsh` 把一个已推送到 `origin/main` 的提交构建进 `~/.local/share/agent-fleet/releases/dsh/<sha>/`（带逐文件 sha256 的 `release.json`），把 `releases/dsh/live` 指过去，并更新桌面 profile：

```sh
node scripts/fleet.mjs release dsh [--sha SHA]
node scripts/fleet.mjs release dsh --rollback
```

拒绝的情况：提交不在 `origin/main` 上；现役发布目录被手改过（与 `release.json` 不符）；同一提交的目录不完整。`--rollback` 不能与 `--sha` 同用。`--connector` 不再接受。

回滚恢复上一次记录的 desktop profile，并在有记录时把 `live` 指回上一份发布。正在运行的桌面版要退出重开才会加载。

## 日常操作

任务 worktree／分支收尾见根 [AGENTS.md](../AGENTS.md)「worktree 不遗留」。

| 要做什么 | 怎么做 |
| --- | --- |
| 改某个实例的 context | 改 overlay，执行 `contexts --apply`，重启对应 connector（`launchctl kickstart -k gui/$(id -u)/<label>`） |
| 看本机服务是否加载 | `node scripts/fleet.mjs doctor` |
| 升级 DSH 插件 | `release dsh --sha <reviewed SHA>`，退出并重开 DeepSeek Harness |

会话级切模候选（未部署）的固定源码、离线测试与调用入口见 [DSH session-only](dsh-session-only.md)。能力缺失明确不支持，不把旧 `session/selectModel` 当作等价 fallback。connector 侧的旧补丁文件已从本仓库删除。

## 外部 Codex Desktop 交接

**Desktop 交互控制交还门槛**：独立同 host 执行基线不以退订能力为创建前置，Desktop 项保持未验；仅当完成交还时，确认 connector 暴露 `thread/unsubscribe` 且 watcher 不抢其他 loaded 线程，否则 `blocked_native_writer_release`。同 host 共享 app-server 与 Desktop 自带 backend 也要交还 writer；本次 turn 终态后由同一 connector 退订，再截图核实项目下可见且无横幅。connector 重启不释放独立 native writer。目标仍为 Desktop 可见后 agent `turn/start`，见[交接契约](codex-desktop-handoff.md)。
