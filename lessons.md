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
