# 网络迁移与桌面验收

## 网络边界

Agenvo connector 主动通过 WSS `/connect` 连接 relay；客户端的 HTTPS `/mcp` 和 OAuth 在 relay。Mac 无需为 Agenvo 新增公网入站、DERP、P2P 打洞、自定义 DNS 或 TUN。以 fork 的 `packages/connector/src/connection.ts` 和上游 [安装说明](https://github.com/Xuanwo/agenvo/blob/main/docs/installation.md)、[架构](https://github.com/Xuanwo/agenvo/blob/main/design/architecture.zh-CN.md) 为依据；部署版本以 overlay 与 launchd 实际发布路径为准。

不要把 Tailscale 的 DERP 规则认作 reason-network 专用规则。[reason-network DESIGN.md](https://github.com/reason-machines/reason-network/blob/main/DESIGN.md) 中的 iroh/QUIC、n0 DNS、pkarr/mDNS 与 HTTPS relay 属于另一条路径。旧规则是否保留取决于当前消费者，不能仅凭迁移完成就删除。

2026-10-20 迁移前只读检查：

- Surge GUI 显示当前配置 Atlas、出站模式为规则判定。后续概览明确显示系统代理“由于子网设置，Surge 已暂停”、增强模式“禁用”（转换值 0）；进程页面显示“无进程”。因此不能把已加载 Atlas 等同于 Agenvo 流量由本机 Surge 接管。受限 shell 的 `scutil --proxy` 空字典不作为这一结论的独立依据。
- Atlas 含 Tailscale 直连、UDP 端口/进程例外、Home Tailnet 地址段路由、真实 IP 例外、DNS 劫持和 TUN 排除；它们也服务其他设备与 tailnet，不能删除。静态文件未找到 reason-network/iroh 专属规则，但远端 RULE-SET 未展开。
- DSH endpoint 是 Unix socket；Herdr 是本机原生服务；ZCode 当前 endpoint 是 tailnet 地址上的 HTTP 接收端。故“relay 无需 tailnet”不等于“当前 ZCode 部署无需 tailnet”。
- 四个 connector 的 launchd 路径均固定于发布提交 `e0d1daf43ffa6f02ba9ee14a068fb30e8ce7d399`，不是开发 checkout。
- 四份 connector 配置指向同一 HTTPS relay；`connection.ts` 将其改为 WSS 并追加 `/connect`，实际目标域名仅保留在 overlay，不写入仓库。`lsof` 观察到四个对应 PID 均有到同一 `198.18.*:443` 的 ESTABLISHED TCP，dispatch 另有到自身 tailnet 接收端的 TCP。配置与 socket 证据支持该 WSS 路径，但未用 TLS 抓包证明 socket 的目标名称。
- 本次未在本机 Surge 观察到上述连接及命中规则。`198.18.*` 是与 fake-IP 相符的现象，不能据此断言由哪台上游网关解析或代理。路由查询被沙箱以 `socket: Operation not permitted` 阻止，DNS 查询未提供有效数据；没有扩大访问。Surge 页面的外部 IP 是该页面的探测结果，不能当作 connector 的已证实出口。

### 重连可观察性

connector 的 stdout/stderr 仍指向 `/dev/null`，本机没有连接历史。断线次数从下面三处只读来源数，查看时不改任何配置：

- **本机**：各 connector 的 `status.json` 每次状态变化都会更新 `state` 与 `updatedAt`。每台 Mac 每 2 秒读一次，`offline`/`connecting` 回到 `online` 就是一次重连。
- **relay**：每 15 秒读 `/api/admin/state`，某台设备的 `epoch` 变一次，就是 relay 接受了一次新连接（`versionObservedAt` 是 hello 时刻）。脚本请求要带 curl 类的 User-Agent，Python `urllib` 的默认 UA 会被 Cloudflare 边缘直接返回 403。
- **网关**：在第二台 Mac 上，`surge-cli rule explain https://<relay 主机>/connect` 看命中规则和出口，`surge-cli --raw dump active` 看 relay 连接的策略、起始时间和字节数。

在 agenvo checkout 里运行 `npx wrangler tail --config wrangler.local.json --format json`，能看到 Durable Object 记的 `Connector <deviceId> disconnected with code <code>`，但不能用它数断线：WebSocket 升级请求和 `Connector … connected` 日志在 tail 里看不到；tail 自己的连接也走机场线路，会断开、会丢事件；relay 记的 `1006` 多半属于更早断掉的连接（见下）。

2026-10-23 17:49–18:21 三处同时记录（31.5 分钟）：

- 本机实际断线 30 次（主力 Mac 的 4 个 connector 共 27 次，第二台 Mac 的 dsh 3 次），成串出现，18:01–18:07 占 22 次。
- relay 的 28 次 `epoch` 变化都能在 ±20 秒内对上本机的一次重连，没有别的进程或机器在用这些设备身份。两台 Mac 上按 deviceId 搜索，凭据也只各有一份。
- 19 次能精确定时的断线里，18 次发生在 connector 发出 30 秒心跳后 0.07–0.26 秒，正好是经这条线路到 Cloudflare 的一个往返（复用连接上的请求往返 65–240 ms）；另 1 次是 18:07:40 两台 Mac 的 5 个 connector 在 1 秒内同时断开。
- tail 记到 23 条 `1006`，没有一条是 relay 自己发现现役连接断开时记下的：21 条属于早就被新连接替换的连接，1 条在被替换的同时清理，1 条无法判定。

原因：relay 主机在网关的 Surge 里没有专门规则，落到 `FINAL` → `Daily`（机场新加坡 Smart 组）→ 经国内中转的机场节点。这个出口到 Cloudflare 的路由不稳：同一个出口 IP 的请求落在 8 个 Cloudflare 接入点，新加坡和欧美日混在一起。一条连接中途被换到别的路径或接入点就失效了；connector 下一次心跳写进去，一个往返后收到重置，立即重连。Cloudflare 原接入点上的那条连接收不到任何东西，成了孤儿，要等新连接替换它，或几分钟到十几分钟后超时，relay 才记 `1006`。所以 `1006` 的计数既滞后，也对不上是哪次断线；17:10–17:15 那次样本里「relay 记到的 dsh 断开多于本机实际重连」就是这个原因。

影响：出口失效到下一次心跳之间（最多 30 秒），relay 仍把调用写进孤儿连接，命令送不到，10 秒后返回 `execution_unknown`；connector 重连时，旧连接上进行中的调用一律结束为 `execution_unknown`（可能已执行）；新连接 hello 完成前（约 0.1–0.3 秒）返回 `device_offline`；断线期间的事件不补发。派活技能因此要求：`unknown` 的写操作先读原生状态再决定，订阅事件也保留轮询。源码行为不变：30 秒心跳、超过 90 秒未收到 pong 时断开、带抖动的指数重连。

处理：2026-10-23 用户批准后，network-profiles 的 Atlas profile 给 relay 主机加了 `DIRECT` 规则（`[Rule]` 第一条），18:51 经该仓库的部署脚本下发。部署时 Surge 重载让 5 个 connector 同时断开一次，2 秒内全部重连到直连。按上面三处来源复测 18:51–19:23（31.6 分钟）：本机断线 1 次，与心跳无关（基线 31.5 分钟 30 次）；6 次 `epoch` 变化里 5 次是那次重载，全部对得上本机重连。直连落在 Cloudflare 欧洲接入点，复用连接往返约 225–295 ms，比机场线路慢，但长连接稳定。直连将来变差时，按同样方法试自建美国机的 `Atlas-US`，只看本机重连与 `epoch`，不看 tail。本仓库不替用户改网关配置。ZCode 同机回环迁移已另行获得明确批准并完成，见下节。不要为主动事件唤醒开 Mac 入站端口。诊断报告与复测脚本在 `~/Projects/_artifacts/agent-fleet/relay-1006-diagnosis/`。

只读结构检查（不输出主机、规则值、URL 或认证信息）：

```sh
node scripts/audit-network.mjs <当前配置文件>
```

脚本不调用网络、不改配置，也不能替代 GUI 确认实际启用配置、远端规则与连接路由。

## 获准的 ZCode 同机回环迁移

接收端与 dispatch connector 在同一台机器，当前代码已支持回环 HTTP；connector 只支持 HTTP(S) endpoint，没有现成的 Unix socket 传输。无需改代码或卸载 Tailscale。

用户逐项确认后实施三处配置：

1. 接收端 launchd 的环境变量设置 `DISPATCH_HOST=127.0.0.1`、`DISPATCH_ALLOWED_IPS=127.0.0.1`、`ZCODE_DISPATCH_DISCOVERY=0`。
2. `~/.config/agenvo/dispatch/config.json` 中目标 ZCode 实例 endpoint 改为 `http://127.0.0.1:7791`。
3. fleet overlay 对应实例 endpoint 同步更新。认证 key、source、执行权限、原生任务记录保持不变。

变更前保存三份原文件、原权限和 SHA-256；备份目录 0700、备份文件 0600，备份位置交付给用户，不提交个人配置。先确认 `/health` 的 `running=0`，再重新加载接收端定义；回环 `/health` 成功后才同步 endpoint 并重启 dispatch connector。endpoint 改变会使 scope 指纹变化，使用现有管理员凭据核对并批准该实例的新指纹，不能把它误诊为网络不通。

第一次 bootout 后立即 bootstrap 返回错误 5，已自动恢复原文件并验证原地址健康，三份原文件哈希一致。随后增加卸载后短暂等待和有界注册重试，成功完成；不能由一次成功推断所有 launchd 错误 5 都是时序问题。

迁移后证据：`lsof` 仅见 `127.0.0.1:7791 LISTEN`；三份原文件权限保留；回环和 Agenvo `health` 都为 `ok=true, running=0, strict_runner=app, app_available=true`。Agenvo 回读原 job `bc2bc1454d801170` 仍为 `runner=app, state=completed, exit_code=0`，requestId `410cef04-8915-4a69-a4bb-8c4904248bf0`。没有重复模型任务，没有删除共享 Tailscale 或 Surge 规则。

回滚：先确认无运行任务，恢复三份已验证备份及原权限，重新加载接收端定义并重启 dispatch connector；核对 relay 当前 scope，必要时重新批准恢复后的指纹；最后验证原地址健康与已有 job 回读。不要对正在使用的文件盲目覆盖，先检查是否有迁移后的并发修改。当前迁移没有更换发布版本；以后重新生成 launchd 时也应保留这三个回环变量，否则默认 discovery 会重新引入 tailnet 依赖。

## 原生会话验收记录

通过当前任务的 Agenvo 连接器先发现 schema，再派只读任务。源仓当时干净，HEAD `69152a0`。没有使用 Magpie、Claude 登录、录音、语音付费测试或修改业务项目。

| 运行时 | 接受与执行证据 | 结果与连续性 |
| --- | --- | --- |
| DSH | session `session-80421c01-192a-4342-a6e2-2df2b92b6a7b`；第一轮 requestId `fleet-20261020-80421-dsh-1`；`tool/call` seq 20 实际执行 `pwd && git rev-parse --short HEAD`，`tool/result` seq 23 无错误 | 原始回复标记 `FLEET-20261020-80421-DSH`、HEAD `69152a0`；seq 28 `turn/end completed`。第二轮 requestId `fleet-20261020-80421-dsh-2` 不重复 nonce，回复 `nonce=amber-4821 HEAD=69152a0 19*23=437`；seq 38 `turn/end completed` |
| ZCode | job `bc2bc1454d801170`；session `sess_5762380c-f2c6-4096-b3b0-752bbd6798e1` | `runner=app, state=completed, exit_code=0`；标记 `FLEET-20261020-80421-ZCODE`，cwd 指向源仓，HEAD `69152a0`。schema 无远程追问接口；不把另建 job 算作连续性 |
| Herdr | `session.list` 确认 default；隔离 workspace `w4`、pane `w4:p1`、agent `fleet-80421`、terminal `term_65e40643901072`。用户专项批准后重试 `agent.start`，requestId `07d99de4-7243-4379-a147-22bcbcc54b4b` | Codex 原生终端显示 YOLO mode，第一轮实际执行命令并回报 `FLEET-20261020-80421-HERDR`、HEAD `69152a0`；第二轮回复 `nonce=cobalt-6193，HEAD=69152a0，31*17=527`，read requestId `153f3b6b-b944-44da-bd86-97b610a2fe36`。同会话连续性通过 |

Herdr 首次拒绝发生在 `agent.start`，理由是尚未提交只读任务前即给予 unrestricted execution；收到用户针对 full-access 启动的专项批准后，仅重试原操作一次。随后 Codex 原生目录信任菜单阻塞启动；读取 visible 终端确认目标为本测试仓库后，按获准启动范围选择 Trust and continue（原生提示说明会保存信任决定），未处理任何登录。该状态应与工具自动审批拒绝区分。

两轮 prompt requestId 分别为 `cfec2a55-01c7-4982-996a-0736932855d1` 和 `2def747a-6a8c-42a9-b034-07e26ec49f06`。验收结束时 agent 已 `done, interactive_ready=true`，但 `agent.get.startup.outcome` 仍保留之前 `agent_not_ready` 的历史失败；以当前原生状态和终端实际结果核对，不把旧 startup 结果当作当前执行失败。blocked 时读取超过屏幕行数可能得到 `agent_not_idle`，改用 `agent.read source=visible`，不要因此重复启动。保留隔离 pane，未打断已有会话。

## 主动唤醒是另一项验收

本次 DSH 前两轮证明“输入 → 原生执行 → 回读 → 同会话追加”成立；第三轮在主会话建立专用订阅后，验证了原生事件 → 主会话触发 → 原生结果回读。子任务提交后未轮询。主会话回传的证据：

- 输入 requestId `fleet-push-20261020-0738-dsh-1`；Agenvo 接受回执 `d546a0ff-bce6-4ab1-8216-08cc683c1d8a`，收到回执后时间 07:39:23 UTC。
- eventId `4d26a4da-f47c-41ef-bc6c-0df756f81560`，事件时间 `2026-10-20T07:39:24.566Z`，同一 DSH session 的 turn 3、seq 49、completed；主会话收到事件触发消息时间 07:39:37Z。
- 主会话随后 `session/follow`，read requestId `766a59b1-5420-4946-8533-4c47ec6e6c73`、cursor 49；assistant/message seq 47 精确为 `FLEET-PUSH-20261020-0738 nonce=amber-4821`，turn/end seq 49 completed。

这次 DSH 推送验收通过；不扩展为断线重放、其他运行时或所有平台的推送通过。事件触发和回读证据由主会话报告，本地执行子任务不冒称独立观察了主会话 UI。

只读平台检查已确认 Agenvo 出现在 event sources，且 `discover_webhook_schema` 返回 `runtime.changed`，所以缺口不在事件 schema 不可发现。`peek` 返回的两项自动化分别用于定期维护和旧入口回执，没有展示 Agenvo 测试订阅；返回值未包含完整 trigger 元数据，不能据此证明 relay 全局没有订阅。这是订阅建立前的检查，不覆盖随后主会话建立的专用订阅；此次报告尚未收录订阅 ID。

主会话最小测试：用 Automations 的 event-source discovery 取得当前 `connector_id`，使用下列 trigger，经平台正常订阅流程创建一次事件监听（不带轮询 schedule、不手工生成 callback/签名密钥），然后在上表已有 DSH 会话追加唯一标记的一轮，避免新建会话。不要把监听创建成功算作事件送达成功。

```json
{
  "connector_id": "<event-source discovery 返回的 Agenvo connector_id>",
  "event": "runtime.changed",
  "params": {
    "deviceId": "<search 返回的 deviceId>",
    "instanceId": "<search 返回的 DSH instanceId>",
    "threadId": "<已有 DSH sessionId>",
    "nativeTypes": ["turn/end"]
  }
}
```

schema 要求 `deviceId`、`instanceId`；`serviceId`、`threadId`、`nativeTypes` 可选。DSH fork 的 `serviceId` 是实例 ID，不能照抄 Codex/Herdr 的 `default`；此测试省略它。`agenvo.resync_required` 可越过 thread/nativeTypes 过滤，处理程序仍须检查事件类型。专用订阅按用户后续指示保持启用；本执行任务不修改它或其他订阅。

按上游 [events.md](https://github.com/Xuanwo/agenvo/blob/main/docs/events.md)，平台提供 callback 与签名密钥，不手工新建凭据。后续由主会话：

1. 确认已发现 `runtime.changed` 并建立目标实例订阅，记录订阅 ID、有效期和 session 过滤条件。
2. 派唯一标记的小任务后停止轮询；保留平台收到事件的时间、事件 ID、nativeType 和 threadId。
3. 确认同一主会话确实因该事件恢复执行，随后回读原生结果。完成信号仍不等于业务成功。

事件可能重复、乱序，无历史重放；断线或 `agenvo.resync_required` 后读取原生状态，不盲目重新派活。DSH 先用 `session/follow maxMessages:1`；需要工具证据才按 cursor 用 `session/page` 小页读取，超限不视为未完成。Herdr 用真实 session 名，blocked 时先读终端，不能把 accepted/idle 当成功。
