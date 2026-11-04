# Agent Network：当前派活传输（2026-11-02）

**这是跨 ANet 身份委派的唯一传输。** 平台原生子任务是执行环境，不注册第二套网络身份或任务协议。 不经 CommHub MCP、不经 `anet.sonori.dev`、不经 #90 门铃桥。那些见 README「遗留附录」。

Desktop 进窗、把 cwd 钉在 `~/Projects` 的项目上，不是这条传输。边界在 [reason.md](reason.md)。同一件活不要既 `delegate` 又进窗。签到另线，本页不宣称 Reason 已通。

只引用官方文档：<https://docs.agentnetwork.org.cn>。本页只记本机用法，不另造层。

## 现状

- 版本：官方 anet **0.3.9**（[llms.txt](https://docs.agentnetwork.org.cn/llms.txt)：Current release 0.3.9）。
- 本机身份：`default`、`turing`、`cindy`，一机多身份，各自 `~/.anet/ids/<name>`（`default` 用 `~/.anet`）。见 [Identity](https://docs.agentnetwork.org.cn/docs/en/latest/topics/identity.html)、[CLI：Identity and data directory](https://docs.agentnetwork.org.cn/docs/en/latest/reference/cli.html)。
- 入站：三个身份都是 `inbound policy closed`（默认值），只接 allow／trust 名单上的对端。见 [Delegation：inbound policy](https://docs.agentnetwork.org.cn/docs/en/latest/topics/delegation.html)、[Config：inbound.policy](https://docs.agentnetwork.org.cn/docs/en/latest/reference/config.html)。
- 名单：只 allow 自有 AID（`turing` ⇄ `cindy` 互相 allow）。不加外人，不放宽 policy。
- 派活：`anet --id turing delegate <AID> "…" --watch`。`--watch` 在 completed 时退出 0，failed／rejected／canceled 退出 1，input-required 退出 3（[CLI：Exit status](https://docs.agentnetwork.org.cn/docs/en/latest/reference/cli.html)）。参数只收完整 AID。
- 宿主插件：Cindy 侧是 [plugins/agent-network-cindy](../plugins/agent-network-cindy/README.md)，经 `anet --id cindy mcp`（stdio）用官方 MCP 工具 `list_tasks`／`get_task`／`reply_task`／`send_message`（[MCP tools](https://docs.agentnetwork.org.cn/docs/en/latest/reference/mcp-tools.html)）。

## 教训

- **peers allow／trust、放宽 inbound 要在终端确认**（[CLI：Confirmation on the terminal](https://docs.agentnetwork.org.cn/docs/en/latest/reference/cli.html)）。非 TTY 调用返回 `tty_required`。本机 0.3.9 的 `guide overview` / `guide setup` 明确要求 operator 本人在终端确认；agent 只给出命令，不代开 PTY 回 `yes`。`allow` 是接单，`trust` 还允许本地自动执行，两者不可混称。
- **`ANET_HOME` 决定身份根目录**（默认 `~/.anet`，[Config：Environment variables](https://docs.agentnetwork.org.cn/docs/en/latest/reference/config.html)）。shell 里残留错的 `ANET_HOME` 会让 `--id` 指向别的身份树。本机命令写成 `env -u ANET_HOME ANET_DATA_DIR=$HOME/.anet ~/.local/bin/anet --id <name> …`。
- **同一身份只挂一个常驻处理者。** 两个插件／worker 轮询同一身份会重复接单；换插件先停旧的再 smoke。

## 官方架构与本仓边界

| 所有者 | 唯一职责 | Fleet 的边界 |
| --- | --- | --- |
| ANetCore | 身份、签名、编码和线路类型 | 不复制或 fork 一套协议实现 |
| ANet | 节点身份、准入、任务、重试与证据 | 经已安装 CLI / 官方 MCP 调用；不替代其任务状态 |
| Hub | 注册与密封消息中继 | 不是读取任务正文的集中派发器 |
| agent-fleet | 可复用宿主适配、派发契约、诊断、测试与运维说明 | 私钥、token、任务数据库和恢复资料不进 Git |
| 消费者 | 业务分工、宿主安装入口 | 引用本仓正本，业务逻辑留在业务库 |

依据：[官方概览](https://docs.agentnetwork.org.cn/docs/zh-cn/latest/intro/overview.html)、[协议](https://docs.agentnetwork.org.cn/docs/zh-cn/latest/reference/protocol.html)。跨仓契约见 [dispatch-contract.md](dispatch-contract.md)，技能正本见 [delegate-via-anet](../skills/delegate-via-anet/SKILL.md)，盘点与迁移证据见 [anet-alignment.md](anet-alignment.md)。

## 版本、身份与宿主接入

2026-11-03 本机只读探测：`anet --version` 为 **0.3.9 / d1ad854c**。官方文档列 Hub 0.3.3、Core 0.16.2、插件 0.3.0；这是文档版本，不是本机 Hub/Core 部署证明。升级先读实际可执行文件的 `guide overview`、`guide setup`，不静默回退 0.2 或旧传输。

每个运行身份有独立数据目录，同一身份不复制到两台机器并发。显式选择 executable、identity 和数据根，选择失败就停止，不从 PATH 猜其他二进制或身份。不要读取/复制身份目录里的私钥、token 和任务数据。

官方插件与 `agents wire` 是替代入口，同名配置可遮蔽插件；只保留一种接线。工具 schema 取自当前 MCP 会话，使用 installed binary 的 guide，不维护固定工具清单副本。[Codex 接入](https://docs.agentnetwork.org.cn/docs/zh-cn/latest/agents/codex.html)、[MCP](https://docs.agentnetwork.org.cn/docs/zh-cn/latest/howto/mcp.html)。

文档对 daemon 自动启动存在版本差异。本机 `guide setup` 说明 MCP 在条件允许时启动所属节点；CLI 在沙箱内返回 `daemon_not_running`，沙箱外 `status --json` 与 `peers list --json` 确认已有节点实际在线：这是沙箱访问限制，不能把该错误直接当作停机或新增授权门槛。本轮未启动节点或修改接入权限；接线、权限修改及真实宿主安装单独验收。

## 结果与回流

- ANet 的 `completed`、`effect_status`、`receipt_verified`、`delivery` / `reason` 分别记录；回执有效不等于业务正确。
- 超时/断连可能已经执行，效果未知记 `UNVERIFIED`，保存 task id、message id、state_seq 后回读同一任务，不自动重新派发。
- 适配器只保存宿主任务关联与待核对回流，不另造网络状态机。分页不能丢单；请求方取消后停止继续驱动本地任务并报告宿主取消能力的边界。进度消息必须来自实际宿主状态，不能用心跳掩盖失联。
- A2A `pushNotifications:false`；SSE 是在线事件流，不是离线宿主唤醒。必要本地回流适配可保留，不能宣称官方已提供离线推送。
- macOS 本地控制/A2A 后端优先 Unix socket；控制 token 与 A2A token 不混用，沙箱失败不能降级 direct。服务会话与交互会话独立。

依据：[MCP 结果](https://docs.agentnetwork.org.cn/docs/zh-cn/latest/howto/mcp.html#reading-a-result)、[工具](https://docs.agentnetwork.org.cn/docs/zh-cn/latest/reference/mcp-tools.html)、[限制](https://docs.agentnetwork.org.cn/docs/zh-cn/latest/reference/known-limitations.html)、[本地 A2A](https://docs.agentnetwork.org.cn/docs/zh-cn/latest/howto/local-a2a.html)。源码测试、生成包、宿主加载、真实任务回流四层分别报告；没有后两层证据就标未验证。
