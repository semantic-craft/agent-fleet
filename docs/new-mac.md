# 新 Mac 加入（给接手的 agent）

适用于新买或重装的 Mac，例如再添一台 Mac mini。照顺序做，每一步都有检查；命令细节在 [runbook](runbook.md) 的对应小节，这里不重复。

## 先弄清用户要什么

- **Codex 是主力。** 先保证 Codex 能被远程指挥（列出、新建、续写 thread）。
- **DSH（DeepSeek Harness）是同等能力的后备**，防止 Codex 额度用完或出故障：它必须有和 Codex 同级的原生能力，任务出现在 DSH 桌面左栏、可以在桌面里接手；派活、追问、中断都走 DSH 的原生会话，不退回 headless。
- 然后按需装 ZCode 和 Herdr（agy、codex CLI 在里面跑）。
- 一律 full access，访问控制靠 relay 授权。
- 只有一条远程通路（Agenvo）。不要恢复 Yobiru、各种 bridge 或 dot 网关（[ADR-0006](decisions/0006-coexistence-and-retirement.md)）。
- 这台机器的个人值（名字、地址、项目目录）只写进私有 overlay，不写进本仓库。

## 步骤

| # | 做什么 | 检查 |
| --- | --- | --- |
| 1 | 装 Node 24.13+、Git，装好 Codex（CLI 和 Desktop 共用 `~/.codex`）和 DSH 桌面版，都登录好（ZCode、Herdr 按需） | `node -v`；Codex 和 DSH 都能正常对话 |
| 2 | 拿到私有 overlay：`git clone git@github.com:<owner>/agent-fleet-overlay.git ~/.config/agent-fleet`，在 `fleet.json` 的 `machines` 里为这台机器加一项（名字、`hostnames`、`projectsRoot`、实例和 context）。实例 context 照已有机器的写法改 | `node scripts/fleet.mjs doctor --machine <名字>` 能识别这台机器 |
| 3 | 准备 agenvo fork 并构建（[runbook §1](runbook.md#1-准备-agenvo-fork每台机器)），再按 [runbook §3、§4](runbook.md#3-原生服务常驻) 接好 Codex：`codex-app-server` 常驻，加上 `agenvo-codex-app-server` 实例 | relay 上 Codex 实例 online；`fleet.mjs check` 中 `thread/list` 为 ok |
| 4 | **装 DSH 插件**：克隆 dsh-grokbot-dispatch（名字是历史遗留，它就是正式的 DSH 插件），按其 README 链进 DSH 桌面版的 `desktop` profile，再重启 DSH 桌面版 | 出现 `~/.local/state/dsh-grokbot-dispatch/native.sock`，权限 0600 |
| 5 | 挂 DSH 实例并配对：`agenvo-dsh instance add`，`fleet.mjs contexts --apply`，然后 `connect` 和 `service install`（[runbook §4](runbook.md#4-添加实例并配对每台机器每种-connector)）。这台机器没有管理员密钥时，用 `--no-browser` 拿到配对码，在有密钥的机器上批准 | relay 上这个实例显示 online 且已批准 |
| 6 | 登录自启动：`fleet.mjs launchd desktop-agents-autostart`；如果这台机器不装 ZCode，就删掉模板里那一行 | 注销重登后 DSH 自己打开 |
| 7 | **验收 DSH 后备能力**：`fleet.mjs check --dispatch` 必须在新会话里跑完一轮；再用 `--instance <id> --session <已有会话>` 往你手动开的会话里追加一轮 | 两项都 OK，DSH 桌面左栏能看到这两轮 |
| 8 | 按需加 Herdr、ZCode，每加一个跑一次 `check` | `check` OK |
| 9 | 把 overlay 的改动提交并推送 | `git -C ~/.config/agent-fleet status` 干净 |

## 机器角色

- 常开的机器（Mac mini）适合放长任务和夜间任务，在 context 里写明"常开、适合长任务"。
- 笔记本合盖后上面的实例全部离线，context 里也写明这一点，助手据此选机器。
