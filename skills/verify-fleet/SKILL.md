---
name: verify-fleet
description: agent-fleet 引擎的验收技能。改动涉及桌面宿主适配器（crates/fleet-daemon）、外部 agent 收据（agents.d / herdr-dispatch.sh）、doctor/schema 门禁、fleet-cli MCP 服务或 scripts/release 时，交付前跑本技能做真实派活回读验收。Agent 专用，日常任务不自动触发。
disable-model-invocation: true
---

# verify-fleet · agent-fleet 真实派活验收

验收对象是**真实安装的引擎**（`~/.local/bin/fleet-cli` 指向的那份），不是隔离 fixture。
本技能只证明到「真实服务调用／真实宿主」这一层；桌面侧栏的肉眼可见性属用户层，
超出本技能范围时如实记 `REVIEW_REQUIRED`。

## Launch

本引擎是常驻 CLI，无需启动服务。确认三个事实：

```sh
cd ~/Projects/agent-fleet && git rev-parse HEAD           # 记录源 SHA 与 dirty diff
ls -l ~/.local/bin/fleet-cli                               # 被验二进制路径
~/.local/bin/fleet-cli doctor --gate; echo "gate-exit=$?"  # 0=通过（见 Doctor）
```

若被验的不是刚构建的二进制（`scripts/release` 的 install 步骤负责对齐），先跑
`python3 scripts/release --skip-check`（跳过 check 仅装新二进制 + MCP 冒烟），否则验收
对象错位，结果无效。

## Doctor

1. `~/.local/bin/fleet-cli doctor --gate` — 退出码必须 0。非 0 时 stderr 会列出
   `host <h>: live schema diverged ...`（缺哪些列）；这表示宿主 app 升级后适配器
   期望过期，先修 `schema_expectations`（crates/fleet-daemon/src/doctor.rs）再验收。
2. `~/.local/bin/fleet-cli doctor` 里逐宿主看 `live.schema_status`：
   - agy/zcode/openai-dots/dsh：期望 `aligned`
   - cindy：期望 `no-local-gateway` + `dispatch_channel: anet`（0.1.126 无本地注入面，
     这是设计内的诚实报告，不是故障）
3. herdr：`herdr status server` 应 `status: running`；`herdr agent list` 记录当前是否
   有活跃 agent（两种状态都要能验，见 Drive）。

## Drive

每个功能一条唯一标记探测（格式 `FLEET_<名>_<日期><随机>`），要求对端只回一行标记、
不改文件。**回读产物，不信退出码**：

- **dispatch-desktop**（features/dispatch-desktop.md）：
  `fleet-cli send --from zcode/<slug> --to <host>/<slug> --title ... --prompt '【只读探测】只回复一行：<标记>'`
  收据 tier 必须 `thread-visible`；随后 `fleet-cli hosts read-session`／`message_status`
  或宿主真实数据库回读该标记回合存在。
- **dispatch-herdr**（features/dispatch-herdr.md）：
  - 无活跃 agent：`fleet-cli send --to herdr/<slug> --prompt ...` 必须**失败**，stderr 含
    `no active herdr agent`，且 `herdr pane list` 相关 pane 内容不出现 fleet-message
    XML（禁止 pane 粘贴）。
  - 有活跃 agent：同命令 + `--expect-marker <标记>`，收据 tier 只有在 agent 输出回读
    到标记时为 `completed`，否则 `accepted`。
- **dispatch-cindy-routing**（features/dispatch-cindy-routing.md）：
  `fleet-cli send --to cindy/<slug> --prompt ...` 必须显式失败并提示 ANet 路由
  （`route this task via the ANet cindy identity`），不得伪造 accepted。
- **mcp-surface**（features/mcp-surface.md）：对 `~/.local/bin/fleet-cli mcp` 走 stdio
  JSON-RPC：initialize → tools/list（至少 list_team/send_message/message_status/
  check_inbox）→ tools/call list_team（canonical_desktop_hosts 恰好 5 个）。
  `python3 scripts/release` 的 mcp-smoke 步骤即为此驱动，可直接复用其输出。
- **schema-gate**（features/schema-gate.md）：临时伪造一个 diverged 报告不可行（gate
  只读真实库），改用单测证明：`cargo test -p fleet-daemon --lib gate_failures schema_contract`。

## Evidence

- 证据目录：`~/Projects/_status/agent-fleet-verify-<YYYYMMDD>-<n>/`（checkout 外，勿用
  /tmp）。每次运行新建目录，写入 `result.json`：逐功能 `PASS/FAIL/BLOCKED/NOT_RUN`、
  源 SHA + dirty diff（`git rev-parse HEAD` + `git diff`）、被验二进制 sha256、每条命令
  全文与退出码、收据 JSON、宿主库回读结果。
- 层级标注：dispatch-* = 真实宿主调用；mcp-surface = 真实 stdio 服务；schema-gate =
  合成单测（不得升格为宿主证据）。
- 侧栏肉眼可见性（用户层）：保留截图或人工确认记录，标 `REVIEW_REQUIRED`，不用程序
  断言冒充。

## Cleanup

- 本技能只创建：探测回合（各宿主真实会话里的一条标记消息）、mailbox 记录、必要时
  `~/Projects/<slug>` 下的探测目录。这些是宿主正常数据，**不删除**；在 result.json 里
  列出创建的 delivery_id 清单备查。
- 不杀宿主进程、不动 `~/.local/state/agent-fleet` 以外的任何状态、不改
  `~/.local/bin`（装新二进制只经 scripts/release）。
- 验收中途修改验收入口本身（check/release 脚本）则本轮作废，改完重跑。

## Helpers

- `python3 scripts/release` — check→build→install→MCP 冒烟的完整发布闭环（含 mcp-smoke
  驱动，可单独参考其源码当 stdio 驱动范例）。
- `python3 scripts/check` — 全量离线门禁（ receipts 落 `.scratch/pstack-relay/checks/`）。
- `fleet-cli hosts read-session --host <h> ...` / MCP `message_status` — 回读接口。
- 功能地图：`features/README.md`。功能源码入口也在各 feature 文件里。
