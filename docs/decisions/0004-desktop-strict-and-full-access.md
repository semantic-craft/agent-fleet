# ADR-0004：桌面可见优先、默认 full access

状态：已采纳（2026-10-19）

## 决定
- 派给 DSH/ZCode 的任务固定 `runner: "app"`，并标记 `source: "agenvo"`。桌面 app 不在时，接收端返回 503 或让 job 明确失败，不静默退回 headless 或 CLI。
- 会话标题带来源标记（DSH 显示 `[Agenvo]`，ZCode 显示 `[来源: agenvo]`），完成回报只落本地，不回流到 Grok/Yobiru。
- 执行权限默认 full access：DSH `danger-full-access`、ZCode `yolo`、Codex `danger-full-access` + `approvalPolicy: never`。
- 桌面 app 在登录后自启动（launchd 模板 `desktop-autostart`），这样严格模式下的失败只剩"电脑睡着了"一种。

## 理由
用户要在 app 里看到并接手每一个任务；静默退回 headless 会让结果"看不见"。安全边界放在谁能访问 relay，而不是每一步的执行审批。

## 代价
同一 macOS 用户下的 full access 不是安全隔离；需要隔离的任务改走 `hub_run` 的受限策略。
