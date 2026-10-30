# ADR-0001：远程控制面是 CommHub（anet）

状态：已采纳（2026-10-31）。2026-10-19 曾把 Agenvo relay 写成唯一远程控制面；现役控制面是 Agent Network 的 CommHub。

## 背景

用户从几个入口指挥多台电脑上的多种运行时。控制面只留一套鉴权、任务和记账，避免每个「入口 × 运行时」各接一条链路。

## 决定

远程控制面是 CommHub（Agent Network／anet）。本机默认 `http://127.0.0.1:9200`，MCP 在 `/mcp`。客户端与门铃在本仓 `adapters/commhub`；服务来自 `~/Projects/_forks/agent-network`。

- 外环调用方（Grok）作为 CommHub MCP 客户端派活：`send_task`，需要记账时写需求卡。
- 本机接收端用 [#90](https://github.com/semantic-craft/agent-fleet/pull/90) 门铃桥 `HerdrCommHubBridge`：SSE `/events/<alias>`，`get_inbox`／`ack_inbox`，结束时 `send_reply`。
- dot 已有原生平台任务工具且电脑已授权时，Codex 走原生通道，不绕经 CommHub。
- `fleet.mjs check` 默认检查 CommHub。`--legacy-agenvo` 只检查旧 relay。

hub 地址、node token、Cloudflare Access 放环境变量或 overlay，不进本仓库。

## 代价

- hub 可达性、token 与 Access 策略要单独保管。
- 桌面和 CLI 的原生附着仍由 connector 与门铃桥完成；控制面不执行 agent。

## 适用范围

DSH、GrokBot 的 Codex Desktop 可见性仍按[调用方路由](../caller-routing.md)各自验收。同 host 执行与跨 backend 的 writer 交接分开记录。
