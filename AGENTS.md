# agent-fleet 维护约定

本仓库是"助手经 Agenvo 给自有电脑上的 agent 派活"这套基础设施的设计原件。没有对话记忆的 agent 只要按本文件和链接的决策行事，就应能做出与原设计一致的选择。

## 先读什么

| 任务 | 读 |
| --- | --- |
| 任何改动前 | [docs/architecture.md](docs/architecture.md)、[lessons.md](lessons.md) |
| 接入新的 agent 运行时或新机器 | [docs/decisions/](docs/decisions/) 全部、[docs/runbook.md](docs/runbook.md) |
| 判断某个 `~/Projects` 目录能否动 | [docs/landscape.md](docs/landscape.md) |
| 改脚本 | [scripts/fleet.mjs](scripts/fleet.mjs)、[test/](test/)；`npm test` |

## 决策规则（按优先级）

1. **先用上游。** 运行时若已有 Agenvo 官方 connector（Codex app-server、Herdr、Paseo、OpenCode、Amp、Lody），直接用，不写自己的。CLI 型 agent（claude、codex CLI、agy 等）一律放进 Herdr pane，不新增 connector。见 [ADR-0003](docs/decisions/0003-herdr-for-cli-agents.md)。
2. **只为"没有原生管理接口、但用户要求在其桌面 app 里可见"的运行时写 connector**，而且要写成对现有接收端 HTTP 契约的薄封装，不引入调度、队列或状态机。目前只有 DSH 和 ZCode 两种。见 [ADR-0002](docs/decisions/0002-minimal-agenvo-fork.md)。
3. **fork 补丁最小化。** 对 agenvo 的改动只限新增 kind 与 `apps/dispatch`。上游一旦支持外部 kind，就去掉 fork。不在 fork 里写部署记录或个人值。
4. **个人值不进本仓库。** 域名、IP、账号、主机名、路径写在 overlay（默认 `~/.config/agent-fleet/fleet.json`，可用 `AGENT_FLEET_CONFIG` 覆盖）。如果某项改造离开个人值就没意义，先怀疑这项改造的必要性。
5. **桌面可见、full access。** 派给 DSH/ZCode 的任务必须 `runner: "app"`，app 不在就明确失败，不退回 headless。执行权限默认 full access，访问控制靠 relay 的设备配对与 OAuth 授权。见 [ADR-0004](docs/decisions/0004-desktop-strict-and-full-access.md)。
6. **一个 relay，一个 owner。** 所有已授权客户端都能访问所有已批准实例；测试授权用完即撤，不碰用户的正式授权。

## 验收与交付

- 生产验收走真实入口：OAuth → relay `/mcp` → connector → 原生服务；只看 `/health` 不算。`fleet.mjs check` 是最小基线。
- 区分"输入已接受""原生轮次完成""业务目标完成"。
- 改实例 `source`、endpoint 等 scope 字段后要在 relay 重新批准；只改 `context` 不需要。
- 私有依赖仓库不跑 GitHub 托管 runner，检查在本机执行；不要等一个永远不会来的 CI。
- 提交与 PR 写清：回退测试、真实使用情况、没跑到的项目。
