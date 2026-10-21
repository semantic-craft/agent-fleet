# ADR-0004：桌面可见优先、默认 full access

状态：已采纳（2026-10-19）

## 决定
- DSH 经 `agenvo-dsh` 直接在桌面版 Host 里建会话、提交输入，天然在桌面可见；桌面版不在时 socket 不存在，实例离线。派给 ZCode 的任务固定 `runner: "app"`，并标记 `source: "agenvo"`。桌面 app 不在时，接收端返回 503 或让 job 明确失败，不静默退回 headless 或 CLI。
- ZCode 会话标题带来源标记 `[来源: agenvo]`，完成回报只落本地。DSH 原生会话不加标记；需要时调用者自己 `session/rename`。
- 执行权限默认 full access：DSH `danger-full-access`（`session/create` 不带 `permission` 时由 connector 用原生 `/permission` 设定）、ZCode `yolo`、Codex `danger-full-access` + `approvalPolicy: never`。
- 桌面 app 在登录后自启动（launchd 模板 `desktop-autostart`），这样严格模式下的失败只剩"电脑睡着了"一种。

## 理由
用户要在 app 里看到并接手每一个任务；静默退回 headless 会让结果"看不见"。安全边界放在谁能访问 relay，而不是每一步的执行审批。

## 代价
同一 macOS 用户下的 full access 不是安全隔离；暂无完整的隔离通路。只读任务（例如独立审稿）在 DSH 上用 `session/create {request:{workspaceId}, permission:"read-only"}` 建会话：`session/create` 成功返回时会话已是 read-only；之后的 `session/prompt` 不带 `permission` 时保持会话当前权限。`session/prompt` 带 `permission` 时先切到指定预设再提交。
