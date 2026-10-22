---
name: delegate-via-agenvo
description: Hand work to another coding agent (DeepSeek Harness/DSH, Codex, Claude Code, or agy/codex/dsh headless in Herdr, on this or another Mac) through the Agenvo MCP server, then follow it to a verified result. Use when the user asks to have DSH/Codex/another agent do something, to run work in parallel on another agent, or to check on or continue such work.
---

# 经 Agenvo 把活交给其他 agent

前提：当前 agent 已连接名为 `agenvo` 的 MCP 服务器，它提供 `search` 和 `execute` 两个工具。连不上时，告诉用户运行 `codex mcp login agenvo`（或在自己的 MCP 设置里重新授权）。

## 步骤

1. **发现**：调用 `search {"query": ""}`，列出实例和在线状态，读每个实例的 `context`。它写了这台机器、项目目录、默认权限，以及这个实例适合做什么。
2. **找方法**：用共同术语搜索，例如 `search {"query": "dsh create work context"}`、`"dsh submit input"`、`"dsh read output"`、`"dsh interrupt"`、`"dsh list"`。选定方法后，带上 `includeSchema: true` 再搜一次，拿到参数 schema。不要凭记忆写方法名，不同 connector 的方法名不一样。
3. **派活**：用 `execute` 调 `call({deviceId, instanceId}, method, params)`。
   - 对方没有你的上下文。任务文本要写全：目标、仓库和路径、已知事实、约束、验收方式，以及是否允许提交、推送或开 PR。
   - `project` 或 `cwd` 必须是对方机器上真实存在的目录。
   - 新建会话后先显式选模型，再提交输入：DSH 用 `session/selectModel {request: {sessionId, provider, model, reasoningEffort}}`，返回的 `selected` 就是这个会话之后各轮使用的模型。每个会话各选一次，实例的全局默认保持原样。
   - 派出后立刻记下返回的 id（session 或 thread），并告诉用户。
4. **跟进**：结果不会自动回来，要自己去读。
   - 按固定节奏读：派出约 60 秒后读第一次，之后每 2–3 分钟读一次；约 10 分钟仍未完成就停止等待，把会话 id 和当前状态报告给用户。每次读取单独一次 `execute`，在调用之间等待。
   - 先用便宜的状态判断是否完成，再读输出：DSH 读 `session/projections` 的 `turnOutline`，最后一项的 `response` 非空即本轮已结束；然后用 read output 类的方法（DSH `session/follow`）取结果。
   - 需要追问或纠偏时，用 submit input 发到**同一个**会话，不要另开新会话。
   - 方向错了就 interrupt，说明原因后重新交代。
5. **验收**：输入"已接受"、agent"空闲"、轮次结束，都不等于任务完成。读到实际结果、对照第 3 步写下的验收方式核对之后，才算完成。最后给用户的汇报要写清：结论、对方做了什么、哪些没做或没验证、需要用户决定的事。

## 选哪个 agent

- 用户点名了就用点名的。
- 没点名时默认用 Codex，经 Herdr pane 运行 `codex exec`。Codex 不可用（额度用尽、离线、报错）时改用 DSH：DSH 能力和 Codex 同级，能新建、追问、中断。
- 要在桌面 app 里看得见、能手动接手：Codex 用下述线程链接；也可选 DSH 实例。
- 要在 Antigravity 桌面版里看得见：目前做不到。桌面版（2.25.0）的 `@conversation` 只搜桌面自己的全文索引，`agy -p` 派出的 CLI 会话进不了这个索引（桌面日志报 `not found in any store`，2026-10-22 实测）。加 `--new-project` 只会在桌面侧栏生成一个空项目（显示 No conversations yet），会话本身仍不可见、`@conversation` 搜不到（2026-10-22 用户实测），所以派活不要加 `--new-project`。需要桌面可见时不要选 agy；agy 会话在 Herdr 终端里看，或用 `agy --conversation <id>` 续接。
- 要长时间运行、不怕笔记本合盖：选第二台 Mac 上的实例。
- Codex、Claude Code 和 agy 无头命令，以及 `dsh headless`，选 Herdr 实例。Codex、Claude Code 与 agy 无头命令按下文用 `pane.run`；要在终端里交互操作 agy 时用 `agent.start`、`agent.prompt`。读实例 `context` 里的首次信任目录处理；`dsh headless` 按下节「DSH headless 经 Herdr，在桌面项目下可见」执行。
- 默认是 full access。用户没有要求时，不要降低权限，也不要替用户回答对方 agent 提出的业务决定问题。

出错时，先读原生状态再决定是否重试。返回 `unknown` 的写操作不要直接重发。

## DSH headless 经 Herdr，在桌面项目下可见

用户明确要 CLI 通道时，在目标机器的 Herdr workspace（cwd 为项目绝对路径）用 `pane.run` 启动，桌面 Host 与 DSH 实例仍须在线。

1. 命令带 `--json`，例如 `dsh --profile headless --json "<任务>"; echo __EXIT=$?`。任务正文长时先写文件，经 stdin 的 `-` 传入：

   ```sh
   dsh --profile headless --json - < "/tmp/<任务名>.txt"; echo __EXIT=$?
   ```

2. 立即读取第一行 session 事件（`{"type":"session","sessionId":"<id>","cwd":"<cwd>"}`），记录 sessionId 和 cwd。
3. 拿到 sessionId 立刻经同机 DSH 实例调用一次 connector 方法 `workspace/attachSession {path: <cwd>, sessionId}`，对应插件路由 `/bridge/workspace/attachSession`。CLI 运行中调用安全：登记不拿会话写锁。核对返回 `ok:true`，把返回的项目 title 和 sessionId 告诉用户：「DSH 桌面左栏项目『<title>』下」。失败时报告错误，不能声称已在项目下可见。
4. 项目路径已存在时不改名，直接归入。新项目默认用 `basename(realpath)`；与已有项目 title 重名时用 `"<basename> · <短父路径>"`。短父路径取 realpath 的父目录，去掉开头 `/private`，把 home 前缀换成 `~`，再去掉开头 `/`。
   例如 `/private/tmp/turing-koe-p1/koe` 重名时为 `koe · tmp/turing-koe-p1`，`$HOME/work/koe` 为 `koe · ~/work`；调用者也可显式给 title，仅用于新项目。
5. 按前述节奏读输出，核对 `__EXIT=` 与业务结果；CLI 追问沿用 `--session-id <sessionId>`，每轮保留 `--profile headless --json`。
6. **只在最后一轮 CLI 已退出、确定不再用 `--session-id` 追问后**，经 DSH 实例用 `session/rename {request:{sessionId, title:"[CLI] <任务名>"}}` 改名。rename 会让桌面 Host 拿住该会话写锁，之后 CLI 续写报 `already owned by an active write handle`（2026-10-22 隔离实测，等 45 秒仍未释放）。改名后的追问走桌面或 DSH 实例的 `session/prompt`。

反向接管不可行：桌面/Host 先建的会话，CLI `--session-id` 报 `runs under agent preset "standard", which the one-shot runner does not compose`。需要 CLI 通道时从 headless 新建，再登记到项目。

## Codex：官方 exec 经 Herdr 派活

1. 在目标机器的 Herdr 实例（ID 从实例 context 获取）先调 `session.list`，再用它返回的名字调 `workspace.create {session: "<session 名>", cwd: "<项目绝对路径>"}`，取得 root pane。`cwd` 取实例 context 中 `~/Projects/<project>` 展开的绝对路径；其他机器使用其实际实例与路径。
2. 在 root pane 用 `pane.run` 运行以下命令。先确保 `~/Projects/_status` 存在；任务名和每轮输出路径唯一，避免把上轮文件当作本轮完成。任务正文一律先写入文件，再用 `-` 从 stdin 传入，避免 shell 展开正文；`-o`、`-C` 和所有输出路径都加引号。每轮都显式带 `-m <模型>` 和 `-c model_provider="magpie"`，请求经本机 magpie：

   ```sh
   codex exec --json -o "$HOME/Projects/_status/<任务名>.md" -C "<cwd>" -s danger-full-access -m <模型> -c model_provider="magpie" - < "/tmp/<任务名>.txt" > "/tmp/<任务名>.jsonl"; echo __EXIT=$?
   ```

3. 启动后立即读 JSONL 第一行：`type: "thread.started"` 对应的 `thread_id` 就是线程 id，立刻记下并告诉用户，同时给出 `codex://threads/<thread_id>`。这次读 id 不代替结果轮询。
4. 派出约 60 秒后第一次检查结果，之后每 2–3 分钟检查。用 `pane.read` 看到本轮 `__EXIT=` 行，或本轮 `-o` 文件已写出，就读取结果；`-o` 保存最后一条助手消息，过程和错误看 JSONL 的 `turn.completed`、`turn.failed`、`error`。文件写出不替代退出码与失败事件核对，业务验收仍按上面的步骤执行。
5. 追问在同一 pane 用原 thread id，下一句同样先写入文件，任务文件和输出都用新一轮文件名。`resume` 子命令不接受 `-C`、`-s`，二者放在 `resume` 之前；每轮都重复 `-s danger-full-access`、`-m <模型>` 和 `-c model_provider="magpie"`：

   ```sh
   codex exec --json -o "$HOME/Projects/_status/<任务名>-2.md" -C "<cwd>" -s danger-full-access -m <模型> -c model_provider="magpie" resume <thread_id> - < "/tmp/<任务名>-2.txt" > "/tmp/<任务名>-2.jsonl"; echo __EXIT=$?
   ```

6. 中断用 `pane.send-keys` 发送 `["ctrl+c"]`。进程退出后 writer 已交还，用户在 Codex 桌面版打开 `codex://threads/<thread_id>`，已观察到完整历史、正常输入框，且没有 “This is open in another app” 占用横幅。exec 线程不一定出现在侧栏列表里，必须把链接告诉用户。

目标机器的 Codex 实例只用 `thread/list`（`sourceKinds` 含 `exec`，`modelProviders: []`）与 `thread/read` 读取历史。不要用它的 `thread/start` / `turn/start` 派活：常驻共享 server 会继续占着 writer。线程 source 是 `exec`，`--thread-source vscode` 不改变它。

## Claude Code：经 magpie 无头运行并显式选模型

前提：目标机器上 Claude Code 的 `ANTHROPIC_BASE_URL`（`~/.claude/settings.json` 的 `env` 或环境变量）指向本机 magpie `http://127.0.0.1:3425`。下面的命令启动前先检查 3425，不在线就不启动 Claude Code，直接以 `__EXIT=97` 结束。

与 Codex 一样先创建 Herdr workspace、取得 root pane，任务正文先写入文件，再用 `pane.run` 从 stdin 传入：

```sh
if nc -z 127.0.0.1 3425 2>/dev/null; then claude -p --dangerously-skip-permissions --model deepseek/deepseek-flash --output-format json < "/tmp/<任务名>.txt" > "/tmp/<任务名>.json"; echo __EXIT=$?; else echo "magpie 127.0.0.1:3425 不在线"; echo __EXIT=97; fi
```

新建与 `--resume` 每轮都保留 `--dangerously-skip-permissions`（默认 full access），并显式传 `--model`。Claude Code 只用两个模型，都经 magpie：首选 `deepseek/deepseek-flash`（即上下示例）；可选第二模型 `minimax-code/MiniMax-M3.1-Flash`，用时把示例里的 `--model deepseek/deepseek-flash` 换成 `--model minimax-code/MiniMax-M3.1-Flash`，续接沿用新建时的模型。新建与续接分别使用本节上方和下方的完整无头命令，不使用交互式 `agent.start kind claude`。

按同样的 60 秒、之后每 2–3 分钟节奏用 `pane.read` 跟进，看到 `__EXIT=` 后读取 JSON：结果在 `result`，会话 id 在 `session_id`，同时核对 `is_error` 和退出码。拿到会话 id 后记下并告诉用户，追问续接同一会话：

```sh
if nc -z 127.0.0.1 3425 2>/dev/null; then claude -p --dangerously-skip-permissions --resume <session_id> --model deepseek/deepseek-flash --output-format json < "/tmp/<任务名>-2.txt" > "/tmp/<任务名>-2.json"; echo __EXIT=$?; else echo "magpie 127.0.0.1:3425 不在线"; echo __EXIT=97; fi
```

中断同样用 `pane.send-keys ["ctrl+c"]`。

## Antigravity：agy 无头经 Herdr，结果在终端查看

Antigravity 桌面版没有公开的派活接口，派活一律在 Herdr pane 里运行官方 CLI `agy`。CLI 会话存放在 `~/.gemini/antigravity-cli/conversations/`；桌面版（2.25.0）的 `@conversation` 只查 `~/.gemini/antigravity/conversation_summaries.db` 的 `conversations_fts`，搜不到 CLI 会话（2026-10-22 实测）。派活不加 `--new-project`，该参数只在桌面侧栏生成空项目。

1. 与 Codex 一样先创建 Herdr workspace、取得 root pane，任务正文先写入文件。用 `pane.run` 运行下面的命令，每轮显式传 `--model`（名称用 `agy models` 列出的 slug，例如 `gemini-4-argon-high`），并保留 `--dangerously-skip-permissions`：

   ```sh
   agy -p "$(cat "/tmp/<任务名>.txt")" --output-format json --dangerously-skip-permissions --model <模型> > "/tmp/<任务名>.json" 2> "/tmp/<任务名>.err"; echo __EXIT=$?
   ```

2. 按同样的 60 秒、之后每 2–3 分钟节奏用 `pane.read` 跟进，看到 `__EXIT=` 后读 JSON：`status` 为 `SUCCESS` 或 `ERROR`，结果在 `response`，会话 id 在 `conversation_id`，失败原因在 `error` 和 `.err` 文件。记下 `conversation_id` 并告诉用户。
3. 追问续接同一会话，下一句同样先写文件：

   ```sh
   agy -p "$(cat "/tmp/<任务名>-2.txt")" --conversation <conversation_id> --output-format json --dangerously-skip-permissions --model <模型> > "/tmp/<任务名>-2.json" 2> "/tmp/<任务名>-2.err"; echo __EXIT=$?
   ```

4. 查看：把 `conversation_id`、cwd 和结果文件路径告诉用户；用户在终端执行 `herdr` 查看，或用 `agy --conversation <id>` 续接。桌面版目前调不出 CLI 会话，不要让用户去桌面里找。
5. 中断用 `pane.send-keys` 发送 `["ctrl+c"]`。

`agent.start kind agy` 的交互式用法只在需要终端里逐步操作、处理信任提示时使用；派活和拿结果用上面的无头命令。
