# ADR-0006：自建 MCP 远程派活统一 CommHub

状态：已采纳（2026-10-31）。2026-10-19 曾统一到 Agenvo relay。

## 适用范围澄清

本决定约束自建 MCP 连接，不排除 dot 已提供且已授权的原生平台任务通道。dot→Codex 优先原生。Grok 外环走 CommHub MCP；本机接收端走 #90 门铃桥。见[调用方路由](../caller-routing.md)。不要求第三方 agent 拥有 dot 工具。

## 决定

- 自建 MCP 远程派活统一走 CommHub：一个 hub。外环接 `/mcp`，本机节点用门铃。
- 新入口作为 MCP 客户端接同一个 hub，不为某个入口另搭传输链路。
- 2026-10-19：Agenvo relay 不为 Grok 打开 CIMD。grok.com 的 Cloudflare 防护会拦 Worker 读取客户端元数据，当时让 Grok 走动态注册。该事实只约束旧 relay。

## 理由

鉴权、任务和记账保持一条链路。

组件的增减与移除步骤见 [maintenance](../maintenance.md)。
