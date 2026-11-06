# Rust fleet engine 接力队列

## 用户愿景复述与引擎总目标（2026-11-05 记录）

1. **双源合一的架构定位**：本引擎是 [sleep2agi/agent-network](https://github.com/sleep2agi/agent-network)（及官方 ANet Hub 的身份发现、任务账本、单通道收据关联与跨节点路由）与 [reason-machines/reason-network](https://github.com/reason-machines/reason-network)（会话目录发现、项目级 `cwd` 跟随会话、消息直达已打开的桌面项目线程）两个项目融合而成的开源 Rust 引擎。
2. **五大桌面 Agent 互相派活与左侧项目会话可见**：`antigravity-desktop`（Antigravity Desktop）、`openai-dots`（Codex Desktop）、`deepseek-harness`（DeepSeek Harness Desktop）、`cindy`（Cindy Desktop）与 `zcode`（ZCode Desktop）五个桌面 Agent 之间支持双向互相派活。任何一端派出的任务都在目标桌面客户端左侧栏对应的项目（`~/Projects/<slug>`）下挂载并显示真实会话线程，后续追问留在同一会话内。
3. **Reason Network 级别的对外开放开源引擎**：
   - 外部开源或第三方 Agent 可通过标准 JSON-RPC 2.0 stdio MCP 服务（`fleet-cli mcp`）直接接入，查询在线团队与项目会话（`list_team`）、向任一宿主派活或续问（`send_message`）并回读关联收据（`message_status`）。
   - 外部开发者可通过插件清单（`<state_root>/agents.d/<id>.json` 与 `fleet-cli agents register`）零改 Rust 核心接入新 Agent。
   - 任何一台电脑安装本引擎后，均可通过 `fleet-cli setup`、`fleet-cli doctor`、`fleet-cli who`、`fleet-cli send` 与 `fleet-cli mcp` 搭建起五大桌面 Agent 及外部开源 Agent 之间的派活互通服务。

总目标：ANet-based Rust fleet engine absorbing agenvo + Reason delivery；兼容 `https://anet.sonori.dev`。Slice A–P 已按各自验收边界完成：A–G 完成引擎骨架、会话目录、Hub 客户端与收据层级（已合入 `main` PR #115）；H–L 完成 REA 桌面档案与四桌面宿主受控建目录、侧栏挂载及 `/poteto-mode` 跨 Agent 投递适配器；M–P 完成真实桌面侧栏可见性验证（Antigravity Desktop Hub 直连与 DSH 双帧压缩头同步）；Q–T 补齐第 5 桌面宿主 `zcode`、开放外部 Agent 注册清单、`fleet-cli who/send/mcp/setup/doctor` 及 Reason Network 级开源文档与闭环验收。Codex (`openai-dots`) 通过 Magpie 本地网关默认使用 `zcode/GLM-5.3-Flash`（`model_reasoning_effort = "max"`）。

## 工作源与约束

- 基线为 `main`；H–L 扩展在 `agent/slice-h-rea-desktop-20261105` 分支完成。
- 日志与历史收据备份在 `~/Projects/_status/agent-fleet-rust-engine-20261104/`，本工作区账本在 `.scratch/pstack-relay/decisions.tsv`。
- Rust 是目标实现。vendor/anet 仅作协议参考；旧 agenvo 仅提供机制线索。现有线上路径保持到真实验收通过。
- 不读 PRIVATE 或业务仓；不改 Hub 身份、Reason 配置或部署。禁止同活同时发 ANet task 和 Reason 进窗。
- 模型由 dispatcher 配置（Codex 经 Magpie 默认指定 `zcode/GLM-5.3-Flash` 与 `max` 推理档位）。brief 禁止 model self-attest 和 MODEL_WRONG 回路，不以 agent 自报作为模型证据。
- 技术分叉自行用隔离原型判断。不可逆外部操作停止交外循环。

## 现状 vs 目标差距与防冲突护栏

| 项目 | 本轮现场 | 目标与缺口 |
| --- | --- | --- |
| 实现 | Slice B 已建立 Rust 四 crates 与只读 session directory | Slice C 已增加 Hub transport；Slice D 增加 daemon 执行核心，Slice E 已有原生已加载线程 adapter，Desktop 自带 stdio 接线仍未完成 |
| Hub | 旧 architecture 有排除 Sonori Hub 的陈述，与当前 brief 冲突 | 本轮架构说明采用 Sonori 正本；不声称已做线上探测 |
| Reason | docs/reason.md 仅定义投递边界，签到另线 | 五项机制均须 Rust 实现与行为证据 |
| 并行工作 | fleet-canon-workdir-hub worktree 存在，另有未提交 Rust 工作线索 | 不复制、不覆盖；B 先核对所属分支与实现差异 |
| 验证 | scripts/check 汇总 Cargo、隔离 fixture、relay selftest 与 npm test | 真实 Hub/Desktop 证据仍缺；不运行生产 fleet check |
| hooks | init 写 .agents/hooks.json | 文件生成不代表宿主已加载；本轮手动 advance 验证 |

## 验证杠杆与吞吐检查点

运行 `scripts/check`。它运行 `cargo check --workspace`、`cargo test --workspace`、`cargo test --workspace session_directory`、`cargo test -p fleet-hub`、`cargo test -p fleet-daemon cwd_follows_session -- --nocapture`、`cargo test -p fleet-daemon desktop_delivery -- --nocapture`、`cargo test --workspace delivery_tier`、`cargo test --workspace brief_policy`、接力控制器自测和现有 `npm test`，不加载凭据、不运行生产 fleet check。
Slice B 已将 Cargo 编译与 session directory 隔离行为测试接入同一入口。Slice C 增加隔离 Hub HTTP fixture。Slice D 增加两个临时项目中的真实子进程执行和拒绝时零执行检查。每次运行保留完整输出与源码 SHA-256 清单，回执位于 `.scratch/pstack-relay/checks/<时间>/receipt.json`。Desktop 可见性保留至 G 的真实验收，不由 E 的放宽验收推导。
先用可重复离线检查完成 A；最高不确定性是 session 识别与进已开线程，B 优先建立目录契约，E 必须补真实 Desktop 证据。其余切片按依赖顺序推进，不并发改同一工作区。

本轮结果：`scripts/check` 退出 0，311 tests pass，private guard 与 caller routing 均 OK。relay selftest 通过。初次检查发现 hooks/scratch 未被 worktree 的 exclude 忽略及文档含机器别名，已在 Git common exclude 和文档中修正，重跑通过。

## 切片表

| Slice | 交付 | 验收位置 |
| --- | --- | --- |
| A | 盘点与接力入口 | 下方 Slice A 的 Done Predicate |
| B | Rust crates 与 session directory | 下方 Slice B 的 Done Predicate |
| C | Hub client 与单通道收据 | 下方 Slice C 的 Done Predicate |
| D | Node daemon 与 cwd-follows-session | 下方 Slice D 的 Done Predicate |
| E | Inject opened Desktop/thread | 下方 Slice E 的 Done Predicate |
| F | Delivery tier 与 brief 边界 | 下方 Slice F 的 Done Predicate |
| G | 外循环验收与收口 | 下方 Slice G 的 Done Predicate |
| H | REA 观察分析 Antigravity Desktop 与 Codex Desktop 项目/会话机制 | 下方 Slice H 的 Done Predicate |
| I | 扩展 fleet-proto 与 fleet-daemon 的项目目录创建与四端会话生命周期契约 | 下方 Slice I 的 Done Predicate |
| J | 落地 Antigravity Desktop 侧栏建项目与会话投递适配器 | 下方 Slice J 的 Done Predicate |
| K | 打通 OpenAI Dots (Codex Desktop)、DSH、Cindy 三端侧栏建目录与会话投递 | 下方 Slice K 的 Done Predicate |
| L | 四端 /poteto-mode (pstack-relay) 跨 Agent 自我开发闭环验收 | 下方 Slice L 的 Done Predicate |
| M | 收尾 Maitra 旧挂起通道、放宽 DeepSeek 思考模式超时并更新未知账目单次重试授权 | 下方 Slice M 的 Done Predicate |
| N | 经 fleet-cli 向 DeepSeek Harness (DSH) 分派 glaux-book 会话并完成 Maitra 全文翻译 | 下方 Slice N 的 Done Predicate |
| O | 在 DSH 会话下完成 Maitra 译文导入、源文对照审校、中文 EPUB 构建与 Agent Acceptance | 下方 Slice O 的 Done Predicate |
| P | 经 DSH 完成 Maitra 中文 EPUB 挂回 Zotero PQ9G5JUI、字节回读核验与全链路收口 | 下方 Slice P 的 Done Predicate |
| Q | 补齐第 5 桌面宿主 ZCode (`zcode`) 侧栏适配器与开放外部 Agent 注册清单 (`agents.d`) | 下方 Slice Q 的 Done Predicate |
| R | 落地 Reason Network 级 CLI 与 MCP 开放入口 (`fleet-cli who/send/mcp/setup/doctor/agents`) | 下方 Slice R 的 Done Predicate |
| S | 升级仓库对外开源文档与架构设计 (`README.md`、`DESIGN.md`、`docs/desktop-rea-dossier.md`) | 下方 Slice S 的 Done Predicate |
| T | 五大桌面 Agent + 外部开源 Agent 互通闭环验证、合入 `main` 与零遗留收口 | 下方 Slice T 的 Done Predicate |

### [x] Slice A: 盘点与接力入口
- **Playbook**: `Investigation`
- **Outcome**: 建立现状清单、离线验证入口与 Reason 五项契约，生成 B 提示词后停止。
- **Done Predicate**: `scripts/check` 通过，或完整记录既有回归缺口；`scripts/pstack-relay selftest` 通过；五项契约与后续命令齐全。
- **Evidence**: ~/Projects/_status/agent-fleet-rust-engine-20261104/slice-a-receipt.md;docs/architecture.md;scripts/check

### [x] Slice B: Rust crates 与 session directory
- **Playbook**: `Feature`
- **Outcome**: 建立 fleet-proto、fleet-hub、fleet-daemon、fleet-cli；实现 session directory 的发现、稳定 thread 标识与过期记录拒绝，先用隔离夹具验证。只做 B，完成后交棒。沿用本页约束，禁止模型自证。
- **Done Predicate**: `cargo check --workspace`、`cargo test --workspace session_directory` 和 `scripts/check` 通过；测试覆盖缺失、歧义、过期 session，断言不会误建线程。
- **Evidence**: .scratch/pstack-relay/slice-b-receipt.md;.scratch/pstack-relay/checks/20261104T135837.838871Z/receipt.json

### [x] Slice C: Hub client 与单通道收据
- **Playbook**: `Feature`
- **Outcome**: Rust Hub client 对接 Sonori Hub 协议；task 与 delivery 共用关联标识，超时先回读，不跨通道重复投递。
- **Done Predicate**: `cargo test -p fleet-hub`、`scripts/check` 通过；隔离 Hub fixture 重放鉴权失败、超时、重复响应和回读，断言单次投递与收据关联。
- **Evidence**: .scratch/pstack-relay/slice-c-receipt.md;.scratch/pstack-relay/checks/20261104T141116.956677Z/receipt.json

### [x] Slice D: Node daemon 与 cwd-follows-session
- **Playbook**: `Feature`
- **Outcome**: daemon 从被选 session 读取项目 cwd；拒绝缓存目录、缺失目录和 session 变化，不用启动 daemon 的 cwd 覆盖会话目录。
- **Done Predicate**: `cargo test -p fleet-daemon cwd_follows_session`、`scripts/check` 通过；在两个临时项目切换 session，观察执行命令返回正确 cwd，失效时零执行。
- **Evidence**: .scratch/pstack-relay/slice-d-receipt.md;.scratch/pstack-relay/checks/20261104T141628.592823Z/receipt.json

### [x] Slice E: Inject opened Desktop/thread
- **Playbook**: `Feature`
- **Outcome**: Rust 投递接口吸收 Reason 进窗机制，复用已经打开的 Desktop/thread；回读输出、续问、中断与事件留在同一线程。
- **Done Predicate**: `cargo test -p fleet-daemon desktop_delivery`、`scripts/check` 通过；开发机 真实已开线程完成一次可见投递与结果回读，保存 thread id、cwd 和关联收据；mock 不能代替此证据。
- **Evidence**: .scratch/pstack-relay/slice-e-receipt.md;~/Projects/_status/agent-fleet-rust-engine-20261104/slice-E-receipt-outer.md;.scratch/pstack-relay/checks/20261104T142841.176788Z/receipt.json

### [x] Slice F: Delivery tier 与 brief 边界
- **Playbook**: `Feature`
- **Outcome**: 收据区分 accepted、thread-visible、running、completed、failed、unknown；模型配置归 dispatcher，brief 禁止模型自证及 MODEL_WRONG 门槛。
- **Done Predicate**: `cargo test --workspace delivery_tier`、`cargo test --workspace brief_policy`、`scripts/check` 通过；拒绝虚报可见或完成层级；带自证门槛的 brief 被边界检查拒绝，正常 brief 可投递。
- **Evidence**: .scratch/pstack-relay/slice-f-receipt.md;.scratch/pstack-relay/checks/20261104T144603.182728Z/receipt.json

### [x] Slice G: 外循环验收与收口
- **Playbook**: `Shipping`
- **Outcome**: 开发机 Desktop 可见完成一次 Rust 引擎任务；Hub 保持兼容；说明实现、合并、部署和人工验收各自状态。
- **Done Predicate**: `cargo check --workspace`、`cargo test --workspace`、`scripts/check` 通过；真实 Sonori Hub 与 开发机 Desktop 同一任务收据闭环，独立审查后由外循环收口。
- **Evidence**: .scratch/pstack-relay/slice-g-receipt.md;.scratch/pstack-relay/checks/20261104T144947.302679Z/receipt.json;.scratch/pstack-relay/slice-g-final-audit.md

### [x] Slice H: REA 观察分析 Antigravity Desktop 与 Codex Desktop 项目/会话机制
- **Playbook**: `Investigation`
- **Outcome**: 使用 REA 对 Antigravity Desktop (`/Applications/Antigravity.app`、`~/.local/libexec/agy/agy`、`conversation_summaries.db`、Hub `LanguageServerService`) 与 Codex Desktop (`state_5.sqlite`、`queue_1.sqlite`、`.codex-global-state.json`、`codex app-server`) 进行静态与状态库取证，完成 `docs/desktop-rea-dossier.md` 并排定 Slice H–L 接力队列。
- **Done Predicate**: `test -f docs/desktop-rea-dossier.md && scripts/check && python3 scripts/pstack-relay selftest` 通过；档案包含可复核 REA Evidence ID、SHA-256 摘要、SQLite/RPC 契约及四端统一 `DesktopHostAdapter` 映射。
- **Evidence**: docs/desktop-rea-dossier.md;docs/pstack-relay.md;.scratch/pstack-relay/checks/20261105T010754.427650Z/receipt.json

### [x] Slice I: 扩展 fleet-proto 与 fleet-daemon 的项目目录创建与四端会话生命周期契约
- **Playbook**: `Feature`
- **Outcome**: 在 `fleet-proto` 与 `fleet-daemon` 中新增统一的 `DesktopHostAdapter` 接口（覆盖 `openai-dots`、`cindy`、`deepseek-harness`、`antigravity-desktop` 四宿主），支持受控创建项目目录 (`ensure_project_dir`)、侧栏项目注册 (`ensure_sidebar_project`)、新建侧栏会话 (`create_session`) 与续接追问 (`deliver_turn`)，并拒绝路径穿越与越界目录创建。
- **Done Predicate**: `cargo test --workspace` 与 `scripts/check` 通过；隔离测试覆盖四端适配器的项目目录创建、侧栏项目 ID 绑定、会话新建/续接及非法路径拒绝。
- **Evidence**: crates/fleet-proto/src/host.rs;crates/fleet-daemon/src/host_adapter.rs;crates/fleet-daemon/tests/host_adapter.rs;.scratch/pstack-relay/checks/20261105T020255.186329Z/receipt.json

### [x] Slice J: 落地 Antigravity Desktop 侧栏建项目与会话投递适配器
- **Playbook**: `Feature`
- **Outcome**: 基于 Slice H 的 REA 证据，在 `fleet-daemon` 中实现 Antigravity Desktop 适配器：支持发现 `~/.gemini/antigravity/daemon/hub_<port>.json` 或调用 `agy --new-project` / `--project <id>` / `--add-dir <dir>` / `--conversation <id>` 创建项目目录、挂载侧栏项目并投递或续接对话，回读 `conversation_summaries.db` 验证 `project_id` 与 `workspace_uris` 落盘。
- **Done Predicate**: `cargo test -p fleet-daemon antigravity` 与 `scripts/check` 通过；测试验证新建目录与会话在 `conversation_summaries.db` 中绑定目标 `project_id` 而非落入无项目桶，且收据包含 `conversation_id` 与 `workspace_uris`。
- **Evidence**: crates/fleet-daemon/src/antigravity.rs;crates/fleet-daemon/src/host_adapter.rs;crates/fleet-daemon/tests/antigravity.rs;.scratch/pstack-relay/checks/20261105T023057.001479Z/receipt.json

### [x] Slice K: 打通 OpenAI Dots (Codex Desktop)、DSH、Cindy 三端侧栏建目录与会话投递
- **Playbook**: `Feature`
- **Outcome**: 在 `fleet-daemon` 中补齐 OpenAI Dots (`codex app-server` `project/create` + `thread/start` / `turn/start` 及 `state_5.sqlite` `projects`/`project_roots` 校验)、DSH (`POST /v1/sessions` 绑定新建目录 `cwd` + `POST /v1/sessions/<id>/turns`) 与 Cindy (`cindy_session_send` / Gateway session `cwd` 绑定) 的建目录与侧栏新会话/续接能力。
- **Done Predicate**: `cargo test -p fleet-daemon` 与 `scripts/check` 通过；三端各自通过隔离夹具与本地协议测试证明新目录创建、侧栏项目挂载、会话可见性与同一线程续接。
- **Evidence**: crates/fleet-daemon/src/tri_hosts.rs;crates/fleet-daemon/src/host_adapter.rs;crates/fleet-daemon/tests/tri_hosts.rs;.scratch/pstack-relay/checks/20261105T023924.621791Z/receipt.json

### [x] Slice L: 四端 /poteto-mode (pstack-relay) 跨 Agent 自我开发闭环验收
- **Playbook**: `Shipping`
- **Outcome**: 通过 `fleet-cli` 向 OpenAI Dots、Cindy、DeepSeek Harness、Antigravity Desktop 四个桌面宿主执行跨 Agent `/poteto-mode` (`pstack-relay`) 自我开发投递验收，留存四端侧栏会话可见证据、目录创建证据与收据闭环。
- **Done Predicate**: `cargo check --workspace`、`cargo test --workspace` 与 `scripts/check` 通过；四端收据与验证证据齐全并完成独立审查。
- **Evidence**: crates/fleet-daemon/src/relay_self_dev.rs;crates/fleet-cli/src/main.rs;crates/fleet-cli/tests/poteto_relay_four_hosts.rs;.scratch/pstack-relay/slice-l-four-hosts/four-hosts-closure-bundle.json;.scratch/pstack-relay/slice-l-receipt.md;.scratch/pstack-relay/slice-l-final-audit.md;.scratch/pstack-relay/checks/20261105T025915.381857Z/receipt.json

### [x] Slice M: 收尾 Maitra 旧挂起通道、放宽 DeepSeek 思考模式超时并更新未知账目单次重试授权
- **Playbook**: `Feature`
- **Outcome**: 终止挂起的旧 `codex exec` 进程以保障单通道投递；将生产 runtime `15c46390` 的 `providers.toml` 中 `deepseek-default` (`thinking = "enabled"`, `concurrency_limit = 1`) 客户端超时从 `120` 秒提升至 `600` 秒；按用户明确授权更新 `qa/second-leg-authorization.json` 与 `provider_usage.py`，在完整保留全部 29 条原调用记录（含 `call-00000013`、`call-00000027`、`call-00000029`）的前提下，对 `chapter_001 / reflect / chunkIndex=1` 与 `chapter_002 / first-pass / chunkIndex=1` 各开放 1 次续跑重试。
- **Done Predicate**: `~/.cache/glaux-book/runtimes/15c46390fad61027006032ccd452a4a3b1d90e8e/.venv/bin/python ~/Projects/_status/maitra-zh-epub-20261104/evidence/check-second-leg-authorization.py` 与 `scripts/check` 通过；原账本 29 条记录字节不变且零网络调用。
- **Evidence**: docs/pstack-relay.md;~/Projects/_status/maitra-zh-epub-20261104/evidence/check-second-leg-authorization.py;~/Projects/_status/maitra-zh-epub-20261104/receipt.md;.scratch/pstack-relay/checks/20261105T044112.870592Z/receipt.json

### [x] Slice N: 经 fleet-cli 向 DeepSeek Harness (DSH) 分派 glaux-book 会话并完成 Maitra 全文翻译
- **Playbook**: `Feature`
- **Outcome**: 通过 `fleet-cli hosts create-session --host deepseek-harness`（携带 `--dsh-root ~/.dsh --dsh-bin ~/.local/bin/dsh --project-dir ~/Projects/glaux-book`）向 DeepSeek Harness 分派续跑任务，在 DSH 桌面侧栏挂载会话并从既有断点续跑 `061_不受著作权保护的代码_Maitra` 剩余 28 次 `deepseek-flash` (`thinking = "enabled"`) 翻译调用，直至 `chapter_001` 与 `chapter_002` 两个单元全部完成。
- **Done Predicate**: `~/.dsh/storages/workspace.json` 与 `~/.dsh/storages/session_projcache/sessions/<session_id>.json` 落盘可查；`~/Library/Application Support/glaux-book/zh-Hans/061_不受著作权保护的代码_Maitra/chapters/translated/chapter_001.md` 与 `chapter_002.md` 均生成且 `qa/translation-run-report.json` 中 2/2 单元状态为 `completed`。
- **Evidence**: ~/.dsh/storages/workspace.json;~/.dsh/storages/session_projcache/sessions/session-5430ab10-379d-4b28-a5b1-aa99cfad563e.json;~/Projects/_status/maitra-zh-epub-20261104/evidence/slice-n-summary.json;~/Projects/_status/maitra-zh-epub-20261104/evidence/dsh-desktop-sidebar-glaux-book.png;.scratch/pstack-relay/checks/20261105T055244.597955Z/receipt.json

### [x] Slice O: 在 DSH 会话下完成 Maitra 译文导入、源文对照审校、中文 EPUB 构建与 Agent Acceptance
- **Playbook**: `Feature`
- **Outcome**: 在 DSH 的 `glaux-book` 会话中继续执行译文导入 (`translate import`)、源文与 351 条注释绑定核验、阅读版中文 EPUB 构建 (`output/reading/Sonali Maitra_2026_不受著作权保护的代码.epub`) 及 `glaux-book` Agent Acceptance 验收。
- **Done Predicate**: `test -f "$HOME/Library/Application Support/glaux-book/zh-Hans/061_不受著作权保护的代码_Maitra/output/reading/Sonali Maitra_2026_不受著作权保护的代码.epub"` 通过，记录 EPUB 绝对路径与 SHA-256，且项目 QA / Agent Acceptance 证据齐全。
- **Evidence**: ~/Projects/_status/maitra-zh-epub-20261104/receipt.md;~/Projects/_status/maitra-zh-epub-20261104/evidence/agy-desktop-vis-glaux-book.json;~/Projects/_status/maitra-zh-epub-20261104/evidence/dsh-desktop-sidebar-glaux-book.png;.scratch/pstack-relay/checks/20261105T063504.739850Z/receipt.json

### [x] Slice P: 经 DSH 完成 Maitra 中文 EPUB 挂回 Zotero PQ9G5JUI、字节回读核验与全链路收口
- **Playbook**: `Shipping`
- **Outcome**: 在 DSH 会话中将验收通过的中文 EPUB 作为 imported attachment 挂回 Zotero 父条目 `PQ9G5JUI`（严禁使用废条 `XW32DCWX`，严禁推送 Kobo/Boox），回读新附件 key 并比对落盘文件与产出 EPUB 的 SHA-256 字节一致，更新 `~/Projects/_status/maitra-zh-epub-20261104/receipt.md` 为 `DONE`。
- **Done Predicate**: `zotero-cli` 回读 `PQ9G5JUI` 下新增中文 EPUB 附件 key 且 SHA-256 与构建产物一致；`~/Projects/_status/maitra-zh-epub-20261104/receipt.md` 与 `scripts/check` 全部通过。
- **Evidence**: ~/Projects/_status/maitra-zh-epub-20261104/receipt.md;~/Projects/_status/maitra-zh-epub-20261104/evidence/agy-desktop-sidebar-glaux-book.png;~/Projects/_status/maitra-zh-epub-20261104/evidence/dsh-desktop-sidebar-glaux-book.png;.scratch/pstack-relay/checks/20261105T063504.739850Z/receipt.json

### [x] Slice Q: 补齐第 5 桌面宿主 ZCode (`zcode`) 侧栏适配器与开放外部 Agent 注册清单 (`agents.d`)
- **Playbook**: `Feature`
- **Outcome**: 在 `fleet-proto` 将 `DesktopHostKind` 扩充为五大桌面宿主（`openai-dots`、`cindy`、`deepseek-harness`、`antigravity-desktop`、`zcode`），并新增 `ExternalAgentManifest` 开放插件契约；在 `fleet-daemon` 实现 `ZcodeAdapter`（对接 `~/.zcode/v2/tasks-index.sqlite` 的 `projects`/`project_roots`/`tasks` 表、`~/.zcode/cli/db/db.sqlite` 的 `session`/`message` 表及 `zcode --cwd <dir> --surface desktop --json`）与 `ExternalAgentRegistry`（支持 `<state_root>/agents.d/<id>.json` 注册与自定义命令派活）。
- **Done Predicate**: `cargo test -p fleet-proto` 与 `cargo test -p fleet-daemon` 通过；测试覆盖五大桌面宿主的项目建目录、侧栏挂载、会话新建与同线程续问，以及外部开源 Agent 清单注册与派活。
- **Evidence**: crates/fleet-proto/src/host.rs;crates/fleet-daemon/src/tri_hosts.rs;crates/fleet-daemon/src/host_adapter.rs;crates/fleet-daemon/tests/tri_hosts.rs;.scratch/pstack-relay/checks/20261105T075237.391994Z/receipt.json

### [x] Slice R: 落地 Reason Network 级 CLI 与 MCP 开放入口 (`fleet-cli who/send/mcp/setup/doctor/agents`)
- **Playbook**: `Feature`
- **Outcome**: 在 `fleet-cli` 保持 `sessions`、`hosts`、`relay` 完全向后兼容的前提下，新增 `who`（枚举五大桌面宿主、会话目录与外部 Agent 状态）、`send`（支持 `--to <host>[/<slug>]` 一键建目录、挂载左侧栏项目并创建或续接会话）、`mcp`（标准 JSON-RPC 2.0 stdio MCP 服务，暴露 `list_team`、`send_message`、`message_status`、`register_agent`）、`setup`、`doctor` 与 `agents` 子命令。
- **Done Predicate**: `cargo test -p fleet-cli` 通过；集成测试验证五大桌面宿主的 `/poteto-mode` 接力、`fleet-cli who/send/setup/doctor/agents` 及 `fleet-cli mcp` JSON-RPC 2.0 协议交互。
- **Evidence**: crates/fleet-cli/src/main.rs;crates/fleet-cli/tests/poteto_relay_four_hosts.rs;.scratch/pstack-relay/checks/20261105T075237.391994Z/receipt.json

### [x] Slice S: 升级仓库对外开源文档与架构设计 (`README.md`、`DESIGN.md`、`docs/desktop-rea-dossier.md`)
- **Playbook**: `Feature`
- **Outcome**: 编写对外开放的 `README.md` 与 `DESIGN.md`，阐明 `sleep2agi/agent-network` 与 `reason-machines/reason-network` 双源合一架构、五大桌面宿主侧栏会话挂载契约、标准 MCP 接入与第三方开源 Agent 插件规范，同时保留 `caller-routing.md` 与历史兼容段落以通过全部 Node 路由与文档契约测试。
- **Done Predicate**: `npm test`、`node scripts/lint-routes.mjs`、`node scripts/guard.mjs private` 与 `node scripts/guard.mjs landscape` 全部通过。
- **Evidence**: README.md;DESIGN.md;docs/rust-engine.md;docs/desktop-rea-dossier.md;.scratch/pstack-relay/checks/20261105T075237.391994Z/receipt.json

### [x] Slice T: 五大桌面 Agent + 外部开源 Agent 互通闭环验证、合入 `main` 与零遗留收口
- **Playbook**: `Shipping`
- **Outcome**: 运行完整 `scripts/check` 与 `python3 scripts/pstack-relay selftest` 验证五大桌面宿主与外部 Agent 派活闭环，将验证通过的提交合入 `main`，清理临时 worktree 与分支，确保零 worktree 遗留。
- **Done Predicate**: `scripts/check` 退出 0，`git worktree list` 无遗留临时 worktree，`.scratch/pstack-relay/decisions.tsv` 与 `docs/pstack-relay.md` 证据闭环。
- **Evidence**: docs/pstack-relay.md;.scratch/pstack-relay/checks/20261105T075237.391994Z/receipt.json





### [x] Slice U: 修复真实 Runtime 自动探测与彻底废除生产 Accepted 影子降级
- **Playbook**: `Feature`
- **Outcome**: 在 `crates/fleet-cli/src/main.rs` 的 `parse_runtime_options` 中，为 `zcode`、`deepseek-harness`、`openai-dots` 和 `cindy` 补齐像 `antigravity` 一样的本地默认路径与 socket 自动探测逻辑；若探测不到或无法连接则直接报错拒绝投递，严禁在生产派活与 MCP 中静默退回 `<state_root>/<host>-state.json` 伪造 `tier: accepted`。
- **Done Predicate**: `fleet-cli send --to zcode/agent-fleet --prompt "test"` 在未显式传 `--zcode-root` 时自动命中本机 `~/.zcode` 与 `~/bin/zcode`，不再生成虚假会话 `zcode-session-1`；`./scripts/check` 绿灯。
- **Evidence**: crates/fleet-cli/src/main.rs;crates/fleet-daemon/src/host_adapter.rs;crates/fleet-cli/tests/poteto_relay_four_hosts.rs;.scratch/pstack-relay/checks/20261105T161019.738571Z/receipt.json

### [x] Slice V: 修复真实 ZCode 表结构对齐与消除 SQL 注入写死时间戳
- **Playbook**: `Bug fix`
- **Outcome**: 对照本机真实 `~/.zcode/v2/tasks-index.sqlite` 与 `~/.zcode/cli/db/db.sqlite` 结构（`hosts` 表无 `label` 列等），重构 `tri_hosts.rs` 中 `ensure_zcode_sidebar_project` 与 `attach_zcode_session_to_tasks_index` 的 SQL 语句，使用当前真实毫秒时间戳替换写死的 `1_762_329_600_000`；优先复用 ZCode 自身 CLI 机制完成项目与任务挂载。
- **Done Predicate**: `fleet-cli send --from antigravity-desktop/agent-fleet --to zcode/agent-fleet --prompt "ping"` 成功返回 `tier: thread-visible` 且 ZCode 数据库内真实存在该任务；`./scripts/check` 绿灯。
- **Evidence**: crates/fleet-daemon/src/tri_hosts.rs;crates/fleet-daemon/src/host_adapter.rs;.scratch/pstack-relay/checks/20261105T162751.448242Z/receipt.json

### [x] Slice W: DSH 消除进程强杀、优先 Gateway 模式并打通全链路模型透传
- **Playbook**: `Bug fix`
- **Outcome**: 消除 `tri_hosts.rs` 中 `stop_running_dsh_desktop_app_if_matching_home` 的 `kill -9` 强杀逻辑；DSH 本地默认适配优先复用活跃 HTTP 网关或无头执行，并在 `run_dsh_headless_command` 及各适配器中将 `--model` / `model` 严格透传到底层（如 DSH、ZCode、Antigravity），消除写死 ISO 时间戳 `2026-11-05T04:00:00.000Z`。
- **Done Predicate**: 向 DSH 派发任务不导致运行中的 `/Applications/DeepSeek Harness.app` 进程重启（PID 保持不变），且 `--model` 参数在会话回执与执行日志中体现；`./scripts/check` 绿灯。
- **Evidence**: .scratch/pstack-relay/checks/20261105T165829.356698Z/receipt.json

### [x] Slice X: 对等信封升级会话级寻址 (host/slug#session) 与双向防串信投递
- **Playbook**: `Feature`
- **Outcome**: 将 `<fleet-message>` 的 `from` 扩展支持 `host/slug#session_id` 格式；在 `dispatch_unified_message` 中若检测到 `reply_to` 或带会话地址的回复，优先调用适配器的 `deliver_turn` 精准送回发件方原会话，而非随机新建或落入同项目旧会话；`FleetMailboxStore` 收发信箱按会话隔离，先投递成功再持久化已发记录。
- **Done Predicate**: DSH 或 ZCode 针对 Antigravity 派发任务的 reply 精准追加至原 Antigravity `conversation_id`，不再创建悬空孤儿会话，mailbox 不发生跨会话抢信；`./scripts/check` 绿灯。
- **Evidence**: .scratch/pstack-relay/checks/20261105T174334.800774Z/receipt.json

### [x] Slice Y: 接入 Herdr 派活清单与 doctor 真实环境契约探测
- **Playbook**: `Feature`
- **Outcome**: 编写 `<state_root>/agents.d/herdr.json` 并注册到 `ExternalAgentRegistry`，通过 `herdr agent prompt/read` 接入 Herdr 终端窗格派活；在 `fleet-cli doctor` 中增加 `--live` 或默认真实探测，只读检查五大桌面宿主数据库模式（schema）、进程与 socket 可连通性，消除写死的 `"ready": true`。
- **Done Predicate**: `fleet-cli send --to herdr/agent-fleet --prompt "echo 1"` 成功分发至 Herdr；`fleet-cli doctor` 真实反映本机 ZCode、DSH、AGY 数据库列对齐状态；`./scripts/check` 绿灯。
- **Evidence**: .scratch/pstack-relay/checks/20261105T182412.690094Z/receipt.json
