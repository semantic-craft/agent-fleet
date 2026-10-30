# agent-fleet

迁移与升级先读[兼容来源、模型选择保护及在途恢复门槛](docs/migration-and-upgrades.md)；历史通过不代表未知新版本可用。

远程控制面是 CommHub（Agent Network／anet），见 [ADR-0001](docs/decisions/0001-agenvo-as-control-plane.md)。按调用方能力选择派活入口：dot 有原生平台任务工具时，直接给 Codex 创建任务、执行并读回结果。Grok 外环走 CommHub MCP；本机接收端走 #90 门铃桥。DSH、GrokBot 的 Codex Desktop 交接仍按调用方路由单独验收。

## 入口与状态

| 调用方 → 目标 | 路线 | 边界 |
| --- | --- | --- |
| dot → Codex | 可用且已授权的原生平台任务通道 | 本次维护已实际使用；依赖平台工具及电脑连接授权，跨机先发现环境 |
| Grok 外环 → 节点 | CommHub MCP（hub `/mcp`） | `send_task` 与需求卡；地址和 token 只在环境变量或 overlay |
| 本机接收端（Herdr 等） | #90 门铃桥 | SSE `/events/<alias>`，`get_inbox`／`ack_inbox`／`send_reply` |
| DSH → Codex Desktop；GrokBot → Codex Desktop | 桌面交接仍单独验收 | 同 host 执行基线已验，实际调用方与 Desktop 可见仍未验；不能用 dot 原生工具替代 |

详细选择与验收见[调用方路由](docs/caller-routing.md)。仅跨 backend 的 `blocked_native_writer_release` 不表示 dot 原生 Codex 派活不可用。也不假定任意第三方 agent 拥有平台任务工具。

## 目的

1. **按真实能力派活**：创建、执行、读回各自走支持的入口，输入接受不等于业务完成。
2. **控制面用 CommHub**：远程派活走 Agent Network 的 CommHub（anet），不另造 dispatcher。部分桌面 connector 仍来自 [Xuanwo/agenvo](https://github.com/Xuanwo/agenvo) 的小 fork，那是附着层。
3. **只做边际补丁**：只补上游缺失且明确需要的能力；源候选与已部署能力分开。
4. **可复用**：保存架构、重建与验收步骤；个人值只放仓库外 overlay，见 [fleet.example.json](fleet.example.json)。

`fleet.mjs check` 验 CommHub。`check --legacy-agenvo --dispatch` 才验 DSH／Herdr connector，不覆盖 dot 原生任务或外部 Codex writer 交接。界面验收按[提示词](prompts/acceptance-codex.md)逐路线记录，不把一个阻断概括为全系统失败。

```text
dot（具有平台任务工具及授权） ──原生平台任务通道──► Codex
Grok 外环 ──CommHub MCP──► anet 节点
本机接收端 ──#90 门铃桥──► Herdr／Antigravity／ZCode 等
DSH／GrokBot 的 Codex Desktop 可见性另验
                      └─仅跨 backend 转移时核验 writer handoff
```

本仓库不实现或替代平台任务服务，不承诺跨入口共享同一会话、writer 迁移或所有客户端具有同等工具。

## 组成

| 层 | 来源 | 本仓库做什么 |
| --- | --- | --- |
| 控制面 CommHub | Agent Network（`~/Projects/_forks/agent-network`） | [adapters/commhub](adapters/commhub/client.mjs)：MCP 客户端与 #90 门铃桥 |
| Codex/Herdr connector | 上游 [Xuanwo/agenvo](https://github.com/Xuanwo/agenvo)，原样使用 | 记录部署方式与验收 |
| `dsh` connector | agenvo 的小补丁分支（新增实例 kind 与原生的 `apps/dsh`；分支里还有 ZCode 用的 `apps/dispatch`，见 [landscape](docs/landscape.md)），见 [ADR-0002](docs/decisions/0002-minimal-agenvo-fork.md) | 固定分支、说明何时可以去掉 |
| 桌面插件与接收端 | 本仓库 [adapters/dsh](adapters/dsh/README.md)（DSH 原生桥接插件） | 代码、测试与 `fleet.mjs release` 部署 |
| 机器清单、context、launchd、验收 | 本仓库 | [scripts/fleet.mjs](scripts/fleet.mjs)、[templates/](templates/)、[prompts/](prompts/) |

## 从哪里开始

- 想理解为什么这样设计：[docs/architecture.md](docs/architecture.md)、[docs/decisions/](docs/decisions/)。
- 新 Mac 加入（例如再添一台 Mac mini）：[docs/new-mac.md](docs/new-mac.md)；完整命令：[docs/runbook.md](docs/runbook.md)。
- 想知道本机 `~/Projects` 里那么多相似目录各是什么：[docs/landscape.md](docs/landscape.md)。
- 给 ChatGPT、dot、Codex 的现成提示词：[prompts/](prompts/)。Grok 外环见 [prompts/grok.md](prompts/grok.md)（CommHub MCP）。桌面／CLI 附着步骤仍在[派活技能](https://github.com/semantic-craft/xw-stack/blob/main/skills/delegate-via-agenvo/SKILL.md)。
- 例行维护和跟进上游：[docs/maintenance.md](docs/maintenance.md)。
- Agent 接手维护：先读 [AGENTS.md](AGENTS.md) 和 [lessons.md](lessons.md)。

## 检查

- [scripts/gate.sh](scripts/gate.sh)：只读 PR 合并门禁，`gate.sh <owner/repo> <pr> <sha>`；离线正文检查用 `gate.sh --check-body <文件>`。

```sh
npm install
git config core.hooksPath .githooks       # 一次性：提交前自动跑 guard
npm test                                  # 配置解析、context 渲染与私有值检查
node scripts/fleet.mjs doctor             # 本机 connector 与原生服务状态
node scripts/fleet.mjs check              # 默认检查 CommHub `/mcp`（`ANET_HUB`，否则本机 127.0.0.1:9200）
node scripts/fleet.mjs check --legacy-agenvo          # 旧 relay：OAuth + MCP，列出实例，结束时撤销测试授权
node scripts/fleet.mjs check --legacy-agenvo --dispatch   # 另给每个 Herdr 实例跑一轮 tab（不调模型），给每个 dsh 实例派一个只读任务，等到原生 turn/end 读回结果
node scripts/fleet.mjs upstream           # fork 与上游的差距，以及是否到了去掉 fork 的时候
node scripts/fleet.mjs release agenvo|dsh     # 把已推送的提交装进固定版本的发布目录并切换 launchd（--rollback 回退）
```

License: Apache-2.0。

### DSH 侧的调用适配

外环派活走 CommHub MCP。下列适配是 DSH 进程里的调用方，不是控制面。[原生 scoped 客户端候选](adapters/dsh-agenvo-native/README.md)复用 DSH credentials、官方工具投影与 SDK OAuth；源码与离线 fixture 不等于真实授权或已部署。此前[外置桥候选](adapters/dsh-agenvo/README.md)的固定 transport 曾安装并短暂启动，暂停后已按用户要求移入废纸篓，客户端注册/授权页已产生，但未确认 grant、未加载 DSH 插件。它与入站 `agent-fleet-dsh` 分开，不新增 relay，不把 raw execute 冒称持久幂等调度器。真实接入所需用户授权、范围、迁移与撤销步骤见该说明。
