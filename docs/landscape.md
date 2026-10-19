# `~/Projects` 里与 agent 协作相关的目录

快照：2026-10-19。各项何时退役按依赖判断，见 [ADR-0006](decisions/0006-coexistence-and-retirement.md)。用来回答"这个目录能不能动、该去哪改"。状态的含义：

- **核心**：本架构的组成部分。
- **接收端**：桌面 app 的原生适配层。
- **治理**：保留的规则或审计型派发。
- **退役中**：被 Agenvo 取代，等 [ADR-0006](decisions/0006-coexistence-and-retirement.md) 的条件满足后停用。
- **冻结**：不再发展。
- **无关**：名字相近，但不属于这套基础设施。
- **已归档**：已移到 `~/Projects/_archive/`。

移动或删除任何目录前，先查 launchd（`~/Library/LaunchAgents`）、`~/.config`、各 app 的配置以及其他仓库对它的引用。没有引用不等于没人用：当天还有提交的仓库，说明有其他 agent 正在维护它。

| 目录 | 是什么 | 状态 |
| --- | --- | --- |
| `agent-fleet` | 本仓库：设计、清单、脚本、提示词 | 核心 |
| `agenvo` | 上游 Agenvo 的最小 fork（新增 `dsh`/`zcode` kind、原生 DSH connector `apps/dsh` 和 ZCode 用的 `apps/dispatch`）；relay 与所有 connector 都从这里构建 | 核心 |
| `dsh-grokbot-dispatch` | DSH 桌面插件：native bridge（Unix socket 上转发桌面 Host 原生会话接口，供 `agenvo-dsh` 附着）；另有 Grok 用的 `/v1/jobs` 接收端 | 接收端 |
| `zcode-dispatch` | ZCode 接收端与插件；线上通过 agent-interop 的部署脚本固定在指定提交 | 接收端 |
| `hermes-dsh-dispatch` | 第二台 Mac 上的 DSH 插件（Hermes ↔ DSH），vendored dsh-grokbot-dispatch，同样挂 native bridge | 接收端 |
| `agent-interop-private` / `-hub` / `-hub-lease` / `-prod` | 同一个私有仓库的四个 checkout：`hub_run`（远程入口随 dot 网关退役，代码冻结）、dot 网关、门铃和 reason-net（退役中）。合并方案见其 `docs/operations/AGENT_INFRA_MIGRATION.md` | 治理 + 退役中 |
| `agent-collab` | Issue/CLAIM/非作者评审合并的协作协议 | 治理 |
| `bot-handbook` | Grok Bot 共用的经验与决策正本 | 治理：Grok 机器停用后归档 |
| `grokbot-box-services` | Grok Bot 那台机器上的第三方服务台账 | 治理：Grok 机器停用后归档 |
| `agent-bridges` | 后台 bridge 服务的部署源码（launchd 仍在用） | 退役中：Grok 接入 relay 尝试之后（成败都退役） |
| `dot-relay` | dot-relay 的开发 checkout（ChatGPT DOT 旧通路） | 退役中：ChatGPT 接上 Agenvo 当天 |
| `codex-task-bridge` | Grok Bot → Codex Desktop 的桥（公开仓库 grokbot-to-codex） | 退役中：Grok 接入 relay 尝试之后（成败都退役） |
| `deepseek-grok-bridge` | Grok Bot → DSH 的插件 | 退役中：Grok 接入 relay 尝试之后（成败都退役） |
| `hermes-grokbot-api-bridge` | Grok Bot → Hermes 的桥 | 退役中：Grok 接入 relay 尝试之后（成败都退役） |
| `tailmsg-adapters` | Yobiru/tailmsg 适配器 | 退役中：Grok 接入 relay 尝试之后（成败都退役） |
| `dispatch-deploy-semantic-craft` | tailmsg 配置引用的接收端部署副本 | 退役中：Grok 接入 relay 尝试之后（成败都退役） |
| `agent-dispatch-mesh` | Yobiru 时代派发网的说明与恢复文档 | 已归档 |
| `agent-scripts` | `cas.py`（Codex app-server 客户端）和 sonori 派发脚本，功能已被 codex connector 取代 | 已归档 |
| `zcode-acp` | 第三方上游的克隆（william0wang/zcode-acp），没有引用 | 已归档 |
| `pplx-zcode-handshake` | 一次性握手试验仓库，没有引用 | 已归档 |
| `dsh-zlib`、`zcode-plugins` | Z-Library 图书技能和插件 | 无关 |
| `esp32-iphone-agent`、`zotero-bridge` | 硬件 iPhone 操控实验；Zotero 文献桥 | 无关 |
