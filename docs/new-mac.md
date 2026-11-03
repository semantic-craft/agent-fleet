# 新 Mac 加入（给接手的 agent）

迁移与升级先读[兼容来源、模型选择保护及在途恢复门槛](migration-and-upgrades.md)；历史通过不代表未知新版本可用。

适用于新买或重装的 Mac，例如再添一台 Mac mini。照顺序做，每一步都有检查；命令细节在 [runbook](runbook.md) 的对应小节，这里不重复。

## 先弄清用户要什么

- **Codex 是主力，入口按调用方区分。** dot 有原生平台任务工具且电脑连接已授权时优先原生通道；跨机先发现环境。本机多 agent 派活走官方 anet（[anet.md](anet.md)），不走已删除的 CommHub MCP 或 #90 门铃桥。DSH、GrokBot 的 Codex Desktop 交接另验，其 writer 阻断不阻挡前者。见[调用方路由](caller-routing.md)。
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
| 3 | 按 [runbook](runbook.md#1-原生服务常驻) 把 Codex app-server 与 Herdr server 常驻。fork 安装与旧 relay 配对已删除，见 [ADR-0002](legacy/0002-minimal-agenvo-fork.md) | `fleet.mjs doctor` 能看到这两项服务；外部 Codex 交接按下述独立记录，不阻挡 dot 原生任务验收 |
| 4 | **装 DSH 插件**：`node scripts/fleet.mjs release dsh --sha <reviewed SHA>`（把 [adapters/dsh](../adapters/dsh/README.md) 链进 DSH 桌面版的 `desktop` profile） | 出现 `~/.local/state/agent-fleet-dsh/native.sock`，权限 0600 |
| 5 | 若 overlay 仍列出 DSH 实例：`fleet.mjs contexts --apply`。旧 connector 配对步骤已删除，见 [ADR-0001](legacy/0001-agenvo-as-control-plane.md) | `fleet.mjs doctor` 里该实例不是 NOT CONFIGURED，或确认这台机器不再挂该 connector |
| 6 | 登录自启动：`fleet.mjs launchd desktop-agents-autostart` | 注销重登后 DSH 自己打开 |
| 7 | **验收 DSH 后备能力**：在 DSH 桌面新建一轮并追加一轮，确认左栏可见。不要用已删除的 `check --dispatch` | 桌面左栏能看到这两轮 |
| 8 | 把 overlay 的改动提交并推送 | `git -C ~/.config/agent-fleet status` 干净 |

## Codex 派活验收

先按调用方区分：dot 原生平台任务的创建／执行／读回，在连接授权及环境发现完成后独立验收，不以外部 writer handoff 为前提；缺平台工具就报告能力缺失。以下步骤只验 DSH／GrokBot 经 Agenvo 的外部 Codex Desktop 路线，不是全部 Codex 的准入条件。

下列步骤属于历史 Desktop 交接记录（[交接契约](codex-desktop-handoff.md)），不是现行派活。现行派活见 [anet.md](anet.md)。这不是 Herdr CLI 模板验收。

**Desktop 交互控制交还门槛**：独立同 host 执行基线不以退订能力为创建前置，Desktop 项保持未验；仅当完成交还时，确认 connector 暴露 `thread/unsubscribe` 且 watcher 不抢其他 loaded 线程，否则 `blocked_native_writer_release`。同 host 共享 app-server 与 Desktop 自带 backend 也要交还 writer；本次 turn 终态后由同一 connector 退订，再截图核实项目下可见且无横幅。connector 重启不释放独立 native writer。目标仍为 Desktop 可见后 agent `turn/start`，见[交接契约](codex-desktop-handoff.md)。
1. discovery 固定目标 deviceId/instanceId；确认目标机器上的 cwd 与 brief 可读，核对当前模型/权限和 schema。
2. `thread/start` 建空线程 → `threadId`；仅需跨 backend 时按已验证路径完成原生 ownership 交接。
3. 在目标机器打开 `codex://threads/<threadId>?prompt=…`。核对正确 threadId、cwd、brief；Desktop 可见且无占用横幅。
4. 经 agenvo 对同一线程 `turn/start` 提交后，核对唯一只读标记、实际模型和同一 threadId 的终态；再 `turn/start` 第二轮并核对两轮历史。只打开或预填不算执行；Desktop 未见会话不算完整桌面路线通过，但不否定已取得的同 host 执行结果。
5. 本次 turn 终态后由同一 connector `thread/unsubscribe` 交还 Desktop；截图核实左栏项目下可见且主区无 writerConflict／Fork chat／open in new chat／another app。记录证据和未验项；只清理本次确认为空闲且无消费者的测试资源，另按授权操作，不自动删除分支或任务资料。


## 机器角色

- 常开的机器（Mac mini）适合放长任务和夜间任务，在 context 里写明"常开、适合长任务"。
- 笔记本合盖后上面的实例全部离线，context 里也写明这一点，助手据此选机器。
