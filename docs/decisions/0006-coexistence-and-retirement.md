# ADR-0006：只保留一条远程通路，其余全部退役

状态：已采纳（2026-10-19）。这一版取代同日的两个早期版本，它们分别是"稳定一周后退役"和"Grok 迁出前保留 Yobiru"。

## 决定

所有远程派活只走 Agenvo。不为任何次要入口保留第二条传输链路：保留第二条的维护成本，高于失去那个入口的代价。

| 旧组件 | 去向 | 时机 |
| --- | --- | --- |
| dot 网关、门铃、long-poll、tunnel（agent-interop 的 `dot/`），`dot-relay`，reason-net | 退役 | ChatGPT 里加上 Agenvo 连接器，并且验收通过的当天 |
| Yobiru/tailmsg 及其适配器、部署副本；`agent-bridges` 和三个 Grok 专用桥 | 退役 | 先在 grok.com 上把 relay 加为自定义连接器：连上了就当天退役；连不上，就放弃让 Grok 派活，同样当天退役。不为 Grok 保留 Yobiru |
| `hub_run` 的远程入口 | 退役（代码冻结，不删） | 与 dot 网关一起退役。它只经 DOT 旧网关调用；需要审计的代码任务改走 agent-collab 的 PR 评审 |
| `bot-handbook`、`grokbot-box-services` | 归档 | Grok 那台机器停用后 |
| agent-collab 协议 | 保留 | 它是治理规则，不是传输方式 |

## Grok 的接入事实（2026-10-19 核实）

- Grok 在 grok.com/connectors 支持自定义远程 MCP 加 OAuth。它优先用 URL 形式的 client 身份（CIMD，`https://grok.com/oauth/mcp-client.json`）。
- grok.com 的 Cloudflare 防护会拦截 Worker 发出的请求，返回 403 challenge 页。所以即使在 relay 上打开 CIMD，relay 也取不到这份元数据。因此不打开 CIMD，让 Grok 走动态注册（relay 默认支持）。
- 如果 Grok 不接受动态注册，按上表直接放弃 Grok 派活，不写绕过方案。

## 退役步骤

1. 在 [landscape](../landscape.md) 把状态改成"退役中"。
2. 用 `launchctl bootout` 停掉服务，plist 移进 `~/Library/LaunchAgents/_retired/`。
3. 跑一次 `fleet.mjs check`。
4. 把目录移到 `~/Projects/_archive/<主题>-<YYYYMMDD>/`，并把这些仓库从每日维护任务里摘掉。
5. 归档对应的 GitHub 仓库。
