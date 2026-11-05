# 四端桌面建目录与侧栏会话投递 REA 观察档案

## Overview

本档案记录对 Antigravity Desktop (`/Applications/Antigravity.app` 与 `~/.local/libexec/agy/agy`) 及 OpenAI Dots / Codex Desktop (`/Applications/ChatGPT.app` 与 `~/.codex/`) 的静态二进制、Electron ASAR、SQLite 状态库与本地 RPC 协议观察结果，并与本仓已接通的 DeepSeek Harness (DSH) 与 Cindy 汇合为统一的四端桌面投递契约。

调查目标是将创建项目目录 (`~/Projects/<slug>`)、挂载桌面侧栏项目 (`project_id` / `workspace_uris`)、新建或续接侧栏可见对话 (`conversation_id` / `thread_id`) 收敛为可证伪的 Rust `DesktopHostAdapter` 接口，支撑四端通过 `/poteto-mode` (`pstack-relay`) 跨 Agent 自我开发。

## Key Concepts

- **REA Evidence Chain**: 本次观察由 `reverse-engineer-anything` (REA) 生成离线可复核证据，包含 Electron Fuses、JavaScript 模块与 IPC 事实表、Go 构建元数据与 SQLite 数据库 schema 及样本行。
- **Hub Language Server (Antigravity Desktop)**: `/Applications/Antigravity.app` 的 Electron 主进程不直接渲染本地静态页面，而是拉起 `/Applications/Antigravity.app/Contents/Resources/bin/language_server` (`--subclient_type hub`)，由该本地服务提供 HTTPS Web UI 与 Connect/gRPC-Web 接口。
- **App-Server State Store (Codex Desktop)**: `/Applications/ChatGPT.app` (Owl/Electron 壳) 与 `codex app-server` 共享 `~/.codex/state_5.sqlite`、`~/.codex/queue_1.sqlite` 与 `~/.codex/.codex-global-state.json`。侧栏项目由 `projects` 与 `project_roots` 表及 `.codex-global-state.json` 共同维护。
- **Sidebar Visibility Condition**: 单纯调用 CLI (`agy -p` 或 `codex exec`) 默认落入无项目或 CLI 专用桶 (`default-cli-project` 或 `project_id = NULL`)。要在桌面侧栏指定项目下可见，投递必须绑定目标项目的 `project_id` 与工作区根路径。

## REA 证据总表

| 目标产物 | 路径 (脱敏) | SHA-256 摘要 | REA 工具与 Evidence ID |
| --- | --- | --- | --- |
| Antigravity Electron Framework | `/Applications/Antigravity.app/Contents/Frameworks/Electron Framework.framework/Versions/A/Electron Framework` | `39b05d3eb61159157f8759f5c39b09d426842873006350b486b666cb413403ad` | `inspect_electron_fuses`: `ev_998000f3594732126d2d27b49710ee7f5f930f861d47d132356b8843ab072203` |
| Antigravity `app.asar` | `/Applications/Antigravity.app/Contents/Resources/app.asar` | `290e1b7a1caa430a0c5367c7b433e8d284e2ca5d0b305e91e5ce8ec926f43283` | `analyze_javascript_application`: `ev_3608cb1a3552b00d67828442c6ac1299198443ceadaa419cf2409259d50904d2`, `ev_6ff6fda50bca782ae3e9c49b704b06bcfeb1030951d7c15c9fee932e76c9541f` |
| Antigravity `dist/languageServer.js` | `app.asar/dist/languageServer.js` | `6f671aaf1a6812e75bb692e1def7a0b6eb3b2ac27b12cdc066e0b7f39ac723c4` | `inspect_analysis_view`: `ev_890c87228893364cbc9ea60b17590f9f280d4c787bb8c481010528f3c4d45d2b`, `ev_45ffd98f4f05c27b7bb984bf7ab0ff9278882f85b2889a94f69af355fb29908d` |
| Antigravity IPC / Bridge Facts | `app.asar` (270 facts: 74 IPC channels, 62 handlers) | `290e1b7a1caa430a0c5367c7b433e8d284e2ca5d0b305e91e5ce8ec926f43283` | `inspect_analysis_view`: `ev_7575c8e6d7ec3e3d052a3fe9252fae9137ed43af555b140e9fdd62cae705ad01`, `ev_666023d13af32f2322160cfa45e51f0647949befb79dc89112aab64fe363207e`, `ev_92be141f873f9450bbf479cc7b2446108d6445049db8b4d89c60d226fe84b1b3` |
| Antigravity `language_server` | `/Applications/Antigravity.app/Contents/Resources/bin/language_server` | `fb6e0fc3a1e091e773a3e91ce3e5a4dc93a5f0d30bf305a1ddd70008a2a26a74` | `inspect_go_binary`: `ev_d3b301eb06207bcf57ef1806283cb162a059a41c472e2a909021dd17124e7841` |
| Antigravity Standalone CLI `agy` | `~/.local/libexec/agy/agy` | `e61679b8ba54abae9a56534208e21f65e1dede6e4eb0144dbd64535f42d07724` | `inspect_go_binary`: `ev_192ecb1f31a6c0ef9af7485def21887514134d2d503994742da0b97b0a0ec677` |
| Antigravity `conversation_summaries.db` | `~/.gemini/antigravity/conversation_summaries.db` | `098d0b9a55b7ebd0708d758b02b9714e87646867c70306e2d44ab00c85021362` | `inspect_sqlite_database`: `ev_9d0886ee080bed67ef294e6654627714788925944c975eed9e8d2ef6e0c9df05`, `ev_8e555506f223cb2b3a5ed6069baa1e7a0dfa7d915db242f3f7f3382d4ec889a8` |
| Codex Desktop `state_5.sqlite` | `~/.codex/state_5.sqlite` | `35164a02c8f0f852ab1b5a7d26a600f72207b9e6cebe576cab1eb53c5092ae48` | `inspect_sqlite_database`: `ev_02af4823a9944693e9f3f2d845fcd9028775a81842995fe182137b12ab042eab`, `ev_343a862bd79c1f58761635b955a0c823254dcc69b07567895e793f01f0426cd8`, `ev_945a02727a11f62ac76efdc53f70cfd4991c2f76309b6a8d82d7d47a38b4bf2c` |
| Codex Desktop `queue_1.sqlite` | `~/.codex/queue_1.sqlite` | `5ffa745fc4869650357e6b0b81767fda0104430ed72e9ba69ab9ff5793974ee2` | `inspect_sqlite_database`: `ev_9ea139b98ac05d48c6baa2e6884df0f25b4458f004112fa8dea8365ba81cbaa4` |

## How It Works

### 1. Antigravity Desktop (`antigravity desktop`)

#### 1.1 宿主启动与本地守护发现 (`ev_45ffd98f4f05c27b7bb984bf7ab0ff9278882f85b2889a94f69af355fb29908d`)

`app.asar/dist/main.js` 通过 `dist/languageServer.js` 启动内嵌 Go 二进制 `/Applications/Antigravity.app/Contents/Resources/bin/language_server` (`go1.28-20261025-RC00`, `ev_d3b301eb06207bcf57ef1806283cb162a059a41c472e2a909021dd17124e7841`)，固定传入参数：

```text
--standalone --override_ide_name antigravity --subclient_type hub --https_server_port <port> --csrf_token <token> --app_data_dir antigravity --enable_sidecars
```

`language_server` 启动后在 `~/.gemini/antigravity/daemon/hub_<httpPort>.json` 写入发现文件，结构包含 `pid`、`httpsPort`、`httpPort`、`lsVersion` 与 `csrfToken`（客户端调用时置于请求头 `x-codeium-csrf-token`）。活跃会话锁保存在 `~/.gemini/antigravity/presence/<conversation_id>.lock`。

#### 1.2 项目与侧栏会话存储 (`ev_9d0886ee080bed67ef294e6654627714788925944c975eed9e8d2ef6e0c9df05`)

- **SQLite 摘要库 (`~/.gemini/antigravity/conversation_summaries.db`)**:
  单表 `conversation_summaries` 含 21 列：
  `conversation_id` (PRIMARY KEY)、`title`、`preview`、`step_count`、`last_modified_time`、`workspace_uris` (JSON 数组，如 `["file:///.../Projects/agent-fleet"]`)、`status` (`CASCADE_RUN_STATUS_IDLE` / `CASCADE_RUN_STATUS_RUNNING`)、`source` (桌面 UI 会话为空字符串 `""`，独立 CLI 会话为 `"CORTEX_TRAJECTORY_SOURCE_CLI"`)、`project_id` (侧栏项目 UUID，或 `"default-cli-project"` / `"outside-of-project"`)、`agent_name`、`parent_conversation_id`、`nesting_depth`、`battle_id`、`winning_conversation_id`、`not_fully_idle`、`killed`、`last_user_input_time`、`last_user_input_step_index`、`app_data_dir`、`raw_summary` (BLOB)、`group_id`。
- **项目元数据与 UI 布局**:
  `~/.gemini/antigravity/agyhub_summaries_proto.pb` 维护 `project_id` 到 `workspace_uris` (`file://` URI) 的映射。
  `~/Library/Application Support/Antigravity/app_storage.json` 维护 `projectsOrder`、`lastCreatedProjectId`、`new-convo-last-selected-project` 与 `antigravity-multi-conversation-layout-v3-<project_id>`。

#### 1.3 创建项目与会话投递通道

Antigravity 提供两条互补的原生通道：

1. **Hub `LanguageServerService` Connect RPC 通道**（面向已运行的 Antigravity Desktop 守护进程 `~/.gemini/antigravity/daemon/hub_<port>.json`）：
   - 目录与项目方法：
     - `/exa.language_server_pb.LanguageServerService/CreateDirectory`
     - `/exa.language_server_pb.LanguageServerService/CreateProject`
     - `/exa.language_server_pb.LanguageServerService/CreateScratchProjectFolder`
     - `/exa.language_server_pb.LanguageServerService/ListProjects`
     - `/exa.language_server_pb.LanguageServerService/ReadProject`
     - `/exa.language_server_pb.LanguageServerService/ReadProjects`
     - `/exa.language_server_pb.LanguageServerService/UpdateProject`
     - `/exa.language_server_pb.LanguageServerService/AddTrackedWorkspace`
     - `/exa.language_server_pb.LanguageServerService/ProjectUpdatesStream`
   - 会话（Cascade）生命周期与聚焦方法：
     - `/exa.language_server_pb.LanguageServerService/StartCascade`
     - `/exa.language_server_pb.LanguageServerService/SendUserCascadeMessage`
     - `/exa.language_server_pb.LanguageServerService/SendAllQueuedMessages`
     - `/exa.language_server_pb.LanguageServerService/CancelCascadeInvocation`
     - `/exa.language_server_pb.LanguageServerService/WaitForConversationFullyIdle`
     - `/exa.language_server_pb.LanguageServerService/GetCascadeTrajectory`
     - `/exa.language_server_pb.LanguageServerService/GetAllCascadeTrajectories`
     - `/exa.language_server_pb.LanguageServerService/SmartFocusConversation`
     - `/exa.language_server_pb.LanguageServerService/StreamCascadeSummariesReactiveUpdates`
2. **CLI 与 ACP 通道** (`~/.local/libexec/agy/agy`, `ev_192ecb1f31a6c0ef9af7485def21887514134d2d503994742da0b97b0a0ec677`)：
   - 显式传 `--new-project` 或 `--project <ID or name>` 与 `--add-dir <dir>` 可将 CLI 会话绑定到指定侧栏项目，避免落入 `default-cli-project`。
   - 显式传 `--conversation <ID>` 续接既有 `conversation_id`。
   - `agy acp` 启动标准 Agent Client Protocol (ACP) stdio 服务。

### 2. OpenAI Dots / Codex Desktop (`openai dots`)

#### 2.1 宿主结构与状态库 (`ev_02af4823a9944693e9f3f2d845fcd9028775a81842995fe182137b12ab042eab`)

`/Applications/ChatGPT.app` 采用 Owl 运行时包装 Electron (`Contents/Resources/owl-electron-app.json`, `runtimeArchiveSha: 7638714ac828ef6845baceecf13368b622e6352460e28d264ebff53986504137`)，底层与 `codex` 二进制 (`0.166.0-aarch64-apple-darwin`) 共享 `~/.codex/` 状态目录：

- **`~/.codex/state_5.sqlite` (`ev_02af4823a9944693e9f3f2d845fcd9028775a81842995fe182137b12ab042eab`)**:
  - `projects` 表 (`ev_343a862bd79c1f58761635b955a0c823254dcc69b07567895e793f01f0426cd8`)：`id` (UUIDv7 TEXT PK)、`name`、`metadata` (JSON TEXT)、`position`、`created_at_ms`、`updated_at_ms`。
  - `project_roots` 表 (`ev_945a02727a11f62ac76efdc53f70cfd4991c2f76309b6a8d82d7d47a38b4bf2c`)：`project_id` (FK -> `projects.id`)、`position`、`path` (绝对路径，如 `~/Projects/<slug>`)。
  - `threads` 表（43 列）：`id` (UUIDv7 PK)、`rollout_path` (`~/.codex/sessions/YYYY/MM/DD/rollout-...-<thread_id>.jsonl`)、`cwd`、`title`、`source` (`"vscode"` / `"exec"` / `"cli"`)、`originator` (`"Codex Desktop"` / `"turing-codex-client"` / `"codex_exec"`)、`preview`、`recency_at_ms`、`project_id` (FK -> `projects.id`)、`thread_section_id` 等。
  - 侧栏可见索引：`idx_threads_visible_recency_at_ms` 与 `idx_threads_project_recency` 要求 `archived = 0` 且 `preview <> ''`，按 `project_id` 与 `recency_at_ms DESC` 排序。
- **`~/.codex/queue_1.sqlite` (`ev_9ea139b98ac05d48c6baa2e6884df0f25b4458f004112fa8dea8365ba81cbaa4`)**:
  - `queued_items` (`id`, `thread_id`, `payload_json`, `queue_order`, `created_at_ms`, `updated_at_ms`) 与 `queued_thread_revisions`，维护线程待执行队列。
- **`~/.codex/.codex-global-state.json`**:
  - 维护 Electron 侧栏视图状态：`local-projects`、`project-order`、`selected-project`、`thread-project-assignments`、`sidebar-project-thread-orders` 与 `app-server-project-id-by-legacy-project-id-by-host`。

#### 2.2 创建项目与侧栏会话投递通道

- **`codex app-server` JSON-RPC v2 通道**:
  `codex` 内置 `app-server` 提供完整的项目与线程 JSON-RPC 方法集：
  - 项目与目录：`fs/createDirectory`、`project/list`、`project/read`、`project/create`、`project/import`、`project/update`、`project/move`、`project/delete`。
  - 线程与轮次：`thread/start`、`thread/resume`、`thread/read`、`thread/list`、`thread/metadata/update`、`thread/name/set`、`thread/queue/add`、`thread/queue/start`、`turn/start`、`turn/steer`、`turn/interrupt`。
- **Deep Link 与窗口激活**:
  - `codex://threads/<thread_id>` 可直接聚焦已存在线程。

### 3. 四端统一适配器映射 (`DesktopHostAdapter`)

| 桌面宿主 | 项目目录创建与侧栏挂载 | 新建侧栏会话 (`create_session`) | 续接/追问已开会话 (`deliver_turn`) | 状态回读与收据证据 |
| --- | --- | --- | --- | --- |
| **OpenAI Dots** (`Codex Desktop`) | `fs::create_dir_all` + `codex app-server` `project/create`（写 `state_5.sqlite` 的 `projects`/`project_roots` 并同步 `.codex-global-state.json`） | `thread/start` (绑定 `cwd` 与 `project_id`) + `turn/start` | 已加载线程走原生 `thread/resume` + `turn/start` (`docs/native-thread-selection.md`)；离线恢复走 `codex exec resume <thread_id>` | `state_5.sqlite` (`threads.project_id`, `rollout_path`) + `codex://threads/<id>` |
| **Antigravity Desktop** (`agy`) | `fs::create_dir_all` + Hub RPC `CreateProject` / `AddTrackedWorkspace` 或 `agy --new-project --project <name> --add-dir <dir>` | Hub RPC `StartCascade` + `SendUserCascadeMessage` (或 `agy --project <id> -p`) | Hub RPC `SendUserCascadeMessage` + `SmartFocusConversation` (或 `agy --conversation <id> --project <id> -p`) | `conversation_summaries.db` (`project_id`, `workspace_uris`, `status`) + `presence/<id>.lock` |
| **DeepSeek Harness** (`DSH`) | `fs::create_dir_all` + DSH Gateway 会话 `cwd` 绑定 (`http://127.0.0.1:41937/v1`) | `POST /v1/sessions` (传入 `cwd: ~/Projects/<slug>`) | `POST /v1/sessions/<id>/turns` (SSE 实时流式推送至桌面侧栏，见 `docs/dsh-session-only.md`) | `GET /v1/sessions/<id>` / `GET /v1/sessions/<id>/events` |
| **Cindy** (`cindy`) | `fs::create_dir_all` + Gateway `sessions.patch` / 工作区目录绑定 | Gateway `sessions.create` / `chat.send` (`sessionKey`) | `plugins/agent-network-cindy` (`cindy_session_send` / `sessions.send`) | Gateway `sessions.get` / transcript JSONL |

## Where Things Live

- **Antigravity Desktop 守护发现**: `~/.gemini/antigravity/daemon/hub_<httpPort>.json`
- **Antigravity 会话摘要 SQLite**: `~/.gemini/antigravity/conversation_summaries.db`
- **Antigravity 项目 Protobuf 索引**: `~/.gemini/antigravity/agyhub_summaries_proto.pb`
- **Antigravity 活跃会话锁**: `~/.gemini/antigravity/presence/<conversation_id>.lock`
- **Codex Desktop 状态 SQLite**: `~/.codex/state_5.sqlite` 与 `~/.codex/queue_1.sqlite`
- **Codex Desktop 侧栏全局状态**: `~/.codex/.codex-global-state.json`
- **Codex 会话 Rollout 日志**: `~/.codex/sessions/YYYY/MM/DD/rollout-<timestamp>-<thread_id>.jsonl`
- **DSH 本地会话网关**: `http://127.0.0.1:41937/v1/sessions` (`docs/dsh-session-only.md`)
- **Cindy 插件桥接实现**: `plugins/agent-network-cindy/`

## Gotchas

1. **Antigravity CLI 默认落入 `default-cli-project`**: 直接调用 `agy -p "..."` 不传 `--project` 时，`conversation_summaries.db` 中 `source` 记为 `"CORTEX_TRAJECTORY_SOURCE_CLI"` 且 `project_id` 记为 `"default-cli-project"`，不会挂入桌面侧栏对应项目树。必须通过 Hub `LanguageServerService` 或显式传 `--project <project_id>` / `--new-project` 绑定项目。
2. **Codex Desktop 侧栏可见性依赖 `preview <> ''` 与 `project_id`**: `state_5.sqlite` 的部分索引 `idx_threads_visible_recency_at_ms` 含 `WHERE preview <> ''`，且项目树下索引 `idx_threads_project_recency` 要求 `archived = 0 AND project_id IS NOT NULL`。仅在磁盘生成空 rollout 文件不会出现在侧栏项目下。
3. **禁止直接裸写活跃 WAL 数据库**: Antigravity 的 `language_server` 与 Codex 的 `app-server` 均以 WAL 模式持有 SQLite 连接。创建项目或会话应优先调用宿主自身的本地协议 (`LanguageServerService` Connect RPC / `agy` CLI、Codex `app-server` JSON-RPC)，由宿主进程自身落盘并触发前端响应式流更新 (`StreamCascadeSummariesReactiveUpdates` / `thread/started` / `project/changed`)。
4. **CSRF 与本地凭据隔离**: `~/.gemini/antigravity/daemon/hub_<port>.json` 中的 `csrfToken` 仅在运行时内存中读取并填入本地 loopback 请求头，严禁写入日志、收据或 Git 追踪文件。
