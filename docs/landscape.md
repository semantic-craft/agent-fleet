# `~/Projects` 里与 agent 协作相关的目录

快照：2026-10-19。用来回答"这个目录能不能动、该去哪改"。状态的含义：

- **核心**：本架构的组成部分。
- **接收端**：桌面 app 的原生适配层。
- **治理**：保留的规则或审计型派发。
- **移除中**：正按 [maintenance](maintenance.md) 的"移除组件"步骤移除。
- **无关**：名字相近，但不属于这套基础设施。

2026-10-20 起，别人的代码按关系分放：只读参考在 `_ref/`，上游是别人的 fork 或封装在 `_forks/`；`fleet.json` 的 `agenvo.checkout` 指向本机实际位置。

属于本架构的 launchd 任务：`io.agenvo.connector.*`（各 connector）、`dev.xw.*`、`fleet.mjs launchd` 按 `templates/launchd/` 生成的任务（标签前缀取 `LABEL_PREFIX`，默认 `dev.fleet`），以及 ZCode 接收端 `dev.semantic-craft.zcode-dispatch`（`fleet.mjs release zcode` 按入口路径识别它）。

移动或删除任何目录前，先查 launchd（`~/Library/LaunchAgents`）、`~/.config`、各 app 的配置以及其他仓库对它的引用。没有引用不等于没人用：当天还有提交的仓库，说明有其他 agent 正在维护它。

| 目录 | 是什么 | 状态 |
| --- | --- | --- |
| `agent-fleet` | 本仓库：设计、清单、脚本、提示词，以及两个桌面端适配层 `adapters/dsh`（DSH 原生桥接插件）和 `adapters/zcode`（ZCode 接收端），由 `fleet.mjs release` 部署 | 核心 |
| `_forks/agenvo` | 上游 Agenvo 的最小 fork（新增 `dsh`/`zcode` kind、原生 DSH connector `apps/dsh` 和 ZCode 用的 `apps/dispatch`）；relay 与所有 connector 都从这里构建，connector 由 `fleet.mjs release agenvo` 装进发布目录运行，checkout 可以随意切分支 | 核心 |
| `agent-plugins` | 各 coding agent 自有插件的安装根（2026-10-20）：`claude/`（Claude mod，marketplace xw-claude）、`codex/`（writing-core 构建件，xw-codex）、`zcode/`（zlib 技能包）、`dsh/`（dsh-zlib、lcu-dsh 子模块，dsh-english-coach、dsh-pomodoro） | 无关：不是通信基础设施；其中 `dsh/lcu-dsh`（LCU Computer Use 插件）线上在用 |
| `_ref/esp32-iphone-agent`、`zotero-bridge` | 硬件 iPhone 操控实验；Zotero 文献桥 | 无关 |
