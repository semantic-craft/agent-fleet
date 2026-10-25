# 可迁移恢复与持续升级

本入口把服务重建、在途任务恢复和升级验收分开。离线演练不等于新物理机器验收，也不安装运行时、重新授权或切换生产。现有 `new-mac.md` / `runbook.md` 仍是安装事实源。

最新逐组件来源取证与安装输入见 [runtime-provenance](runtime-provenance.md)。历史表中的 unknown 以该页新增证据细化；当前包与候选不能混同。

## 来源与兼容性分层

| 层 | 可复用来源与已知事实 | 未知 / 新机必须确认 |
| --- | --- | --- |
| 控制面 | `Xuanwo/agenvo`；自有 fork `semantic-craft/agenvo` 的 `xw/dispatch-connectors`；上游研究基线 `a89415e557c415faf5626354f8e87233eb69d877` | 研究 SHA 不是现役部署锁；从已批准 release.json / relay 发布记录核实实际 SHA，不能用浮动分支代替 |
| DSH adapter | 本仓 `adapters/dsh`，由 `release dsh --sha <reviewed SHA>` 发布；本轮基础代码来自 agent-fleet `baf66e015fb89ab4876561910554444c57f7b075` | 安装后 Host 内部 Typert/workspaceRegistry 契约须再验；包的宽版本范围不代表兼容已验 |
| Agent runtime | 官方 `deepseek-ai/deepseek-harness`；2026-10-24 线路证据 DSH 0.2.1-rc.3、安装依赖 pi-ai 1.1.0 | rc.3 tag `1f81c047fc48fa190a683e469daa699857e9afe8` 只供源码比较，不宣称等于已装二进制。Codex/Herdr 精确可重建版本本表 unknown，需从各官方发行及实际安装取证 |
| Provider gateway | Magpie；模型路线已通过本机独立部署运行 | 实际已部署源码/构建/签名关系 unknown，必须补其正式发行来源、包哈希、补丁来源和版本；不能据历史测试源码冒认二进制 |
| 模型 | `scripts/route-profiles.mjs`：provider、model、effort、preset 分开记录 | 模型别名同名也可能换后端，版本字符串不能证明行为不变 |
| Harness / 存储 | `bounded-dsh-v4`、FileJournal format 1、稳定 native request ID；本仓依赖由 package-lock.json 固定 | 新契约不能直接读取旧在途保留项；保留原 observer，不自动迁移/重发 |

历史已验组合与升级候选分别保存，不能把 unknown 填成 latest 或“兼容”。当前证据仅覆盖：Argon medium PTC 单次只读工具和 standard 标记；MiniMax low standard 单文件 read；Sol low/high PTC 标记。Sol standard low/high 为已知失败，high PTC 工具失败。MiniMax 桌面路线已取消；CLI 与模型仍可保留，不需要桌面 app 作为此路线依赖。

## 空白机器：非秘密准备清单

1. 从本仓配置样例生成新的私有 overlay，替换主机名、projectsRoot、服务标签、实例 ID、端点，不复制旧机器的身份。`FLEET_BIN` 配置 CLI 安装位置；模板同时包含 Apple Silicon 和 Intel 常见 PATH，仍须 `command -v` 核验。安装 Node >=24.13、Git；持久 journal 还要求 Python 3 标准库及本地 POSIX 文件系统。
2. 从各官方发行来源安装所需运行时并由用户重新登录。沿用原 relay 时不重新生成管理员密钥；在原管理端核验新设备指纹、批准配对，再由用户完成客户端 OAuth。新 relay 才按 runbook §2 创建并安全保存管理员密钥。仓库不保存或复制登录令牌、浏览器资料、配对凭据。
3. Magpie 是这些模型路线的额外前提：先确认官方安装来源、实际构建和用户重新登录，再按其正式配置入口连接 DSH provider。逐项核对 OpenAI Responses、Anthropic、其他适用协议和模型目录；不能只复制模型名。当前来源关系 unknown 时暂停该模型路线的可重建声明，不下载猜测来源或永久覆盖签名应用。
4. 安装已审固定 SHA 的 Agenvo fork 与 DSH adapter，按 runbook 配对、生成 launchd、检查权限与在线状态。桌面插件需要用户安全关闭/重开 DSH；先确认无活动任务。不得因安装失败回落 headless。
5. 用户项目、CLI 身份和原生会话数据属于独立备份范围。只拿仓库能重建代码和新服务，不能还原旧会话。设备/实例/session IDs 是迁移映射或历史证据，不可当新机模板值。
6. 使用下面的证据门槛再准入生产；新授权、软件安装和现役部署变更仍需适用批准。

## DSH 模型选择：全局副作用保护

消除全局副作用的源码候选与 capability-gated caller 已独立提供，见 [DSH session-only](dsh-session-only.md)；尚未部署，不影响下述旧接口保护。新接口仅 applied，不提供 crash/restart 持久化保证。

`session/selectModel` 可能持久化全局默认。它不是隔离会话设置；不允许技能直接承诺“全局默认保持原样”。优先使用已选好且核验匹配的独占会话，不为复用 profile 自动切模型、effort 或 preset。

确需选择时：

1. 明确用户授权这次暂时写默认及恢复；协调所有用户/自动化选择入口，并暂停新会话创建。仅检查当前空闲或拿本进程锁不足以证明独占。无法协调就阻断，不抢写。
2. 从正式只读接口记录原默认、目标会话选择和目标值（provider/model/effort）；核对当前 schema。`planModelSelection` 给出纯前置计划，不执行 mutation。计划不能当权限或全局锁。
3. 在协调窗口内显式选择一次；unknown 时只读原会话及默认，绝不再次发送“试试看”。核实选中目标后，使用已确认不会改变目标会话的正式恢复方式恢复原默认，例如独立恢复锚点会话。不能在目标会话直接选回原模型后声称它仍选中目标。
4. 恢复前再次读取默认：仅等于本次临时值时才允许操作员恢复；等于原值也不能仅凭轮询判定已恢复（此前选择可能尚有异步默认写）；出现第三方值立即停止，不覆盖并行修改。`reviewModelRestoration` 对漂移/未知/会话不匹配返回 blocked，始终 `mutationAllowed:false`。DSH 缺少 CAS，读取和写入之间仍存在竞争，故协调窗口必须保持到最后回读。
5. adapter 必须提供正式的默认持久化队列结清/恢复 barrier，证明之前的临时默认写不会迟到覆盖。`persistenceBarrier` 必须绑定本次计划的 operationId 和 sessionId，只接受该实际取证结果，不是由调用方随手填 true 的许可。当前 DSH 未在此实现可验证 barrier：因此自动恢复验收阻断为 `default_persistence_unproven`，不能用等待若干秒或多次读到原值替代证明。只有 barrier 已证实、同时回读原默认已恢复和目标会话仍是目标值，才释放协调窗口并提交业务。恢复失败时停止新派活，报告原值/现值的非秘密差异及恢复未完成；不改全局 provider、权限或预算兜底。

纯接口示例：

```js
import { planModelSelection, reviewModelRestoration } from '../scripts/model-selection.mjs';
const plan = planModelSelection({ before, desired, sessionId, coordination });
// coordination records actual external coordination, not an automatic permission grant.
// After an authorized operator action, obtain fresh native observations:
const check = reviewModelRestoration(plan, {
  currentDefault, selectedSession: { sessionId, selection },
  selectionOutcome: 'confirmed', coordination, persistenceBarrier, // adapter-proven; unavailable => blocked
});
// blocked => no write; operator_action_required => coordinate and restore, then reread.
```

## 服务迁移不等于在途恢复

新服务可有新身份；旧任务恢复必须保留原生身份和关联证据。FileJournal 故意不保存正文，因此调用方必须自行保管原 TaskSpec/context、原 native session/request ID、任务/context 指纹、原 journal、policy/schema 版本和目标 runtime 身份。敏感正文仍留其业务存储，不能写到此仓库或通用日志。

只有能核对同一原生历史的目标才可继续 observe。新机重新配对产生新 deviceId 时必须显式证明目标 native runtime/history 的连续性，不能仅按 session 同名放行。原机器失联、历史缺失或未知准入时保持 blocked/unknown；不得创建新 ID、删除 journal、换目录或恢复旧 journal 副本来重跑写任务。取消隔离仍有效。完成任务与仅保留旧回执可独立迁移；恢复旧 in-flight run 不是安装步骤。

## 升级与回滚门槛

1. 登记候选的 runtime / adapter / provider / model / policy / schema 身份，与已验记录比较。未知来源、未审变更、缺能力或不兼容存储先阻断；候选不覆盖已验快照。
2. 离线跑契约回归和隔离恢复演练。不能表达的能力明确 unsupported；各 runtime/provider 在 adapter 声明，不推断所有 cancel、event replay、tool schema 都相同。
3. 经授权在隔离新会话做 canary：模型实际 header/effort/preset、精确输出、受限只读工具参数/结果、终态、权限与默认恢复全部核实。marker 只能晋升 marker 能力。模型别名未变也需行为 canary，定期或疑似后端变化时失效重验。
4. 新机重登/重启后按 doctor → 原生读 → 真实云端派活/回读 → 有界工具执行分层验收。主动推送单列：重建订阅、记录 ID/有效期、等待实际完成事件唤醒并回读；resync 不是完成事件，不能复用历史专用订阅做试验。
5. 只有所需能力的真实证据齐备才批准该组合；不自动 fallback 到别的模型或权限。小范围切换且保留原签名包/发布 SHA/配置快照；rollback 按 runbook，比较当前配置防止覆盖外部变更，回退后重新验证业务。
6. release 的范围回滚不是事务恢复：发布需串行、检查活动任务，缺跨进程锁或中途失败时按 pending 记录人工核验，不重复重启。源版本回退不保证用户数据格式可回退；升级前确认官方数据迁移/恢复支持，无支持时暂停。

证据等级：`offline`（合成契约）→ `local-live`（原生运行）→ `cloud-live`（真实授权入口）→ `new-host-live`（新机安装/重新授权/重启后）。等级不可替代；存储进程重启测试不等于机器掉电/硬件故障证明。部署审批与证据齐备是两个条件。

离线入口：[契约API与演练](compatibility-contracts.md)，`node scripts/recovery-drill.mjs`。真实 FileJournal 进程崩溃/恢复与发布回滚算法回归分别用 `node --test test/file-journal.test.mjs test/release-lib.test.mjs test/release-scope.test.mjs`；它们仍是模拟原生端/文件系统 fixture，不会对现机运行时执行回滚。
