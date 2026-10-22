# 给 Codex 的界面级验收提示词（computer use）

把下面整段交给 Codex，在 <主力 Mac> 上用 computer use 执行。

```text
你在我的 Mac（<主力 Mac>）上做一次端到端验收：我在 ChatGPT 里能否经 Agenvo 直接给各个桌面 agent 派活并拿到结果。用 computer use 操作真实界面，截图存到 <EVIDENCE_DIR>/evidence/，文件名带步骤号。只验收、不修代码；发现问题写进报告。

## 背景事实
- Relay：<RELAY> ，MCP 端点 <RELAY>/mcp
- 已批准的实例（全部 full access）：
  - <dsh 实例>：DSH 桌面版（DeepSeek Harness）@ <主力 Mac>，原生方法 session/list、workspace/create、session/create、session/prompt、session/follow、session/cancel 等
  - <第二台的 dsh 实例>：DSH 桌面版 @ <第二台 Mac>，同 <dsh 实例>
  - <codex 实例>：Codex app-server，只用 thread/list（sourceKinds 含 exec、modelProviders: []）和 thread/read 读历史；派活走 <herdr 实例>
  - <herdr 实例>：Herdr 终端工作区，workspace.create、pane.run、pane.read、session.list；agy 用 pane.run 跑无头 agy -p，需要终端交互时用 agent.start、agent.prompt
- 管理员密钥在 ~/.config/agenvo/admin-secret。它只能粘贴到 <RELAY> 的登录页，不能贴进 ChatGPT 对话，也不能出现在截图、日志或报告里。

## 步骤
1. 准备：确认 DeepSeek Harness app 已打开；在终端运行 `curl -s <RELAY>/health`，结果截图。
2. 添加连接器：打开 ChatGPT（桌面 app 或 chatgpt.com，用我已登录的账号），进入 Plugins → Add → Add custom MCP server。名称填 Agenvo，URL 填 <RELAY>/mcp，认证选 OAuth，client 凭据留空。点 Continue to Agenvo，在 Agenvo 登录页粘贴管理员密钥，在授权页确认 client 和回调后点 Allow，回到 ChatGPT。截图：连接器已创建；授权页（截图前遮住或避开密钥输入框）。
   如果遇到工作区不允许自定义 MCP、需要我二次验证，或需要付费升级，就停下来，在报告里写明卡在哪一步。
3. 发现：在 ChatGPT 新对话里启用 Agenvo，发送：「用 Agenvo 的 search 列出所有实例和在线状态，并读每个实例的 context，用表格给我。」预期 4 个实例全部 online。截图。
4. 派活，为各执行 agent 各派一个只读任务，都在 ChatGPT 里用自然语言下达（Codex 经 Herdr 执行）：
   - 「让 <dsh 实例> 在 ~/Projects/_forks/agenvo 新建会话，只读地列出 apps/ 下的子目录名，等完成后把结果给我；再在同一个会话里追问一句"哪个是 DSH 的 connector"。」
   - 「让 <第二台的 dsh 实例> 在 project=agenvo 里只读地读 FORK.md 第一行，等完成后给我结果。」
   - 「在 <herdr 实例> 用 workspace.create + pane.run 跑 codex exec --json -o "<本轮结果文件>" -C "<项目绝对路径>" -s danger-full-access -m <模型> -c model_provider="magpie" -（任务正文先写入文件经 stdin 传入），只读地用三句话总结 ~/Projects/_forks/agenvo 的 FORK.md 说明的补丁范围，JSONL 写到独立文件，命令结束输出 __EXIT=$?。从 thread.started 拿到 thread_id 后告诉我，等进程退出并读取 -o 结果，再用 codex exec --json -o "<下一轮结果文件>" -C "<项目绝对路径>" -s danger-full-access -m <模型> -c model_provider="magpie" resume <thread_id> -（同样写文件经 stdin 传入）追问一句“哪个补丁对应 DSH”，给我两轮结果。第二轮进程退出后，在 Codex Desktop 用 codex://threads/<id> 打开该线程截图。」（如果 Codex 报用量上限，如实记录。）
   - 「在 <herdr 实例> 里（session 用 session.list 返回的名字，通常是 default）用 workspace.create（cwd 为 ~/Projects/_forks/agenvo）+ pane.run 跑 agy -p "$(cat "<任务文件>")" --output-format json --dangerously-skip-permissions --model <模型> > "<本轮结果文件>"; echo __EXIT=$?，任务是只读地列出 tests/ 下的文件；看到 __EXIT 后读 JSON 的 status、response，把 conversation_id 告诉我；再用同一命令加 --conversation <conversation_id> 追问一句"其中哪个和 dispatch 有关"，把两轮结果给我。」
   每个任务截两张图：ChatGPT 里的结果；对应桌面 app 或终端里的会话。DSH（<主力 Mac> 和 <第二台 Mac>）的会话应出现在左栏 agenvo 工作区下，两轮对话都在；Codex 用 codex://threads/<id> 打开线程，截图需显示两轮历史、正常输入框且没有 “This is open in another app” 横幅；exec 线程不一定出现在侧栏，不以左栏出现新线程作为标准；Herdr 的会话用终端 `herdr` 查看；agy 另在 Antigravity 桌面版输入框的 `@conversation` 下拉里找这条会话并截图，找不到就写明，CLI 两轮结果与桌面查看结果分开记录，不能互相替代。<第二台的 dsh 实例> 的会话在 Mac mini 上，无法截图就写明，以 ChatGPT 返回的 completed 结果为准。
5. 事件与 dots：在 ChatGPT 里尝试创建一个 dot，让它订阅 Agenvo 的 runtime.changed 事件（DSH 的 turn/end）。然后给 <dsh 实例> 派一个只读小任务，观察 dot 是否在任务完成后自动收到事件并告诉我结果。如果 ChatGPT 账号里没有 dots 功能或找不到入口，写明并跳过。
5b. Grok：打开 grok.com/connectors（用我已登录的账号），添加自定义连接器，URL 填 <RELAY>/mcp，按提示走 OAuth：在 Agenvo 登录页粘贴管理员密钥，然后点 Allow。如果成功，在 Grok 新对话里让它用 search 列出实例，再给 <dsh 实例> 派只读任务并读回结果。如果失败（例如报 invalid_client、或要求付费），记下原文，不要尝试绕过。
6. 不做清理：保留 ChatGPT 连接器和授权，这就是正式要用的授权。

## 报告
写到 <EVIDENCE_DIR>/report.md，并在对话里输出同样内容：
- 总结论一句：通过 / 部分通过 / 不通过。
- 逐项表：步骤、预期、实际、截图文件名、通过与否。
- 每个实例：session id 或 thread id、桌面会话是否可见、结果是否正确。
- 未验证项及原因，例如 Codex 额度用尽、dots 不可用、<第二台 Mac> 无法截图。
- 发现的问题，按严重程度排序。
```
