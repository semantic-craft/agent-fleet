# EGM drift report — 2026-11-03

对照基线：`778df92528fabc06e9977a6cfed4922ba1f5fd20` 的本地源码与本轮 5 份 EGM.md。范围：根 README、plugins 下两份 README、docs/ 顶层 20 份 Markdown，及新增契约。按文件顺序读取并对照相关本地实现、manifest、模板和候选 patch；不把 docs/legacy 中明确标注的历史目标当作当前实现承诺。外部仓库、官方协议实时版本、本机身份/安装、网络回流和历史验收数字未重新核验；“No drift detected”仅指本地可核对的范围，不为外部事实背书。

BFS 全树授权已给出；17 个子目录剪枝后，生成根、scripts、plugins、plugins/agent-network-cindy、plugins/agent-network-cindy/node 五个 EGM.md。隐藏/系统、依赖、输出、符号链接、忽略项、纯测试与纯文档/配置目录不生成契约。原有 README 均保留。下列差异只报告，不修业务代码；P1 为可能影响操作结果判断的接口缺口，P2 为可误导操作的文档/实现差异，P3 为索引或覆盖缺口。

## README.md

No drift detected.

## plugins/README.md

- **P3 — 版本索引陈旧。** 表格标记 current (0.3.0)，[ghost.json](../plugins/agent-network-cindy/ghost.json) 的 version 为 0.3.1。建议以 manifest 同步索引；不是要求升级或重装插件。

## plugins/agent-network-cindy/README.md

- **P1 — 宿主 IPC 包装缺少明确契约/证据。** README 宣称 send_to_peer 返回任务 ID、bridge_status 查询状态；[main.js:toolCall](../plugins/agent-network-cindy/main.js) 要求 `cindy.node.request` 返回 `{ok,result}`，而 [worker.cjs:bridge/status/sendToPeer](../plugins/agent-network-cindy/node/worker.cjs) 的 JSON-RPC result 分别直接是状态对象与 `{sent,outcome,...}`，没有 ok/result。离线 VM 将 worker 的原始 result 形状返回给 main.js，mcp=up 仍得到 BRIDGE_FAILED。**这证明透传 result 时不兼容，不证明真实 Cindy 是否另包一层**；需明确宿主 API 返回约定，并在同一宿主边界做集成验证。若发送已被接受而 UI 报错，不可换新 callId 重试。此次未运行真实发送、未更改宿主。
- **P2 — “read-only”遗漏本地写入。** [worker.cjs:bridge/status](../plugins/agent-network-cindy/node/worker.cjs) 调用 loadConfig/saveStatus 并重写 status.json。建议注明不改变 ANet/宿主任务，但刷新本地状态快照。
- **P3 — 打包文档未覆盖新增契约的收录规则。** [pack.py:SKIP_NAMES/skip](../plugins/agent-network-cindy/pack.py) 排除 README.md，却不排除 EGM.md；今后打包会纳入本轮新增的两份插件契约，既有 .cindy 包本轮保持原样。建议在下次发行审查中明确文档是否随包，不能将旧包称为包含本轮文档的重建产物。

## docs/AGENTS.shared.md

No drift detected.

## docs/anet-alignment.md

No drift detected.

## docs/anet.md

No drift detected.

## docs/architecture.md

No drift detected.

## docs/caller-routing.md

No drift detected.

## docs/codex-desktop-handoff.md

No drift detected.

## docs/codex-handoff-probe.md

No drift detected.

## docs/compatibility-contracts.md

No drift detected.

## docs/dispatch-contract.md

No drift detected.

## docs/dsh-session-only.md

- **P2 — 已移除的 connector 候选仍被后文当作保留实现。** 开头已声明 connector patch 于 2026-11-02 移除，但后文仍称 Agenvo 会公布/复查 capability，并在 Recorded offline results 写 “Both patches pass”。[manifest](../patches/dsh-session-only/manifest.json) 只保留 runtime，[目录](../patches/dsh-session-only/) 无 connector patch。历史测试可保留为带日期的历史证据，当前复现范围应明确仅 runtime；[selectSessionModel](../scripts/session-model-selection.mjs) 是调用方校验器，不能补出缺失的 connector 实现。

## docs/harness.md

- **P2 — fleet transport 集成描述过期。** 开头称 fleet.mjs 提供 authenticated call，Structural enforcement 又称 public fleet path 绑定 discovered device/instance；[fleet.mjs:commands/check](../scripts/fleet.mjs) 当前没有 transport/client/harness 接线，check 仅提示退役并退出 2。应改为调用者注入 transport 的库契约，不承诺现有 CLI 已接入。
- **P2 — smoke CLI 与现状冲突。** 末尾仍称 “The smoke CLI retains fresh IDs”，但本页中段已写 CLI 移除；[dsh-check.mjs:dshTurn](../scripts/dsh-check.mjs) 保留的是可调用库，随机 requestId/进程内 journal 属于该函数。建议全文统一库与 CLI 的边界。
- **P2 — 单文件工具输出措辞强于校验。** 文档要求结果 text block “with just one numbered line”，[harness.mjs:matchesToolContract](../scripts/harness.mjs) 只过滤编号行并要求唯一匹配，额外不带行号的文本仍可能通过。若约定只允许严格单行，需后续代码修复；若只限制编号行，需收窄文字。本轮不改判定逻辑。

## docs/landscape.md

No drift detected.

## docs/maintenance.md

- **P2 — doctor 失败通知承诺没有退出码支持。** 每周 guard + doctor 和“不通过时”指引暗示服务缺失会导致通知；[fleet.mjs:doctor](../scripts/fleet.mjs) 捕获 launchctl 失败后仅打印 MISSING，不设置非零状态。[fleet-weekly-check.plist](../templates/launchd/fleet-weekly-check.plist) 仅在命令失败时执行 osascript，因此 guard 成功、服务缺失时不会据此通知。建议后续明确 doctor 的告警/退出契约。

## docs/migration-and-upgrades.md

No drift detected.

## docs/native-thread-selection.md

- **P2 — check-request 的目录校验承诺过宽。** 步骤 1 将 prepare 与 check-request 并列后称“无匹配目录就失败”；[native-thread-selection.mjs:checkNativeThreadRequest](../scripts/native-thread-selection.mjs) 仅核对意图、工具名及 model/thinking，只有 prepareNativeThread 接收并校验 catalog。应明确目录验证由 prepare/宿主承担，不能把 check-request 当完整目录门禁。

## docs/network-and-desktop-acceptance.md

No drift detected.

## docs/new-mac.md

- **P2 — doctor 不再检查 connector 实例。** 步骤 5 要求 doctor 中实例不是 NOT CONFIGURED；[fleet.mjs:doctor](../scripts/fleet.mjs) 只遍历 machine.services 并输出 loaded/MISSING，从不产生该实例状态。实例 context 检查现由 contexts/contextPlan 提供，应替换该验收指引。

## docs/runbook.md

- **P2 — doctor 命令注释范围过宽。** 验收段称“本机 launchd 服务与 connector 状态”，[fleet.mjs:doctor](../scripts/fleet.mjs) 不读取 connector 状态，仅执行 launchctl print。建议收窄到服务加载性，并把 contexts 的职责单列。

## docs/runtime-provenance.md

- **P2 — 保留的 patch 数量与候选指针过期。** 最后一节仍称 runtime patch 与 capability-gated adapter patch 都 retained；[DSH manifest](../patches/dsh-session-only/manifest.json) 只有 runtime，[dsh-session-only.md](./dsh-session-only.md) 已明示 connector patch 移除。建议历史候选事实与当前可复现输入分开写。

## docs/runtime-recovery.md

No drift detected.

## EGM.md

- **P2 — 验证入口接线漂移（已在新契约揭示，未修代码）。** [package.json:scripts.check](../package.json) 指向 `node scripts/fleet.mjs check`，故全局 check 选择它后必以 2 退出，而离线测试实际在 scripts.test。已在尚未生成文档的干净基线上运行 check，结果为退役提示/退出 2。建议后续单独修复验证入口；此次不能为追求绿色而改 package.json。

## scripts/EGM.md

No drift detected.

## plugins/EGM.md

No drift detected.

## plugins/agent-network-cindy/EGM.md

No drift detected.

## plugins/agent-network-cindy/node/EGM.md

No drift detected.
