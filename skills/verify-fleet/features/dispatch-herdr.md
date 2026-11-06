# dispatch-herdr · Herdr 外部 agent 诚实收据

## 用户可见行为
`fleet-cli send --to herdr/<slug>` 把任务投给 herdr 里**活跃的** agent；没有活跃 agent
时明确失败并给出启动指引，绝不把 prompt 粘进任意 shell 面板。收据分级：
`completed` 仅当 `--expect-marker` 请求且派发脚本从 agent 输出回读到标记。

## 源码入口
- `scripts/herdr-dispatch.sh` — 派发器正本（FLEET_TIER 协议、无 pane 兜底、标记轮询）
- `crates/fleet-cli/src/main.rs` `ensure_herdr_agent_registered` — 装载（include_str! 同步）
- `crates/fleet-daemon/src/host_adapter.rs` `ExternalAgentRegistry::dispatch` — 收据映射

## 已知上游坑（验收时留意）
- herdr `default_known_agent_idle_fallback`：agent 死掉后 herdr 仍报 idle，
  `agent prompt` 会把文本敲进 pane 里的普通 shell。我们的对策：无标记回读
  tier 永不超过 accepted；`--expect-marker` 时轮询 agent 终端输出（默认 90s，
  `HERDR_WAIT_TIMEOUT_MS` 可调），标记不出现就是 accepted，绝不虚报 completed。
- agent start 后立即派发会与 TUI 启动竞态：确认 `herdr agent read` 出现 agent
  TUI 横幅后再派发。
- `agent prompt` 按 tab label 有歧义路由风险：同名 tab 会抢投递（本仓旧脚本
  的 pane 兜底曾造出僵尸同名 tab），派发前 `herdr tab list` 查重。

## 实测配方
```sh
# A. 无活跃 agent（先 herdr agent list 确认目标名不在列）→ 必须失败
fleet-cli send --to herdr/agent-fleet --prompt 'probe'; echo $?
# 断言：非 0；stderr 含 "no active herdr agent"；herdr pane 无 XML 粘贴痕迹

# B. 有活跃 agent（herdr agent start <名> --kind <kind> --pane <id>）
M="FLEET_HERDR_$(date +%Y%m%d)$RANDOM"
fleet-cli send --to herdr/agent-fleet --prompt "只回复一行：$M" --expect-marker "$M"
# 断言：marker 回读成功 → tier=="completed"；agent 未回 → tier=="accepted"
```

## 最近验收
`~/Projects/_status/agent-fleet-verify-*/result.json` 的 dispatch-herdr 字段。
