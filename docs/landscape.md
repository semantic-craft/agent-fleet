# `~/Projects` 里与 agent 协作相关的目录

快照：2026-10-19。退役路线（Yobiru/tailmsg、Dot 网关与 dot-relay、各种 Grok↔X bridge、agent-interop、hermes-dsh-dispatch 等）以及已并入本仓库的 dsh-grokbot-dispatch、zcode-dispatch的本地副本、部署副本和状态目录已全部删除，不再列出；只剩下表中的目录。各项何时退役按依赖判断，见 [ADR-0006](decisions/0006-coexistence-and-retirement.md)。用来回答"这个目录能不能动、该去哪改"。状态的含义：

- **核心**：本架构的组成部分。
- **接收端**：桌面 app 的原生适配层。
- **治理**：保留的规则或审计型派发。
- **冻结**：不再发展。
- **无关**：名字相近，但不属于这套基础设施。

2026-10-20 起，别人的代码按关系分放：只读参考在 `_ref/`，上游是别人的 fork 或封装在 `_forks/`；`fleet.json` 的 `agenvo.checkout` 指向本机实际位置。

移动或删除任何目录前，先查 launchd（`~/Library/LaunchAgents`）、`~/.config`、各 app 的配置以及其他仓库对它的引用。没有引用不等于没人用：当天还有提交的仓库，说明有其他 agent 正在维护它。

| 目录 | 是什么 | 状态 |
| --- | --- | --- |
| `agent-fleet` | 本仓库：设计、清单、脚本、提示词，以及两个桌面端适配层 `adapters/dsh`（DSH 原生桥接插件）和 `adapters/zcode`（ZCode 接收端），由 `fleet.mjs release` 部署 | 核心 |
| `_forks/agenvo` | 上游 Agenvo 的最小 fork（新增 `dsh`/`zcode` kind、原生 DSH connector `apps/dsh` 和 ZCode 用的 `apps/dispatch`）；relay 与所有 connector 都从这里构建，connector 由 `fleet.mjs release agenvo` 装进发布目录运行，checkout 可以随意切分支 | 核心 |
| `agent-plugins` | 各 coding agent 自有插件的安装根（2026-10-20）：`claude/`（Claude mod，marketplace xw-claude）、`codex/`（writing-core 构建件，xw-codex）、`zcode/`（zlib 技能包）、`dsh/`（dsh-zlib、lcu-dsh 子模块，dsh-english-coach、dsh-pomodoro） | 无关：不是通信基础设施；其中 `dsh/lcu-dsh`（LCU Computer Use 插件）线上在用 |
| `_ref/esp32-iphone-agent`、`zotero-bridge` | 硬件 iPhone 操控实验；Zotero 文献桥 | 无关 |

## `~/Projects` 的布局

- 每个仓库在 `~/Projects/<仓库名>` 只有一份克隆：目录名与 GitHub 仓库名一致，工作区干净，停在默认分支（通常是 `main`），跟踪 `origin`，用 `git pull --ff-only` 跟上远端。
- 别人的代码按关系分放：上游是别人的 fork 或封装放 `~/Projects/_forks/<仓库名>`，只读参考克隆放 `~/Projects/_ref/<仓库名>`。
- 开发在临时 worktree 里做，例如 `git worktree add ~/Projects/.worktrees/<仓库名>/<分支> -b <分支> origin/main`。分支合并后，`git worktree remove <路径>`（含 submodule 的 worktree 要加 `--force`，先确认 submodule 也干净）再 `git worktree prune`，用 `git branch -d <分支>` 删本地分支，`git fetch --prune` 清掉远端已删的跟踪分支。squash 合并的分支 `git branch -d` 认不出已合并，先用 `gh pr list --head <分支> --state merged --json number,headRefOid` 核对 PR 已合并，且 `headRefOid` 与 `git rev-parse <分支>` 一致，再决定是否删除。
- `~/Projects` 只放 git 仓库和 `_forks/`、`_ref/`、`.worktrees/` 这几个容器；产物、验收证据、交接材料等非仓库内容放在 `~/Projects` 之外。仓库在机器之间经 git 远端同步。
