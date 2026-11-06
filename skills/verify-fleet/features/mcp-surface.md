# mcp-surface · MCP stdio 服务面

## 用户可见行为
外部/第三方 agent 经标准 JSON-RPC 2.0 stdio 接入 `fleet-cli mcp`，可列团队
（list_team）、派活/续问（send_message）、回读收据（message_status）、查信箱
（check_inbox）。这是 Reason Network 级开放接口的对外承诺。

## 源码入口
- `crates/fleet-cli/src/main.rs` `mcp_tools_schema` 与 `"send_message" =>` 分支
- 驱动范例：`scripts/release` 的 `mcp_smoke()`

## 实测配方
```sh
# 直接复用发布闭环的冒烟（对已安装二进制）：
python3 scripts/release --skip-check   # mcp-smoke 步骤即完整驱动
# 断言：initialize OK；tools/list 含 list_team/send_message/message_status/check_inbox；
#       tools/call list_team 返回恰好 5 个 canonical_desktop_hosts
```
新工具参数（如 expect_marker）要在 tools/list schema 与处理器两侧同时验证。

## 最近验收
`~/Projects/_status/agent-fleet-verify-*/result.json` 的 mcp-surface 字段。
