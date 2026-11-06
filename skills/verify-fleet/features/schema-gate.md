# schema-gate · 宿主 DB 契约门禁

## 用户可见行为
宿主桌面 app 升级改库表后，`fleet-cli doctor --gate` 立刻失败并列出缺失列
（Slice V 式烂法 5 分钟发现），而不是派活时静默写坏宿主状态。

## 源码入口
- `crates/fleet-daemon/src/doctor.rs` — `schema_expectations`（单一事实源）、
  `evaluate_gate_failures`、`probe_host_live_contract`
- `crates/fleet-cli/src/main.rs` `run_setup_or_doctor` — `--gate` 旗标与退出码 2

## 实测配方
```sh
~/.local/bin/fleet-cli doctor --gate; echo $?      # 真实环境：期望 0
cargo test -p fleet-daemon --lib schema_contract   # 期望列表 ↔ 适配器 SQL 同步
cargo test -p fleet-daemon --lib gate_failures     # diverged→fail / missing→pass 分级
```
注意：单测是合成证据（不得升格）；宿主真实库的 diverged 只能靠本机 --gate。

## 最近验收
`~/Projects/_status/agent-fleet-verify-*/result.json` 的 schema-gate 字段。
