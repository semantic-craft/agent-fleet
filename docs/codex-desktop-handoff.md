# Codex Desktop 空线程交接契约

默认目标（先通过下述创建前门槛）：`thread/start` 建空线程 → 按已验证原生 ownership 路径交接→ 目标机打开 `codex://threads/<id>?prompt=…` 使 Desktop 可见 → 经 agenvo `turn/start` 提交。不是改回 exec；会话仍须 Desktop 可见。

## 当前支持边界

当前路线为 **experimental / blocked_native_writer_release**。本仓 runbook 的 connector 与共享 native app-server 是独立进程；connector 重启不释放独立 native writer，不能用作交接步骤。已核对的 adapter 暴露 thread/turn 方法，但没有已验证的单线程 ownership 交接接口。

**创建前门槛**：在 `thread/start` 之前，必须已有适用于目标 deviceId/instanceId、runtime/adapter/Desktop 版本组合的受支持原生交接路径，以及获准隔离 canary 的完整证据。证据须覆盖创建 → 原生 ownership 交接 → 目标 Desktop 可见 → agent 对同一线程 `turn/start` → 结果回读及 Desktop 仍可接手。没有证据就阻断，不先创建“试试看”。本轮没有这类 live 证据。

可见性、控制权、提交者是三个独立条件。connector online、transport close、loaded 列表变化、空闲、预填链接或占用横幅消失，都不能单独证明 writer 归属；Desktop 可见也不证明之后 agent 提交不会重新争抢 writer。不要把 archive/unarchive、interrupt 或重启共享 app-server 猜成释放接口；不自动降级到 exec 或用户 Send。若需研发交接路径，应另做明确获准的隔离 canary，不以普通派活绕过创建前门槛；本契约不授权停止服务。

离线调用方可使用 `scripts/codex-handoff.mjs` 的 `checkCodexHandoff` 校验创建/提交门槛。它只验证由可信验收提供者输入的证据完整性与关联，不探测服务、不证明证据真实性，也未接管现有 dispatch。技能的人工调用必须遵守同一门槛；不得为通过校验填造证据。

## 同机 brief 与所有权

- discovery 决定目标 `deviceId` / `instanceId`。Herdr 写文件入口必须与 Codex **同一 deviceId**，不能按实例名字相似或固定主机名配对。
- home、projectsRoot、cwd 取该目标的 context/overlay，不能用调用方的 `$HOME`。brief 的绝对路径只属于该目标文件系统。
- 仅写经任务授权的目标目录，回读确认内容与路径。缺同机文件入口、授权或可读证据时停止，不跨机器复制私人正文。
- 若用户客户端不在目标机器，交付“需在目标机器打开”的链接；本机打开同名 URI 不证明远端会话可见。

## 状态与验收

1. `prepared`：创建前门槛已通过；同机 cwd/brief 已回读，模型/权限符合授权。
2. `created`：一次 `thread/start` 获得 threadId；unknown 只查原生状态，不再创建。没有可靠关联结果就标未确定。
3. `handoff_verified`：按受支持路径完成本线程原生交接并取得关联证据（deviceId/instanceId/threadId）；Desktop 可见性另验。观察期间避免 `thread/resume` 等可能重新取 writer 的调用。
4. `desktop_visible`：deep link 已在目标机打开，Desktop 可见该线程（含预填 prompt）；尚未提交不算任务已运行。
5. `submitted`：当前线程 ownership 与 Desktop 可见证据均成立，且组合 canary 已证明 agent 提交后的控制权行为，才经 agenvo 对同一线程 `turn/start`；unknown 只回读，不重复提交。回报带 deep link 与 turn 结果。
6. `completed`：回读匹配 thread/turn、实际输出和终态，按任务验收。占用横幅消失或空闲状态不是业务成功。

新机器/升级组合须重新验收，不能沿用其他版本/机器的交接证据。沿用 PR57–59 原则：版本与行为证据分离、unknown 不重发、迁移服务不等于恢复在途任务，未验证能力不得自动降级。Herdr Claude JSON/session_id/--resume 与 Codex thread/turn 是不同消费者。

## 本轮证据

只读核对现有源码、文档和派活技能；没有重启服务、真实 thread/start、跨机写 brief 或模型请求。本轮回归只防文档入口、模板和验收契约再次漂移，不证明 native writer 已能交接。
