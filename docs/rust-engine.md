# Rust engine：实现与 Hub 兼容边界

Rust engine 位于本仓四个 crate，现行 Hub 正本为 `https://anet.sonori.dev`。
本页以仓内实现为准；`SEEN` 表示有源码及可重放 fixture，不能推导为现网投递通过。
正式身份、任务账本和跨身份委派继续使用本仓接线的官方 ANet CLI 0.3.9，见 [anet.md](anet.md)。
Hub 的部署版本与 CLI 版本是两个边界；本页的 HTTPS 探测不证明服务端版本。

## 四个 crate 当前负责什么

| Crate | SEEN：当前实现 | 尚未接通的边界 |
| --- | --- | --- |
| `fleet-proto` | `SessionId`、`SessionRecord`、`SessionSelector`、`DeliveryTier`、`DesktopHostKind`（五大桌面宿主：`openai-dots`、`cindy`、`deepseek-harness`、`antigravity-desktop`、`zcode`）、`InboxMessage`（含 `<fleet-message>` 对等信封格式化）、`ExternalAgentManifest` 以及 `validate_brief` | 不是 ANet 密码协议实现，也没有完整任务账本 |
| `fleet-hub` | 调用者提供密封 envelope 和授权证明；relay send、持久关联、重启后 status 回读 | 不生成身份、签名或 token，不密封 envelope，不验证 dack |
| `fleet-daemon` | 只读 session directory、cwd 校验与实际子进程执行、Codex 原生 Unix WebSocket 投递对象、五大桌面宿主侧栏项目与会话适配器（`DesktopHostAdapter`）、GrokBot 与五大桌面端双向幂等信箱（`FleetMailboxStore`）、`wire_agent_configs` 自动接线及外部开源 Agent 注册表（`ExternalAgentRegistry`） | 无 runtime 配置时仅落本地状态并返回 `accepted`；带 runtime 配置时回读各桌面原生状态库验证 `thread-visible` |
| `fleet-cli` | `sessions`、`hosts`、`relay`、`who`、`send`、`inbox`、`hook`、`agents`、`setup`（含 `--wire-agents true`）、`doctor` 与标准 JSON-RPC 2.0 stdio `mcp` 服务（5 工具：`list_team`、`send_message`、`check_inbox`、`message_status`、`register_agent`） | 跨机远程派活仍配合官方 ANet 0.3.9 CLI |

源码入口分别为 [proto](../crates/fleet-proto/src/lib.rs)、[hub](../crates/fleet-hub/src/lib.rs)、
[daemon](../crates/fleet-daemon/src/lib.rs)、[cli](../crates/fleet-cli/src/main.rs)。
完整架构与五大桌面宿主侧栏存储契约见 [DESIGN.md](../DESIGN.md)。

## 与官方 ANet 的兼容边界

`fleet-hub` 是客户端传输适配器，不是 Hub 服务端。它要求 HTTPS origin（仅本机 fixture 可用 HTTP），
发送 `POST /relay/send`，正文为调用者准备好的 CBOR envelope，使用 `X-ANet-Wire: 3`。
授权分 capability 和 session proof；status 通过 `POST /relay/status` 携带 RID、handle 与可选 via_hub，
不带发送授权或身份头，每次请求隔离连接，禁用重定向与自动重试。

身份、注册、inbound policy、allow/trust、MCP 接入与正式 delegate 仍走官方 ANet 0.3.9；
密封、签名和 delivery token 验证归 ANetCore。本仓没有将这些能力接成 Rust 生产发送入口。
测试中的固定字节仅用于 loopback fixture，不能拿去现网冒充有效凭据。

发送前按 correlation id 持久声明，关联 task id、Hub、授权摘要、envelope 摘要和 delivery handle。
已有声明只能回读；首次超时且没有 RID 时为 `UnknownNoRid`，既不能凭 task id 假造查询，也不能重发。
journal 必须稳定保留，文件权限为 0600；它含回读 handle，属于运行数据，不入 Git。
`queued` / `duplicate` 只支持 `accepted`；Hub 返回 `done` 后仍为 `DoneUnverified` / `unknown`，
不证明任务完成或 Desktop 可见。Hub 与 Desktop 尚未共享持久声明，跨通道去重仍是集成缺口。

## 将 work_dir 与 Reason 机制吸收为本仓类型

- **SEEN：work_dir 归属于选中的 session。** 从 agenvo 的目录约束吸收行为，落点为
  `SessionRecord.project_cwd` 与不可复制、单次消费的 `SelectedSession`。`NodeDaemon` 选择和执行时
  重新校验记录、目录 inode 与缓存边界，子进程通过目录 fd 执行 `fchdir`，清空继承环境；无 daemon cwd 兜底。
  [cwd 行为测试](../crates/fleet-daemon/tests/cwd_follows_session.rs) 实际执行 `/bin/pwd` 并核对拒绝时零执行。
- **SEEN：Reason session directory 的稳定关联。** 本仓 `SessionId` 使用 host、desktop、原生 thread_id；
  record 包含 project_cwd、open/closed、observed_at、expires_at（Unix 秒）。目录只读，生产者需原子替换记录。
  缺失、歧义、重复身份、过期或关闭都拒绝选择，不静默新建线程。CLI list 包含失效记录供诊断，resolve 才做可用性选择。
- **SEEN：已开线程投递。** `DesktopDelivery` 绑定显式 socket 和 session，每次写入前复核 loaded thread 与 cwd，
  续问沿用同一线程。没有自动 reconnect/resend；`events` 区分 poll 观察与实际收到的 native 通知。
  完成 turn 不推导 `thread-visible`。这吸收的是投递机制，不安装 Reason、不复制整个上游，也不抢 codex-host 的壳。

设计出处及原生接线限制见 [architecture.md](architecture.md#slice-e-native-desktop-adapter放宽验收完成gui-可见性未验)
与 [reason.md](reason.md)。现行接线与 Rust 实现的验收状态必须分开。

## 可重放 smoke

无凭据的现网检查只做 HTTPS 根路径 GET，不创建任务、不发送模拟 token：

```sh
curl --silent --show-error --max-time 15 --proto '=https' \
  --output /dev/null \
  --write-out 'Hub root HTTP %{http_code}; TLS verify %{ssl_verify_result}\n' \
  https://anet.sonori.dev/
```

HTTP 200 与 TLS verify 0 只证明此端点当次可达，不证明 relay wire 兼容、身份授权或投递完成。
当次执行结果保存在任务回执，不将一次性日志提交到仓库。

无 PRIVATE 凭据的 relay smoke 使用现有 loopback fixture：

```sh
cargo test -p fleet-hub smoke_restart_readback_preserves_task_without_claiming_completion
cargo test -q
cargo check
```

该 smoke 经过真正的本地 HTTP 请求，提交后重建客户端，仅凭 correlation id 回读，核对持久收据、
相同任务关联、单次 send、无授权 status，以及未验证 dack 不能提升为 completed。
完整 fixture 还覆盖拒绝、超时、冲突、连接隔离、重定向及 wire 不一致。

**Live-send：BLOCKED。** 本切片没有来自 ANetCore 的真实密封 envelope、授权 proof 和 dack 验证接线；
已有架构记录也没有可核实的 Desktop renderer IPC 与可见线程证据。不能以独立 app-server 的 loaded-list、
CLI rollout 或 HTTPS 200 替代 Desktop 可见验收。本切片按 brief 使用 fixture，不编造 token、不发送真实任务。
后续需接通 Core 提供的输入与验证，并取得所选 Desktop socket、原生线程、项目 cwd 和 renderer 可见证据；
同一任务只选一条通道，结果未知先回读。
