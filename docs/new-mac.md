# 新 Mac 加入（给接手的 agent）

迁移与升级先读[兼容来源、模型选择保护及在途恢复门槛](migration-and-upgrades.md)；历史通过不代表未知新版本可用。

适用于新买或重装的 Mac，例如再添一台 Mac mini。照顺序做，每一步都有检查；命令细节在 [runbook](runbook.md) 的对应小节，这里不重复。

## 先弄清用户要什么

- **Codex 是主力，入口按调用方区分。** dot 有原生平台任务工具且电脑连接已授权时优先原生通道；跨机先发现环境。本机多 agent 派活走官方 anet（[anet.md](anet.md)），不走已删除的 CommHub MCP 或 #90 门铃桥。DSH、GrokBot 的历史外部路线在[归档](legacy/caller-routing.md)，不是现行入口。
- **DSH（DeepSeek Harness）是同等能力的后备**，防止 Codex 额度用完或出故障：它必须有和 Codex 同级的原生能力，任务出现在 DSH 桌面左栏、可以在桌面里接手；派活、追问、中断都走 DSH 的原生会话，不退回 headless。
- Herdr 只做 pane 宿主，不是派活控制面，也不是 Codex Desktop 的模型执行者，不负责释放 native writer。
- 实例 context 里可能仍有旧 connector 文案。原生平台任务遵守其自身权限与用户范围。
- 本机派活传输是官方 anet，见 [anet.md](anet.md)。[ADR-0006](legacy/0006-single-remote-path.md) 只记录旧的单远程通路，不替代原生平台任务。
- 这台机器的个人值（名字、地址、项目目录）只写进私有 overlay，不写进本仓库。

## 步骤

| # | 做什么 | 检查 |
| --- | --- | --- |
| 1 | 装 Node 24.13+、Git，装好 Codex（CLI 和 Desktop 共用 `~/.codex`）、Herdr 和 DSH 桌面版，Codex 与 DSH 都登录好 | `node -v`；Codex 和 DSH 都能正常对话 |
| 2 | 拿到私有 overlay：`git clone git@github.com:<owner>/agent-fleet-overlay.git ~/.config/agent-fleet`，在 `fleet.json` 的 `machines` 里为这台机器加一项（名字、`hostnames`、`projectsRoot`、实例和 context）。实例 context 照已有机器的写法改 | `node scripts/fleet.mjs doctor --machine <名字>` 能识别这台机器 |
| 3 | 按 [runbook](runbook.md#1-原生服务常驻) 常驻仍列出的本机服务。不要生成已归档的 `herdr-server` plist。Herdr 只做 pane 宿主，本仓没有它的现行 launchd 路径 | `fleet.mjs doctor` 只核对 overlay 里仍列出的服务 |
| 4 | **不要装 DSH socket 插件**。`fleet.mjs release dsh` 已退役，见 [legacy/dsh-adapter.md](legacy/dsh-adapter.md)。DSH 桌面应用本身仍可作为后备 | 不出现新的 `agent-fleet-dsh` 安装步骤 |
| 5 | 若 overlay 仍列出 DSH 实例：`fleet.mjs contexts --apply`。旧 connector 配对步骤已删除，见 [ADR-0001](legacy/0001-agenvo-as-control-plane.md) | `fleet.mjs doctor` 里该实例不是 NOT CONFIGURED，或确认这台机器不再挂该 connector |
| 6 | 登录自启动：`fleet.mjs launchd desktop-agents-autostart` | 注销重登后 DSH 自己打开 |
| 7 | **验收 DSH 后备能力**：在 DSH 桌面新建一轮并追加一轮，确认左栏可见。不要用已删除的 `check --dispatch` | 桌面左栏能看到这两轮 |
| 8 | 把 overlay 的改动提交并推送 | `git -C ~/.config/agent-fleet status` 干净 |

## Codex 派活验收

dot 已有原生平台任务工具且电脑已授权时，按该平台自己的创建、执行和回读验收。缺工具就报告能力缺失。

现行派活见 [anet.md](anet.md)。旧 Desktop 交接全文在 [legacy/codex-desktop-handoff.md](legacy/codex-desktop-handoff.md)，不要按它验收。这不是 Herdr CLI 模板验收。


## 机器角色

- 常开的机器（Mac mini）适合放长任务和夜间任务，在 context 里写明"常开、适合长任务"。
- 笔记本合盖后上面的实例全部离线，context 里也写明这一点，助手据此选机器。
