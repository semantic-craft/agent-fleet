# 给 Codex 的界面级验收提示词（computer use）

把下面整段交给 Codex，在 <主力 Mac> 上用 computer use 执行。

```text
你在我的 Mac（<主力 Mac>）上做一次端到端验收：我在 ChatGPT 里能否经 Agenvo 直接给各个桌面 agent 派活并拿到结果。用 computer use 操作真实界面，截图存到 <EVIDENCE_DIR>/evidence/，文件名带步骤号。只验收、不修代码；发现问题写进报告。

## 背景事实
- Relay：<RELAY> ，MCP 端点 <RELAY>/mcp
- 已批准的实例（全部 full access）：
  - <dsh 实例>：DSH 桌面版（DeepSeek Harness）@ <主力 Mac>，原生方法 session/list、workspace/create、session/create、session/prompt、session/follow、session/cancel 等
  - <zcode 实例>：ZCode 桌面版 @ <主力 Mac>，方法 jobs.create / jobs.get / health
  - <第二台的 dsh 实例>：DSH 桌面版 @ <第二台 Mac>，同 <dsh 实例>
  - <codex 实例>：Codex app-server，thread/start、turn/start、thread/list 等
  - <herdr 实例>：Herdr 终端工作区，agent.start、agent.prompt、session.list 等
- 管理员密钥在 ~/.config/agenvo/admin-secret。它只能粘贴到 <RELAY> 的登录页，不能贴进 ChatGPT 对话，也不能出现在截图、日志或报告里。

## 步骤
1. 准备：确认 DeepSeek Harness 和 ZCode 两个 app 都已打开；在终端运行 `curl -s <RELAY>/health`，结果截图。
2. 添加连接器：打开 ChatGPT（桌面 app 或 chatgpt.com，用我已登录的账号），进入 Plugins → Add → Add custom MCP server。名称填 Agenvo，URL 填 <RELAY>/mcp，认证选 OAuth，client 凭据留空。点 Continue to Agenvo，在 Agenvo 登录页粘贴管理员密钥，在授权页确认 client 和回调后点 Allow，回到 ChatGPT。截图：连接器已创建；授权页（截图前遮住或避开密钥输入框）。
   如果遇到工作区不允许自定义 MCP、需要我二次验证，或需要付费升级，就停下来，在报告里写明卡在哪一步。
3. 发现：在 ChatGPT 新对话里启用 Agenvo，发送：「用 Agenvo 的 search 列出所有实例和在线状态，并读每个实例的 context，用表格给我。」预期 5 个实例全部 online。截图。
4. 派活，每个实例各一个只读任务，都在 ChatGPT 里用自然语言下达：
   - 「让 <dsh 实例> 在 ~/Projects/agenvo 新建会话，只读地列出 apps/ 下的子目录名，等完成后把结果给我；再在同一个会话里追问一句"哪个是 DSH 的 connector"。」
   - 「让 <zcode 实例> 在 project=agenvo 里只读地说明 apps/dispatch/src/config.ts 中 fullAccess 的作用，等完成后给我结果。」
   - 「让 <第二台的 dsh 实例> 在 project=agenvo 里只读地读 FORK.md 第一行，等完成后给我结果。」
   - 「让 <codex 实例> 在 ~/Projects/agenvo 新建一个 thread，只读地用三句话总结 FORK.md 说明的补丁范围，给我结果。」（如果 Codex 报用量上限，如实记录。）
   - 「在 <herdr 实例> 里（session 用 session.list 返回的名字，通常是 default）用 agent.start 起一个 claude（带 --dangerously-skip-permissions），cwd 为 ~/Projects/agenvo，让它只读地列出 tests/ 下的文件；完成后再用 agent.prompt 追问一句"其中哪个和 dispatch 有关"，把两次回答都给我。」
   每个任务截两张图：ChatGPT 里的结果；对应桌面 app 里新出现的会话。DSH（<主力 Mac> 和 <第二台 Mac>）的会话应出现在左栏 agenvo 工作区下，两轮对话都在；ZCode 的会话应带 agenvo 来源标记；Codex 的新 thread 应出现在 Codex Desktop 里；Herdr 的会话用终端 `herdr` 查看。<第二台的 dsh 实例> 的会话在 Mac mini 上，无法截图就写明，以 ChatGPT 返回的 completed 结果为准。
5. 严格桌面模式：退出 ZCode app，在 ChatGPT 里再给 <zcode 实例> 派一个只读任务。预期立刻返回拒绝（HTTP 503 或 rejected），不会在后台无界面执行。截图后重新打开 ZCode。
6. 事件与 dots：在 ChatGPT 里尝试创建一个 dot，让它订阅 Agenvo 的 runtime.changed 事件（DSH 的 turn/end、ZCode 的 job.finished）。然后给 <zcode 实例> 派一个只读小任务，观察 dot 是否在任务完成后自动收到事件并告诉我结果。如果 ChatGPT 账号里没有 dots 功能或找不到入口，写明并跳过。
6b. Grok：打开 grok.com/connectors（用我已登录的账号），添加自定义连接器，URL 填 <RELAY>/mcp，按提示走 OAuth：在 Agenvo 登录页粘贴管理员密钥，然后点 Allow。如果成功，在 Grok 新对话里让它用 search 列出实例，再给一个 zcode 实例派只读任务并读回结果。如果失败（例如报 invalid_client、或要求付费），记下原文，不要尝试绕过。
7. 不做清理：保留 ChatGPT 连接器和授权，这就是正式要用的授权。

## 报告
写到 <EVIDENCE_DIR>/report.md，并在对话里输出同样内容：
- 总结论一句：通过 / 部分通过 / 不通过。
- 逐项表：步骤、预期、实际、截图文件名、通过与否。
- 每个实例：job id 或 thread id、桌面会话是否可见、结果是否正确。
- 未验证项及原因，例如 Codex 额度用尽、dots 不可用、<第二台 Mac> 无法截图。
- 发现的问题，按严重程度排序。
```
