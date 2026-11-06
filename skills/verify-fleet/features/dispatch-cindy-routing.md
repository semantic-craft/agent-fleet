# dispatch-cindy-routing · Cindy 显式 ANet 路由

## 用户可见行为
Cindy 0.1.126 没有本地 gateway / session API，直连侧栏挂载不可用。`fleet-cli send
--to cindy/<slug>` 必须显式失败并指路 ANet cindy 身份（docs/anet.md），不得伪造
accepted。doctor 如实报 `no-local-gateway` + `dispatch_channel: anet`。

## 源码入口
- `crates/fleet-daemon/src/host_adapter.rs` `RuntimeNotConfigured(Cindy)` Display
- `crates/fleet-daemon/src/doctor.rs` Cindy 探测分支
- 路由正本：`docs/anet.md`、`plugins/agent-network-cindy`

## 实测配方
```sh
fleet-cli send --to cindy/agent-fleet --prompt 'probe'; echo $?
# 断言：非 0；错误含 "route this task via the ANet cindy identity"
fleet-cli doctor | python3 -c 'import json,sys;d=json.load(sys.stdin);\
c=[h for h in d["canonical_hosts"] if h["host"]=="cindy"][0];\
print(c["live"]["schema_status"], c["live"].get("dispatch_channel"))'
# 断言：no-local-gateway anet（若 Cindy 未来提供 gateway.sock，此条目需整图复验）
```

## 最近验收
`~/Projects/_status/agent-fleet-verify-*/result.json` 的 dispatch-cindy-routing 字段。
