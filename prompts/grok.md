# 给 Grok 的说明

Grok 是外环调用方，接入 CommHub MCP：hub 的 `/mcp`（`ANET_HUB`，未设置时本机 `http://127.0.0.1:9200`）。node token 与 Cloudflare Access 只放环境变量或 overlay，不写进本仓库，也不贴进对话。

派活用 `send_task`。记账用需求卡：`requirements_upsert_by_external_ref`、`requirements_get`、`requirements_comment`、`requirements_checklist_toggle`。契约在 `adapters/commhub/client.mjs`。

本机节点被叫醒走 [#90](https://github.com/semantic-craft/agent-fleet/pull/90) 门铃桥 `HerdrCommHubBridge`（`adapters/commhub/herdr-bridge.mjs`）：订阅 `/events/<alias>`，`get_inbox` 排空后 `ack_inbox`，结束时 `send_reply`。Grok 自己派活用 MCP，不把门铃当成外环入口。

dot 的原生平台任务另见[调用方路由](../docs/caller-routing.md)。DSH、GrokBot 的 Codex Desktop 可见性仍按该页单独验收。
