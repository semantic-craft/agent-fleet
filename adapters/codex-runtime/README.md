# Codex owned runtime

独立本地服务，经新私有 Unix WebSocket `/rpc` 兼容 Agenvo CodexConnection；不替换共享 app-server。运行 `node cli.mjs --config PATH`，配置包含绝对路径 `command`（Codex binary）、`stateDir`、`socketPath`、`cwd`、`codexHome`。不新增认证凭据，不解析原生会话目录。实际原生版本写入启动日志；安全检查依赖原生协议能力，未知响应保留 worker。

每个显式 `thread/start` 或已交还 thread 的 `thread/resume` 启动一个专属 stdio app-server。读取历史走既有 worker 或不 resume 的短期 observer。连接断线不结束 worker、不重放用户输入；重连 `initialized` 后仅补发仍未决的原生 server requests。

`turn/start` 的响应 ID 和 `turn/completed` 精确终态分别记录。完成后原子封闭新输入，确认全部已知 turns、loaded threads、collab receiver threads、分页历史、后台终端、排队输入、待答请求及执行中的 RPC，再 EOF。只有进程正常退出且检查期间没有生命周期变更才记 released，并发出带 `managedRuntime` 的 `thread/closed`。后台资源未结束时保留 worker，定时只读重查；未知状态不自动恢复。`thread/read` 原生结果附加 `managedRuntime` 供诊断。

检查期间输入明确拒绝；released 后需显式 `thread/resume` 再 `turn/start`。桌面持有 writer 时保留原生冲突。native `process/*`、`command/exec`、fork及未列入方法表的操作目前明确拒绝，不能将服务称为全协议透明代理。未知通知、缺失身份/终态、RPC超时或丢失确认均保守标 unknown。子任务依赖已验证的 collab receiver ID 与整个 server 的 loaded list；未能证明完整性的生命周期保持阻塞。读API安全失败同样不强制终止进程。

同一私有 Unix socket 的 `GET /health` 返回 `{ok,service,pid,units}`；`POST /shutdown` 原子拒绝新输入，仅全部 units 已 released 才正常关闭，否则 409 并撤回封闭。CLI持有 `stateDir/service.lock` 独占锁；正常关闭只移除本进程 token 匹配的服务锁，绝不删除原生 writer 锁。SIGTERM/SIGINT 遇未释放任务会拒绝；不能承诺系统关机或 SIGKILL 后任务继续。

崩溃后已有服务锁/socket不会被自动删除或覆盖。`node cli.mjs --config PATH --status` 仅离线读取自有 metadata，非 released 标 unknown；它不证明旧进程死亡。恢复必须先人工核对旧托管进程、每个 thread 的原生状态及所有背景工作，确认没有失去跟踪的执行后才能制定清理自有残留和重新启动方案；没有自动接管/自动删锁恢复命令。单纯PID不存在不足以证明所有后代已结束。

生产安装、endpoint/relay重新批准、独立不可变发布与真实 OAuth→relay 入口验收单独实施。这里的mock/原生隔离回归不等于Desktop UI验收，不包含生产会话。

恢复核对顺序（首版没有自动恢复命令）：

1. 用 `--status` 读取本服务的记录，保存 unit ID、thread IDs、精确 turn IDs、childPid/childObservedAt、managerPid 和已观察到的后台 processId/osPid。PID与时间仅辅助关联，不能证明进程身份或退出。
2. 对记录的 manager/child/background PID 使用系统只读进程查询，核对启动时间和父子关系；存在、身份不明、或仍有后代进程时保留残留，不启动第二owner。历史记录的后台列表可能不完整，不能据空列表证明没有后代。
3. 使用官方只读 `thread/read`（不 resume）和正确分页的 `thread/turns/list` 核对每个记录的 thread 与精确 turn；活跃、缺失、异常或未确认输入一律仍 unknown。若原worker仍可通过现存服务接口访问，检查其整个loaded list、background terminals、queue和pending，优先等待正常release，不强制关闭它。
4. writer是否释放不能仅由磁盘历史或PID推断。只有确认原托管进程和相关工作都结束，并由明确授权的原生客户端显式resume成功后，才有writer接管证据；此动作有所有权副作用，不属于离线inspect，也不得自动执行。冲突则停止，不删除原生锁。
5. 以上仍无法确认时，本版不提供安全自动恢复路径，保留服务锁/socket并报告阻碍。能够确认时另行审定仅处理本服务残留的维护动作和未知记录修复；不得恢复/重放旧输入。本版不把人工删除服务锁当成一条通用修复命令。

通知分类依据 Codex 0.163.0 导出的官方 `ConfigWarningNotification`、`ThreadQueueChangedNotification`、`McpToolCallProgressNotification` schema：`configWarning` 只提供配置诊断，不表示任务活动；`thread/queue/changed` 与 `item/mcpToolCall/progress` 会使进行中的安全快照失效，前者触发重新查询队列，后者不视为 turn 完成。字段缺失和其他未识别通知仍保守处理。原生明确拒绝 `thread/start` 时，仅在未记录 thread/turn/子任务引用/待答请求且原生 loaded list 确认为空后正常关闭新 worker；执行结果未知或已有活动时保留，并把原生拒绝原样返回，不重放创建。
