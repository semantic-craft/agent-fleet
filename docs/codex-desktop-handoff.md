# Agenvo → Codex Desktop：执行与 writer 交接

适用者：DSH→Agenvo→Codex Desktop、GrokBot→Agenvo→Codex Desktop；blocked 不表示 Codex 整体不可用。dot 原生平台任务仍按[调用方路由](caller-routing.md)。操作步骤以[派活技能](../skills/delegate-via-agenvo/SKILL.md#codex-desktop)为正本。

## 独立执行与 Desktop 交接门槛

独立同 host 执行基线只要求已授权的同一 backend 具备观察、创建、提交和回读能力，不以 `thread/unsubscribe` 或 watcher 交接检查作为创建前置；Desktop 可见性和交互控制保持未验。固定 discovery 的 deviceId/instanceId/backend、cwd 和模型后，才在该范围建线程。

交还 Desktop 交互控制前，目标 connector 必须暴露 `thread/unsubscribe`，且 `watchEvents` 不再对未明确订阅的 loaded 线程自动 resume；缺失时仅该交接项报 `blocked_native_writer_release`，不否定独立执行能力。仅跨 backend 转移需要另行证明 writer 交接。新机和升级重新验收，不能把源码合并当作当前部署能力。

共享 app-server 与 Desktop 自带 app-server 即使在同 host、共用 CODEX_HOME，也属于不同连接。共享端保持 loaded/subscribed 时，Desktop 可出现 `localConversation.writerConflict`。仅共享路径、loaded、历史回读或 deep link 不能单独证明 Desktop 可见或 writer 已交还。

`thread/start` → Desktop 可见性检查 → `turn/start` 真正提交 → 读本次 turn 终态和答案 → **持有订阅的 connector 连接**调用 `thread/unsubscribe {threadId}` → 重新打开 Desktop 并截图。`not_submitted` → `created` → `desktop_visible` → `submitted` → 终态 → 退订 → 无横幅验收；任一 UI 步骤受限，执行基线可独立验证，但 Desktop 项仍未验。

退订原生结果为 `unsubscribed`、`notSubscribed` 或 `notLoaded`；connector 对成功响应移除 resumeTargets，重连不再自动 resume。最后订阅者离开后原生服务经过宽限期 unload，并可发 `thread/closed`。`notSubscribed` 不证明其他连接没有订阅。若仍有横幅，检查其他订阅者及旧 watcher；不要反复发送输入。

connector `kickstart` 不等于放锁，也不证明 native writer 已释放。常规流程不需要重启；若特定 brief 要求重启 connector，核对无其他在途调用，重连后显式 resume 本线程取得订阅，再由该连接退订。不要以重启共享 native 服务作为正式交接手段。

## 回程与验收

先读本次 turnId 的 completed/failed/interrupted 和实际回答，再退订。退订后 notifications 可能停止，后续回读走 `thread/read`／`thread/turns/list` 持久历史；读取历史不恢复订阅。需要续写时才显式 resume。accepted 不是完成；unknown 只回读原 thread/turn，不重发。

验收同时要求：agent 真实 `turn/start`、Desktop 左栏项目下出现同一 thread、主区无 “This is open in another app”／“Fork chat”／“open in new chat”。截图须包含两处；无截图不能报告桌面通过。cwd 必须匹配 Desktop 的 Projects 项目，projectId 可空。模型/provider、版本、threadId、turnId、退订 status 和截图路径分别记录。

`checkCodexHandoff` 仍是其他 topology=cross-backend 实验的离线完整性校验，不认证实际证据，也不替代本流程。旧[隔离 probe](codex-handoff-probe.md)不是这条已知原生退订路径的前置。

## 配置与消费者

overlay 是实例 context 正本。安装修复后的 connector 后，在目标机运行 `node scripts/fleet.mjs contexts --apply`，仅重启变更的 connector，随后 `contexts` 和 Agenvo discovery 回读确认；context 不改变 scope，无需重新批准。回滚 context 时恢复私有备份再执行相同命令。

全局 Grok／Magpie 消费方对齐本仓派活技能的第 5 步；不把消费者副本当成正本。分别从 DSH、GrokBot 发起的端到端验收独立记录，直接 Agenvo 烟雾不代替它们。
