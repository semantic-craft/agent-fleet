# 架构

> **舰队与任务账本的正本是官方 anet 0.3.9。** 本仓库不实现 Agent Network、Reason 或 codex-host，只做接线、插件、契约和薄技能。身份与 `delegate` 见 [anet.md](anet.md)。Desktop 进窗的边界见 [reason.md](reason.md)。Cindy 插件见 [plugins/agent-network-cindy](../plugins/agent-network-cindy/README.md)。CommHub MCP、#90 门铃桥与 agenvo fork 的安装步骤已退出；档案在 [legacy/](legacy/README.md)。

## 体验验收

手机上的 Grok 与 Desktop（Codex、agy、Cindy、ZCode）要双向：发出去看得见，结果回得来，cwd 等于 `~/Projects` 里的那个项目。ANet、Reason、codex-host 是达到这条体验的模块，不是产品名目。

主 A 可以换：Grok、Dots 或 Cindy。换的是人对着说话的那一个，不是再造一套舰队，也不是再开一本任务账。

未签到的 Reason 不算这条验收已经打绿。签到与进窗试点另线。

## 体系落在 `~/Projects`

这套体系落在专属的 **`~/Projects` 目录**：少仓、每个仓库一份克隆、工作区干净。主 A 可换，仓库边界不跟着换。哪些目录能动，见 [landscape.md](landscape.md)。PODAS 只是伞名，`~/Projects` 里没有 PODAS 目录，也不另开仓。

开发仍在 worktree，不改主 checkout。收尾见根 [AGENTS.md](../AGENTS.md)。

## 循环

人对着主 A 说话。外循环是 orchestrator：规矩在 outer-loop-kit，决定这件活走哪一条、谁验收、收据写到哪。内循环是亚里士多德式编排：把一件活拆开、排次序、收回来，仍留在已经选定的那一条通道上。worker 之间可以互派；互派沿用已选的通道，不另起一套会社编制，也不再画一张组织图。

下面四层是模块手段，不是四个部门：

| 层 | 模块 | 本仓 |
| --- | --- | --- |
| A 规矩 | outer-loop-kit：分流、版本钉、业务验收 | 不复制长文。派活契约见 [dispatch-contract.md](dispatch-contract.md) |
| B 舰队 | 官方 ANet：身份、任务账本、收据；Cindy 桥与烟雾 | 接线与插件，见 [anet.md](anet.md)。不 fork 上游 |
| C 投递 | Reason：进已开的 Desktop 窗，cwd 钉在 `~/Projects` 的项目上 | 边界说明 [reason.md](reason.md)。签到另线，本文不宣称已通 |
| D 壳 | codex-host | 不抢壳、不 fork。源码在上游 |

## 同活单通道

舰队与任务账本只走 ANet。Desktop 进窗投递只走 Reason。同一件活禁止两条一起发。无头、烟雾、Cindy、任务账本用 ANet（[delegate-via-anet](../skills/delegate-via-anet/SKILL.md)）。要进已开窗口并让 cwd 等于项目，用 [dispatch-desktop-reason](../skills/dispatch-desktop-reason/SKILL.md)。分流的长文在 outer-loop-kit；版本号只钉在那里一处。本页不复制命令，也不再钉一份版本表。

电脑不开入站端口。合盖、断网、关闭聊天，都不应当中断已经在跑的 agent。投递改道之后，这条仍然有效。

## 当前舰队路径

跨 ANet 身份委派只走官方 anet。不经 CommHub MCP、不经 `anet.sonori.dev`、不经 #90 门铃桥。做法见 [anet.md](anet.md)。编码 agent 用官方 `anet agents wire` 与 `anet mcp` 接入。Herdr 只做 pane 宿主，不是派活控制面。dot 若已有原生平台任务工具且电脑已授权，Codex 可走原生通道（平台执行环境，不是另一套 ANet 控制面，也不是 Reason）。DSH、GrokBot 的历史外部路线在 [legacy/caller-routing.md](legacy/caller-routing.md)，不是现行入口。

DSH 桌面 socket 插件已退役，见 [legacy/dsh-adapter.md](legacy/dsh-adapter.md)。它不是控制面。

## 历史外部路线（不再安装）

不要重新安装 fork 或门铃。历史 discovery 使用 deviceId／instanceId，全文在归档。dot 的原生平台任务不走 CommHub。旧 Desktop 交接见 [legacy/codex-desktop-handoff.md](legacy/codex-desktop-handoff.md)。
