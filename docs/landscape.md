# `~/Projects` 里与 agent 协作相关的目录

历史运行快照：2026-10-19 至 2026-11-02（本轮未重新验证服务状态）；2026-11-02 源码盘点更新：本机派活 = 官方 anet；`agent-fleet` 含 `plugins/agent-network-cindy`；`_forks/agenvo` 标遗留；补 `outer-loop-kit`、`alma-relay-ops`、`dsh-superagent-prototype`。2026-11-04 补三行非目录：Reason 与 codex-host 的源码仍在上游，本仓只有接线文档；PODAS 只是伞名。用来回答"这个目录能不能动、该去哪改"。状态的含义：

- **核心**：本架构的组成部分。
- **接收端**：桌面 app 的原生适配层。
- **治理**：保留的规则或审计型派发。
- **移除中**：正按 [maintenance](maintenance.md) 的"移除组件"步骤移除。
- **无关**：名字相近，但不属于这套基础设施。

2026-10-20 起，别人的代码按关系分放：只读参考在 `_ref/`，上游是别人的 fork 或封装在 `_forks/`。本仓库示例不再包含 fork checkout 字段。

属于本架构的 launchd 任务：`io.agenvo.connector.*`（各 connector）、`dev.xw.*`、`fleet.mjs launchd` 按 `templates/launchd/` 生成的任务（标签前缀取 `LABEL_PREFIX`，默认 `dev.fleet`）。旧 Codex worker `io.agent-fleet.codex-runtime` 已退役（2026-10-22）：主力 Mac 上用 `launchctl bootout` 卸载，plist 移到 `~/Library/LaunchAgents/_retired/io.agent-fleet.codex-runtime.plist.retired-20261022`；stateDir `~/.local/state/agent-fleet-codex/`（含 `service.lock`、`runtime.json`、`native.sock`）、配置 `~/.config/agent-fleet/codex-runtime.json`、发布目录和日志原样保留，锁文件不删。退役依据：没有调用方（agenvo 的 codex 实例连 `dev.xw.codex-app-server`，Codex 派活走 Herdr 里的 `codex exec`）；旧进程和它的 worker 均已退出，没有进程持有锁或 socket；卸载时没有重启其他 connector。

ZCode 不再派活（#34 起仓库里没有它的接收端代码），但主力 Mac 上 `dev.semantic-craft.zcode-dispatch` 接收端（从 `~/.local/share/agent-fleet/releases/zcode/` 运行）和 dispatch connector 仍在运行，relay 上 `zcode` 实例仍已批准、助手 `search` 仍能看到它，overlay 已不列它（`fleet.mjs check` 会打 WARNING），部署的 `desktop-agents-autostart` 仍会打开 ZCode。状态：**移除待定**——用户 2026-10-23 决定暂时保留；要移除时按 [maintenance](maintenance.md) 的"移除组件"停掉并在 relay 撤销，在那之前不要停它的服务或撤销实例。

移动或删除任何目录前，先查 launchd（`~/Library/LaunchAgents`）、`~/.config`、各 app 的配置以及其他仓库对它的引用。没有引用不等于没人用：当天还有提交的仓库，说明有其他 agent 正在维护它。

| 目录 | 是什么 | 状态 |
| --- | --- | --- |
| `agent-fleet` | 本仓库：多 agent 协作的接线正本。不实现 ANet／Reason／codex-host。协议说明（[anet](anet.md)、[reason](reason.md)）、契约、薄技能、脚本与自制派活插件 `plugins/`（如 `plugins/agent-network-cindy`）。`adapters/dsh` 已退役，见 [legacy/dsh-adapter.md](legacy/dsh-adapter.md) | 核心 |
| Reason（无目录） | 投递层。源码在上游，不 clone 进 `~/Projects`，不 fork。接线说明见 [reason.md](reason.md)。签到另线，不宣称已通 | 核心＝接线文档；源码仍上游 |
| codex-host（无目录） | 壳。源码在上游，不 clone 进 `~/Projects`，不 fork，不在本仓实现 | 核心＝接线文档；源码仍上游 |
| PODAS（无目录） | 伞名。`~/Projects` 里没有这个目录，也不是 Git 仓，不是第三正本 | 伞名，无目录 |
| `_forks/agent-network` | `sleep2agi/agent-network`（文档 anet.sh）的旧实现，与当前 Research 0.3.9 不同；不能作为当前协议来源或被吞并进 fleet | 旧实现，待核实消费者后退役 |
| `xw-skills` | xw-stack 提供技能分发入口，xw-tooling 维护安装清单与宿主接线；ANet 派发机制正本归 fleet，消费者迁移状态见 [盘点](anet-alignment.md) | 分发消费者，保留 |
| `_forks/agenvo` | 上游 Agenvo 的最小 fork（历史 connector／relay）。本机派活已改官方 anet，见 [anet.md](anet.md)。本仓库不再提供安装或发布命令；档案见 [ADR-0002](legacy/0002-minimal-agenvo-fork.md) | 遗留 |
| `agent-plugins` | 只做 marketplace／安装根：`claude/`、`codex/`、`zcode/`、`dsh/` 等自有插件；多 agent 派活插件不放这里，在 `agent-fleet/plugins/` | 无关：不是通信基础设施；其中 `dsh/lcu-dsh`（LCU Computer Use 插件）线上在用 |
| `outer-loop-kit` | 外环业务规则（outer-loop）；ANet 通用派活契约应引用本仓，本轮发现的正文副本及迁移状态见 [盘点](anet-alignment.md) | 治理 |
| `alma-relay-ops` | 多设备穿透／回家底座的运维仓（上游 yetone/alma-relay） | 无关：网络底座，不是派活通道 |
| `_ref/esp32-iphone-agent`、`zotero-bridge` | 硬件 iPhone 操控实验；Zotero 文献桥 | 无关 |

2026-11-04 目录复核：历史条目 `dsh-superagent-prototype` 当前不在 `~/Projects`、`_ref` 或 `_forks` 下，因此移出当前目录表。其迁移或移除原因未核实，不据此声称已归档，也不据此删除或恢复任何内容。

源码盘点与部署状态分开记录；[ANet 对齐盘点](anet-alignment.md)列出重复机制、消费者及未验证项。历史目录行不代表当前目录存在或服务仍运行。

## `~/Projects` 的布局

- 每个仓库在 `~/Projects/<仓库名>` 只有一份克隆：目录名与 GitHub 仓库名一致，工作区干净，停在默认分支（通常是 `main`），跟踪 `origin`，用 `git pull --ff-only` 跟上远端。
- 别人的代码按关系分放：上游是别人的 fork 或封装放 `~/Projects/_forks/<仓库名>`，只读参考克隆放 `~/Projects/_ref/<仓库名>`。
- 开发在临时 worktree 里做，例如 `git worktree add ~/Projects/.worktrees/<仓库名>/<分支> -b <分支> origin/main`。分支合并后，`git worktree remove <路径>`（含 submodule 的 worktree 要加 `--force`，先确认 submodule 也干净）再 `git worktree prune`，用 `git branch -d <分支>` 删本地分支，`git fetch --prune` 清掉远端已删的跟踪分支。squash 合并的分支 `git branch -d` 认不出已合并，先用 `gh pr list --head <分支> --state merged --json number,headRefOid` 核对 PR 已合并，且 `headRefOid` 与 `git rev-parse <分支>` 一致，再决定是否删除。 完整「worktree 不遗留」条款见根 [AGENTS.md](../AGENTS.md)。
- `~/Projects` 只放 git 仓库和 `_forks/`、`_ref/`、`.worktrees/`、`_status/` 这几个容器，外加 `~/Projects/README.md` 定的 `_artifacts/`（本机报告与验收证据）和 `_shared-work/`（跨机同步的非 Git 产物）。仓库在机器之间经 git 远端同步。
- `_status/` 放任务回执（本机派活经 anet；历史目录也可能来自 Agenvo）：一个任务一个目录 `_status/<任务名>/`，放这个任务各轮的 `brief-N.md`、`result-N.*`、`events-N.jsonl`、`stderr-N.log`、`exit-N`（见[派活技能](https://github.com/semantic-craft/agent-fleet/blob/main/skills/delegate-via-anet/SKILL.md)）。调研报告、截图等放 `_artifacts/<项目>/<任务>/`，不属于任何项目的放 `~/agent-tasks/`。
- 派出去改代码的任务在 `.worktrees/<仓库>/<任务名>/`（Herdr `worktree.create`），不在主 checkout 里开发，也不另做 `/tmp` 克隆。
