# verify-fleet 功能地图

| 功能 | 文件 | 何时验 |
| --- | --- | --- |
| 五宿主桌面派活（侧栏挂载 + 同线程续问 + 模型透传） | [dispatch-desktop.md](dispatch-desktop.md) | 任何 fleet-daemon 适配器改动后 |
| Herdr 外部 agent 派活（诚实收据） | [dispatch-herdr.md](dispatch-herdr.md) | herdr-dispatch.sh / ExternalAgentRegistry 改动后 |
| Cindy 显式 ANet 路由（无本地 gateway） | [dispatch-cindy-routing.md](dispatch-cindy-routing.md) | Cindy 探测/路由改动后 |
| MCP stdio 服务面 | [mcp-surface.md](mcp-surface.md) | fleet-cli mcp / 工具 schema 改动后 |
| Schema 契约门禁 | [schema-gate.md](schema-gate.md) | doctor / schema_expectations / 宿主 app 升级后 |

每个功能条目含：用户可见行为、源码入口、实测配方、最近一次验收结论位置。
