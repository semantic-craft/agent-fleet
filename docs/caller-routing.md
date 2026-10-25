# 按调用方能力选择派活入口

## 三条路线，不互相冒充

| 调用方／目标 | 选择 | 不可外推 |
| --- | --- | --- |
| dot → Codex，当前调用方已有原生平台任务工具和电脑连接授权 | 优先原生平台任务通道，创建／执行／读回 | 不能推断 DSH、GrokBot 或任意 agent 也有这些工具 |
| dot 或其他已授权 MCP 调用方 → DSH／Herdr | Agenvo，发现实例与 schema 后调用 | 不证明 Codex writer handoff 通过 |
| DSH → Agenvo → Codex Desktop；GrokBot → Agenvo → Codex Desktop | 保留且需要实现的外部派活路线 | 不能用 dot 原生通道替代，直接及 DSH 同 host 文本执行已验；GrokBot 发起与 Desktop 可见仍未验 |

用户目标仍是 Codex Desktop 可见后由 agent 提交，不退回 exec 或用户 Send。
[交接契约](codex-desktop-handoff.md)和[probe 候选](codex-handoff-probe.md)的
`blocked_native_writer_release` 只约束实际跨 backend writer 转移，不能阻断同 host 执行或第一条日常派活。

## dot 原生任务通道

用户明确要独立 Codex 会话且指定模型/档位时，使用
[原生会话选择契约](native-thread-selection.md)生成并检查创建参数。会话类型与模型选择
属于该次请求；写在子 agent 提示词中不能设置主会话模型。未指定模型时保留平台默认，
不把本契约的显式选择要求推广成每次都替用户指定模型。

先发现当前平台实际提供的创建、执行／续接、状态／结果读取工具及其 schema。
工具名与接口随平台而异；本仓不定义统一的 cloud-task API，也不保证某个固定
工具名在所有账号可用。调用方缺工具或电脑未连接／未授权时，明确报告该前提，
不能伪造支持。跨机先发现环境，核对目标主机和项目路径，不以调用方本机目录代替。

在用户授权的新任务范围内创建一个独立任务，传入完整目标、工具范围、验收条件；
保存返回的原生任务 ID。等待并通过同一原生入口读回实际结果和终态。unknown 创建
先核对原任务，不重复建任务。桌面是否可见按平台返回的打开入口及实际观察分别记录，
不要把任务执行成功推断为其他 app-server 的 writer 已释放。

本次维护已通过原生平台委托在连接的电脑上完成源码工作、测试与结果回读；这是
现有会话的事实，不是新物理机器、所有模型或所有第三方客户端的普遍验收。
本轮路由纠正没有新增真实任务或调用模型。这里只记录泛化事实，不保存私人轨迹。

## 外部调用方与迁移

DSH、GrokBot 的 Codex Desktop 派活需求明确保留。它们有 Agenvo MCP 访问时按
[技能](../skills/delegate-via-agenvo/SKILL.md)发现方法；不能引导它们调用未提供的
平台专属任务 API。外部创建／（仅跨 backend 的原生交接）／Desktop 可见／同线程 agent `turn/start`
必须逐项验证；当前未通过就只报告该路线阻断，不废弃需求、不改写其他路线状态。

新机器验收分开记录：原生任务工具、连接授权、目标发现、任务结果；Agenvo 的
DSH／Herdr 回读；外部 Codex 交接。账号、版本、平台工具和模型变化都需重验适用
契约。不同入口的任务 ID、会话与权限独立，不自动跨入口接续、迁移 writer 或重放。

## 按三个调用方分别验收

“顺畅”需要实际派活、目标会话可见且 ID 关联正确、同会话继续、结果返回四项。
以下是证据现状，不是完成声明；blocked 是待解决，不能当完成定义。

| 调用方路线 | 实际派活 | 目标会话可见与关联 | 同会话继续 | 结果返回 | 当前结论 |
| --- | --- | --- | --- | --- | --- |
| dot → 原生平台 → Codex | 当前维护委托已有执行事实 | 有原生任务关联；本轮未单独做逐轮 UI 可见性验收 | 本轮未单列独立验收证据 | 当前维护任务已有结果回读 | 日常通道已使用，四项专项矩阵尚未全部验收 |
| DSH → Agenvo → Codex Desktop | 已验：原生 MCP 发起纯文本任务 | 未验 Desktop 可见／控制权 | 已验：同 thread 两轮 | 已验：持久历史精确答案回到 DSH | 同 host 文本闭环通过；Desktop 项仍待解决，非完成 |
| GrokBot → Agenvo → Codex Desktop | 未验 | 未验 writer／Desktop 关联 | 未验 | 未验 | 外部需求待解决，非完成 |

两条外部路线即使共用 connector，也要从各自调用方的真实授权入口分别验证。
已有 GrokBot→Agenvo→其他运行时、Agenvo→DSH／Herdr 的成功不填入上述 Codex 格子。
离线 guard、probe mock 或 dot 原生任务成功均不能补作外部两条的端到端证据。
下一轮 live 验收先解决正式接入支持与必要授权，再用唯一无敏感任务逐项回读；
本轮仅纠正文档与回归，不创建任务、不突破 UI／进程探测拒绝或扩大访问。

## 机器可读路由

唯一结构源是 `scripts/caller-routes.mjs`；`createCallerRouter` 仅从宿主注入的函数能力判定可用入口，任务 JSON 的 true/verified 不能启用外部 writer 路线。宿主可注册 same-host 的 observeTarget/create/read/startTurn 函数能力，resolver 返回 available 仅表示入口可用且要求派活前真实观察，不代表调用方或 Desktop 已验。没有注册就明确能力缺失，不推定必须交接。`fleet check` 的 DSH 分支调用此 resolver 后进入 Harness。原生平台工具仍由平台宿主提供，resolver 不代理或认证平台权限，也不构成真实验收。

<!-- caller-routes:start -->
| Caller | Target | Route | Acceptance |
| --- | --- | --- | --- |
| dot | codex | native-platform | partial-maintenance-evidence |
| dsh | codex | agenvo-desktop | unverified |
| grokbot | codex | agenvo-desktop | unverified |
<!-- caller-routes:end -->

`npm run lint:routes` 校验表与跨文件禁用的旧路由表述，包含在 `npm test` 中。lint 只检查明确规则，不声称理解任意自然语言矛盾。

同 host 的[脱敏直接基线](evidence/codex-same-host.json)独立于上述三调用方验收，不填充 DSH/GrokBot 成功格。

## DSH 原生调用方的真实文本闭环

[脱敏回执](evidence/dsh-codex-same-host.json)记录一次真实 DSH Agent 调用 Agenvo search/execute，创建一个 Codex thread、串行提交两轮、读取完整持久历史，再把结果返回 DSH。不是 dot 直接调用代证。两轮均 completed 且精确命中不同标记，模型工具项为 0；宿主既有 hooks/MCP 启动仍会运行，不能称整个进程零文件活动。通知读取出现 gap，成功依据是完整持久历史，不是主动推送。

复用步骤：核对现有专用 caller 的 fleet-agenvo、空闲和 commands/list；只读 agenvo-status 已 connected 时不要重开 OAuth。让该 DSH Agent 使用已授权的 Agenvo MCP 发现目标 schema；创建一次隔离 thread，第一轮完成回读后才在同 thread 提交第二轮。保存每次 native ID 和 envelope requestId，unknown 先查原 ID，不能重复创建。用 thread/turns/list 的 full items 和 nextCursor 核对最终文本及工具项，再核对 DSH 自身终态。只保留脱敏证据，不发布完整通知或 hooks 内容。

本次未更改模型默认值、权限或服务。Codex 回执的 provider=openai 不是底层供应商/协议证明。Desktop UI、交回控制权、GrokBot 发起和新机迁移仍需各自验收；机器可读完整 Desktop 路由状态继续 unverified。
