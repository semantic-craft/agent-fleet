# ADR-0007：Codex 专属运行时生命周期

状态：已采纳设计，用户已明确批准新增专属本地服务、私有 Unix endpoint 和 relay 实例 scope 重批；部署仍须通过隔离验证、独立审稿与现场空闲检查。

## 原因

共享同一个 Codex home 能让不同客户端读取历史，但不同 app-server 不能同时持有同一 thread 的 writer。已验证的 Codex 0.163.0 中，`thread/unsubscribe` 不会立即卸载 thread；官方描述为最后订阅移除后仍须满足 30 分钟无活动宽限期。没有已验证的公开 RPC 能确定性立即释放单个 thread 的 writer。

隔离实验验证了专用 stdio app-server 正常 EOF 退出后另一 server 可以续接同一 thread。一次 idle 查询不足以证明可关闭：接受新 turn 后可能短暂读到旧 idle，必须等待确切 accepted turn 终态并检查其余资源。

## 决定与既有约定的关系

这是 ADR-0002/ADR-0003 对“不另建管理层、CLI 统一放 Herdr”约束的明确、受用户批准的特例：新增独立本地生命周期托管服务，只为确定性释放 Codex writer。它仍使用现有 Agenvo Codex connector、原生 thread/turn 协议和原生历史，不新增 connector kind、任务调度、自动输入重试或消息重放。

Connector 继续 attach-only；托管服务只能管理自己创建的 stdio 子进程。原共享 app-server 与已有业务会话保持不动。切换使用新 Unix endpoint 并重新批准实例 scope，不能在旧 socket 背后偷偷替换服务。

只有封闭输入、核验全部 accepted turn、子任务、后台终端、队列、交互及执行中请求后，才能正常 EOF 并等待进程退出。未知状态保留运行时且报告阻碍。部署通过原子 shutdown 协议，不能使用强杀、删原生锁或重启共享 server 取得所有权。

## 代价与边界

需要持久 thread→运行时映射和执行不确定状态，额外启动开销，以及崩溃后的人工核对流程。进程结束不等于任务业务成功；正常 EOF 的协议实验也不等于桌面 UI 实机验收。现场没有验证的 UI 项必须明确标记。

服务发布、endpoint 切换、relay scope 批准与真实入口验收分别记录，见[发布说明](../codex-runtime-release.md)。不得把基础退订补丁或服务健康检查宣称为完整根治。
