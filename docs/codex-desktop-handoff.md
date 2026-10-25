# Agenvo → Codex：执行、桌面可见与跨 backend 交接

适用者：DSH→Agenvo→Codex Desktop、GrokBot→Agenvo→Codex Desktop。dot 有原生平台任务工具时仍优先原生通道。这里 blocked 不表示 Codex 整体不可用，见[调用方路由](caller-routing.md)。

## 三种证据

- 同 host 执行：在同一已授权 backend 创建 thread、提交 turn、读回完成结果并继续。无需先释放 writer。宿主通过 discovery/schema 与实际观察固定目标身份；socket 路径不能单独证明官方 Desktop backend。
- 官方 Desktop 可见：受支持 UI 入口中看见相同 thread 的实际会话。loaded、共享 CODEX_HOME、历史回读、deep link 都不能单独证明可见或控制权。
- 跨 backend handoff：只有确实要转移 writer 时才需要原生 ownership 交接证据。**创建前门槛**：没有目标版本/设备组合的受支持交接 canary 就 `blocked_native_writer_release`，不尝试创建再迁移。connector 重启不释放独立 native writer。

目标仍是 Desktop 可见后 agent `turn/start`。获准的同 host 基线允许独立验证执行，不当作完整桌面路线通过，不退回 exec 或用户 Send。跨 backend 仍 experimental。

## 状态与提交

`not_submitted` → `created` →（仅跨 backend：`handoff_verified`）→ `desktop_visible` → `submitted` → 原生终态与业务结果。
独立执行基线可没有 desktop_visible，但其结果必须显式保留 Desktop 未验。ACK/accepted 不是完成。unknown 创建或提交只回读原关联，不重发。取消只作用于有归属证据的本轮，不以 timeout 伪装 cancelled。

`checkCodexHandoff` 仅接受 topology=cross-backend；其他输入返回 handoff_not_applicable，不能用它批准或阻断普通同 host 执行。它是可信输入的离线完整性校验，不认证证据真实性，不替代 dispatch 授权。host router 仅表示能力已注册，任务 JSON 的 boolean 不构成 live 证据。

新机/升级须重新观察身份和行为；迁移服务不等于恢复在途请求。保留 PR57–59 的 unknown 不重发及版本兼容契约。

## 当前证据

见[脱敏同 host 回执](evidence/codex-same-host.json)：2026-10-25 直接 Agenvo 两轮纯文本、同 thread、均 completed、0 工具。不是 DSH 或 GrokBot 发起；Desktop 可见和跨 backend handoff 未验。当前 DSH scope 无 Agenvo 工具，OAuth 接入与后台生命周期均 pending。

[隔离 probe 候选](codex-handoff-probe.md)仅研究跨 backend，不是同 host 基线的必需前置；未部署，不启动它或重启服务来补证据。
