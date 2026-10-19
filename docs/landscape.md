# `~/Projects` 里与 agent 协作相关的目录

快照：2026-10-19。退役路线（Yobiru/tailmsg、Dot 网关与 dot-relay、各种 Grok↔X bridge、agent-interop、hermes-dsh-dispatch 等）的本地副本、部署副本和状态目录已全部删除，不再列出；只剩下表中的目录。各项何时退役按依赖判断，见 [ADR-0006](decisions/0006-coexistence-and-retirement.md)。用来回答"这个目录能不能动、该去哪改"。状态的含义：

- **核心**：本架构的组成部分。
- **接收端**：桌面 app 的原生适配层。
- **治理**：保留的规则或审计型派发。
- **冻结**：不再发展。
- **无关**：名字相近，但不属于这套基础设施。

移动或删除任何目录前，先查 launchd（`~/Library/LaunchAgents`）、`~/.config`、各 app 的配置以及其他仓库对它的引用。没有引用不等于没人用：当天还有提交的仓库，说明有其他 agent 正在维护它。

| 目录 | 是什么 | 状态 |
| --- | --- | --- |
| `agent-fleet` | 本仓库：设计、清单、脚本、提示词 | 核心 |
| `agenvo` | 上游 Agenvo 的最小 fork（新增 `dsh`/`zcode` kind、原生 DSH connector `apps/dsh` 和 ZCode 用的 `apps/dispatch`）；relay 与所有 connector 都从这里构建，connector 由 `fleet.mjs release agenvo` 装进发布目录运行，checkout 可以随意切分支 | 核心 |
| `dsh-grokbot-dispatch` | DSH 桌面插件：native bridge（Unix socket 上转发桌面 Host 原生会话接口，供 `agenvo-dsh` 附着）；另有 Grok 用的 `/v1/jobs` 接收端。只走 HTTP 直连，Yobiru 传输已删（[#9](https://github.com/semantic-craft/dsh-grokbot-dispatch/pull/9)） | 接收端 |
| `zcode-dispatch` | ZCode 接收端与插件；线上由 `fleet.mjs release zcode` 固定在指定提交。只走 HTTP 直连，Yobiru 传输已删（[#9](https://github.com/semantic-craft/zcode-dispatch/pull/9)） | 接收端 |
| `bot-handbook` | Grok Bot 共用的经验与决策正本 | 治理：Grok 机器停用后归档 |
| `grokbot-box-services` | Grok Bot 那台机器上的第三方服务台账 | 治理：Grok 机器停用后归档 |
| `lcu-dsh` | DSH 桌面版里的 LCU Computer Use 插件（2026-10-19 从 agent-collab 拆出） | 无关：不是通信基础设施，但线上在用 |
| `dsh-zlib`、`zcode-plugins` | Z-Library 图书技能和插件 | 无关 |
| `esp32-iphone-agent`、`zotero-bridge` | 硬件 iPhone 操控实验；Zotero 文献桥 | 无关 |
