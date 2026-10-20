# 网络迁移与桌面验收

## 网络边界

Agenvo connector 主动通过 WSS `/connect` 连接 relay；客户端的 HTTPS `/mcp` 和 OAuth 在 relay。Mac 无需为 Agenvo 新增公网入站、DERP、P2P 打洞、自定义 DNS 或 TUN。以 fork 的 `packages/connector/src/connection.ts` 和上游 [安装说明](https://github.com/Xuanwo/agenvo/blob/main/docs/installation.md)、[架构](https://github.com/Xuanwo/agenvo/blob/main/design/architecture.zh-CN.md) 为依据；部署版本以 overlay 与 launchd 实际发布路径为准。

不要把 Tailscale/Headscale 的 DERP 规则认作 reason-network 专用规则。reason-network 的 iroh/QUIC、n0 DNS、pkarr/mDNS 与 HTTPS relay 属于另一条路径。旧规则是否保留取决于当前消费者，不能仅凭迁移完成就删除。

2026-10-20 只读检查：

- Surge GUI 显示当前配置 Atlas、出站模式为规则判定。AX 中增强模式和系统代理是按钮，没有返回可靠开关值；受限 shell 的 `scutil --proxy` 返回空字典，不能据此断言真实宿主已关闭代理。
- Atlas 含 Tailscale/Headscale 直连、UDP 端口/进程例外、Home Tailnet 地址段路由、真实 IP 例外、DNS 劫持和 TUN 排除；它们也服务其他设备与 tailnet，不能删除。静态文件未找到 reason-network/iroh 专属规则，但远端 RULE-SET 未展开。
- DSH endpoint 是 Unix socket；Herdr 是本机原生服务；ZCode 当前 endpoint 是 tailnet 地址上的 HTTP 接收端。故“relay 无需 tailnet”不等于“当前 ZCode 部署无需 tailnet”。
- 四个 connector 的 launchd 路径均固定于发布提交 `e0d1daf43ffa6f02ba9ee14a068fb30e8ce7d399`，不是开发 checkout。
- 未取得 connector 到 relay 的 Surge 活动连接、命中规则与出口证据；不能仅由 FINAL 规则推断实际 WSS 出口。

最小建议是暂不调整网络。先用 Surge 活动连接核实 WSS 的规则/出口，只有出现连接或重连问题才讨论针对 relay 主机的规则。若希望移除 ZCode 的 tailnet 依赖，先独立验证本机回环监听，再提出 endpoint 变更及实例重新批准方案；本次未执行。不要为主动事件唤醒开 Mac 入站端口。

只读结构检查（不输出主机、规则值、URL 或认证信息）：

```sh
node scripts/audit-network.mjs <当前配置文件>
```

脚本不调用网络、不改配置，也不能替代 GUI 确认实际启用配置、远端规则与连接路由。

## 原生会话验收记录

通过当前任务的 Agenvo 连接器先发现 schema，再派只读任务。源仓当时干净，HEAD `69152a0`。没有使用 Magpie、Claude 登录、录音、语音付费测试或修改业务项目。

| 运行时 | 接受与执行证据 | 结果与连续性 |
| --- | --- | --- |
| DSH | session `session-80421c01-192a-4342-a6e2-2df2b92b6a7b`；第一轮 requestId `fleet-20261020-80421-dsh-1`；`tool/call` seq 20 实际执行 `pwd && git rev-parse --short HEAD`，`tool/result` seq 23 无错误 | 原始回复标记 `FLEET-20261020-80421-DSH`、HEAD `69152a0`；seq 28 `turn/end completed`。第二轮 requestId `fleet-20261020-80421-dsh-2` 不重复 nonce，回复 `nonce=amber-4821 HEAD=69152a0 19*23=437`；seq 38 `turn/end completed` |
| ZCode | job `bc2bc1454d801170`；session `sess_5762380c-f2c6-4096-b3b0-752bbd6798e1` | `runner=app, state=completed, exit_code=0`；标记 `FLEET-20261020-80421-ZCODE`，cwd 指向源仓，HEAD `69152a0`。schema 无远程追问接口；不把另建 job 算作连续性 |
| Herdr | `session.list` 确认 default；隔离 workspace `w4`、pane `w4:p1` | 启动 Codex 的 full-access 参数被当前客户端自动审批拒绝，未执行；没有改用其他入口绕过。执行、结果和追问均未验收 |

Herdr 的拒绝发生在 `agent.start`，理由是尚未提交只读任务前即给予 unrestricted execution。恢复需取得针对该操作的新授权证据，先确认 pane 当前状态，再按审批规则重试；不能将拒绝误诊为 Herdr 服务故障。保留隔离 pane 供核查，未打断已有会话。

## 主动唤醒是另一项验收

本次 DSH 两轮证明“输入 → 原生执行 → 回读 → 同会话追加”成立。**没有证明 `runtime.changed` 主动唤醒 dot 主会话。** 子任务主动调用 `session/follow` 得到 completed 不能替代事件送达证据；当前未建立并核对主会话订阅。

按上游 [events.md](https://github.com/Xuanwo/agenvo/blob/main/docs/events.md)，平台提供 callback 与签名密钥，不手工新建凭据。后续由主会话：

1. 确认已发现 `runtime.changed` 并建立目标实例订阅，记录订阅 ID、有效期和 session 过滤条件。
2. 派唯一标记的小任务后停止轮询；保留平台收到事件的时间、事件 ID、nativeType 和 threadId。
3. 确认同一主会话确实因该事件恢复执行，随后回读原生结果。完成信号仍不等于业务成功。

事件可能重复、乱序，无历史重放；断线或 `agenvo.resync_required` 后读取原生状态，不盲目重新派活。DSH 先用 `session/follow maxMessages:1`；需要工具证据才按 cursor 用 `session/page` 小页读取，超限不视为未完成。Herdr 用真实 session 名，blocked 时先读终端，不能把 accepted/idle 当成功。
