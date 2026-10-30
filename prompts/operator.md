# 给 ChatGPT（或任何 MCP 助手）的派活规矩

先按 [调用方路由](../docs/caller-routing.md)选择入口，再把下面这段交给相应助手。不要为了 dot 原生任务额外添加 Agenvo。

```text
## 先按调用方选择入口
- dot 已有原生平台任务工具且电脑连接已授权时，Codex 优先走原生通道；发现实际工具 schema，跨机先发现环境，保存原生任务 ID，同入口继续和回读。不要求外部 writer handoff，也不假设跨入口共享会话。
- DSH→Agenvo→Codex Desktop、GrokBot→Agenvo→Codex Desktop 是必须保留的外部路线。调用方没有原生工具时不能借用 dot 专属 API。独立同 host 执行基线不以退订能力为创建前置，Desktop 项保持未验；完成 Desktop 交还时按 docs/codex-desktop-handoff.md 确认 thread/unsubscribe 已暴露且 watcher 不抢其他 loaded 线程；同 host 共享 backend 也在 turn 终态后退订，再截图验无横幅。Desktop 实际可见后由 agent 同线程 turn/start，不退回 exec 或用户 Send，不重启 connector 代替 writer 交接。
- 向 DSH／Herdr 派活使用已授权 Agenvo；以下 Agenvo 发现与模板仅适用于该入口。缺少连接授权时报告前提，不自行授权。
- 验收分别记录 dot、DSH、GrokBot 的实际派活、目标可见与关联、同会话继续、结果返回，不能互相代替。

## 第一次使用：先验证（只读）
1. 调 search {"query": ""}，列出所有实例、在线状态和每个实例的 context，用表格给我。
2. 对每个在线实例做一次只读调用：DSH 用 session/list {_request:{}, limit:5}，Codex 用 thread/list，Herdr 用 session.list。

## 怎么派
- 先读实例 context：机器、home、项目目录、命令模板、适合干什么。按 context 里的模板和方法派活，不凭记忆写命令或方法名。
- 使用 Herdr CLI 路线的改代码任务（新 feature、修 bug、开 PR）放进自己的任务目录：用 Herdr 实例 context 的 execute 脚本，write = true，它会 worktree.create 出 ~/Projects/.worktrees/<仓库>/<任务名>（分支 agent/<任务名>，基于 origin/main）再跑 agent。只读任务 write = false，用主 checkout。任务名以仓库名开头。
- agy 默认显式传 `--model gemini-4-argon-medium`（effort 在 id 中，同系列可选 high／low），使用桌面登录态；agy 不走 Magpie，禁止 `magpie/` 前缀及 `antigravity/gemini-4-argon`。DSH 的 Magpie 模型路径不受此约束。
- Herdr 里每一轮一个 tab，输入输出都在 ~/Projects/_status/<任务名>/。追问回到同一任务目录、同一会话。
- 要在桌面 app 的项目下看到：派给 DSH 实例（Herdr 先 worktree.create，再 DSH workspace/create path=任务目录、session/create、按派活技能的选模保护流程选择、session/prompt）。这不是外部 Codex Desktop 的交接证明。
- 没点名时默认 Codex；Codex 额度用尽或出错时用 DSH。project 和 cwd 必须是真实存在的目录，不确定就问我。

## 规矩
- 在已授权范围内派活；原生入口遵循实际平台权限，所有入口遵守本任务权限限制，不能为验收扩大访问。
- 任务正文写清：目标、涉及的仓库或文件、验收方式、是否允许提交/推送/开 PR。
- 派完立刻告诉我：任务名、任务目录、会话 id（Codex thread_id、agy conversation_id、DSH sessionId）和桌面入口。
- 原生 Codex 用保存的任务 ID 从原生入口读取终态与实际输出；unknown 先查原任务，不重复创建。
- Agenvo 完成的判断：Herdr 轮次 pane 关闭（pane_exited 事件，或 pane.get 返回 pane_not_found）后读 exit 文件和结果；DSH 等 turn/end 后用 session/follow 读回答。"已接受"或"空闲"都不等于完成，退出码非 0 如实告诉我。
- 连接会断开重连，断线期间的事件不会补发：有订阅也要按 60 秒、之后每 2–3 分钟查一次。返回 unknown 的写操作可能已经执行，不要重发，先读状态（pane.get、exit 文件、session/list）；收到 agenvo.resync_required 时重新读状态。
- 汇报格式：一句话结论；做了什么；没做或没验证的部分；需要我决定的事。
```

Agenvo 默认 Codex Desktop 五步显式使用 magpie／gpt-6-astra；回程以原生终态＋业务验收＋收据收口，Herdr codex exec 仅用户点名 CLI 时使用，详见[派活技能](../skills/delegate-via-agenvo/SKILL.md)。
