# scripts 组件契约

## Overview

本目录包含 fleet 运维 CLI、离线证据校验、保留的 DSH harness 与历史调用方辅助库。源码仍存在不表示它是当前派活入口；当前操作按 [ANet 文档](../docs/anet.md)。本契约描述本地源码，外部运行时与注入的 transport 不归本目录实现。

## Interface

| 文件与符号 | 边界 |
| --- | --- |
| [fleet.mjs](./fleet.mjs): `commands`, `doctor`, `contexts`, `launchd`, `check` | `doctor` 检查 overlay 的 launchd 服务；`contexts [--apply]` 比较或更新 connector context；`launchd NAME` 输出模板；`caller-plan --session SID --workspace WID` 输出创建描述。`check` 和 `release` 均以 2 退出，属于退役入口。 |
| [lib.mjs](./lib.mjs): `CONNECTOR_DIRS`, `expandHome`, `configPath`, `loadConfig`, `localMachine`, `connectorConfigPath`, `contextPlan`, `writeJson0600`, `render` | 配置路径、解析、主机选择、context 差异、JSON 写入及模板替换。`AGENT_FLEET_CONFIG` 优先于默认配置路径。 |
| [lib.mjs](./lib.mjs): `approvalRedirect`, `unlistedInstances`, `herdrValue`, `paneGone`, `forkRef`, `checkProject` | 保留的重定向、实例筛选、信封解包、pane 状态与旧 fork/project 辅助函数；不是新的派活入口。 |
| [harness.mjs](./harness.mjs): `HARNESS_VERSION`, `TaskSpec`, `createReadOnlyFixtureTask`, `RunContext`, `RunResult` | `bounded-dsh-v4` 冻结任务、上下文和结果；fixture 工厂只构造请求，不读文件。 |
| [harness.mjs](./harness.mjs): `ProviderAdapter`, `DshProviderAdapter.invoke`, `BaseHarness`, `ReadOnlyVerificationHarness`, `selectHarness` | 注入原生 transport 与固定验收策略；基础能力为 observe/submit。 |
| [harness.mjs](./harness.mjs): `HarnessRunner.run`, `HarnessRunner.prepareExternalRun` | 本地提交/观察，或先预留后返回外部提交描述；默认 journal 是进程内 Map。 |
| [file-journal.mjs](./file-journal.mjs): `FileJournal.acquire`, `assertHeld`, `confirmHeld`, `get`, `set` | 显式私有目录中的可选持久恢复存储；`acquire` 返回释放函数。 |
| [journal-lock.py](./journal-lock.py): `main`, `emit` | 内部 POSIX 锁助手，参数为 directory/filename；JSON 状态 locked/busy/held/unavailable，stdin 的 check/nonce 握手复核锁身份。 |
| [caller-routes.mjs](./caller-routes.mjs): `CALLER_ROUTES`, `createCallerRouter`, `routeTable` | 历史路由能力判断与 Markdown 表生成；只检查 host 注入能力，不执行派活。 |
| [dsh-check.mjs](./dsh-check.mjs): `dshTurn` | 可调用的历史 smoke 库：需要时创建 workspace/session，再经 harness 提交；fleet CLI 已不导入它。 |
| [dsh-native-caller.mjs](./dsh-native-caller.mjs): `FLEET_PRESET`, `fleetCallerCreate`, `observeFleetCaller`, `checkFleetCaller` | 构造 `fleet-agenvo` 会话描述、观察投影、检查注入的命令目录；不安装 provider。 |
| [codex-handoff.mjs](./codex-handoff.mjs): `checkCodexHandoff` | cross-backend create/submit 的纯证据预检；不是实际 writer 交接。 |
| [compatibility.mjs](./compatibility.mjs): `identityHash`, `assessCompatibility`, `assessRecovery` | 校验输入的十字段身份、有界 canary 与恢复依赖；不认证证据来源。 |
| [model-selection.mjs](./model-selection.mjs): `planModelSelection`, `reviewModelRestoration` | 全局默认选择/恢复的纯预检，所有结果均 `mutationAllowed:false`。 |
| [session-model-selection.mjs](./session-model-selection.mjs): `selectSessionModel` | 经注入 transport 读取 capability，再最多一次调用 `session/selectModelForSession`。 |
| [native-thread-selection.mjs](./native-thread-selection.mjs): `prepareNativeThread`, `checkNativeThreadRequest`, `checkNativeThreadObservation` | 原生独立会话参数与回读校验；CLI 为 prepare/check-request/check-observation 加 JSON 文件。 |
| [route-profiles.mjs](./route-profiles.mjs): `ROUTE_PROFILES`, `routeProfileError`, `routeProfileEvidenceMatches`, `observedRouteError` | 冻结的 marker、单次 read-fixture 与已知失败快照；不是当前模型健康发现。 |
| [audit-network.mjs](./audit-network.mjs): `auditProfile` | 解析给定 Surge 文本并输出存在性与计数；CLI 读取显式文件，不打印规则值。 |
| [release-evidence.mjs](./release-evidence.mjs): `verifyRelease` | CLI/函数核对指定 release 目录、40 位 revision 与文件哈希；不证明运行进程或来源真实性。 |
| [recovery-drill.mjs](./recovery-drill.mjs): `runSimulation`, `--child` | 合成恢复/回退演练；无参数 CLI 启动临时 HOME 子进程。 |
| [guard.mjs](./guard.mjs): `privateGuard`, `landscapeGuard` | CLI private/landscape；无参数运行两者。检查私有值及本机项目表，未识别参数目前不运行检查。 |
| [public-identifiers.mjs](./public-identifiers.mjs): `withoutPublicIdentifiers`, `overlayContains` | 私有值扫描中公共审查者标识的精确例外。 |
| [lint-routes.mjs](./lint-routes.mjs): `lintRoutes` | 校验文档中的历史路由表、入口指针和禁止措辞；CLI 输出结果。 |
| [gate.sh](./gate.sh): `check_body`, `--check-body` | 离线 PR 正文检查；在线参数 owner/repo、PR、完整 SHA，用 gh GraphQL 读取合并门禁数据。 |

## Invariants

- [harness.mjs:TaskSpec/RunContext](./harness.mjs)：任务声明 sideEffects、验收及预算；上下文必须使用当前 harness 版本和稳定 sessionId/requestId。指纹包含任务、上下文、adapter target 和策略类，冲突不复用。
- [harness.mjs:DshProviderAdapter.invoke](./harness.mjs)：只允许 projections/modelCatalog/follow/page/prompt/cancel 六个 session 方法。read-only 任务在提交前后检查实际权限；原生历史须连续、关联同一 rpcId/turn/header，文字成功不代替工具契约。
- [harness.mjs:HarnessRunner](./harness.mjs)：attempted 先写 journal，再发 mutation；不重试 prompt/cancel。未知结果保留原 ID 观察。prepareExternalRun 的 descriptor 含正文，不能当作脱敏 RunResult。超时仅结束观察，不证明原生运行停止。
- [file-journal.mjs:FileJournal/secure](./file-journal.mjs)、[journal-lock.py:main](./journal-lock.py)：存储目录显式提供且已存在，要求当前 UID、非符号链接、关闭 group/other 权限；文件须单硬链接。format 1、2 MiB 上限、字段白名单、稳定 inode；pending 请求和 cancelAttempted 隔离阻止其他请求接管。无清空/自动迁移入口。
- [compatibility.mjs:assessCompatibility/assessRecovery](./compatibility.mjs)：生产 canary 需 observed、未过期且限定范围通过；simulation 不能返回 verified。恢复须同一 native ID 与六项依赖哈希，成功仅 observe-only/maySubmit:false。
- [route-profiles.mjs:observedRouteError](./route-profiles.mjs)：production 中已知失败组合不能通过省略 profile 绕过。快照不授权扩大工作量或更改模型设置。
- [model-selection.mjs:reviewModelRestoration](./model-selection.mjs)、[session-model-selection.mjs:selectSessionModel](./session-model-selection.mjs)：协调/独占由调用者负责；前者需绑定 operationId/sessionId 的持久化证据，后者缺 capability 不写、不回退，unknown 不重试，applied 仍 durable:false。
- [native-thread-selection.mjs:prepareNativeThread/checkNativeThreadObservation](./native-thread-selection.mjs)：模型/档位仅由 selection 提供，目录须支持，回读须同 thread/turn 且 executed；Desktop 与业务仍 unverified。它不拦截宿主直接调用。
- [lib.mjs:loadConfig/render](./lib.mjs)：实例 ID 全局唯一、connector 必须受支持，未知模板变量报错。[fleet.mjs:doctor](./fleet.mjs) 的 MISSING 只输出，不自动设失败退出码。
- [gate.sh:check_body/ROLLUP](./gate.sh)：正文要求固定 squash 结尾且无指定墓碑词；在线门禁要求精确 head、OPEN、MERGEABLE、CLEAN、无未解决线程。CI 结果只列出，不决定门禁通过；本工具不执行合并。

## Side Effects

- [fleet.mjs:doctor/contexts/launchd](./fleet.mjs)：读 overlay、connector 配置和模板；doctor 执行 launchctl print；contexts --apply 替换 context 文件并提示重启；launchd 只向 stdout 输出 plist，不安装服务。
- [lib.mjs:writeJson0600](./lib.mjs)：写固定 `.tmp` 再 rename；创建 mode 为 0600，但没有 fsync、独占创建或既有临时文件权限修复，不能等同 FileJournal 的持久化协议。
- [harness.mjs:HarnessRunner](./harness.mjs)、[dsh-check.mjs:dshTurn](./dsh-check.mjs)、[dsh-native-caller.mjs:observeFleetCaller/checkFleetCaller](./dsh-native-caller.mjs)、[session-model-selection.mjs:selectSessionModel](./session-model-selection.mjs)：通过注入调用读写远端。只读任务仍会提交会话消息；这些库不是可随意执行的离线检查。
- [file-journal.mjs:FileJournal](./file-journal.mjs)：启动 Python 锁助手，写临时文件、fsync、rename、fsync 目录；释放时关闭 stdin，必要时 SIGTERM 助手。锁文件保留，正文不进入 journal。
- [guard.mjs:privateGuard/landscapeGuard](./guard.mjs)：执行 git ls-files，读取受管文件与可能存在的私有 overlay，枚举本机 Projects。[gate.sh:gql](./gate.sh) 在线模式经 gh 发 GitHub 只读请求并生成退出时清理的临时文件。
- [audit-network.mjs:CLI](./audit-network.mjs)、[release-evidence.mjs:verifyRelease](./release-evidence.mjs)、[native-thread-selection.mjs:CLI](./native-thread-selection.mjs)、[lint-routes.mjs:CLI](./lint-routes.mjs)：读显式文件或仓库文档并输出结果。[recovery-drill.mjs:CLI](./recovery-drill.mjs) 创建并最终清理临时 HOME；runSimulation 本身只计算合成结果。

## Verification

从仓库根目录运行 [package.json](../package.json) 定义的 `npm test`。文档定向检查为 `node scripts/lint-routes.mjs`；private guard 应使用空的临时 HOME/显式不存在的 AGENT_FLEET_CONFIG，避免加载真实 overlay。`npm run check` 当前调用退役的 fleet check，退出 2，不能代替测试。不要把 doctor、contexts --apply、联网 gate 或真实 DSH 调用混入文档验收。
