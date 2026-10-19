---
name: delegate-via-agenvo
description: Hand work to another coding agent (DeepSeek Harness/DSH, ZCode, Codex, or agy/codex/dsh headless in Herdr, on this or another Mac) through the Agenvo MCP server, then follow it to a verified result. Use when the user asks to have DSH/ZCode/another agent do something, to run work in parallel on another agent, or to check on or continue such work.
---

# 经 Agenvo 把活交给其他 agent

前提：当前 agent 已连接名为 `agenvo` 的 MCP 服务器，它提供 `search` 和 `execute` 两个工具。连不上时，告诉用户运行 `codex mcp login agenvo`（或在自己的 MCP 设置里重新授权），不要改走其他通道。

## 步骤

1. **发现**：调用 `search {"query": ""}`，列出实例和在线状态，读每个实例的 `context`。它写了这台机器、项目目录、默认权限，以及这个实例适合做什么。
2. **找方法**：用共同术语搜索，例如 `search {"query": "dsh create work context"}`、`"dsh submit input"`、`"dsh read output"`、`"dsh interrupt"`、`"dsh list"`。选定方法后，带上 `includeSchema: true` 再搜一次，拿到参数 schema。不要凭记忆写方法名，不同 connector 的方法名不一样。
3. **派活**：用 `execute` 调 `call({deviceId, instanceId}, method, params)`。
   - 对方没有你的上下文。任务文本要写全：目标、仓库和路径、已知事实、约束、验收方式，以及是否允许提交、推送或开 PR。
   - `project` 或 `cwd` 必须是对方机器上真实存在的目录。
   - 派出后立刻记下返回的 id（session、thread 或 job），并告诉用户。
4. **跟进**：结果不会自动回来，要自己去读。
   - 优先读原生状态和输出（read output 类的方法），隔一段时间再读一次。不要连续高频轮询。
   - 需要追问或纠偏时，用 submit input 发到**同一个**会话，不要另开新会话。只支持 `jobs.*` 的实例（目前是 ZCode）做不到追问，这种情况下新派一个任务，把上一轮的结论写进去。
   - 方向错了就 interrupt，说明原因后重新交代。
5. **验收**：输入"已接受"、agent"空闲"、轮次结束，都不等于任务完成。读到实际结果、对照第 3 步写下的验收方式核对之后，才算完成。最后给用户的汇报要写清：结论、对方做了什么、哪些没做或没验证、需要用户决定的事。

## 选哪个 agent

- 用户点名了就用点名的。
- 要在桌面 app 里看得见、能手动接手：选 DSH 或 ZCode 实例。ZCode 适合本机验证和 iOS 真机测试。
- 要长时间运行、不怕笔记本合盖：选第二台 Mac 上的实例。
- 要多轮对话，或要用 agy、codex CLI：选 Herdr 实例（`agent.start` 起 agent，`agent.prompt` 追加输入）。不要在 Herdr 里起 claude，那里的 claude 有意不登录。读实例 `context` 里的首次信任目录处理和 `dsh headless` 用法。
- 默认是 full access。用户没有要求时，不要降低权限，也不要替用户回答对方 agent 提出的业务决定问题。

出错时，先读原生状态再决定是否重试。返回 `unknown` 的写操作不要直接重发。
