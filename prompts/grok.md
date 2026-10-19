# 给 Grok Bot 的说明

Grok Bot 迁出旧 bridge 后，作为普通 MCP 客户端接入：在它的 MCP 设置里添加 `<RELAY>/mcp`（OAuth），然后使用和 [operator.md](operator.md) 相同的规矩。
不要再给 Grok 配专用 bridge、Yobiru 地址或共享密钥；授权只走 relay 的 OAuth，可以在 `<RELAY>/admin` 单独撤销。
