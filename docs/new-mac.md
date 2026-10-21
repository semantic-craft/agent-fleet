# 新 Mac 加入（给接手的 agent）

适用于新买或重装的 Mac，例如再添一台 Mac mini。照顺序做，每一步都有检查；命令细节在 [runbook](runbook.md) 的对应小节，这里不重复。

## 先弄清用户要什么

- **Codex 是主力。** 先保证 Codex 能经 Herdr 运行 `codex exec` 新建任务、通过 `codex exec resume` 续接；Codex connector 只列出线程与读取历史。
- **DSH（DeepSeek Harness）是同等能力的后备**，防止 Codex 额度用完或出故障：它必须有和 Codex 同级的原生能力，任务出现在 DSH 桌面左栏、可以在桌面里接手；派活、追问、中断都走 DSH 的原生会话，不退回 headless。
- 安装 Codex 时必须一起装 Herdr，并接好 `herdr server` 与 Herdr connector；Codex 派活依赖这条通路。
- 一律 full access，访问控制靠 relay 授权。
- 远程通路只有 Agenvo（见 [ADR-0006](decisions/0006-single-remote-path.md)）。
- 这台机器的个人值（名字、地址、项目目录）只写进私有 overlay，不写进本仓库。

## 步骤

| # | 做什么 | 检查 |
| --- | --- | --- |
| 1 | 装 Node 24.13+、Git，装好 Codex（CLI 和 Desktop 共用 `~/.codex`）、Herdr 和 DSH 桌面版，Codex 与 DSH 都登录好 | `node -v`；Codex 和 DSH 都能正常对话 |
| 2 | 拿到私有 overlay：`git clone git@github.com:<owner>/agent-fleet-overlay.git ~/.config/agent-fleet`，在 `fleet.json` 的 `machines` 里为这台机器加一项（名字、`hostnames`、`projectsRoot`、实例和 context）。实例 context 照已有机器的写法改 | `node scripts/fleet.mjs doctor --machine <名字>` 能识别这台机器 |
| 3 | 准备 agenvo fork 并构建（[runbook §1](runbook.md#1-准备-agenvo-fork每台机器)），再按 [runbook §3、§4](runbook.md#3-原生服务常驻) 接好 Codex 与 Herdr：`codex-app-server` 和 `herdr server` 常驻，添加、配对并批准 `agenvo-codex-app-server` 与 `agenvo-herdr` 实例，安装两者 connector 服务 | relay 上 Codex、Herdr 实例均 online；`fleet.mjs check` 中 `thread/list`、`session.list` 均为 ok；再完成下述 Codex 派活验收 |
| 4 | **装 DSH 插件**：`node scripts/fleet.mjs release dsh`（把 [adapters/dsh](../adapters/dsh/README.md) 链进 DSH 桌面版的 `desktop` profile） | 出现 `~/.local/state/agent-fleet-dsh/native.sock`，权限 0600 |
| 5 | 挂 DSH 实例并配对：`agenvo-dsh instance add`，`fleet.mjs contexts --apply`，然后 `connect` 和 `service install`（[runbook §4](runbook.md#4-添加实例并配对每台机器每种-connector)）。这台机器没有管理员密钥时，用 `--no-browser` 拿到配对码，在有密钥的机器上批准 | relay 上这个实例显示 online 且已批准 |
| 6 | 登录自启动：`fleet.mjs launchd desktop-agents-autostart` | 注销重登后 DSH 自己打开 |
| 7 | **验收 DSH 后备能力**：`fleet.mjs check --dispatch` 必须在新会话里跑完一轮；再用 `--instance <id> --session <已有会话>` 往你手动开的会话里追加一轮 | 两项都 OK，DSH 桌面左栏能看到这两轮 |
| 8 | 把 overlay 的改动提交并推送 | `git -C ~/.config/agent-fleet status` 干净 |

## Codex 派活验收

通过 Agenvo 调用 Herdr 实例的 `workspace.create`（`session` 取 `session.list` 返回值，`cwd` 为真实项目绝对路径），取得 root pane，再用 `pane.run` 执行。先确保 `~/Projects/_status` 存在，为本次验收选唯一文件名，把任务正文写入文件后经 stdin 传入：

```sh
printf '只回复 OK\n' > "/tmp/<验收名>.txt"; codex exec --json -o "$HOME/Projects/_status/<验收名>.md" -C "<项目绝对路径>" -s danger-full-access -m <模型> -c model_provider="magpie" - < "/tmp/<验收名>.txt" > "/tmp/<验收名>.jsonl"; echo __EXIT=$?
```

从 JSONL 的 `thread.started` 记录取得 `thread_id`；用 `pane.read` 确认 `__EXIT=0`，并确认 `-o` 文件内容为 `OK`。随后在同一 pane 续接一次（`-C`、`-s` 放在 `resume` 之前，每轮重复 `-s`、`-m`、`-c model_provider="magpie"`）：

```sh
codex exec --json -o "$HOME/Projects/_status/<验收名>-2.md" -C "<项目绝对路径>" -s danger-full-access -m <模型> -c model_provider="magpie" resume <thread_id> - < "/tmp/<验收名>.txt" > "/tmp/<验收名>-2.jsonl"; echo __EXIT=$?
```

再次确认 `__EXIT=0` 和本轮结果 `OK`，记录同一线程的两轮结果。进程退出后在 Codex Desktop 打开 `codex://threads/<thread_id>`，检查完整历史、正常输入框和没有 “This is open in another app” 占用横幅；不要求线程出现在侧栏。

## 机器角色

- 常开的机器（Mac mini）适合放长任务和夜间任务，在 context 里写明"常开、适合长任务"。
- 笔记本合盖后上面的实例全部离线，context 里也写明这一点，助手据此选机器。
