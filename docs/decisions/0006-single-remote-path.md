# ADR-0006：远程派活只走 Agenvo

状态：已采纳（2026-10-19）

## 决定

- 所有远程派活只走 Agenvo：一个 relay，每台电脑上的每种运行时只挂一个 connector。
- 新的入口（ChatGPT、dots、Grok Bot 或以后的客户端）作为 MCP 客户端接 relay 的 `/mcp`（OAuth），不为某个入口单独搭传输链路。

## 理由

只维护一条传输链路：鉴权、事件、可见性和教训都只有一套。为次要入口另开一条链路的维护成本，高于失去那个入口的代价。

## Grok 的接入事实（2026-10-19 核实）

- Grok 在 grok.com/connectors 支持自定义远程 MCP 加 OAuth。它优先用 URL 形式的 client 身份（CIMD，`https://grok.com/oauth/mcp-client.json`）。
- grok.com 的 Cloudflare 防护会拦截 Worker 发出的请求，返回 403 challenge 页。所以即使在 relay 上打开 CIMD，relay 也取不到这份元数据。因此不打开 CIMD，让 Grok 走动态注册（relay 默认支持）。
- 2026-10-19 实测：Grok 走动态注册成功，授权页显示 client Grok、回调 grok.com；search 与 ZCode 派活都通过。

组件的增减与移除步骤见 [maintenance](../maintenance.md)。
