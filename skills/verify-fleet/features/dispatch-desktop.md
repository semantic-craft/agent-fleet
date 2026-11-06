# dispatch-desktop · 五宿主桌面派活

## 用户可见行为
`fleet-cli send --to <host>/<slug>` 在目标桌面（Antigravity / ZCode / DSH / ChatGPT）
左侧栏对应项目下挂载会话，首回合可见，续问进同一线程，`--model` 严格透传。

## 源码入口
- `crates/fleet-daemon/src/host_adapter.rs` — `DesktopHostAdapter` trait 与各宿主实现
- `crates/fleet-daemon/src/antigravity.rs` — Antigravity hub/DB 适配
- `crates/fleet-daemon/src/tri_hosts.rs` — codex app-server / DSH / zcode / cindy
- `crates/fleet-cli/src/main.rs` `dispatch_unified_message` — 派活入口与信封

## 实测配方（每宿主一条，标记唯一）
```sh
M="FLEET_$(echo <host> | tr a-z A-Z)_$(date +%Y%m%d)$RANDOM"
fleet-cli send --from zcode/agent-fleet --to <host>/agent-fleet \
  --title "verify $M" --prompt "【只读探测】不要修改任何文件。请只回复一行：$M"
# 断言：exit 0、tier=="thread-visible"、session_id 非空
# 回读：fleet-cli hosts read-session 或宿主真实库（见 SKILL.md Evidence）
```
DSH 可加 `--model deepseek/deepseek-flash` 验证模型透传（回执含 model 字段）。
openai-dots 的 project/create 契约：`roots:[{path}]` + `idempotencyKey`（codex 0.166+）。
openai-dots 回读时序：`state_5.sqlite` 行 ~1s 落盘，侧栏 `preview` 在 5–46s
（约回合完成时）才回填——收据在 preview 未回填时如实给 `running`，回填后才是
`thread-visible`（见 `read_codex_thread_with_retry` / `codex_thread_preview_visible`）。

## 最近验收
`~/Projects/_status/agent-fleet-verify-*/result.json` 的 dispatch-desktop 字段。
