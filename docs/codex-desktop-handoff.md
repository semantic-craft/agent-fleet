# Codex Desktop 空线程交接契约

保留 PR60 的默认目标：`thread/start` 建空线程 → 原生 writer 交接 → 目标机预填链接 → 用户 Send。不是改回 exec，也不是自动开跑。

## 当前支持边界

本仓 runbook 把 connector 与共享 native app-server 作为两个独立进程管理。已记录的 Agenvo 源码基线 `cd10fb92e1da731b307b85f8e94e8db8b6b8808a` 的 `apps/codex-app-server/src/codex.ts` 暴露 thread/start、resume、read、loaded/list 等方法，未暴露经过验证的单线程 writer-release 操作；`connection.ts` 连接已有 endpoint，而不是拥有该服务进程的生命周期。

因此当前状态为 **`blocked_native_writer_release`**。创建线程前检查此条件，避免留下无法交接的空线程。connector 重启、online、transport close、原生 loaded 列表变化、线程空闲、预填链接成功，都不能单独证明 Desktop 已取得 writer。不要把 archive/unarchive、interrupt 或重启共享 app-server 猜成释放接口；也不要自动从 Desktop 交接降级到 exec。

支持路径是：按选定实例重新发现方法/schema，找到有明确生命周期语义且适用于其实际部署的原生交接方式，在获准的隔离验收中证明精确线程可由 Desktop 接手。若没有，就停在此阻断并请用户在 Desktop 自己建立任务。需要停止共享服务的方案必须另行盘点所有消费者并获准；本契约不授权停止服务。

## 同机 brief 与所有权

- discovery 决定目标 `deviceId` / `instanceId`。Herdr 写文件入口必须与 Codex **同一 deviceId**，不能按实例名字相似或固定主机名配对。
- home、projectsRoot、cwd 取该目标的 context/overlay，不能用调用方的 `$HOME`。brief 的绝对路径只属于该目标文件系统。
- 仅写经任务授权的目标目录，回读确认内容与路径。缺同机文件入口、授权或可读证据时停止，不跨机器复制私人正文。
- 若用户客户端不在目标机器，交付“需在目标机器打开”的链接；本机打开同名 URI 不证明远端会话可见。

## 状态与验收

1. `prepared`：同机 cwd/brief 已回读，模型/权限符合授权，原生交接方式已证实可用。
2. `created`：一次 thread/start 获得 threadId；unknown 只查原生状态，不再创建。没有可靠关联结果就标未确定。
3. `handoff_verified`：执行已获准的原生交接，Desktop 在目标机显示精确线程且可接手。观察期间避免 thread/resume 等可能重新取 writer 的调用。
4. `awaiting_user_send`：链接仅预填，由用户按 Send；不能报告任务已运行。
5. `completed`：回读匹配 thread/turn、实际输出和终态，按任务验收。占用横幅消失或空闲状态不是业务成功。

新机器/升级组合须重新验收，不能沿用其他版本/机器的交接证据。沿用 PR57–59 原则：版本与行为证据分离、unknown 不重发、迁移服务不等于恢复在途任务，未验证能力不得自动降级。Herdr Claude JSON/session_id/--resume 与 Codex thread/turn 是不同消费者。

## 本轮证据

只读核对现有源码、文档和 PR60 review；没有重启服务、真实 thread/start、跨机写 brief、UI Send 或模型请求。本轮回归只防文档入口、模板和验收契约再次漂移，不证明 native writer 已能交接。
