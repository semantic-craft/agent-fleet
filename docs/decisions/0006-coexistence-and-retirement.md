# ADR-0006：与旧机制并存与退役

状态：已采纳（2026-10-19）

| 机制 | 决定 | 条件 |
| --- | --- | --- |
| `hub_run`（agent-interop-private） | 保留 | 需要锁定 head、可审计、独立评审的代码任务 |
| agent-collab 协议 | 保留 | 治理规则，与传输方式无关 |
| Yobiru/tailmsg、dot-relay、dot 网关与门铃、codex-task-bridge 的 Grok 入口、各种 Grok↔X bridge | 退役 | 新链路稳定运行一周，且已确认没有 launchd 或其他消费者 |
| reason-net | 冻结 | 本架构不需要 agent 之间的横向消息；以后真有需要再评估 |
| Grok Bot | 迁成普通 MCP 客户端 | 它支持 OAuth MCP 后直接连接 relay，不再走专用 bridge |

退役步骤：先在 [landscape](../landscape.md) 里把状态改成"退役中"，停 launchd、观察一周，再把目录移到 `~/Projects/_archive/`，最后归档 GitHub 仓库。
