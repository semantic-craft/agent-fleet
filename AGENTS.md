# agent-fleet 维护约定

本仓库是"助手经 Agenvo 给自有电脑上的 agent 派活"这套基础设施的设计原件。没有对话记忆的 agent 只要按本文件和链接的决策行事，就应能做出与原设计一致的选择。

## 先读什么

| 任务 | 读 |
| --- | --- |
| 任何改动前 | [docs/architecture.md](docs/architecture.md)、[lessons.md](lessons.md) |
| 新 Mac 加入、重装 | [docs/new-mac.md](docs/new-mac.md) |
| 接入新的 agent 运行时或新机器 | [docs/decisions/](docs/decisions/) 全部、[docs/runbook.md](docs/runbook.md) |
| 判断某个 `~/Projects` 目录能否动 | [docs/landscape.md](docs/landscape.md) |
| 改脚本 | [scripts/fleet.mjs](scripts/fleet.mjs)、[test/](test/)；`npm test` |
| 例行维护、跟进上游、新增组件前 | [docs/maintenance.md](docs/maintenance.md) |

## 决策规则（按优先级）

0. **Codex 是主力，DSH 是同等能力的后备。** 默认把活交给 Codex；DSH 必须保持和 Codex 同级的原生能力（列出、读输出、新建、追问、中断、事件，任务在 DSH 桌面可见），以便 Codex 不可用（额度、故障）时无缝顶上。任何改动都不能让 DSH 退回到只能派任务的程度。

1. **先用上游。** 运行时若已有 Agenvo 官方 connector（Codex app-server、Herdr、Paseo、OpenCode、Amp、Lody），直接用，不写自己的。CLI 型 agent（claude、codex CLI、agy 等）一律放进 Herdr pane，不新增 connector。见 [ADR-0003](docs/decisions/0003-herdr-for-cli-agents.md)。
2. **只为上游没有 connector、但用户要求在其桌面 app 里可见的运行时写 connector**。优先附着运行时自己的原生会话接口（DSH：桌面 Host 的 Typert 端点，经插件 native bridge），保留原生方法名；只有拿不到原生接口时才薄封装接收端 HTTP 契约（ZCode）。都不引入调度、队列或状态机。见 [ADR-0002](docs/decisions/0002-minimal-agenvo-fork.md)。
3. **fork 补丁最小化。** 对 agenvo 的改动只限新增 kind、`apps/dsh` 与 `apps/dispatch`。上游一旦支持外部 kind，就去掉 fork。不在 fork 里写部署记录或个人值。
4. **个人值不进本仓库。** 域名、IP、账号、主机名、路径写在 overlay（默认 `~/.config/agent-fleet/fleet.json`，可用 `AGENT_FLEET_CONFIG` 覆盖）。如果某项改造离开个人值就没意义，先怀疑这项改造的必要性。
5. **桌面可见、full access。** 派给 DSH/ZCode 的任务必须 `runner: "app"`，app 不在就明确失败，不退回 headless。执行权限默认 full access，访问控制靠 relay 的设备配对与 OAuth 授权。见 [ADR-0004](docs/decisions/0004-desktop-strict-and-full-access.md)。
6. **一个 relay，一个 owner。** 所有已授权客户端都能访问所有已批准实例；测试授权用完即撤，不碰用户的正式授权。

## 验收与交付

- 生产验收走真实入口：OAuth → relay `/mcp` → connector → 原生服务；只看 `/health` 不算。`fleet.mjs check` 是最小基线。
- 区分"输入已接受""原生轮次完成""业务目标完成"。
- 改实例 `source`、endpoint 等 scope 字段后要在 relay 重新批准；只改 `context` 不需要。
- CI：私有仓库的必过检查是常开 Mac mini 上自托管 runner 的轻任务（只调用项目自己的 `scripts/check` 等检查命令；runner 标签与机器名见 overlay），runner 一次只跑一个 job、以低权限独立用户运行，workflow 设 `timeout-minutes`；Swift/iOS 构建和签名在开发笔记本的 runner 上跑；公开仓库用 GitHub 托管 runner。先在 xw-tooling 试点；仓库还没接 runner 时，在本机跑同一条检查命令，把结果写进 PR。
- 提交与 PR 写清：回退测试、真实使用情况、没跑到的项目。
