# ADR-0006：与旧机制并存与退役

状态：已采纳（2026-10-19；同日把"稳定一周"改成按依赖退役）

退役看依赖，不看时间：某个入口一旦不再依赖旧组件，就立即停用并归档该组件。

| 旧组件 | 现在还依赖它的入口 | 何时退役 |
| --- | --- | --- |
| dot 网关、门铃、long-poll、tunnel（agent-interop 的 `dot/`），`dot-relay`，reason-net | ChatGPT DOT 的旧连接器 | ChatGPT 里加上 Agenvo 连接器、`fleet.mjs check` 和界面验收都通过的当天 |
| `agent-bridges`（三个 Grok→Codex/ZCode/DSH 桥），Yobiru/tailmsg 及其适配器、部署副本，`codex-task-bridge`、`deepseek-grok-bridge`、`hermes-grokbot-api-bridge` | Grok Bot | Grok Bot 改为直接连接 relay（OAuth MCP）之后 |
| `bot-handbook`、`grokbot-box-services` | Grok Bot 那台机器 | Grok Bot 完全迁出之后 |
| `hub_run`（agent-interop-private） | 需要锁定 head、独立评审的代码任务 | 保留。注意它目前只经 DOT 旧网关调用；旧网关退役前，要么接受暂时失去远程入口，要么为 hub 另做一个 Agenvo 入口 |
| agent-collab 协议 | — | 保留，它是治理规则，不是传输方式 |

退役步骤：
1. 在 [landscape](../landscape.md) 把状态改成"退役中"。
2. 用 `launchctl bootout` 停掉服务，plist 移进 `~/Library/LaunchAgents/_retired/`。
3. 跑一次 `fleet.mjs check`。
4. 把目录移到 `~/Projects/_archive/<日期>-<主题>/`，并把这些仓库从每日维护任务里摘掉。
5. 归档对应的 GitHub 仓库。
