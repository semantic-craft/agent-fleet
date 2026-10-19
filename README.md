# agent-fleet

让手机上的 ChatGPT dots（以及 Grok Bot 等任何支持 OAuth MCP 的助手）直接给你自己电脑上的编码 agent 派活：
Codex、DSH 桌面版、ZCode 桌面版，以及在 Herdr 终端里运行的 Antigravity（agy）、Codex CLI 和 `dsh headless`。

## 目的

1. **一个入口派活**：主入口是 ChatGPT dots，Grok Bot 等其他 MCP 助手是辅助，都可以直接给本地 agent 派活并拿回结果。本地 agent 包括 DSH、ZCode、Codex 桌面版，以及 Herdr 里的 agy、codex 和 dsh headless。
2. **以上游为底座**：架构完全按 [Xuanwo/agenvo](https://github.com/Xuanwo/agenvo) 的设计来，不另起一套。
3. **只做边际补丁**：上游已覆盖的部分原样使用；只补上游没覆盖、而我们确实需要的部分（目前是 DSH 与 ZCode 桌面版），而且一旦上游覆盖了就删掉。
4. **可复用、对 agent 友好**：没有对话记忆的 agent 读完本仓库，也能独立重建整套系统，并做出和原设计一致的决策。

判断有没有偏离目的：`fleet.mjs check --dispatch` 通过，且 [prompts/acceptance-codex.md](prompts/acceptance-codex.md) 的界面验收通过。

本仓库是这套基础设施的**设计原件与重建入口**：架构、决策、可复用脚本、模板和提示词。它不保存任何个人地址、账号或密钥；
这些值放在仓库外的私有 overlay（见 [fleet.example.json](fleet.example.json)）。

```
ChatGPT / dots / Grok Bot ──MCP(OAuth, HTTPS)──► Agenvo relay（Cloudflare Worker，自有域名）
                                                    ▲ WSS（connector 主动外连，电脑不开入站端口）
        ┌───────────────────────────────────────────┼─────────────────────────────┐
  每台 Mac：agenvo-codex-app-server ─► codex app-server（launchd 常驻，共用 ~/.codex，Desktop 可见）
            agenvo-herdr            ─► herdr server（headless）─► pane 里的 agy / codex / dsh headless
            agenvo-dsh              ─► DSH 桌面版 Host 的原生会话接口（插件 native bridge，Unix socket）
            agenvo-dispatch         ─► ZCode 接收端 ─► 桌面 app 里的可见会话
```

## 组成

| 层 | 来源 | 本仓库做什么 |
| --- | --- | --- |
| relay 与 Codex/Herdr connector | 上游 [Xuanwo/agenvo](https://github.com/Xuanwo/agenvo)，原样使用 | 记录部署方式与验收 |
| `dsh` / `zcode` connector | agenvo 的小补丁分支（新增两个实例 kind、原生的 `apps/dsh` 和 ZCode 用的 `apps/dispatch`），见 [ADR-0002](docs/decisions/0002-minimal-agenvo-fork.md) | 固定分支、说明何时可以去掉 |
| 桌面插件与接收端 | [dsh-grokbot-dispatch](https://github.com/semantic-craft/dsh-grokbot-dispatch)（DSH 原生桥接，两台 Mac 都用它）、[zcode-dispatch](https://github.com/semantic-craft/zcode-dispatch)（`source: "agenvo"`、`runner: "app"`） | 规定契约 |
| 机器清单、context、launchd、验收 | 本仓库 | [scripts/fleet.mjs](scripts/fleet.mjs)、[templates/](templates/)、[prompts/](prompts/) |

## 从哪里开始

- 想理解为什么这样设计：[docs/architecture.md](docs/architecture.md)、[docs/decisions/](docs/decisions/)。
- 新 Mac 加入（例如再添一台 Mac mini）：[docs/new-mac.md](docs/new-mac.md)；完整命令：[docs/runbook.md](docs/runbook.md)。
- 想知道本机 `~/Projects` 里那么多相似目录各是什么：[docs/landscape.md](docs/landscape.md)。
- 给 ChatGPT、dot、Grok、Codex 的现成提示词：[prompts/](prompts/)；让 agent 经 Agenvo 把活交给其他 agent 的技能：[skills/delegate-via-agenvo](skills/delegate-via-agenvo/SKILL.md)。
- 例行维护和跟进上游：[docs/maintenance.md](docs/maintenance.md)。
- Agent 接手维护：先读 [AGENTS.md](AGENTS.md) 和 [lessons.md](lessons.md)。

## 检查

```sh
npm install
git config core.hooksPath .githooks       # 一次性：提交前自动跑 guard
npm test                                  # 配置解析、context 渲染与私有值检查
node scripts/fleet.mjs doctor             # 本机 connector 与原生服务状态
node scripts/fleet.mjs check              # 经 relay 走一遍 OAuth + MCP，列出所有实例，结束时撤销测试授权
node scripts/fleet.mjs check --dispatch   # 另给每个 dsh/zcode 实例派一个只读任务并读回结果（dsh 等到原生 turn/end）
node scripts/fleet.mjs upstream           # fork 与上游的差距，以及是否到了去掉 fork 的时候
```

License: Apache-2.0。
