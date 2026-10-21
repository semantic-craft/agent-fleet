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

### Codex 派活用 exec，进程退出后桌面接手

**在 Herdr pane 里运行官方 `codex exec`，以 `codex exec resume` 追问；进程退出后通过 `codex://threads/<thread_id>` 接手。**

- 起因与后果：常驻 `codex app-server` 共用 home 能读取历史，但远程 turn 完成后仍持有 writer，桌面版显示 “This is open in another app”，只能 Fork。
- 修复与证据：调度方于 2026-10-21 实测 `codex exec --json -o` 完成并退出，`codex exec resume` 可续写，桌面线程链接打开后显示历史与正常输入框，无占用横幅。
- 正确做法：派活走 Herdr，立刻记录 JSONL 中 `thread.started` 的 `thread_id` 并告知用户。Codex connector 只列出线程与读取历史；列表查询包含 `sourceKinds: ["exec"]`、`modelProviders: []`。exec 线程不一定出现在侧栏，因此必须提供线程链接。完整流程见[派活技能](skills/delegate-via-agenvo/SKILL.md)。
- 复发记录：Seen 2x (2026-10-21)。

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

### 私有仓库的轻量必过检查来自常开 Mac mini 的 runner

**私有仓库 PR 上的轻量必过检查由常开 Mac mini 上的自托管 runner 跑，Swift/iOS 和签名在开发笔记本上跑；仓库还没接 runner 时，在本机运行 `scripts/check`，把结果写进 PR 后再合并。**

- 起因与后果：一个 session 一直"等待 CI"，任务停在合并前。
- 修复与证据：改为本机检查后合并。
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

### grok.com 拦截 Worker 获取它的 CIMD 文档

**Cloudflare Worker 上的 relay 不要为 Grok 打开 CIMD；让它走动态注册。**

- 起因与后果：打开 `clientIdMetadataDocumentEnabled` 后，Grok 的授权请求返回 400。临时 Worker 探测 `https://grok.com/oauth/mcp-client.json` 得到的是 403 challenge 页。
- 修复与证据：撤回改动，fork 保持上游原样；记入 [ADR-0006](docs/decisions/0006-single-remote-path.md)。
- 复发记录：Seen 1x (2026-10-19)。

### 链接进 DSH 桌面 profile 的插件，改工作树就可能直接进生产

**改 DSH 插件（`adapters/dsh`）等 `link:` 插件时，在单独的 worktree 里开发；被链接的 checkout 只在验证后切到 main。**

- 起因与后果：开发 native bridge 时直接编辑了被主力 Mac 桌面版链接的 checkout。另一个 agent 恰好给桌面版装插件，触发 profile 重载，桌面 Host 当场加载了半成品，其中一版在 listen 期间改了进程 umask（随后修掉）。没有发现损坏的文件或目录，但这是生产进程。
- 修复与证据：插件去掉 umask 写法并加回归测试；插件日志显示 04:09:54Z、04:12:06Z 两次重载。
- 正确做法：桌面插件的开发与隔离验证用 `DSH_HOME`/`HOME` 临时目录下的 `dsh web`；要上生产时再让链接的 checkout 前进到 main，并在桌面空闲时重载或重启。
- 复发记录：Seen 1x (2026-10-19)。

### DSH 单条消息就可能超过 64 KiB

**读 DSH 输出时 `session/follow` 用 `maxMessages: 1`，判断 turn/end 只看最后一条；遇到 `result_too_large` 等下一次轮询，不要按范围往回翻。**

- 起因与后果：`check --dispatch` 用 `maxMessages: 6` 读快照，真实会话（含读文件的 tool 结果）超过 64 KiB，返回的错误被当成"还没完成"，轮次早已 completed，检查却等满 10 分钟报失败；实例 context 里写的"4–8"也会让助手踩同一个坑。
- 修复与证据：`scripts/fleet.mjs` 的 `dshTurn`；两台 DSH 的 `check --dispatch` 均 turn/end completed；overlay 的 DSH context 改为 1–2 并写明超限时怎么办。
- 复发记录：Seen 1x (2026-10-19)。

### Herdr 的 runtime_unavailable 多半是 session 名错

**Herdr 方法的 `session` 参数必须是 `session.list` 返回的名字（本机只有 `default`）；传错名字也报 `runtime_unavailable`。**

- 起因与后果：验收时 ChatGPT 调 Herdr 得到 `runtime_unavailable`，报告据此判断"发现正常但执行不可用"；经 relay 用 `session: "default"` 实测 workspace.create、agent.start、agent.prompt 全部 accepted，传 `session: "agenvo"` 复现同一错误。
- 修复与证据：overlay 的 Herdr context 写明 session 取值和完整流程。
- 复发记录：Seen 1x (2026-10-19)。

### Claude Code 在 Herdr 用无头模式并显式选模型；agy 首次确认信任

**Claude Code 已登录，在 Herdr pane 里按下方完整命令新建与续接，每轮显式传 `--model deepseek/deepseek-flash`。agy 首次进入项目要处理信任提示。**

- 起因与后果：Claude Code 默认使用设置中的 antigravity 模型，会拒答 Claude Code 系统提示并返回 `is_error: true`，继续等待无用；agy 遇到信任提示时 `agent.prompt` 返回 `agent_blocked`。
- 修复与证据：调度方于 2026-10-21 19:26–19:27 实测 Claude Code 已登录，显式选 `deepseek/deepseek-flash` 后成功，`--resume <session_id>` 也能续接同一会话。调度方于 2026-10-21 19:57 在目标开发机实测，加 `--dangerously-skip-permissions` 的无头命令返回 `is_error: false`。调度方于 2026-10-21 21:11–21:16 实测 `--model deepseek/deepseek-flash` 新建与 `--resume` 均成功，请求经 `~/.claude/settings.json` 的 `ANTHROPIC_BASE_URL` 进入本机 magpie；默认模型 `antigravity/gemini-4-argon[1m]` 在 20:19–20:23 新建与续写均被拒答。agy 经 relay 用 `pane.run` 发空命令回车确认信任后可回答。
- 正确做法：用 `claude -p --dangerously-skip-permissions "<任务>" --model deepseek/deepseek-flash --output-format json > /tmp/<任务名>.json; echo __EXIT=$?`，读取 JSON 的 `result`、`session_id` 与 `is_error`；追问用 `claude -p --dangerously-skip-permissions --resume <session_id> "<下一句>" --model deepseek/deepseek-flash --output-format json`。不用交互式 `agent.start kind claude`。输入已接受不等于轮次完成。
- 复发记录：Seen 1x (2026-10-19)；模型与登录状态于 2026-10-21 更新。

### 从 DSH 的 shell 启动 Electron app 要去掉 ELECTRON_RUN_AS_NODE

**DSH 本身是 Electron，它的 agent shell 继承了 `ELECTRON_RUN_AS_NODE=1`；用 `env -u ELECTRON_RUN_AS_NODE open -a <App>` 启动 ZCode 等 Electron app。**

- 起因与后果：DSH 执行验收时 `open -a ZCode` 让 ZCode 以 Node 模式启动后立刻退出，看起来像 app 坏了。登录 shell 和 launchd 里没有这个变量。
- 复发记录：Seen 1x (2026-10-19)。

### `plutil -replace ProgramArguments.1` 插入而不是覆盖

**改 plist 数组里的一项时，整体替换数组：`plutil -replace ProgramArguments -json '[...]'`。**

- 起因与后果：第一次 `release zcode` 用 `-replace ProgramArguments.1 -string <新路径>`，结果数组变成 `[node, 新, 旧]`，接收端多带了一个旧路径参数启动（碰巧无害）。
- 修复与证据：`scripts/release.mjs` 改为整体替换数组；重跑后 `plutil -extract ProgramArguments` 只有两项。
- 正确做法：不要用带下标的 `-replace` 改数组元素。
- 复发记录：Seen 1x (2026-10-19)。

### 常驻服务不要从 git checkout 运行

**launchd 指向 `~/.local/share/agent-fleet/releases/<组件>/<sha>/`，由 `fleet.mjs release` 切换。**

- 起因与后果：4 个 connector 原来直接跑 `~/Projects/agenvo/apps/*/dist/cli.js`，在 checkout 里切分支、合并上游或重新构建都会在下次重启时改变线上行为。
- 修复与证据：`scripts/release.mjs`；2026-10-19 切换后 `fleet.mjs check --dispatch` 全部 OK。
- 正确做法：`agenvo-* service install` 之后再跑一次 `release agenvo`。
- 复发记录：Seen 1x (2026-10-19)。

### 发布脚本要在无终端（无 TTY）下跑通再合

**`fleet.mjs release` 这类会被 agent 或 launchd 调用的脚本，合并前用 `</dev/null` 在无 TTY 的环境实跑一次。**

- 起因与后果：#6（cccaff6）合并后在主力 Mac 首次 `release dsh`，pnpm 想重建 `node_modules` 时没有 TTY 直接中止，DSH 桥接随之断开，只能手工用同样参数重装。
- 修复与证据：#7（46335b3）给 Host 的 pnpm 加 `CI=true --no-frozen-lockfile`，运行中的 DSH 不会加载新插件时直接说明、不再空等 60 秒；`npm test` 28/28。修正后的命令尚未在第二台 Mac 跑过（#7 自述）。
- 正确做法：发布、安装类脚本的验收至少有一次无 TTY 实跑（`node scripts/fleet.mjs release <组件> </dev/null`），并在 PR 里写明在哪台机器跑过。
- 复发记录：Seen 1x (2026-10-19)。
