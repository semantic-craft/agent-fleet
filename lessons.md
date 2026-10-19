# 项目教训

实现、评审或验收前先读相关条目；用户纠正、返工或漏验暴露了可复用的教训后补记。格式见 xw-tooling 的 lessons 模板。

### 生产环境的 OAuth 批准返回 HTML 页，不是 303

**脚本化 OAuth 时，同时支持 Location 头和页面里回到 client 的链接。**

- 起因与后果：上游 e2e 夹具在本地拿到 303，生产 Worker 返回的是 200 加一个 "Returning to client" 页面，按 303 写的脚本在 `new URL(null)` 处崩溃。
- 修复与证据：`scripts/lib.mjs` 的 `approvalRedirect`，回归测试见 `test/lib.test.mjs`；`fleet.mjs check` 已对生产 relay 验证通过。
- 正确做法：用 `approvalRedirect` 解析，不要假设状态码。
- 复发记录：Seen 1x (2026-10-19)。

### 改实例 scope 要重新批准，只改 context 不用

**改 `source`、endpoint、keyFile 后，在 relay 重新批准该实例；只改 `context` 时重启 connector 就够了。**

- 起因与后果：给 dispatch 实例加上 `source: agenvo` 并重启后，两个实例在 relay 上变成未批准，助手看不到它们。
- 修复与证据：`admin state` 取指纹，再 `admin approve-instance`；之后经 relay 验证通过。
- 正确做法：见 [runbook 日常操作](docs/runbook.md#日常操作)。
- 复发记录：Seen 1x (2026-10-19)。

### Codex Desktop 内置的 app-server 只走 stdio

**要远程管理 Codex，就另起一个 `codex app-server --listen unix://` 常驻，与 Desktop 共用 `~/.codex`。**

- 起因与后果：Desktop 不暴露 socket，connector 无从附着。
- 修复与证据：launchd 模板 `templates/launchd/codex-app-server.plist`；新建的 thread 会出现在 Desktop 共用的 state DB 里。
- 正确做法：使用该模板，connector 的 endpoint 指向 `~/.codex/app-server-control/app-server-control.sock`。
- 复发记录：Seen 1x (2026-10-19)。

### Herdr 常驻要用 `herdr server`

**launchd 里运行 `herdr server`（headless），不要运行 `herdr`（TUI，需要终端）。**

- 起因与后果：Herdr 平时只在打开 TUI 时运行，connector 附着不上。
- 修复与证据：`templates/launchd/herdr-server.plist`；经 relay 调 `session.list` 正常。
- 复发记录：Seen 1x (2026-10-19)。

### Workers 自定义域名要求 zone 在 Cloudflare

**为 relay 选的域名，其 DNS 必须托管在同一个 Cloudflare 账号里；`workers.dev` 子域是账号名，不是品牌。**

- 起因与后果：原有个人域名的 DNS 在别家托管，绑不上；workers.dev 地址暴露了账号名，用户不接受。
- 修复与证据：通过 Cloudflare Registrar 新注册一个域名，以 custom domain 绑定，并关闭 workers_dev。
- 复发记录：Seen 1x (2026-10-19)。

### 私有仓库不跑托管 CI，不要等 checks

**私有仓库的 PR 上 checks 永远为空；在本机运行 `scripts/check`，把结果写进 PR 后再合并。**

- 起因与后果：一个 session 一直"等待 CI"，任务停在合并前。
- 修复与证据：改为本机检查后合并（agent-interop-private#76）。
- 正确做法：见 AGENTS.md 的"验收与交付"。
- 复发记录：Seen 1x (2026-10-19)。

### fork 带着上游 workflow 时，HTTPS 推送需要 workflow 权限

**推送上游 fork 到自己的私有仓库时用 SSH，加 `--no-follow-tags`，并先关闭该仓库的 Actions。**

- 起因与后果：gh 的 OAuth token 缺少 `workflow` 权限，推送被拒；本机的 `push.followTags` 还把上游 tag 也带了出去。
- 复发记录：Seen 1x (2026-10-19)。

### 严格桌面模式依赖 app 在运行

**启用 `runner: "app"` 的同时，配置桌面 app 登录自启动。**

- 起因与后果：app 没开时，派活会立刻得到 503。这是预期行为，但手机端会频繁失败。
- 修复与证据：`templates/launchd/desktop-agents-autostart.plist`。
- 复发记录：待核实。
