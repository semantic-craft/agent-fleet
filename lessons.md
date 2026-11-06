# 项目教训

实现、评审或验收前先读相关条目；用户纠正、返工或漏验暴露了可复用的教训后补记。

### 从 DSH 的 shell 启动 Electron app 要去掉 `ELECTRON_RUN_AS_NODE`

**DSH 本身是 Electron，它的 agent shell 继承了 `ELECTRON_RUN_AS_NODE=1`；用 `env -u ELECTRON_RUN_AS_NODE open -a <App>` 启动 ZCode 等 Electron app。**

- 起因与后果：DSH 执行验收时直接运行 `open -a ZCode` 会让 Electron 应用以 Node 模式启动后立刻退出，表现为应用无法拉起。登录 shell 和 launchd 里没有这个变量。
- 复发记录：Seen 1x (2026-10-19)。

### `plutil -replace ProgramArguments.1` 插入而不是覆盖

**改 plist 数组里的一项时，整体替换数组：`plutil -replace ProgramArguments -json '[...]'`。**

- 起因与后果：对 plist 数组使用带下标的 `-replace ProgramArguments.1 -string <新路径>` 会插入元素而非覆盖原元素。
- 正确做法：不要用带下标的 `plutil -replace` 修改数组元素，始终用 `-json` 整体替换数组。
- 复发记录：Seen 1x (2026-10-19)。

### squash 合并时把 PR 正文带进 commit 正文

**合并用 PR 正文当 commit 正文（`gh pr merge --squash --body-file` 或网页上保留默认正文），至少保留验收三项。**

- 起因与后果：PR 正文写了改动、回滚范围校验和验证，但 squash 合并后 commit 正文为空，只看 `git log` 的人看不到边界和验证披露。
- 验证点：`git log --format='%h %s%n%b' -10 origin/main` 中每个合并提交正文非空，且带验收记录。
- 复发记录：Seen 1x (2026-10-21)。

### 改用户的配置前先读现状，派活模型只照决定清单写进命令

**改机器上的配置文件前，先读当前内容和修改时间；和交代的背景对不上就先停下问，不按交代的「原值」直接覆盖。派活要指定模型，在每条命令或适配器参数里显式传入，不改用户的全局默认模型。**

- 正确做法：动配置前先读文件并核对原值；派活模型通过显式参数传递（见 `crates/fleet-proto/src/brief.rs` 的 `validate_brief` 与 `DesktopDelivery::send_with_model`），不修改用户全局默认设置。
- 复发记录：Seen 1x (2026-10-21)。

### 修复合并、部署过不等于修好，要按原症状实测

**报「已修好」前，按用户看到的原症状在真实路径上复现一次；合并、部署、单测通过都只是前提。**

- 正确做法：修复的验收项写成用户可观察的现象（例如桌面侧栏项目下可见对应会话、无占用横幅、可直接输入），在真实宿主环境复现验证后再报告完成；未实测的部分明确标为未验证。
- 复发记录：Seen 1x (2026-10-21)。

### 派出去改代码的任务进 worktree，不进主 checkout

**改代码的任务在独立 git worktree（如 `~/Projects/.worktrees/<仓库>/<任务名>`）中执行，不直接修改 `main` 主工作区；合并后立即移除临时 worktree 与分支。**

- 正确做法：遵守根 [AGENTS.md](AGENTS.md) 的「worktree 不遗留」条款。合并进 `main` 后执行 `git worktree remove` 与 `git branch -D`，确保主工作区始终停在干净的 `main` 分支。
- 复发记录：Seen 1x (2026-10-23)。

### Agent Network 本机派活与 Cindy 插件边界（2026-11-02）

- `peers allow`／`trust` 要终端确认，非 TTY 得到 `tty_required`；agent 只给出命令，不代开 PTY 回 `yes`。见 [docs/anet.md](docs/anet.md)。
- shell 残留错误 `ANET_HOME` 会让 `--id` 落到别的身份树；用 `env -u ANET_HOME ANET_DATA_DIR=$HOME/.anet anet …`。
- 同一 anet 身份只能有一个常驻处理者；更换插件前先停旧插件再跑 smoke。
- 自制派活插件住 [plugins/](plugins/README.md)；名称表达「宿主 × 协议」（如 `agent-network-cindy`），且不以官方保留前缀 `cindy-` 开头。
- Cindy 0.1.121 清单 `description`／`whenToUse`／工具描述上限 300 字符，超限安装时只报不合法的 `.cindy` 文件；[plugins/agent-network-cindy/pack.py](plugins/agent-network-cindy/pack.py) 先校验再打包。
- Cindy 宿主通过 Electron `utilityProcess` 的包装脚本用 `createRequire` 加载插件 [plugins/agent-network-cindy/node/worker.cjs](plugins/agent-network-cindy/node/worker.cjs)，生产运行时 `require.main === module` 恒为 `false`。不要用 `require.main === module` 保护 worker 启动循环；单元测试用环境变量 `AGENT_NETWORK_CINDY_TEST=1` 跳过自动启动，并保留包装器加载回归测试（[test/cindy-worker.test.mjs](test/cindy-worker.test.mjs)）。

### 删除旧路径后要全仓搜旧关键词与断链再报完成（2026-11-02）

- 起因与后果：清理旧控制面或历史文件时，如果只按文件清单删除而不扫描全仓残留引用和相对链接，容易留下悬空链接或旧路径说明。
- 正确做法：清理类改动合并前，全仓扫描旧关键词、校验全部 Markdown 相对链接存在，并运行 `./scripts/check` 与 `node scripts/guard.mjs` 全部通过后再报完成。
- 复发记录：Seen 1x (2026-11-02)。

### Hub 匿名请求需要隔离连接池（2026-11-04）

- 起因与后果：`fleet-hub` 若让 session 与 capability 投递共用 HTTP client，即使 capability 没有身份头，keep-alive 连接复用仍可能把它关联到同一 session。
- 修复：每次 `send` 和 `status` 使用独立 HTTP client，禁用重定向与自动重试。未验证 `dack` 保留后续回读能力，响应 wire 版本不兼容时拒绝作为证据。
- 验证点：`cargo test -p fleet-hub` 的 `session_and_capability_sends_use_separate_connections`、`unverified_done_can_be_read_again_for_core_verification` 和 `conflicting_response_wire_version_is_rejected` 覆盖该边界。
- 复发记录：Seen 1x (2026-11-04)。

### doctor 读库失败不能报成 schema 漂移（2026-11-06）

**sqlite 只读打开在 hot-WAL 下失败时，gate 报 schema_status=unreadable（或重试后读到列），绝不能把 SQLITE_CANTOPEN 记成「列缺失／diverged」。**

- 起因与后果：`a29dfdd` 前，Antigravity daemon 的 hot-WAL 窗口让 `sqlite3 -readonly` 报 SQLITE_CANTOPEN，gate 误报 `diverged (all columns missing)`。
- 修复与证据：`a29dfdd` — 3 次重试；持续读失败用独立的 `schema_status=unreadable`；`read_sqlite_table_columns` 回退 plain open 仅做 PRAGMA table_info（查询不写；plain open 只做标准 WAL 恢复）。本机 daemon 忙碌下 `doctor --gate` 3/3 绿。
- 正确做法（Done right）：只读探测与 schema 比对分开；打开失败是可达性／锁问题，不是列集合证据。
- 验证点：在 Antigravity daemon 忙碌时跑 `doctor --gate`，确认不出现「all columns missing」；人为不可读路径应得到 unreadable 而非 diverged。
- 复发记录：Seen 1x (2026-11-06)。

### ZCode 与 DSH 桌面侧栏可见性必须对齐宿主源码查询条件与内存态（2026-11-07）

**ZCode Desktop 侧栏按 `provider = 'glm'` 过滤 `v2/tasks-index.sqlite` 且从 `cli/db/db.sqlite` 的 `part` 表渲染消息；DSH Desktop 的 `SingleJsonUnit` 在启动时把 `workspace.json` 读入内存且不监听磁盘文件变化，必须经由 `127.0.0.1:9230` Inspector CDP 同步宿主内存态并广播 `api-session/added`。**

- 起因与后果：`attach_zcode_session_to_tasks_index` 此前未写 `provider` 列（留空为 `NULL`），且在真实 `zcode` CLI 已写入完整消息与 `part` 行后又额外插入无 `part` 行的 `message` 记录，导致会话在 ZCode Desktop 侧栏被 `WHERE provider='glm'` 完全过滤，点开时还会多出空白消息气泡；DSH Desktop 则因 `SingleJsonUnit` 只认内存态，仅改磁盘 `workspace.json` 不会刷新已打开的桌面侧栏。
- 正确做法：`crates/fleet-daemon/src/tri_hosts.rs` 写入 ZCode `tasks` 表时固定填齐 `provider = 'glm'`、`title_overridden = 1`、`cli_deleted = 0` 与 `meta_json`（并将写入列全部纳入 `doctor.rs` 的 `schema_expectations`），仅在无真实 CLI `part` 行时才补写 fallback `message` + `part`；写入 DSH 实机 `~/.dsh` 时通过 `127.0.0.1:9230` 调用 `ctx.workspaceRegistry` 与 `ctx.emit('api-session/added', item)`；`scripts/herdr-dispatch.sh` 在 `set -o pipefail` 下用变量捕获 `herdr status server` 输出再做字符串匹配，禁止管道接 `grep -q` 触发 SIGPIPE 误判服务未运行。
- 复发记录：Seen 1x (2026-11-07)。
