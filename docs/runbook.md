# Runbook

迁移与升级先读[兼容来源、模型选择保护及在途恢复门槛](migration-and-upgrades.md)；历史通过不代表未知新版本可用。

所有命令都在本仓库根目录执行。个人值从 overlay（`~/.config/agent-fleet/fleet.json`，或用 `AGENT_FLEET_CONFIG` 指定）读取，下面用 `<RELAY>` 表示 `relay.baseUrl`。

## 0. 前提

- macOS，Node 24.13 以上。
- 原生运行时自行安装并登录：Codex（`codex` CLI，与 Codex Desktop 共用 `~/.codex`）、Herdr、DSH 桌面版。
- 一个 Cloudflare 账号，以及一个 DNS 托管在该账号里的域名（见 [ADR-0005](decisions/0005-relay-on-cloudflare.md)）。

## 1. 准备 agenvo fork（每台机器）

```sh
git clone -b <agenvo.branch> <agenvo.remote> ~/Projects/_forks/agenvo
cd ~/Projects/_forks/agenvo && npm ci && npm run build
npm link --workspace @agenvo/codex-app-server --workspace @agenvo/herdr --workspace @agenvo/dsh --workspace @agenvo/dispatch
```

## Cloudflare CLI

与 Cloudflare 交互时用 `cf` CLI；项目若仍有 Wrangler 配置文件（wrangler.jsonc／wrangler.toml）且无 cloudflare.config.ts，则该项目继续用 Wrangler；有 Wrangler 配置时不要对该项目跑 cf dev／build／deploy，先 cf migrate。

agenvo fork 目前仍有 `wrangler.jsonc`、无 `cloudflare.config.ts` → **该项目继续用现有 Wrangler 步骤**（下方「2. 部署 relay」里的 `npx wrangler …` 命令保留）。迁到 `cf` 时先在该项目跑 `cf migrate`。未确认已有 `cloudflare.config.ts` 前，部署仍走 Wrangler。

## 2. 部署 relay（只做一次）

在 agenvo checkout 里执行：

```sh
cp wrangler.jsonc wrangler.local.json             # 已被 git 忽略
npx wrangler kv namespace create OAUTH_KV --config wrangler.local.json
# 编辑 wrangler.local.json：name、kv id、vars.BASE_URL=<RELAY>、workers_dev=false、
#   routes=[{"pattern":"<relay 主机名>","custom_domain":true}]
umask 077; openssl rand -hex 32 > ~/.config/agenvo/admin-secret
tr -d '\n' < ~/.config/agenvo/admin-secret | npx wrangler secret put ADMIN_SECRET --config wrangler.local.json
npx wrangler deploy --config wrangler.local.json
curl -s <RELAY>/health
```

把管理员密钥另存进密码管理器。

## 3. 原生服务常驻

```sh
LABEL_PREFIX=dev.yourname node scripts/fleet.mjs launchd codex-app-server > ~/Library/LaunchAgents/dev.yourname.codex-app-server.plist
LABEL_PREFIX=dev.yourname node scripts/fleet.mjs launchd herdr-server     > ~/Library/LaunchAgents/dev.yourname.herdr-server.plist
LABEL_PREFIX=dev.yourname node scripts/fleet.mjs launchd desktop-agents-autostart > ~/Library/LaunchAgents/dev.yourname.desktop-agents-autostart.plist
mkdir -p ~/Library/Logs/codex-app-server ~/Library/Logs/herdr-server
for f in codex-app-server herdr-server desktop-agents-autostart; do launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/dev.yourname.$f.plist; done
```

DSH：每台 Mac 跑 `node scripts/fleet.mjs release dsh --sha <reviewed SHA>`，把本仓库的 [adapters/dsh](../adapters/dsh/README.md) 插件（`agent-fleet-dsh`）链进桌面版的 `desktop` profile；它在 `~/.local/state/agent-fleet-dsh/native.sock`（0600）上转发桌面 Host 的原生会话接口。每次 `release dsh` 之后都要退出并重开 DeepSeek Harness，正在运行的桌面版不会自己加载新插件。

## 4. 添加实例并配对（每台机器、每种 connector）

```sh
agenvo-codex-app-server instance add --id <id> --home ~/.codex --cwd ~/Projects \
  --endpoint unix://$HOME/.codex/app-server-control/app-server-control.sock
agenvo-herdr instance add --id <id> --config-root ~/.config/herdr --cwd ~/Projects
agenvo-dsh instance add --id <id> --endpoint unix://$HOME/.local/state/agent-fleet-dsh/native.sock
agenvo-dispatch instance add --id <id> --endpoint http://HOST:PORT --key-file <ZCode 接收端 key>
```

改了已配对实例的 `endpoint`（换插件、换 socket 路径）后，relay 会把该实例标为未批准，`check` 显示 `NOT IN RELAY`；用管理员 API `POST /api/admin/instances/approve`（`deviceId`、`instanceId` 和 `/api/admin/state` 里该实例的 `fingerprint`）重新批准。

`agenvo-dispatch instance add` 不会设置 `source`：在 `~/.config/agenvo/dispatch/config.json` 里给 ZCode 实例补上 `"source": "agenvo"`。然后：

```sh
node scripts/fleet.mjs contexts --apply      # 把 overlay 里的 context 写进各 connector 配置
AGENVO_ADMIN_SECRET="$(cat ~/.config/agenvo/admin-secret)" agenvo-<connector> connect <RELAY> --name "<机器> / <connector>" --approve
agenvo-<connector> service install
node scripts/fleet.mjs release agenvo --sha <reviewed SHA> # 把刚装好的 connector 改为从固定版本的发布目录运行
```

`service install` 生成的 plist 指向 checkout 里的 `dist/cli.js`；每次 `service install` 之后都要再跑一次 `release agenvo`。

在没有管理员密钥的机器上（例如第二台 Mac）：先用 `connect <RELAY> --no-browser` 拿到配对码和指纹，再在有密钥的机器上执行 `agenvo-dispatch pairing approve CODE --fingerprint SHA256 --base-url <RELAY>`。密钥不要复制到其他机器。

## 5. 验收

网络迁移、桌面连续性和主动事件唤醒分别验收，见[网络与桌面验收](network-and-desktop-acceptance.md)；轮询读到 completed 不证明主会话已被唤醒。

```sh
node scripts/fleet.mjs doctor              # 本机 launchd 服务与 connector 状态
node scripts/fleet.mjs check               # OAuth + search + Codex／Herdr／DSH 实例各一个只读原生调用
node scripts/fleet.mjs check --dispatch    # 另给每个 Herdr 实例跑一轮派活用的 tab（命令结束即 exit，等 pane_not_found 后经第一个 pane 读回退出码，不调模型），给每个 DSH 实例派只读任务等到 turn/end；项目取 overlay 的 check.project（每台派活机器 projectsRoot 下都有的目录），--project 可覆盖，--instance 只查一个实例
node scripts/fleet.mjs check --instance <dsh 实例> --session <sessionId>   # 向一个已有 DSH 会话追加一轮只读输入
```

`check` 只撤销自己创建的测试授权。relay 上已批准、overlay 里却没有的实例会打出 `WARNING`：它们没有受管的 context，也不在检查范围内，要么补进 overlay，要么在 relay 撤销。最后在 ChatGPT 里添加连接器（Plugins → Add custom MCP server，URL 填 `<RELAY>/mcp`，认证选 OAuth），再用 [prompts/acceptance-codex.md](../prompts/acceptance-codex.md) 做界面级验收。

## 发布与回滚

常驻服务从不直接跑 git checkout，否则在 checkout 里切分支或重新构建会立即影响线上。`release` 把一个已推送的提交构建进 `~/.local/share/agent-fleet/releases/<组件>/<sha>/`（带逐文件 sha256 的 `release.json`），把该组件的 launchd plist 指过去，重启并验证：

```sh
node scripts/fleet.mjs release agenvo [--sha SHA]   # 4 个 connector；默认取 overlay 里 agenvo.remote/branch 的最新提交
node scripts/fleet.mjs release agenvo --connector codex [--sha SHA] # 只切换 Codex connector
node scripts/fleet.mjs release agenvo --connector codex --rollback  # 只恢复该范围的上次切换
node scripts/fleet.mjs release dsh    [--sha SHA]   # DSH 插件（adapters/dsh）；改 releases/dsh/live 并更新桌面 profile
node scripts/fleet.mjs release <组件> --rollback     # 恢复上一次切换前的 plist
node scripts/fleet.mjs check                         # 切换后经 relay 端到端验收
```

拒绝的情况：提交不在上述远端分支上；现役发布目录被手改过（与 `release.json` 不符）；同一提交的目录不完整。旧 plist 备份在 `releases/<组件>/backups/`，`current.json` 记录当前版本。

`--connector` 目前仅接受 `release agenvo --connector codex`；其他组件、connector 或非 release 命令会拒绝，`--rollback` 不能与 `--sha` 同用。构建仍复用含四个 connector 的不可变 bundle，但只修改、重启和检查 Codex 的 plist；其他三个 connector 的 plist、进程及全量 `current.json` 不变。Codex 记录在 `current-codex.json`，备份在 `backups/codex/`。

回滚会先校验范围、目标集合、当前入口版本、已记录的 plist 哈希以及旧发布目录完整性，再恢复；后续全量发布使旧 Codex 回滚过期，后续 Codex 发布同样使包含它的旧全量回滚过期。遇到拒绝必须先核对当前版本，不要复制旧记录绕过检查。没有 connector 字段的旧全量记录仍可用，但无法补验当时未记录的 plist 哈希。

范围限定不等于原生任务空闲检查：此命令仍会重启所选 connector，执行前须独立确认它没有活跃任务或待答交互。它不停止共享 app-server、不释放 writer、不改变 endpoint/relay scope，也不代表会话交接已经根治。发布与回滚需串行操作；现有工具不提供跨进程发布锁或中途失败的自动事务恢复。Agenvo 回滚会验证所选 connector 的新 PID 与 online 状态；失败保留带入口及哈希校验的 pending 记录供重试。恢复后仍须经真实 relay 业务入口验收。

## 日常操作

| 要做什么 | 怎么做 |
| --- | --- |
| 改某个实例的 context | 改 overlay，执行 `contexts --apply`，重启对应 connector（`launchctl kickstart -k gui/$(id -u)/<label>`）；不需要重新批准 |
| 改 `source` 或 endpoint | 改 connector 配置并重启，然后在 relay 重新批准：`admin state` 找到指纹，再 `admin approve-instance` |
| 换 relay 域名 | 部署新的 `BASE_URL`，把各 connector 配置里的 `relay` 字段改掉并重启（配对凭据仍有效）；ChatGPT 的连接器要重新添加 |
| 升级 agenvo | 把上游 tag 合并进 fork 分支并推送，重新部署 relay，`release agenvo`，再跑 `check` |
| 撤销访问 | 在 `<RELAY>/admin` 页面撤销，或用 `agenvo-* admin revoke grant|instance|device` |

会话级切模候选（未部署）的固定源码、离线测试与调用入口见 [DSH session-only](dsh-session-only.md)。能力缺失明确不支持，不把旧 `session/selectModel` 当作等价 fallback。

## Codex Desktop 交接

默认 Desktop 空线程流程见[交接契约](codex-desktop-handoff.md)。本 runbook 的独立常驻 app-server 部署没有已验证的单线程 writer 释放路径；connector 发布、回滚或重启均不能充当交接。未确认支持前报告 `blocked_native_writer_release`，不创建或自动重试线程。
