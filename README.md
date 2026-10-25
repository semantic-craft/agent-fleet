# agent-fleet

迁移与升级先读[兼容来源、模型选择保护及在途恢复门槛](docs/migration-and-upgrades.md)；历史通过不代表未知新版本可用。

按调用方能力选择派活入口：dot 有原生平台任务工具时，直接给 Codex 创建任务、执行并读回结果；DSH、GrokBot 经 Agenvo 给 Codex Desktop 派活的需求仍须保留。Agenvo 同时承接 DSH 桌面版与 Herdr CLI 路线。

## 入口与状态

| 调用方 → 目标 | 路线 | 边界 |
| --- | --- | --- |
| dot → Codex | 可用且已授权的原生平台任务通道 | 本次维护已实际使用；依赖平台工具及电脑连接授权，跨机先发现环境 |
| dot／其他已授权 MCP 调用方 → DSH／Herdr | Agenvo | 按原生会话／pane 读取实际结果；模型与工具验收边界分别保留 |
| DSH → Codex Desktop；GrokBot → Codex Desktop | Agenvo → Codex app-server → Desktop | 明确需要保留的外部路线；writer handoff 尚未验通，不能用 dot 原生工具替代 |

详细选择与验收见[调用方路由](docs/caller-routing.md)。外部路线的 `blocked_native_writer_release` 不表示 dot 原生 Codex 派活不可用。也不假定任意第三方 agent 拥有平台任务工具。

## 目的

1. **按真实能力派活**：创建、执行、读回各自走支持的入口，输入接受不等于业务完成。
2. **以上游为底座**：Agenvo 部分复用 [Xuanwo/agenvo](https://github.com/Xuanwo/agenvo)，不另造 dispatcher。
3. **只做边际补丁**：只补上游缺失且明确需要的能力；源候选与已部署能力分开。
4. **可复用**：保存架构、重建与验收步骤；个人值只放仓库外 overlay，见 [fleet.example.json](fleet.example.json)。

`fleet.mjs check --dispatch` 验 Agenvo 的 DSH／Herdr 路径，不覆盖 dot 原生任务或外部 Codex writer 交接。界面验收按[提示词](prompts/acceptance-codex.md)逐路线记录，不把一个阻断概括为全系统失败。

```text
dot（具有平台任务工具及授权） ──原生平台任务通道──► Codex
已授权 MCP 调用方 ──Agenvo──► DSH／Herdr
DSH／GrokBot ──Agenvo──► Codex app-server ──待验 writer handoff──► Codex Desktop
```

本仓库不实现或替代平台任务服务，不承诺跨入口共享同一会话、writer 迁移或所有客户端具有同等工具。

## 组成

| 层 | 来源 | 本仓库做什么 |
| --- | --- | --- |
| relay 与 Codex/Herdr connector | 上游 [Xuanwo/agenvo](https://github.com/Xuanwo/agenvo)，原样使用 | 记录部署方式与验收 |
| `dsh` connector | agenvo 的小补丁分支（新增实例 kind 与原生的 `apps/dsh`；分支里还有 ZCode 用的 `apps/dispatch`，见 [landscape](docs/landscape.md)），见 [ADR-0002](docs/decisions/0002-minimal-agenvo-fork.md) | 固定分支、说明何时可以去掉 |
| 桌面插件与接收端 | 本仓库 [adapters/dsh](adapters/dsh/README.md)（DSH 原生桥接插件） | 代码、测试与 `fleet.mjs release` 部署 |
| 机器清单、context、launchd、验收 | 本仓库 | [scripts/fleet.mjs](scripts/fleet.mjs)、[templates/](templates/)、[prompts/](prompts/) |

## 从哪里开始

- 想理解为什么这样设计：[docs/architecture.md](docs/architecture.md)、[docs/decisions/](docs/decisions/)。
- 新 Mac 加入（例如再添一台 Mac mini）：[docs/new-mac.md](docs/new-mac.md)；完整命令：[docs/runbook.md](docs/runbook.md)。
- 想知道本机 `~/Projects` 里那么多相似目录各是什么：[docs/landscape.md](docs/landscape.md)。
- 给 ChatGPT、dot、Grok、Codex 的现成提示词：[prompts/](prompts/)；让 agent 经 Agenvo 把活交给其他 agent 的技能：[skills/delegate-via-agenvo](skills/delegate-via-agenvo/SKILL.md)。
- 例行维护和跟进上游：[docs/maintenance.md](docs/maintenance.md)。
- Agent 接手维护：先读 [AGENTS.md](AGENTS.md) 和 [lessons.md](lessons.md)。

## 检查

- [scripts/gate.sh](scripts/gate.sh)：只读 PR 合并门禁，`gate.sh <owner/repo> <pr> <sha>`；离线正文检查用 `gate.sh --check-body <文件>`。

```sh
npm install
git config core.hooksPath .githooks       # 一次性：提交前自动跑 guard
npm test                                  # 配置解析、context 渲染与私有值检查
node scripts/fleet.mjs doctor             # 本机 connector 与原生服务状态
node scripts/fleet.mjs check              # 经 relay 走一遍 OAuth + MCP，列出所有实例，结束时撤销测试授权
node scripts/fleet.mjs check --dispatch   # 另给每个 Herdr 实例跑一轮 tab（不调模型），给每个 dsh 实例派一个只读任务，等到原生 turn/end 读回结果
node scripts/fleet.mjs upstream           # fork 与上游的差距，以及是否到了去掉 fork 的时候
node scripts/fleet.mjs release agenvo|dsh     # 把已推送的提交装进固定版本的发布目录并切换 launchd（--rollback 回退）
```

License: Apache-2.0。
