# 架构

> **当前接线采用 ANet，Hub 正本为 `https://anet.sonori.dev`。** 本仓提供独立 Rust fleet engine（`crates/fleet-proto`、`crates/fleet-hub`、`crates/fleet-daemon`、`crates/fleet-cli`），融合 ANet 与 Reason 投递机制并兼容 Hub。codex-host 仍由上游维护。身份与 `delegate` 见 [anet.md](anet.md)。Desktop 进窗的边界见 [reason.md](reason.md)。Cindy 插件见 [plugins/agent-network-cindy](../plugins/agent-network-cindy/README.md)。CommHub MCP、#90 门铃桥与 agenvo fork 的安装步骤已退出。

## 体验验收

手机上的 Grok 与 Desktop（Codex、agy、Cindy、ZCode）要双向：发出去看得见，结果回得来，cwd 等于 `~/Projects` 里的那个项目。ANet、Reason、codex-host 是达到这条体验的模块，不是产品名目。

主 A 可以换：Grok、Dots 或 Cindy。换的是人对着说话的那一个，不是再造一套舰队，也不是再开一本任务账。

未签到的 Reason 不算这条验收已经打绿。签到与进窗试点另线。

## 体系落在 `~/Projects`

这套体系落在专属的 **`~/Projects` 目录**：少仓、每个仓库一份克隆、工作区干净。主 A 可换，仓库边界不跟着换。哪些目录能动，见 [landscape.md](landscape.md)。PODAS 只是伞名，`~/Projects` 里没有 PODAS 目录，也不另开仓。

开发仍在 worktree，不改主 checkout。收尾见根 [AGENTS.md](../AGENTS.md)。

## 循环

人对着主 A 说话。外循环是 orchestrator：规矩在 outer-loop-kit，决定这件活走哪一条、谁验收、收据写到哪。内循环是亚里士多德式编排：把一件活拆开、排次序、收回来，仍留在已经选定的那一条通道上。worker 之间可以互派；互派沿用已选的通道，不另起一套会社编制，也不再画一张组织图。

下面四层是模块手段，不是四个部门：

| 层 | 模块 | 本仓 |
| --- | --- | --- |
| A 规矩 | outer-loop-kit：分流、版本钉、业务验收 | 不复制长文。派活契约见 [dispatch-contract.md](dispatch-contract.md) |
| B 舰队 | 官方 ANet：身份、任务账本、收据；Cindy 桥与烟雾 | 接线与插件，见 [anet.md](anet.md)。不 fork 上游 |
| C 投递 | Reason：进已开的 Desktop 窗，cwd 钉在 `~/Projects` 的项目上 | 边界说明 [reason.md](reason.md)。签到另线，本文不宣称已通 |
| D 壳 | codex-host | 不抢壳、不 fork。源码在上游 |

## 同活单通道

舰队与任务账本只走 ANet。Desktop 进窗投递只走 Reason。同一件活禁止两条一起发。无头、烟雾、Cindy、任务账本用 ANet（[delegate-via-anet](../skills/delegate-via-anet/SKILL.md)）。要进已开窗口并让 cwd 等于项目，用 [dispatch-desktop-reason](../skills/dispatch-desktop-reason/SKILL.md)。分流的长文在 outer-loop-kit；版本号只钉在那里一处。本页不复制命令，也不再钉一份版本表。

电脑不开入站端口。合盖、断网、关闭聊天，都不应当中断已经在跑的 agent。投递改道之后，这条仍然有效。

## 当前舰队路径

跨 ANet 身份委派只走官方 anet。Hub 正本为 `https://anet.sonori.dev`。不经旧 CommHub MCP、不经 #90 门铃桥。做法见 [anet.md](anet.md)。编码 agent 用官方 `anet agents wire` 与 `anet mcp` 接入。Herdr 只做 pane 宿主，不是派活控制面。dot 若已有原生平台任务工具且电脑已授权，Codex 可走原生通道（平台执行环境，不是另一套 ANet 控制面，也不是 Reason）。

## 历史外部路线（不再安装）

不要重新安装 fork 或门铃。dot 的原生平台任务不走 CommHub。

## ARCHITECTURE：Rust Reason delivery 目标契约

本节描述本仓 Rust 引擎（`crates/fleet-proto`、`crates/fleet-hub`、`crates/fleet-daemon`、`crates/fleet-cli`）的架构契约，完整说明见 [DESIGN.md](../DESIGN.md) 与 [rust-engine.md](rust-engine.md)。

`fleet-proto` 保存 session、任务和 delivery receipt 契约；`fleet-hub` 适配 Sonori Hub；`fleet-daemon` 发现 session 并执行投递；`fleet-cli` 提供列出、读取、新建、追问、中断与事件入口。DSH 保留同等能力的后备地位。vendor-Go 不能替代这四项 Rust 实现。

| 契约 | 所属模块与行为 | 可证伪证据 |
| --- | --- | --- |
| session directory | daemon 维护 host、Desktop、thread id、project cwd、存活状态；目录是会话索引，不是第二本任务账 | 两线程消歧，过期记录拒绝，重启后稳定关联；Slice B |
| inject-opened-thread | daemon 向选定的已开 Desktop/thread 投递，续问复用同一 thread，不静默新建或转无头 | 开发机 可见输入与输出，thread id 投递前后一致；Slice E |
| cwd-follows-session | 执行 cwd 来自选中 session 的项目目录；失效或变化时拒绝，不回退 ANet cache 或 daemon cwd | 两个项目切换，真实执行 pwd 与 session 一致；Slice D |
| delivery-tier | receipt 显式区分 accepted、thread-visible、running、completed、failed、unknown；每次提升有对应证据 | accepted 不能被当成可见，超时 unknown 先回读，不换路重发；Slice F |
| ban-model-self-attest | dispatcher 负责模型配置；brief 不要求 worker 自报或返回 MODEL_WRONG；模型证据来自宿主配置或执行元数据 | brief_policy 拒绝自证门槛，正常任务继续执行；Slice F |

任务 id、delivery id、session id 分开存储并关联。选择 ANet 或 Desktop 投递时只执行一条通道，账本仍采用 Hub 契约。真实 Hub 兼容与 Desktop 可见完成分别验收，两者均未在 A 执行。迁移不改变线上 Hub 身份、不代跑 Reason setup。

## Slice B session directory

`fleet-proto` 定义 `SessionId`、`SessionRecord` 和选择条件。稳定身份是结构化的 host、desktop、原生 thread_id，不依赖文件名和进程启动。`fleet-daemon` 读取显式指定目录下的 JSON 记录，统一校验字段、重复身份、消歧及过期规则。`fleet-cli` 提供只读入口。`fleet-hub` 的 Hub 适配见下方 Slice C。

```sh
cargo run -p fleet-cli -- sessions list --directory /tmp/fleet-sessions
cargo run -p fleet-cli -- sessions resolve --directory /tmp/fleet-sessions --host fixture-host --desktop codex --thread thread-1
```

每个 `.json` 文件是一条记录，时间单位为 Unix 秒。生产者应原子替换文件。B 不实现生产者，也不扫描真实 Desktop 的个人数据。

```json
{
  "id": {"host": "fixture-host", "desktop": "codex", "thread_id": "thread-1"},
  "project_cwd": "/fixture/project",
  "state": "open",
  "observed_at": 100,
  "expires_at": 200
}
```

示例时间用于说明格式，当前解析会拒绝它作为投递目标。`list` 展示索引记录，包含过期与关闭记录；`resolve` 必须唯一匹配、状态为 open 且尚未过期。目录缺失、JSON 损坏、未来观察时间、重复身份或多个匹配均失败。匹配先于时效检查，不会因旧 thread 过期改选另一个 thread。失败退出码为 2，标准输出为空，目录保持不变。

目录是读取时的快照，不证明真实 Desktop 存活。后续投递须重新读取并验证目标；项目目录实际存在及 cwd 执行属于 D，真实已开线程属于 E。B 没有创建线程或网络投递能力。

## Slice C Hub client

`fleet-hub` 使用 ANet wire 3 的 `POST /relay/send` 和匿名 `POST /relay/status`。协议依据是 [ANet 0.3.9 的 hubapi](https://github.com/ANetResearch/ANet/blob/d1ad854c/internal/hubapi/hubapi.go) 与 [ANetHub v0.3.3 的 relay](https://github.com/ANetResearch/ANetHub/blob/v0.3.3/internal/aghub/relay.go)。Sonori 是部署目标；本切片仅在隔离 HTTP fixture 验证，不触碰线上身份或部署。

调用者提供已经封装的 CBOR envelope 和 access 认证材料。首次联系使用 session、时间戳和 MAC，capability 投递不附带身份签名头。身份注册、消息封装和密码学校验仍归 ANetCore。Rust 适配层负责 HTTP、持久投递声明、响应解析和关联回读；不另造 Hub task API。

发送前持久记录关联标识、task 标识、目标 Hub 和 envelope 摘要。同一关联标识的再次调用只恢复原记录，不再次发送。客户端没有另通道回退。后续 Desktop 接入必须沿用这份单通道约束；本切片不声称已验证尚未实现的 Desktop 路径。

Hub 在投递响应中返回随机 RID。有 RID 时，回读使用 RID 与 handle，并隔离匿名连接。首次发送超时且整个响应丢失时，没有协议支持的 task-id 查询，必须保留未知结果和禁止重投的声明。已收到 RID 的回读超时也不能恢复发送资格。Hub 的 queued、duplicate 或 done 都不表示任务完成；返回的 dack 只是待 ANetCore 验证的传输证据。

运行 `cargo test -p fleet-hub` 重放隔离场景。`scripts/check` 包含该命令并保存源码指纹、完整输出和 JSON 回执。真实 Hub 及 Desktop 闭环仍由后续切片验收。

### Node daemon 的执行目录

`fleet_daemon::NodeDaemon` 是节点执行核心。调用方用 `select` 取得不可复制的 `SelectedSession`，再交给 `execute` 消费。每次选择都会使此前的选择失效，失败的选择也一样；不同 daemon 的选择不可混用。

项目 cwd 来自 session record。选择与执行均检查目录存在、目录身份和缓存边界。缓存边界包括显式配置的 cache roots、session directory、`.cache`、`.anet` 和 `Library/Caches`，同时核对原始路径与 canonical 路径。显式 roots 必须存在，配置目录被替换时拒绝执行。自定义缓存位置由调用方传入，不能靠目录名自动发现全部缓存。

执行前重新发现 session，用原 selector 解析并比较完整 record。Unix 子进程通过保留的目录文件描述符调用 `fchdir`；daemon 自己不改变 cwd，也没有启动目录兜底。可执行文件必须是绝对路径，子进程环境清空。外部 JSON 写者没有参与事务锁，因此保证以最后重读为执行授权检查点，之后的外部变更不取消已经接受的执行。本层不证明真实 Desktop 存活，也没有接入生产常驻服务。

行为验收入口是 `cargo test -p fleet-daemon cwd_follows_session -- --nocapture`，已纳入 `scripts/check`。测试在同一 daemon 内按 A、B、A 切换临时项目并执行 `/bin/pwd`；拒绝场景中的命令若启动便会写 marker，断言 marker 不存在。

## Slice E native Desktop adapter（放宽验收完成，GUI 可见性未验）

`fleet_daemon::DesktopDelivery` 将一个显式 Unix socket、session directory 中选定的记录和原生 thread 绑定在同一个对象内。它通过原生 Codex WebSocket JSON-RPC 连接已运行的 app-server，不启动 server，不创建或恢复 thread，不安装 Reason。机制参考 [Reason Network 的 native input handoff](https://github.com/reason-machines/reason-network/blob/d530b08c44f1455aa6bc7727914ade402328662f/src/daemon/mail.rs)，只吸收现有线程投递与同线程跟进边界。

连接时与每次写入前检查原生 loaded-thread 列表、thread id、cwd、session 时效和目录身份。未加载、过期、目录被替换或线程不匹配时拒绝。`send` 同时用于首条投递与同线程续问；`read` 回读输出；`interrupt` 只接受本连接收到的活动 turn；`events` 回读后返回明确标记 `source=poll` 的线程变化观察；若连接实际收到原生通知，另外标记 `source=native`。它不是实时推送订阅。原生 `turn/start` 和 `thread/read` 不会自动订阅通知，本实现不为订阅而 resume 或抢占 Desktop。调用者必须选对属于 Desktop 的 socket。loaded 并不证明 GUI 可见。

`DesktopReceipt` 保存 delivery id、session 身份、cwd 与原生 turn id，只表示原生接受。单个连接不重复发送相同 delivery id，发送响应丢失时关闭该连接并返回 unknown，没有自动重连、重发或跨通道回退。跨进程持久去重和 Hub/桌面共用任务声明仍需后续集成；调用者必须保存 unknown 收据，不能通过新建对象重投同一任务。收据的证据分层见下方 Slice F。

可运行的接口示例保持一个进程和一个绑定。先由已有 session directory 生产者写入选定记录，再执行：

```sh
cargo run -p fleet-daemon --example desktop_delivery -- "$SOCKET" "$SESSION_DIRECTORY" "$THREAD_ID"
```

标准输入逐行接受 JSON 操作，例如 `{"op":"send","delivery_id":"unique-operation-id","text":"task text"}`、`{"op":"read"}`、`{"op":"interrupt","turn_id":"accepted-turn-id"}` 和 `{"op":"events"}`。续问使用新的 delivery id 与同一进程的 `send`。发送结果未知时先用原生线程回读核对，不能重复投递。

`cargo test -p fleet-daemon desktop_delivery` 使用隔离 Unix WebSocket fixture 核对请求与拒绝行为；`scripts/check` 保存完整输出与源码指纹。该证据不能替代真实 Desktop。E 的控制 socket 预检返回空 loaded-thread 列表，平台拒绝访问 Codex GUI。外循环随后以 CLI 同线程投递与 rollout 回读完成放宽验收，详见 `.scratch/pstack-relay/slice-e-receipt.md`。控制 socket 属于独立 app-server，不能由其 loaded-list 推断 Desktop 自带 stdio app-server 的可见状态。F 不绕过应用访问限制；没有可核实的 Desktop renderer 接线，因此保留该缺口，CLI rollout 仅作同线程记录证据。

## Slice F 收据与 brief 边界

`DeliveryTier` 是观察结果，不是能逐级推导的等级。原生 turn 完成也不能证明 Desktop 窗口可见。

| 值 | 当前证据要求 |
| --- | --- |
| `accepted` | 原生 `turn/start` 返回 turn id，或 Hub 明确 queued |
| `thread-visible` | 需要 Desktop renderer 对对应投递的可见证明；当前 adapter 没有该证据源，不产生此值 |
| `running` | 回读本连接已接受的同一 delivery、session、cwd、turn，原生状态为 `inProgress` |
| `completed` | 同一关联 turn 的原生状态为 `completed`；不代表任务产物或人工验收通过 |
| `failed` | 原生明确拒绝 `turn/start`，或对应 turn 为 `failed`、`interrupted` |
| `unknown` | 无匹配 turn、重复 turn、未知状态、连接丢失、回读被拒或证据不足；不能据此重发 |

`DesktopReceipt` 的身份和 tier 都不可由调用方修改。`send` 只返回 accepted，`observe(&receipt)` 回读后生成新收据，拒绝其他连接的收据。Hub 回执按 transport state 计算 tier，未验证 dack 不表示任务完成。序列化的 tier 与状态不一致时拒绝读取。

`send_with_model(delivery_id, brief, model)` 将 dispatcher 的可选模型参数单独写入原生 `turn/start.model`，不修改用户默认配置，也不从 brief 提取模型或要求执行者自报。省略参数时沿用原生默认配置，不声称模型身份已验。`desktop_delivery` 示例支持 `send` 的独立 `model` 字段，以及以 `delivery_id` 调用 `observe`。

`validate_brief` 在 Desktop 发送前运行，早于网络预检和 delivery id 声明。带 `MODEL_WRONG` 变体、模型自证以及中英文自报模型要求的 brief 被拒绝，正常任务可继续投递。该检查是保守的词法边界：保留词即使出现在引用或否定句中也会拒绝，不能保证识别任意自然语言改写。Hub transport 只接收已经密封的 envelope，无法检查正文；调用它的 dispatcher 必须在密封前运行同一校验。

`cargo test --workspace delivery_tier` 和 `cargo test --workspace brief_policy` 已接入 `scripts/check`。隔离测试证明边界与零发送拒绝行为；它们不补足真实 Desktop renderer 或真实 Hub 的闭环证据。
