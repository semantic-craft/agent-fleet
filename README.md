# agent-fleet

**agent-fleet 是融合 [sleep2agi/agent-network](https://github.com/sleep2agi/agent-network) 与 [reason-machines/reason-network](https://github.com/reason-machines/reason-network) 优势的开源多 Agent 协同与桌面侧栏派活引擎**。

它同时提供两层核心能力：
1. **五大桌面 Agent 左侧栏项目会话互派**：支持 **Antigravity Desktop** (`antigravity-desktop`)、**Codex Desktop** (`openai-dots` / `codex`)、**DeepSeek Harness Desktop** (`deepseek-harness` / `dsh`)、**Cindy Desktop** (`cindy`) 与 **ZCode Desktop** (`zcode`) 五个桌面端之间互相派活。任何一端发出的任务都会在目标桌面端左侧栏对应的项目（`~/Projects/<slug>`）下自动挂载并显示真实会话线程，后续追问留在同一会话内。
2. **Reason Network 级开放接入与第三方开源 Agent 扩展**：任何电脑运行本引擎后，外部开源 Agent 均可通过标准 JSON-RPC 2.0 stdio MCP 服务（`fleet-cli mcp`）访问团队目录与互相派活，或通过 `<state_root>/agents.d/<id>.json` 插件清单（`fleet-cli agents register`）零改 Rust 核心接入新 Agent。架构与存储契约详见 [DESIGN.md](DESIGN.md) 与 [docs/rust-engine.md](docs/rust-engine.md)。

体系落在专属的 `~/Projects`（少仓；主 A 可换 Grok、Dots 或 Cindy）。四层、外循环 orchestrator、内循环编排和 worker 互派见 [docs/architecture.md](docs/architecture.md)。PODAS 只是伞名，没有目录。

## 五大桌面 Agent 互通矩阵

| 桌面宿主 (`DesktopHostKind`) | 简写别名 | 左侧栏项目与会话落盘契约 | CLI / MCP 目标示例 |
| --- | --- | --- | --- |
| `antigravity-desktop` | `agy`, `antigravity` | `hub_<port>.json` (`CreateProject` + `StartCascade`) + `conversation_summaries.db` | `antigravity-desktop/my-project` |
| `openai-dots` | `codex`, `codex-desktop` | `state_5.sqlite` (`projects` + `project_roots` + `threads`) + `.codex-global-state.json` | `openai-dots/my-project` |
| `deepseek-harness` | `dsh` | `~/.dsh/storages/workspace.json` + `session_projcache` + 双帧 `session.v4.jsonl.zstd` | `deepseek-harness/my-project` |
| `cindy` | `cindy-desktop` | Cindy Gateway (`sessions.patch` + `sessions.create` + `cindy_session_send`) + JSONL | `cindy/my-project` |
| `zcode` | `zcode-desktop` | `~/.zcode/v2/tasks-index.sqlite` (`projects` + `project_roots` + `tasks`) + `cli/db/db.sqlite` | `zcode/my-project` |

## 快速上手（任何电脑一键搭建互通服务）

```sh
# 1. 编译开源 Rust 引擎（包含 fleet-proto、fleet-hub、fleet-daemon、fleet-cli）
cargo build --workspace

# 2. 初始化工作区并生成标准 MCP 配置与健康检查报告
cargo run -p fleet-cli -- setup --projects-root "$HOME/Projects" --state-root "$HOME/.local/state/agent-fleet"
cargo run -p fleet-cli -- doctor --projects-root "$HOME/Projects" --state-root "$HOME/.local/state/agent-fleet"

# 3. 查看当前五大桌面宿主、左侧栏项目会话与已注册外部开源 Agent
cargo run -p fleet-cli -- who --state-root "$HOME/.local/state/agent-fleet"

# 4. 向任意桌面 Agent 派活（自动创建 ~/Projects/<slug> 并挂载到目标桌面左侧栏项目下）
cargo run -p fleet-cli -- send \
  --to zcode/demo-service \
  --projects-root "$HOME/Projects" \
  --state-root "$HOME/.local/state/agent-fleet" \
  --title "Demo Service Architecture" \
  --prompt "Inspect the repository and run ./scripts/check."

# 5. 注册第三方开源 Agent（零改 Rust 核心即可加入派活网络）
cargo run -p fleet-cli -- agents register \
  --state-root "$HOME/.local/state/agent-fleet" \
  --id open-coder \
  --name "OpenCoder CLI" \
  --command /usr/local/bin/open-coder \
  --args "--cwd,{cwd},-p,{prompt}"
```

### 将任意开源 Agent 接入 MCP (`fleet-cli mcp`)

在 Claude Code、Cursor、OpenCode、ZCode、Codex 或 Antigravity 的 MCP 配置中加入：

```json
{
  "mcpServers": {
    "agent-fleet": {
      "command": "fleet-cli",
      "args": [
        "mcp",
        "--projects-root",
        "/path/to/Projects",
        "--state-root",
        "/path/to/state"
      ]
    }
  }
}
```

接入后即可直接调用 `list_team`、`send_message`、`message_status` 与 `register_agent` 四个标准 MCP 工具，实现跨桌面 Agent 的双向派活与同线程追问。

## 当前派活（本机）

舰队身份、任务账本和跨身份委派只走**官方 Agent Network（anet）0.3.9**。正本：[docs/anet.md](docs/anet.md)。官方文档只认 <https://docs.agentnetwork.org.cn>。

编码 agent 接入官方网络：`anet agents wire` 写入客户端条目，`anet mcp` 提供 stdio MCP（[接入编码 Agent](https://docs.agentnetwork.org.cn/docs/zh-cn/latest/howto/coding-agents.html)、[MCP](https://docs.agentnetwork.org.cn/docs/zh-cn/latest/howto/mcp.html)）。

Herdr 只做 pane 宿主，不是派活控制面。不另造 Herdr 与 ANet 之间的桥。

- 身份：`default`／`turing`／`cindy`；inbound `closed`；只 allow 自有 AID。
- 派活：`env -u ANET_HOME ANET_DATA_DIR=$HOME/.anet ~/.local/bin/anet --id turing delegate <AID> "…" --watch`。
- Cindy 宿主插件：[plugins/agent-network-cindy](plugins/agent-network-cindy/README.md)（显示名 Cindy × Agent Network；工具 `send_to_peer`、`bridge_status`）。

不另造控制面层，不经 CommHub MCP、不经 `anet.sonori.dev`、不经 #90 门铃桥做本机派活。那些是历史路径，见文末「遗留附录」。

dot 的原生平台任务与 anet 并列，不互相替代。DSH、GrokBot 的历史外部路线在 [docs/legacy/caller-routing.md](docs/legacy/caller-routing.md)。旧 Desktop 交接在 [docs/legacy/codex-desktop-handoff.md](docs/legacy/codex-desktop-handoff.md)，不是现行步骤。

Desktop 进窗（窗口里看得见、结果回得来、cwd 等于 `~/Projects` 里的项目）走 Reason，边界见 [docs/reason.md](docs/reason.md)。同一件活不要既走 ANet 又走 Reason。Reason 签到另线，本文不宣称已通。薄技能：[skills/dispatch-desktop-reason](skills/dispatch-desktop-reason/SKILL.md)。

迁移与升级仍先读[兼容来源、模型选择保护及在途恢复门槛](docs/migration-and-upgrades.md)。

## 组成（当前）

| 层 | 来源 | 本仓库做什么 |
| --- | --- | --- |
| A 规矩 | outer-loop-kit | 不复制分流长文和版本钉。体验与四层见 [docs/architecture.md](docs/architecture.md) |
| B 舰队 | 官方 anet 0.3.9（上游）+ `crates/fleet-hub` | [docs/anet.md](docs/anet.md) 与 [docs/rust-engine.md](docs/rust-engine.md)：身份、单通道收据与 Wire v3 关联 |
| C 投递 | Reason（上游）+ `crates/fleet-daemon` | 五大桌面宿主侧栏挂载与会话投递适配器。设计见 [DESIGN.md](DESIGN.md) 与 [docs/reason.md](docs/reason.md) |
| D 壳 | codex-host（上游） | 不实现、不 fork、不抢壳 |
| 自制派活插件 | 本仓库 [plugins/](plugins/README.md) | 一宿主一目录；Cindy 见 [agent-network-cindy](plugins/agent-network-cindy/README.md) |
| 跨仓库约定与教训 | 本仓库 | 根 [AGENTS.md](AGENTS.md)（其他仓库的同步源）、[lessons.md](lessons.md) |
| 机器清单／脚本（遗留部署仍可用） | 本仓库 | [scripts/fleet.mjs](scripts/fleet.mjs)、[templates/](templates/)、[prompts/](prompts/) |

通用派发契约：[docs/dispatch-contract.md](docs/dispatch-contract.md)。ANet 技能：[skills/delegate-via-anet](skills/delegate-via-anet/SKILL.md)。Desktop 进窗选用：[skills/dispatch-desktop-reason](skills/dispatch-desktop-reason/SKILL.md)。跨 Projects 来源与迁移：[docs/anet-alignment.md](docs/anet-alignment.md)。

## 从哪里开始

- 引擎架构设计与五大桌面宿主存储契约：[DESIGN.md](DESIGN.md)、[docs/rust-engine.md](docs/rust-engine.md)、[docs/desktop-rea-dossier.md](docs/desktop-rea-dossier.md)。
- 本机舰队与协议：[docs/anet.md](docs/anet.md)、官方 <https://docs.agentnetwork.org.cn>。
- Desktop 进窗边界（未签到，不宣称已通）：[docs/reason.md](docs/reason.md)。
- 判断 `~/Projects` 目录能不能动：[docs/landscape.md](docs/landscape.md)。
- Agent 接手：跨仓库约定是根 [AGENTS.md](AGENTS.md)。[docs/AGENTS.shared.md](docs/AGENTS.shared.md) 只指向它。本仓再读 [lessons.md](lessons.md)、[docs/architecture.md](docs/architecture.md)。
- 新 Mac／旧 connector 运维（遗留）：[docs/new-mac.md](docs/new-mac.md)、[docs/runbook.md](docs/runbook.md)、文末附录。

## 检查

- [scripts/check](scripts/check)：统一运行 Cargo 全量编译测试、五大桌面宿主与 MCP 集成测试、pstack-relay 自检、Node 契约测试与隐私/路由门禁。
- [scripts/gate.sh](scripts/gate.sh)：只读 PR 合并门禁，`gate.sh <owner/repo> <pr> <sha>`；离线正文检查用 `gate.sh --check-body <文件>`。

```sh
npm install
git config core.hooksPath .githooks       # 一次性：提交前自动跑 guard
./scripts/check                           # Rust 引擎 + Node 契约 + 隐私与路由全量门禁
npm test                                  # 配置解析、context 渲染与私有值检查
node scripts/fleet.mjs doctor             # 本机 overlay 里列出的 launchd 服务是否已加载
```

License: Apache-2.0。


## 遗留附录：Agenvo 与 CommHub

> **历史材料，不是现行派活，不要按本节安装、验收或派活。** 派活只走官方 Agent Network（[docs/anet.md](docs/anet.md)）。CommHub MCP、`anet.sonori.dev`、#90 门铃桥、Agenvo relay 与 `delegate-via-agenvo` 都已退出。

档案（只读）：[docs/legacy/](docs/legacy/README.md)。ADR、旧路由、旧 Desktop 交接、旧验收稿和已删除的 DSH socket 插件都在那里。[docs/caller-routing.md](docs/caller-routing.md) 只留指针。`adapters/dsh` 与 `fleet.mjs release dsh` 已删除，理由见 [docs/legacy/dsh-adapter.md](docs/legacy/dsh-adapter.md)。
