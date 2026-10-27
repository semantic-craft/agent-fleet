# 项目教训

### 显式模型选择必须落在所请求的会话层级

- 证据：2026-10-25 创建原生独立会话时漏传 model/thinking，默认 Astra/medium；提示词里的 DeepSeek/high 子 agent 不能改变输入框主会话选择。此前本文件已记录 Claude Code 不显式传模型落到默认值的失败，故“显式模型丢失”是跨入口第二个案例；独立会话/子 agent 混淆只确认本次一次。
- 修复：`scripts/native-thread-selection.mjs` 从唯一 selection 生成原生请求并核对实际轮次；技能与 caller-routing 链到同一契约。没有复制厂商列表、改变全局默认或另建调度服务。
- 验证点：`node --test test/native-thread-selection.test.mjs` 重放漏参、spawn 替代和错误回读；`npm test` 包含此回归。实际两独立会话记录所选模型/high并执行命令，UI与底层provider另验。
- 边界：内置工具的调用拦截点不属于本仓；绕过检查器不能强制阻止。自然语言意图与现有写者停止确认仍需协调者判断。

实现、评审或验收前先读相关条目；用户纠正、返工或漏验暴露了可复用的教训后补记。格式见 xw-tooling 的 lessons 模板。

### Cloudflare 交互走 `cf` CLI

**与 Cloudflare 交互时用 `cf` CLI；项目若仍有 Wrangler 配置文件（wrangler.jsonc／wrangler.toml）且无 cloudflare.config.ts，则该项目继续用 Wrangler；有 Wrangler 配置时不要对该项目跑 cf dev／build／deploy，先 cf migrate。**

- 起因与后果：用户 2026-10-25 定正道（亚里士多德派工）；舰队 agenvo fork 仍有 wrangler.jsonc、无 cloudflare.config.ts，部署步骤继续 Wrangler。
- 修复与证据：正本写入 `docs/runbook.md`「Cloudflare CLI」；全局技能 `cloudflare-cf-cli`（grokbot-skills）。
- 正确做法：有 `cloudflare.config.ts` → `cf`；仅 Wrangler 配置 → 继续 Wrangler，迁时先 `cf migrate`；账号／DNS／Zones／隧道等非仓库构建 → `cf`。
- 验证点：`rg -n 'Cloudflare CLI|cf migrate' docs/runbook.md` 有本节；部署段仍含 `npx wrangler`。
- 复发记录：Seen 1x (2026-10-25)。

### 生产环境的 OAuth 批准返回 HTML 页，不是 303

**脚本化 OAuth 时，同时支持 Location 头和页面里回到 client 的链接。**

- 起因与后果：上游 e2e 夹具在本地拿到 303，生产 Worker 返回的是 200 加一个 "Returning to client" 页面，按 303 写的脚本在 `new URL(null)` 处崩溃。
- 修复与证据：`scripts/lib.mjs` 的 `approvalRedirect`，回归测试见 `test/lib.test.mjs`；`fleet.mjs check` 已对生产 relay 验证通过。
- 正确做法：用 `approvalRedirect` 解析，不要假设状态码。
- 验证点：`node --test test/lib.test.mjs` 通过，其中有 `approvalRedirect(null, html, cb)` 从页面链接取回调的断言；`node scripts/fleet.mjs check` 对生产 relay 通过。
- 复发记录：Seen 1x (2026-10-19)。

### 改实例 scope 要重新批准，只改 context 不用

**改 `source`、endpoint、keyFile 后，在 relay 重新批准该实例；只改 `context` 时重启 connector 就够了。**

- 起因与后果：给 dispatch 实例加上 `source: agenvo` 并重启后，两个实例在 relay 上变成未批准，助手看不到它们。
- 修复与证据：`admin state` 取指纹，再 `admin approve-instance`；之后经 relay 验证通过。
- 正确做法：见 [runbook 日常操作](docs/runbook.md#日常操作)。
- 验证点：
- 复发记录：Seen 1x (2026-10-19)。

### Codex 派活走 Desktop 空会话（仅外部 Agenvo 入口）

dot 原生平台任务通道不受本节阻断；DSH／GrokBot→Agenvo→Codex Desktop 是需保留的外部目标，不能互相替代，见[调用方路由](docs/caller-routing.md)。

**agenvo → Codex 实例 `thread/start` 建空桌面会话 → 释放写锁或已证实原生交接 → 打开 `codex://threads/<thread_id>?prompt=…` 确认 Desktop 可见 → 经 agenvo 对同一线程 `turn/start` 提交 → turn 终态后同一 connector `thread/unsubscribe` 交还 Desktop。**

- 起因与后果：用户要任务出现在 Codex Desktop，并由 agent 经 agenvo 提交开跑；会话仍须 Desktop 可见，且交还后左栏可操作（2026-10-25 10:18／16:58；thc-20261026）。
- 修复与证据：Desktop 正道正文在 [xw-stack 派活技能](https://github.com/semantic-craft/xw-stack/blob/main/skills/delegate-via-agenvo/SKILL.md#codex-desktop) 与本仓[交接契约](docs/codex-desktop-handoff.md)；合入后立刻 Skill 重载验收（全局 workflows 与仓正本一致）。派活正道只留一条。
**Desktop 交互控制交还门槛**：独立同 host 执行基线不以退订能力为创建前置，Desktop 项保持未验；仅当完成交还时，确认 connector 暴露 `thread/unsubscribe` 且 watcher 不抢其他 loaded 线程，否则 `blocked_native_writer_release`。同 host 共享 app-server 与 Desktop 自带 backend 也要交还 writer；本次 turn 终态后由同一 connector 退订，再截图核实项目下可见且无横幅。connector 重启不释放独立 native writer。目标仍为 Desktop 可见后 agent `turn/start`，见[交接契约](docs/codex-desktop-handoff.md)。
- 验证点：`test/codex-handoff-docs.test.mjs` 检查 Desktop 正道含 `thread/unsubscribe` 与可见验收句；`test/codex-handoff.test.mjs` 检查目标/版本/线程关联、证据缺失与 unknown 不重发；离线文档回归不证明实际 writer 已释放。
- 复发记录：Seen 3x（2026-10-25 保留 agent 提交目标；thc-20261026 写清 unsubscribe 交还与 Skill 重载）。

### Herdr 常驻要用 `herdr server`

**launchd 里运行 `herdr server`（headless），不要运行 `herdr`（TUI，需要终端）。**

- 起因与后果：Herdr 平时只在打开 TUI 时运行，connector 附着不上。
- 修复与证据：`templates/launchd/herdr-server.plist`；经 relay 调 `session.list` 正常。
- 验证点：`rg -n '<string>server</string>' templates/launchd/herdr-server.plist` 有结果；目标 Mac 上 `launchctl print gui/$(id -u)/<前缀>.herdr-server` 在运行，经 relay 调 `session.list` 返回正常。
- 复发记录：Seen 1x (2026-10-19)。

### Workers 自定义域名要求 zone 在 Cloudflare

**为 relay 选的域名，其 DNS 必须托管在同一个 Cloudflare 账号里；`workers.dev` 子域是账号名，不是品牌。**

- 起因与后果：原有个人域名的 DNS 在别家托管，绑不上；workers.dev 地址暴露了账号名，用户不接受。
- 修复与证据：通过 Cloudflare Registrar 新注册一个域名，以 custom domain 绑定，并关闭 workers_dev。
- 验证点：
- 复发记录：Seen 1x (2026-10-19)。

### 私有仓库的轻量必过检查来自常开 Mac mini 的 runner

**私有仓库 PR 上的轻量必过检查由常开 Mac mini 上的自托管 runner 跑，Swift/iOS 和签名在开发笔记本上跑；仓库还没接 runner 时，在本机运行 `scripts/check`，把结果写进 PR 后再合并。**

- 起因与后果：一个 session 一直"等待 CI"，任务停在合并前。
- 修复与证据：改为本机检查后合并。
- 正确做法：见 AGENTS.md 的"验收与交付"。
- 验证点：
- 复发记录：Seen 1x (2026-10-19)。

### fork 带着上游 workflow 时，HTTPS 推送需要 workflow 权限

**推送上游 fork 到自己的私有仓库时用 SSH，加 `--no-follow-tags`，并先关闭该仓库的 Actions。**

- 起因与后果：gh 的 OAuth token 缺少 `workflow` 权限，推送被拒；本机的 `push.followTags` 还把上游 tag 也带了出去。
- 验证点：
- 复发记录：Seen 1x (2026-10-19)。

### 严格桌面模式依赖 app 在运行

**启用 `runner: "app"` 的同时，配置桌面 app 登录自启动。**

- 起因与后果：app 没开时，派活会立刻得到 503。这是预期行为，但手机端会频繁失败。
- 修复与证据：`templates/launchd/desktop-agents-autostart.plist`。
- 验证点：`templates/launchd/desktop-agents-autostart.plist` 存在；目标 Mac 上 `launchctl print gui/$(id -u)/<前缀>.desktop-agents-autostart` 已加载，重启后派活不返回 503。
- 复发记录：待核实。

### grok.com 拦截 Worker 获取它的 CIMD 文档

**Cloudflare Worker 上的 relay 不要为 Grok 打开 CIMD；让它走动态注册。**

- 起因与后果：打开 `clientIdMetadataDocumentEnabled` 后，Grok 的授权请求返回 400。临时 Worker 探测 `https://grok.com/oauth/mcp-client.json` 得到的是 403 challenge 页。
- 修复与证据：撤回改动，fork 保持上游原样；记入 [ADR-0006](docs/decisions/0006-single-remote-path.md)。
- 验证点：`rg -n clientIdMetadataDocumentEnabled -g '!lessons.md' .` 无结果（fork 保持上游原样）；`docs/decisions/0006-single-remote-path.md` 存在。
- 复发记录：Seen 1x (2026-10-19)。

### 链接进 DSH 桌面 profile 的插件，改工作树就可能直接进生产

**改 DSH 插件（`adapters/dsh`）等 `link:` 插件时，在单独的 worktree 里开发；被链接的 checkout 只在验证后切到 main。**

- 起因与后果：开发 native bridge 时直接编辑了被主力 Mac 桌面版链接的 checkout。另一个 agent 恰好给桌面版装插件，触发 profile 重载，桌面 Host 当场加载了半成品，其中一版在 listen 期间改了进程 umask（随后修掉）。没有发现损坏的文件或目录，但这是生产进程。
- 修复与证据：插件去掉 umask 写法并加回归测试；插件日志显示 04:09:54Z、04:12:06Z 两次重载。
- 正确做法：桌面插件的开发与隔离验证用 `DSH_HOME`/`HOME` 临时目录下的 `dsh web`；要上生产时再让链接的 checkout 前进到 main，并在桌面空闲时重载或重启。
- 验证点：
- 复发记录：Seen 1x (2026-10-19)。

### DSH 单条消息就可能超过 64 KiB

**读 DSH 输出时 `session/follow` 用 `maxMessages: 1`，判断 turn/end 只看最后一条；遇到 `result_too_large` 等下一次轮询，不要按范围往回翻。**

- 起因与后果：`check --dispatch` 用 `maxMessages: 6` 读快照，真实会话（含读文件的 tool 结果）超过 64 KiB，返回的错误被当成"还没完成"，轮次早已 completed，检查却等满 10 分钟报失败；实例 context 里写的"4–8"也会让助手踩同一个坑。
- 修复与证据：`scripts/fleet.mjs` 的 `dshTurn`；两台 DSH 的 `check --dispatch` 均 turn/end completed；overlay 的 DSH context 改为 1–2 并写明超限时怎么办。
- 验证点：`rg -n 'maxMessages: 1' scripts/fleet.mjs` 在 `session/follow` 与 `session/page` 两处有结果；`node scripts/fleet.mjs check --dispatch` 两台 DSH 均 turn/end completed。
- 复发记录：Seen 1x (2026-10-19)。

### Herdr 的 runtime_unavailable 多半是 session 名错

**Herdr 方法的 `session` 参数必须是 `session.list` 返回的名字（本机只有 `default`）；传错名字也报 `runtime_unavailable`。**

- 起因与后果：验收时 ChatGPT 调 Herdr 得到 `runtime_unavailable`，报告据此判断"发现正常但执行不可用"；经 relay 用 `session: "default"` 实测 workspace.create、agent.start、agent.prompt 全部 accepted，传 `session: "agenvo"` 复现同一错误。
- 修复与证据：overlay 的 Herdr context 写明 session 取值和完整流程。
- 验证点：
- 复发记录：Seen 1x (2026-10-19)。

### Claude Code 在 Herdr 用无头模式并显式选模型；agy 首次确认信任

**Claude Code 已登录，在 Herdr pane 里按官方 Claude Code 默认新建与续接：每轮 `--model opus --effort medium`。2026-10-28（Asia/Shanghai）起不再经 Magpie。可选第二模型只用官方别名（例如 `sonnet`），同样 `--effort medium`。交互式 agy（`agent.start`）首次进入项目可能要处理信任提示；常规派活按派活技能用无头 `agy -p`。**

- 起因与后果：不显式传模型时 Claude Code 用设置里的默认模型，结果随设置变化，曾返回 `is_error: true`，继续等待无用；agy 遇到信任提示时 `agent.prompt` 返回 `agent_blocked`。
- 修复与证据：2026-10-21 曾用显式模型加 `--dangerously-skip-permissions` 跑通新建和 `--resume`；当时请求经本机代理。2026-10-28（Asia/Shanghai）用户决定：交互式 Claude Code 已断开 Magpie，Herdr Claude 派活改为官方默认 `opus` + `--effort medium`。那次代理路径的实测只说明当时能跑通，不再是当前做法。agy 经 relay 用 `pane.run` 发空命令回车确认信任后可回答。
- 正确做法：不要探测本机代理，也不要因为代理不在线就放弃本轮。按[派活技能](https://github.com/semantic-craft/xw-stack/blob/main/skills/delegate-via-agenvo/SKILL.md)「一轮一个 tab」的表格，每轮 `claude -p --dangerously-skip-permissions --model opus --effort medium --output-format json`，正文从 `~/Projects/_status/<任务名>/brief-N.md` 经 stdin 传入，结果写 `result-N.json`，读取 `result`、`session_id` 与 `is_error`。可选第二模型把 `--model opus` 换成官方别名 `sonnet`，仍带 `--effort medium`。第 2 轮起保留 `--model opus --effort medium` 并加 `--resume <session_id>`。不用交互式 `agent.start kind claude`。输入已接受不等于轮次完成。本仓命令模板在 `fleet.example.json` 的 Herdr context；派活技能源在 xw-stack，须使用同一条命令。
- 验证点：`rg -n -- "--model opus --effort medium" fleet.example.json docs/decisions/0003-herdr-for-cli-agents.md` 有结果；Claude 续接仍含 `--resume <session_id>`，并保留同一 `--model` 与 `--effort`。`rg -n "deepseek/deepseek-flash|127.0.0.1:3425" fleet.example.json docs/decisions/0003-herdr-for-cli-agents.md` 无结果。
- 复发记录：Seen 1x (2026-10-19)；模型与登录状态于 2026-10-21 更新；代理路径于 2026-10-28 退出。

### 从 DSH 的 shell 启动 Electron app 要去掉 ELECTRON_RUN_AS_NODE

**DSH 本身是 Electron，它的 agent shell 继承了 `ELECTRON_RUN_AS_NODE=1`；用 `env -u ELECTRON_RUN_AS_NODE open -a <App>` 启动 ZCode 等 Electron app。**

- 起因与后果：DSH 执行验收时 `open -a ZCode` 让 ZCode 以 Node 模式启动后立刻退出，看起来像 app 坏了。登录 shell 和 launchd 里没有这个变量。
- 验证点：
- 复发记录：Seen 1x (2026-10-19)。

### `plutil -replace ProgramArguments.1` 插入而不是覆盖

**改 plist 数组里的一项时，整体替换数组：`plutil -replace ProgramArguments -json '[...]'`。**

- 起因与后果：第一次 `release zcode` 用 `-replace ProgramArguments.1 -string <新路径>`，结果数组变成 `[node, 新, 旧]`，接收端多带了一个旧路径参数启动（碰巧无害）。
- 修复与证据：`scripts/release.mjs` 改为整体替换数组；重跑后 `plutil -extract ProgramArguments` 只有两项。
- 正确做法：不要用带下标的 `-replace` 改数组元素。
- 验证点：`rg -n '"-replace", "ProgramArguments\.' scripts/` 无结果（没有按下标改数组的 `plutil` 调用；`scripts/release.mjs:294` 注释里提到的 `ProgramArguments.1` 不算）；`rg -n '"-replace", "ProgramArguments", "-json"' scripts/release.mjs` 有结果；发布后 `plutil -extract ProgramArguments json -o - <plist>` 只有两项。
- 复发记录：Seen 1x (2026-10-19)。

### 常驻服务不要从 git checkout 运行

**launchd 指向 `~/.local/share/agent-fleet/releases/<组件>/<sha>/`，由 `fleet.mjs release` 切换。**

- 起因与后果：4 个 connector 原来直接跑 `~/Projects/agenvo/apps/*/dist/cli.js`，在 checkout 里切分支、合并上游或重新构建都会在下次重启时改变线上行为。
- 修复与证据：`scripts/release.mjs`；2026-10-19 切换后 `fleet.mjs check --dispatch` 全部 OK。
- 正确做法：`agenvo-* service install` 之后再跑一次 `release agenvo`。
- 验证点：目标 Mac 上 `plutil -extract ProgramArguments json -o - ~/Library/LaunchAgents/<connector>.plist` 指向 `~/.local/share/agent-fleet/releases/`，不含 `~/Projects/`；`node scripts/fleet.mjs check --dispatch` 全部 OK。
- 复发记录：Seen 1x (2026-10-19)。

### 发布脚本要在无终端（无 TTY）下跑通再合

**`fleet.mjs release` 这类会被 agent 或 launchd 调用的脚本，合并前用 `</dev/null` 在无 TTY 的环境实跑一次。**

- 起因与后果：#6（cccaff6）合并后在主力 Mac 首次 `release dsh`，pnpm 想重建 `node_modules` 时没有 TTY 直接中止，DSH 桥接随之断开，只能手工用同样参数重装。
- 修复与证据：#7（46335b3）给 Host 的 pnpm 加 `CI=true --no-frozen-lockfile`，运行中的 DSH 不会加载新插件时直接说明、不再空等 60 秒；`npm test` 28/28。修正后的命令尚未在第二台 Mac 跑过（#7 自述）。
- 正确做法：发布、安装类脚本的验收至少有一次无 TTY 实跑（`node scripts/fleet.mjs release <组件> </dev/null`），并在 PR 里写明在哪台机器跑过。
- 验证点：`rg -n 'CI: "true"' scripts/release.mjs` 有结果；在目标 Mac 上 `node scripts/fleet.mjs release <组件> </dev/null` 退出码 0，并在 PR 里写明机器。
- 复发记录：Seen 1x (2026-10-19)。

### squash 合并时把 PR 正文带进 commit 正文

**合并用 PR 正文当 commit 正文（`gh pr merge --squash --body-file` 或网页上保留默认正文），至少保留验收三项。**

- 起因与后果：#29（786ee70）的 PR 正文写了改动、回滚范围校验和验证，squash 后 commit 正文是空的，只看 `git log` 的人看不到 Codex connector 单独发布与回滚的边界和没跑到的项目。sonori 在 2026-10-20、2026-10-21 也出过同样的事。
- 修复与证据：没有补救提交（main 不改写历史）；本条只约束之后的合并。
- 正确做法：同日 #28（64f87f5）、#23（f80ac7a）的 commit 正文带着验证披露和审稿结论。
- 验证点：`git log --format='%h %s%n%b' -10 origin/main` 中每个合并提交正文非空，且带验收三项。
- 复发记录：Seen 1x (2026-10-21)。


### DSH session/rename 会让桌面 Host 拿住会话写锁

**CLI 会话登记到项目可在运行中做；改名只在最后一轮 CLI 退出且不再用 `--session-id` 追问后做。**

- 起因与后果：调度方于 2026-10-22 在隔离 `dsh web`（临时 HOME/DSH_HOME）实测，`session/rename` 后 CLI 续写报 `already owned by an active write handle`，等待 45 秒仍未释放。
- 修复与证据：派活技能将 `workspace/attachSession` 与 `session/rename` 分开；前者经 registry.create + attachSession 登记并推送 workspace/follow upsert，不拿会话写锁，CLI 可继续续写。2026-10-22 在隔离 dsh web 上用本插件实测：新路径返回 created:true 且重名规则生效；cwd 不一致返回 bridge/attach-failed，不新建空项目；登记后 CLI `--session-id` 续写不报写锁错误。
- 正确做法：拿到 sessionId 立即登记并告知项目 title；完成全部 CLI 轮次后再改为 `[CLI] <任务名>`，之后追问走桌面或 DSH 实例的 `session/prompt`。详见[派活技能](https://github.com/semantic-craft/xw-stack/blob/main/skills/delegate-via-agenvo/SKILL.md#dsh-headless登记到桌面项目)。
- 验证点：隔离环境中登记后 CLI `--session-id` 仍可续写；改名后 CLI 复现写锁错误，桌面或 DSH 实例接手。插件假注册表回归测试见 `adapters/dsh/test/native-bridge.test.mjs`，不替代真实写锁验收。
- 复发记录：Seen 1x (2026-10-22)。

### 命令行派 DSH 的会话要登记到桌面项目，用户才看得到

**`dsh --profile headless` 派出的会话，拿到 sessionId 立刻经同机 DSH 实例 `workspace/attachSession` 登记到项目，再把项目 title 和 sessionId 告诉用户。**

- 起因与后果：2026-10-22 koe 会议纪要第 1 期在主力 Mac 的临时 /tmp 克隆里用 `dsh --profile headless` 派活，会话已经落盘，桌面 Host 的 `session/list` 也列得出，但它没有记到任何项目下，桌面左栏的项目里找不到，用户看不到进度。桌面版只在首次初始化时按 cwd 自动归组，之后 CLI 新建的会话一律不归项目；原生端点也没有把已有会话记进项目的方法。
- 修复与证据：#39 给插件加 `workspace/attachSession`，派活技能写明登记步骤与项目 title 规则；隔离 `dsh web` 实测登记后会话出现在项目下，CLI `--session-id` 可继续续写（见上一条）。
- 正确做法（Done right）：照[派活技能](https://github.com/semantic-craft/xw-stack/blob/main/skills/delegate-via-agenvo/SKILL.md#dsh-headless登记到桌面项目)，第一轮跑起来后从 `events-1.jsonl` 第一行取 sessionId，在同机读取 pane 里经插件 socket 调 `/bridge/workspace/attachSession`，核对 `ok:true`；登记失败如实报告，不说用户能在桌面里看到。
- 验证点：`rg -n 'workspace/attachSession' ~/Projects/xw-skills/live/xw-stack/skills/delegate-via-agenvo/SKILL.md` 有结果；之后每次 headless 派活的回报里都有「DSH 桌面左栏项目『<title>』下」和 sessionId。
- 复发记录：Seen 1x (2026-10-22)。

### 桌面版续发不做自动化验收

**Codex 桌面版能否接着发消息，由用户在桌面里手动发一轮来验证；不为自动化桌面检查去申请辅助功能权限，也不重签 herdr。**

- 起因与后果：2026-10-21 想经 Herdr 用 osascript 自动点 Codex 桌面版，验证派活线程能续发、出现在侧栏。权限记在 herdr 头上（launchd 起的 herdr 0.9.3），herdr 只有 adhoc 签名、没有 Team ID；用户三轮守在主力 Mac 前点 Allow，「控制 System Events」记住了，辅助功能一直没授予（22:17–22:22 六次检查都是 false，这项权限不弹窗，点 Allow 解决不了），自动检查始终没跑通。
- 修复与证据：用户 2026-10-21 22:26 定：不自动化桌面，不申请辅助功能，不重签 herdr；「桌面在同一线程续发」以用户 22:16、22:17 在桌面里发出的两个 turn 为证记为通过；侧栏是否出现不单独测，技能写明 exec 线程不一定在侧栏、必须给链接。
- 正确做法（Done right）：Codex 新建派活用 `codex exec`，追问用 `codex exec resume`，均经 Herdr pane 执行，不需要桌面权限；要验证桌面能续发，请用户在桌面里发一轮并截图或给时间点。授予 herdr 辅助功能等于让 herdr 里跑的任何命令都能控制键盘鼠标，且 adhoc 签名下每次升级 herdr 都可能要重新授权。
- 验证点：`rg -n 'osascript|System Events' ~/Projects/xw-skills/live/xw-stack/skills/delegate-via-agenvo/ prompts/ scripts/` 无派活或验收用的桌面自动化步骤；`rg -n '0 个归入项目' ~/Projects/xw-skills/live/xw-stack/skills/delegate-via-agenvo/SKILL.md` 有结果。
- 复发记录：Seen 1x (2026-10-21)。

### 改用户的配置前先读现状，派活模型只照决定清单写进命令

**改用户机器上的配置文件前，先读当前内容和修改时间；和交代的背景对不上就先停下问，不按交代的「原值」直接覆盖。派活要换模型，就在每条命令里显式写决定清单里的模型，不改用户自己的默认模型。**

- 起因与后果：2026-10-21 22:33 交代把主力 Mac 上 `~/.claude/settings.json` 的默认模型从「gemini-4-argon」换成 `deepseek/deepseek-flash`。实际文件里 5 处都是用户自己 22:16 刚设的 `minimax-code/MiniMax-M3.1-Flash`，改动照样覆盖了，改完才报「和背景对不上」；用户确认 MiniMax 是他有意设的，只好用备份还原。
- 修复与证据：改前做了备份，按 sha256 核对还原后不带 `--model` 跑短会话，用量里是 MiniMax。派活命令每轮显式带 `--model`（#32、#35），用户自己的默认模型保持他设的值。
- 正确做法（Done right）：动用户配置前读文件、记 mtime 与要改的字段原值，和交代不一致就停下报告；派活所用模型查决定清单，写进每条命令（Codex `-m`、Claude Code `--model`、agy `--model`、DSH `session/selectModel`），不靠改用户或实例的全局默认。2026-10-28（Asia/Shanghai）起，Herdr Claude Code 的决定清单是 `--model opus --effort medium`，续接保留这两项并加 `--resume <session_id>`。
- 验证点：`rg -n -- "--model opus --effort medium" fleet.example.json docs/decisions/0003-herdr-for-cli-agents.md` 有结果（Claude Code 续接仍带 `--resume <session_id>`）；派活回报里写明所用模型，且没有改用户 settings 的步骤。
- 复发记录：Seen 1x (2026-10-21)。

### 修复合并、部署过不等于修好，要按原症状实测

**报「已修好」前，按用户看到的原症状在真实路径上复现一次；合并、部署、单测通过都只是前提。**

- 起因与后果：Codex 桌面版「This is open in another app」要先 Fork 的问题，#30 与 agenvo#5 已合并并部署，被当作已修好转告用户。2026-10-21 19:10 经 agenvo 派出的线程在桌面版里仍显示占用：connector 仍连着共享的 app-server，专属运行时又被现有 hook 发出的 `hook/started` 卡住。做那次修复的会话在主力 Mac 上也找不到。
- 修复与证据：#32 让派活在 Herdr pane 里跑官方 `codex exec`，用 `codex exec resume` 追问；2026-10-21 实测进程退出后桌面打开 `codex://threads/<thread_id>` 有完整历史、正常输入框、无占用横幅（见上文 Codex exec 派活那条）。
- 正确做法（Done right）：修复的验收项写成用户看到的现象（例如「桌面打开派活线程，没有占用横幅，能直接输入」），部署后在真实机器上按这个现象复现一次再报；没复现就写「已部署，未实测」。
- 验证点：之后修复类 PR 的正文或合并后回报里，有一条在真实机器上按原症状复现的记录（时间、机器、看到了什么）。
- 复发记录：Seen 1x (2026-10-21)。

### 技能里写的远程方法，部署的 connector 里必须已经有

**派活技能、实例 context 里让助手经 relay 调的方法，以当前发布的 connector 为准；依赖未合并的 fork 补丁时，写明替代做法和可用条件。**

- 起因与后果：#39 合并后技能让派活方经 DSH 实例调 connector 方法 `workspace/attachSession`，但 connector 一侧是 semantic-craft/agenvo#7，至 2026-10-23 仍未合并，发布的 connector（ff2db52）里没有这个方法；按技能派 dsh headless 时登记必然失败，CLI 会话进不了桌面项目。
- 修复与证据：技能改为在同机 Herdr 读取 pane 里经插件 socket 调 `/bridge/workspace/attachSession`（插件 ff35cd8 已部署）；2026-10-23 经 relay 实测返回 `ok:true`，会话归入桌面已有项目 agent-fleet。
- 正确做法：改技能或 context 前，对照 `search {includeSchema:true}` 的结果确认方法存在；新方法只在 connector 发布、`check` 通过后写进技能。
- 验证点：派活技能和 overlay context 里经实例调用的方法名，都能在 `search {"query": "<方法名>", "includeSchema": true}` 里搜到；`rg -n 'workspace/attachSession' ~/Projects/xw-skills/live/xw-stack/skills/delegate-via-agenvo/SKILL.md` 的结果写明它要等 agenvo#7 发布。
- 复发记录：Seen 1x (2026-10-23)。

### 派出去改代码的任务进 worktree，不进主 checkout

**改代码的任务用 Herdr `worktree.create` 建 `~/Projects/.worktrees/<仓库>/<任务名>`，只读任务才用主 checkout；不另做 `/tmp` 克隆。**

- 起因与后果：派活技能原来让 Herdr workspace 的 cwd 直接取 `~/Projects/<项目>`。2026-10-23 核对时 koe、agent-fleet、box-services 三个主 checkout 都停在功能分支上（agent-fleet 还有未推送提交），同一仓库的并行任务会互相踩；更早的派活用 `/private/tmp/turing-*` 克隆，Herdr 里留下 6 个指向 `/tmp` 的 workspace，桌面项目里看不到，`reap` 也不回收。
- 修复与证据：技能与 context 的 execute 脚本在 `write = true` 时调 `worktree.create`（分支 `agent/<任务名>`，基于 `origin/main`，第一轮先 fetch 并快进）；2026-10-23 经 relay 实测建出 worktree、两轮 Codex 都在其中运行，`worktree.remove` 后只剩分支。
- 正确做法：任务名以仓库名开头；追问回同一 worktree；合并后 `worktree.remove` 并删分支。
- 验证点：`rg -n 'worktree.create' ~/Projects/xw-skills/live/xw-stack/skills/delegate-via-agenvo/SKILL.md prompts/operator.md fleet.example.json` 三处都有结果；派活之后 `git -C ~/Projects/<仓库> branch --show-current` 仍是默认分支。
- 复发记录：Seen 1x (2026-10-23)。

### Herdr 里的 CLI 轮次用 pane 退出表示结束，不看 agent 状态

**每一轮新开 tab，命令以 `echo $? > exit-N; exit` 结尾；结束信号是 `pane_exited` 事件或 `pane.get` 返回 `pane_not_found`。**

- 起因与后果：Codex 的 Herdr 集成只上报会话 id，`codex exec` 跑在 pane 里没有 working/done，派过活的 pane 全是 `unknown`，dot 等不到完成事件，派活方只能轮询 `__EXIT=`。用 `herdr pane report-agent` 自报状态也不可靠：CLI 进程一退出 Herdr 就释放该 pane 的 agent，之后同一标签的上报被丢弃，收尾的 idle 只有抢在退出检测前（约 200 ms）才生效（2026-10-23 实测两轮，一轮收到 done，一轮没有）。
- 修复与证据：技能改为一轮一个 tab；2026-10-23 本机订阅 Herdr 事件看到 tab 关闭时发 `pane_exited`，`fleet.mjs check --dispatch` 的 Herdr 一项经 relay 连续通过；zsh 里 ctrl+c 会放弃整行，exit 文件不会写，中断要另发 `echo 130 > exit-N; exit`。
- 正确做法：事件订阅 `nativeTypes: ["pane_exited"]`（下划线是 Herdr 原生事件名）；轮询看 `pane.get`；退出码和结果读 `_status/<任务名>/`。`agent.get` 只在运行中有用（Codex 轮次的 `agent_session.value` 是 thread_id）。
- 验证点：`node scripts/fleet.mjs check --dispatch --instance <herdr 实例>` 输出 `turn pane …: exited, exit file read back`。
- 复发记录：Seen 1x (2026-10-23)。

### 桌面项目下自动开跑用 DSH

**Desktop 交互控制交还门槛**：独立同 host 执行基线不以退订能力为创建前置，Desktop 项保持未验；仅当完成交还时，确认 connector 暴露 `thread/unsubscribe` 且 watcher 不抢其他 loaded 线程，否则 `blocked_native_writer_release`。同 host 共享 app-server 与 Desktop 自带 backend 也要交还 writer；本次 turn 终态后由同一 connector 退订，再截图核实项目下可见且无横幅。connector 重启不释放独立 native writer。目标仍为 Desktop 可见后 agent `turn/start`，见[交接契约](docs/codex-desktop-handoff.md)。

- 起因与后果：用户有时要左栏项目下直接跑起来的会话。
- 修复与证据：DSH 经 `workspace/create {path: <任务目录>}` 的会话在桌面左栏对应项目下（2026-10-20 验收）。
- 正确做法：dot 有平台任务工具与授权时优先原生 Codex；DSH／GrokBot 的外部 Codex Desktop 需求仍保留且单独验收；Agenvo 项目下自动开跑用 DSH。见[派活技能](https://github.com/semantic-craft/xw-stack/blob/main/skills/delegate-via-agenvo/SKILL.md)。
- 验证点：`rg -n 'Codex Desktop|项目下' ~/Projects/xw-skills/live/xw-stack/skills/delegate-via-agenvo/SKILL.md` 有对应表述。
- 复发记录：Seen 1x (2026-10-23；2026-10-25 按 Desktop 正道改写)。

### 经 relay 的写操作返回 unknown 时，可能已执行也可能没送达

**`unknown` 不重发：先读原生状态（`pane.get`、exit 文件、`worktree.list`、`session/list`）再决定；订阅事件也保留轮询。**

- 起因与后果：2026-10-23 两台 Mac 的全部 connector 频繁断线重连（17:49–18:21 实测 31.5 分钟 30 次；relay 日志里的 `1006` 是滞后的孤儿连接清理，不能当断线计数）。断线时进行中的调用返回 `execution_unknown`，之后几秒返回 `device_offline`；同日实测同样的 `pane.run`，一次返回 `unknown` 但命令已执行，一次 `unknown` 且命令没送达。断线期间的事件不补发。
- 修复与证据：技能、operator 与 dot-watcher 写明 unknown 与 `agenvo.resync_required` 的处理；`check --dispatch` 的 Herdr 一项容忍连续 3 次查询失败；证据与观察方法见[网络与桌面验收](docs/network-and-desktop-acceptance.md#重连可观察性)。断开的根因是 relay 主机走了机场线路，出口到 Cloudflare 的路由不稳，不是同一设备身份另有连接；relay 的 `1006` 多半是更早断掉的孤儿连接被清理，不代表当时断线（2026-10-23 17:49–18:21 实测）。用户批准后网关规则让 relay 主机直连，18:51–19:23 复测 31.6 分钟断 1 次（基线 31.5 分钟 30 次）。
- 正确做法：派活脚本返回 `unknown` 时，按任务名查 worktree、轮次 pane 和 `_status/<任务名>/`，确认没执行才重派。
- 验证点：按[网络与桌面验收](docs/network-and-desktop-acceptance.md#重连可观察性)数本机 `status.json` 的重连次数和 `/api/admin/state` 的 `epoch` 变化，不数 tail 里的 `1006`；网关规则调整后取 30 分钟样本，对比 2026-10-23 基线（31.5 分钟 30 次）。
- 复发记录：Seen 1x (2026-10-23)。

### 派活接受、原生终态与目标验收必须分开

**用稳定 RPC ID 关联自己的输入、轮次、请求头、回复和终态；未知提交只回读，不盲重发。**

- 起因与后果：只读最后一条事件可能漏掉回复；别的输入或旧终态也不能证明当前请求完成。会话选模型可能保存全局默认，不等于会话隔离。
- 修复与证据：`scripts/harness.mjs` 在调用层实现有界分页、模型/配置漂移检查和幂等恢复；`test/harness.test.mjs` 使用合成事件验证。`fleet.mjs` 复用该层，未改 connector。
- 正确做法：未知写结果保留原 request ID 和恢复 journal；只有匹配的原生 cancelled 才算取消，队列里的输入不能靠 cancel 声称移除。详见 [harness 契约](docs/harness.md)。
- 验证点：离线测试不替代真实云端与只读工具闭环；默认内存 journal 不承诺跨进程恢复。禁止把 PTC 兼容路线当标准模式修复。
- 复发记录：Seen 1x (2026-10-24)。

### 兼容路线的标记成功不能扩大成通用执行能力

**将模型/preset的验收范围写进显式版本化profile，并检查已有会话，不自动切换。**

- 起因与后果：同一模型的PTC标记往返成功，而standard在已测部署出现空max-tokens；把二者统称“模型已打通”会掩盖差异。
- 修复与证据：`scripts/route-profiles.mjs`、`test/route-profiles.test.mjs`，复用既有HarnessRunner检查配置、指纹与终态；profile只记录有限验收，不替代原生权限或全局模型选择授权。
- 验证点：known-failure零transport；preset/model投影预检不匹配不派活，实际请求header漂移不得误报验收成功；工具事件导致标记profile失败；重试不得切换profile。
- 复发记录：Seen 1x (2026-10-24)。

### 工具验收不能只数tool事件或看最终答案

**有界读取须核对工具名、精确参数、调用与结果关联、成功标志和实际结果行，并检查原生只读权限。**

- 起因与后果：标记成功或toolEvents大于零都不能证明模型按约定读取了指定fixture；其他工具/错误结果仍可能产生正确猜测答案。
- 修复与证据：MiniMax single-fixture profile复用HarnessRunner与ReadOnlyVerificationHarness；回归覆盖参数篡改、额外工具、错误结果、错callId、继承header、权限漂移和持久恢复。
- 验证点：路径/参数/正文只在内存匹配并参与指纹，不写FileJournal；零自动切模/切权限；后验不等于执行前强制限制。
- 复发记录：Seen 1x (2026-10-24)。

### 新机恢复与模型选择必须显式验收

- `session/selectModel` 可能写全局默认；操作窗口需协调所有选择者和新会话创建，漂移不得覆盖。纯前置校验见 `scripts/model-selection.mjs`，回归见 `test/model-selection.test.mjs`；不是服务端CAS。
- 版本字符串与模型别名不代替行为；unknown/candidate不得晋升已验工具能力。服务重建不重发在途任务。入口见 `docs/migration-and-upgrades.md`。

### 双向派活需要收据，连接重启不等于交接

- 起因与后果：旧默认仍可能从已安装技能或 connector context 进入；accepted、空闲和链接打开均不足以证明 Desktop 与业务成功。
- 正确做法：以派活技能的 Magpie 五步和轮询为准；connector kickstart 不证明 native writer 释放，DSH 同值选模仍回读全局默认。原生终态、业务结果和收据分别留证。
- 验证点：`npm test`；真实烟雾逐项报告 UI、模型 header、终态、标记与 receipt。UI 或模型失败就停该路，不换 CLI 冒充。
- 复发记录：Seen 1x (2026-10-25，本次双向派活验收)。

### 事件观察不能替其他客户端取得 writer

- 起因与后果：watchEvents 自动 resume 所有 loaded 线程，导致共享 app-server 保留订阅，Desktop 显示 writerConflict；重启 connector 不能证明放锁。
- 修复：仅恢复明确订阅的 resumeTargets，暴露 thread/unsubscribe，成功后移除目标；完成回读后由同一连接退订。
- 验证：agenvo 的 codex-attach 回归检查 orphan loaded 不被收养、退订后重连不再 resume；桌面必须另验左栏项目与无横幅，原生成功不代替截图。
- 复发记录：Seen 1x (2026-10-25)。

### 控制面故障必须按层恢复

- connector online、native available、调用方 OAuth、任务终态和主动事件是不同证据。DSH app 停止可使 socket 消失而 connector 仍在线；普通启动可恢复，无须先发布插件或重新授权。
- Cloudflare 浏览器签名403不等于 Access 登录失败；按 Ray 查证，安全规则变更另批。模型网关和任务控制入口分开，不强制所有调用方经额外转发层。
- 2026-10-26 新的 DSH 发起、两轮同 Codex thread、原会话回执已验；主动推送、GrokBot和桌面可见性未由此证明。见 [恢复流程](docs/runtime-recovery.md)。既有 FileJournal 回归保护未知准入只读恢复及跨进程去重；不要通过重放真实写操作测试去重。
