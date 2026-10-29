# `~/Projects` 里与 agent 协作相关的目录

快照：2026-10-19。用来回答"这个目录能不能动、该去哪改"。状态的含义：

- **核心**：本架构的组成部分。
- **接收端**：桌面 app 的原生适配层。
- **治理**：保留的规则或审计型派发。
- **移除中**：正按 [maintenance](maintenance.md) 的"移除组件"步骤移除。
- **无关**：名字相近，但不属于这套基础设施。

2026-10-20 起，别人的代码按关系分放：只读参考在 `_ref/`，上游是别人的 fork 或封装在 `_forks/`；`fleet.json` 的 `agenvo.checkout` 指向本机实际位置。

属于本架构的 launchd 任务：`io.agenvo.connector.*`（各 connector）、`dev.xw.*`、`fleet.mjs launchd` 按 `templates/launchd/` 生成的任务（标签前缀取 `LABEL_PREFIX`，默认 `dev.fleet`）。旧 Codex worker `io.agent-fleet.codex-runtime` 已退役（2026-10-22）：主力 Mac 上用 `launchctl bootout` 卸载，plist 移到 `~/Library/LaunchAgents/_retired/io.agent-fleet.codex-runtime.plist.retired-20261022`；stateDir `~/.local/state/agent-fleet-codex/`（含 `service.lock`、`runtime.json`、`native.sock`）、配置 `~/.config/agent-fleet/codex-runtime.json`、发布目录和日志原样保留，锁文件不删。退役依据：没有调用方（agenvo 的 codex 实例连 `dev.xw.codex-app-server`，Codex 派活走 Herdr 里的 `codex exec`）；旧进程和它的 worker 均已退出，没有进程持有锁或 socket；卸载时没有重启其他 connector。

ZCode 不再派活（#34 起仓库里没有它的接收端代码），但主力 Mac 上 `dev.semantic-craft.zcode-dispatch` 接收端（从 `~/.local/share/agent-fleet/releases/zcode/` 运行）和 dispatch connector 仍在运行，relay 上 `zcode` 实例仍已批准、助手 `search` 仍能看到它，overlay 已不列它（`fleet.mjs check` 会打 WARNING），部署的 `desktop-agents-autostart` 仍会打开 ZCode。状态：**移除待定**——用户 2026-10-23 决定暂时保留；要移除时按 [maintenance](maintenance.md) 的"移除组件"停掉并在 relay 撤销，在那之前不要停它的服务或撤销实例。

移动或删除任何目录前，先查 launchd（`~/Library/LaunchAgents`）、`~/.config`、各 app 的配置以及其他仓库对它的引用。没有引用不等于没人用：当天还有提交的仓库，说明有其他 agent 正在维护它。

| 目录 | 是什么 | 状态 |
| --- | --- | --- |
| `agent-fleet` | 本仓库：设计、清单、脚本、提示词，以及桌面端适配层 `adapters/dsh`（DSH 原生桥接插件），由 `fleet.mjs release` 部署 | 核心 |
| `_forks/agenvo` | 上游 Agenvo 的最小 fork（新增 `dsh`/`zcode` kind、原生 DSH connector `apps/dsh` 和 ZCode 用的 `apps/dispatch`）；relay 与所有 connector 都从这里构建，connector 由 `fleet.mjs release agenvo` 装进发布目录运行，checkout 可以随意切分支 | 核心 |
| `agent-plugins` | 各 coding agent 自有插件的安装根（2026-10-20）：`claude/`（Claude mod，marketplace xw-claude）、`codex/`（writing-core 构建件，xw-codex）、`zcode/`（zlib 技能包）、`dsh/`（dsh-zlib、lcu-dsh 子模块，dsh-english-coach、dsh-pomodoro） | 无关：不是通信基础设施；其中 `dsh/lcu-dsh`（LCU Computer Use 插件）线上在用 |
| `_ref/esp32-iphone-agent`、`zotero-bridge` | 硬件 iPhone 操控实验；Zotero 文献桥 | 无关 |

## `~/Projects` 的布局

- 每个仓库在 `~/Projects/<仓库名>` 只有一份克隆：目录名与 GitHub 仓库名一致，工作区干净，停在默认分支（通常是 `main`），跟踪 `origin`，用 `git pull --ff-only` 跟上远端。
- 别人的代码按关系分放：上游是别人的 fork 或封装放 `~/Projects/_forks/<仓库名>`，只读参考克隆放 `~/Projects/_ref/<仓库名>`。
- 开发在临时 worktree 里做，例如 `git worktree add ~/Projects/.worktrees/<仓库名>/<分支> -b <分支> origin/main`。分支合并后，`git worktree remove <路径>`（含 submodule 的 worktree 要加 `--force`，先确认 submodule 也干净）再 `git worktree prune`，用 `git branch -d <分支>` 删本地分支，`git fetch --prune` 清掉远端已删的跟踪分支。squash 合并的分支 `git branch -d` 认不出已合并，先用 `gh pr list --head <分支> --state merged --json number,headRefOid` 核对 PR 已合并，且 `headRefOid` 与 `git rev-parse <分支>` 一致，再决定是否删除。 完整「worktree 不遗留」条款见根 [AGENTS.md](../AGENTS.md)。
- `~/Projects` 只放 git 仓库和 `_forks/`、`_ref/`、`.worktrees/`、`_status/` 这几个容器，外加 `~/Projects/README.md` 定的 `_artifacts/`（本机报告与验收证据）和 `_shared-work/`（跨机同步的非 Git 产物）。仓库在机器之间经 git 远端同步。
- `_status/` 只放经 Agenvo 派出的任务：一个任务一个目录 `_status/<任务名>/`，放这个任务各轮的 `brief-N.md`、`result-N.*`、`events-N.jsonl`、`stderr-N.log`、`exit-N`（见[派活技能](https://github.com/semantic-craft/xw-stack/blob/main/skills/delegate-via-agenvo/SKILL.md)）。调研报告、截图等放 `_artifacts/<项目>/<任务>/`，不属于任何项目的放 `~/agent-tasks/`。
- 派出去改代码的任务在 `.worktrees/<仓库>/<任务名>/`（Herdr `worktree.create`），不在主 checkout 里开发，也不另做 `/tmp` 克隆。
