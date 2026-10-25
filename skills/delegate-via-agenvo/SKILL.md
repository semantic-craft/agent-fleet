---
name: delegate-via-agenvo
description: Hand work to another coding agent (Codex Desktop, DeepSeek Harness/DSH, Claude Code, or agy/dsh headless in Herdr, on this or another Mac) through the Agenvo MCP server, then follow it to a verified result. Use for external DSH/GrokBot-to-Codex Desktop routing and Agenvo DSH/Herdr delegation; native platform task-capable callers should choose their native Codex route first. Use when agenvo／外部桌面接手, or the user asks to have DSH/Codex/another agent do something, or to check on or continue such work.
---

# 经 Agenvo 把活交给其他 agent

前提：当前 agent 已连接名为 `agenvo` 的 MCP 服务器，它提供 `search` 和 `execute` 两个工具。连不上时，告诉用户运行 `codex mcp login agenvo`（或在自己的 MCP 设置里重新授权）。

## 先选入口

先读[调用方路由](../../docs/caller-routing.md)。dot 当前拥有原生平台任务工具且电脑已连接／授权时，Codex 派活优先该通道，不适用下文外部 writer 创建前门槛。DSH→Agenvo→Codex Desktop、GrokBot→Agenvo→Codex Desktop 是需要保留的外部需求，不能用 dot 专属 API 替代；第三方工具缺失时不得假定可用。跨机先做环境发现。下文只描述 Agenvo 路线。

## 步骤

1. **发现**：调用 `search {"query": ""}`，列出实例和在线状态，读每个实例的 `context`。它写了这台机器、home 与项目目录、默认权限，以及这个实例适合做什么。
2. **找方法**：用共同术语搜索，例如 `"dsh create work context"`、`"herdr worktree"`、`"dsh read output"`；选定后带 `includeSchema: true` 再搜一次拿参数 schema。不要凭记忆写方法名。
3. **派活**：外部 Agenvo 调用方派 Codex 按「Codex Desktop」；其余任务目录按「任务目录」选；Herdr 里的 Claude Code／agy／dsh headless 每轮按「一轮一个 tab」跑；DSH 实例按 schema 调原生方法。
   - 对方没有你的上下文。任务正文写全：目标、仓库和路径、已知事实、约束、验收方式，以及是否允许提交、推送或开 PR。
   - 核对实际模型：CLI 每轮显式带模型参数。DSH 优先复用已选好且匹配的独占会话；`session/selectModel` 可能持久化全局默认，绝非隔离设置。若运行时明确发现 `session/selectModelForSession` 且实时 catalog 能力为 true，可经[会话级候选入口](../../docs/dsh-session-only.md)显式选择独占会话；applied 非 durable，仍需核对下一请求 header，旧 runtime 不回退。此候选未部署，不能仅据仓库存在就调用。确需使用旧接口时先按[全局副作用保护](../../docs/migration-and-upgrades.md#dsh-模型选择全局副作用保护)取得授权、协调所有选择入口/暂停新会话、快照、受保护恢复并回读；未知结果不重发，漂移不覆盖。不得自动切模型或默认。
   - 派出后立刻告诉用户：任务名、任务目录、会话 id 和桌面查看入口——DSH 报项目名与 sessionId；Codex 报 thread_id 与 `codex://threads/<thread_id>`；Claude Code 报 `session_id`；agy 报 `conversation_id`；Herdr 里的任务另报 workspace 与轮次 pane。没拿到登记结果或链接时如实说，不说用户能在桌面里看到。
4. **跟进**：采用 call-now / fetch-later，先记录 threadId／sessionId、turnId／requestId，再退出提交调用；结果不会自己回来。
   - 有事件订阅就等事件（Herdr `pane_exited`、DSH `turn/end`）；没有就按节奏查：派出约 60 秒后第一次，之后每 2–3 分钟一次，约 10 分钟仍未结束就停下，把会话 id 和当前状态报告给用户。每次查询单独一次 `execute`。
   - 事件不保证送达：connector 与 relay 之间的连接会断开重连（2026-10-23 实测：relay 主机走机场线路时 31.5 分钟断 30 次，网关规则改走直连后 31.6 分钟断 1 次），断线期间的事件不会补发。所以有订阅也要保留轮询；收到 `agenvo.resync_required` 时重新读状态，不重新派活。
   - 先廉价查状态再读输出：Codex `thread/turns/list` 用 `limit: 1, itemsView: "notLoaded"`，关联本次 turnId；completed 后读同一 turn 的完整输出，failed/interrupted 记录失败。DSH `session/list {_request:{}}` 找本 session 的 running；false 只是读结果信号，再 `session/follow`（maxMessages: 1）核对本 requestId 对应的 `turn/end` 与回答。旧 turn 或空闲不算本轮完成。
   - 有 `events/subscribe` 能力时优先订阅 `runtime.changed` 的 `turn/completed`／`turn/end`，按实例和会话关联；推送只唤醒回读，不证明业务成功。Grok Bot 没有订阅工具就用上述轮询，不临时造 webhook。
   - 追问或纠偏发到**同一个**会话；方向错了就中断，说明原因后重新交代。
5. **验收**：输入"已接受"、agent"空闲"、轮次结束，都不等于任务完成。读到实际结果、对照第 3 步的验收方式核对后才算完成。汇报写清：结论、对方做了什么、哪些没做或没验证、需要用户决定的事。

6. **收据**：外循环验收后写 `~/Projects/_status/<任务>-receipt.md`，含任务、目标实例、threadId／sessionId、turnId／requestId、实际 provider/model/effort、原生终态、业务结论、证据路径、未验项与时间。先写同目录临时文件再原子替换并回读；失败／超时也写明确的 failed／blocked／pending 收据，不能冒充成功。亚／图灵读收据收口；无收据不算收口完成，空则静默但轮询责任仍在派工方。

出错时先读原生状态再决定是否重试。返回 `unknown` 的写操作可能已经执行（实测两种情况都有），不要直接重发。

## 选哪个 agent

- dot 有原生平台任务工具且已授权 → Codex 走原生平台任务通道（见调用方路由），不绕经外部交接。
- Agenvo 路线未点名时默认 Codex Desktop，`model=gpt-6-astra`、`modelProvider=magpie`、effort `medium`，不修改用户全局默认；Magpie 不可用就报告该层失败。
- DSH／GrokBot 派 Codex → **外部 Codex Desktop**（见下节）；该需求保留，同 host 基线已验，调用方及 Desktop 仍未验，不影响 dot 原生路线。
- 用户点名 Claude Code／agy／DSH headless（或同类 CLI／pane）→ **Herdr**（承接用户指定的这类模型线，与 按调用方选择的 Codex 路线分开）。
- 桌面可见且自动开跑 → **DSH** 实例。外部 DSH／GrokBot 经 Agenvo 的 Codex Desktop 走下节五步（同机共享 app-server 与 Desktop 自带 backend 也要交还 writer）。
- Codex 不可用（额度用尽、离线、报错）→ 改 DSH；DSH 与 Codex 同级，能新建、追问、中断。
- Antigravity 桌面版（2.25.0）调不出 `agy -p` 的会话，`--new-project` 只生成空项目；需要桌面可见时不选 agy。
- 要长时间运行、不怕笔记本合盖：选常开那台 Mac 上的实例。
- 默认 full access。用户没有要求时不要降权限，也不要替用户回答对方 agent 提出的业务决定问题。

## 任务目录

- **改代码的任务**（新 feature、修 bug、开 PR）不进 `~/Projects/<repo>` 主 checkout：主 checkout 要停在默认分支，几个任务同时改一个仓库会互相踩。用 Herdr 原生 `worktree.create` 一步建好任务目录和它的终端工作区：路径 `<home>/Projects/.worktrees/<repo>/<任务名>`，分支 `agent/<任务名>`，基于 `origin/main`。第一轮命令先 `git fetch` 并快进到 `origin/main`（模板已含）。
- **只读任务**（读代码、评审、回答问题）用 `workspace.create`，cwd 为主 checkout；它可能不在 main，回报里写明读的是哪个分支。
- 任务名以仓库名开头（如 `koe-unset-keys`），桌面里看到的项目名、Herdr workspace 名都由它来。
- 同一任务的追问回到同一个任务目录和同一个会话。PR 合并后 `worktree.remove {session, workspaceId}`（分支保留，`reap` 也会回收遗漏的 worktree）。
- DSH 实例做改代码的任务：先用同机 Herdr 实例 `worktree.create`，再对 DSH 实例 `workspace/create {request: {path: <worktree 路径>}}`，会话出现在桌面左栏以任务名命名的项目下。
- 任务目录用 Herdr `worktree.create`（见上），与 Desktop 会话的 `cwd` 对齐到同一路径。

## Codex Desktop

适用者：DSH、GrokBot 等外部 Agenvo 调用方；本节不约束 dot 已有原生平台任务工具的路线。

目标：任务出现在 **Codex Desktop** 左栏（会话可见），由 agent 经 agenvo 提交开跑。

按[Codex Desktop 交接契约](../../docs/codex-desktop-handoff.md)执行：

1. **建线程**：从 discovery/schema 固定 deviceId/instanceId/backend；创建前门槛：确认目标 connector 暴露 `thread/unsubscribe` 且事件观察不会收养其他 loaded 线程；缺失时报 `blocked_native_writer_release`，不创建。brief 放同一 deviceId 的授权目录并从目标机器回读；纯文本无需文件。`thread/start {cwd, model: "gpt-6-astra", modelProvider: "magpie"}`，保存 threadId 和模型回执，unknown 只回读。
2. **核对连接**：brief 的 kickstart 步骤只作用于已发现的目标 connector launchd label：`launchctl kickstart -k gui/<uid>/<connector-label>`。先核对它没有本轮之外的在途调用；不能重启 Desktop/native host。connector 重启不释放独立 native writer，不能把 kickstart 成功当作跨 backend ownership 证据。常规流程不需要 kickstart；若 brief 要求，后续须显式 `thread/resume` 重新取得本连接订阅，才能由这条连接退订。
3. **桌面可见**：目标机器打开 `codex://threads/<threadId>?prompt=<URL 编码的正文>`，独立确认同一会话 Desktop 可见。链接只预填，不是提交；共享路径、loaded 或历史不能替代 UI。UI 被拒绝或看不到时停止完整烟雾并保留未验，不换工具绕过拒绝。
4. **提交并回程**：可见后对同一 thread `turn/start`，显式 `model: "gpt-6-astra", effort: "medium"`，继承已核对的 magpie provider。保存 turnId，按上文轮询至原生终态、读实际回答、写收据；unknown 只回读，不重发。不得静默换 Herdr exec 或直连 ChatGPT 冒充 Desktop 成功。

5. **交还 Desktop**：确认本次 turn 达终态、读到结果后（或用户明确要求把 UI 交给 Desktop 输入时），经同一 connector 对同一 thread 调 `thread/unsubscribe {threadId}`。记录原生 status；`notSubscribed` 只表示该连接没有订阅，不能证明其他连接放锁。重新打开 deep link，截图必须同时含左栏项目下的会话及主区，确认没有 `writerConflict`／“This is open in another app”／“Fork chat”／“open in new chat”。退订后 notifications 可能停止；完成判据须在退订前读取，之后只用 `thread/read`／`thread/turns/list` 读持久历史。只有明确续接执行时才 `thread/resume`，结束后再次退订。

只读烟雾正文限定只回复唯一 `MAGPIE_CODEX_OK_<标记>`，不调用工具、不改文件。权限以原生回执为准：connector 可能强制 full access，即使请求 read-only 也不能声称沙箱已只读；收据分别记录任务只读和实际权限。

## DSH Desktop Magpie

选 discovery 中目标机器的 DSH 桌面实例（长任务用常开机器），依次 `workspace/create` → `session/create` → 选模 → `session/prompt`；桌面左栏项目和 sessionId 分别核验。只读烟雾 create/prompt 均传 `permission: "read-only"`，只回复 `MAGPIE_DSH_OK_<标记>`，禁止工具与文件写入。

先读实时 `session/modelCatalog`，例如 `provider: "magpie", model: "antigravity/gemini-4-argon", reasoningEffort: "medium"`。按第 3 步的全局副作用保护选模：优先实际可用的 session-only 接口；旧 `session/selectModel` 会保存全局默认。brief 禁止改私人默认时，只能在已核实目标三元组与当前默认完全相同时显式选择同值并回读默认；默认不匹配或不能协调并发选择就 blocked，不先改后还原。`selected` 不证明推理，`session/follow` 须核对实际请求 header、精确标记与本轮 `turn/end`。模型错误停止该烟雾，不自动换 preset/model 重试。

## 一轮一个 tab（Herdr 里的 Claude Code、agy、dsh headless）

Herdr `codex exec` 仅属遗留／用户明确点名 CLI 的路线，不是 Codex 默认或 Desktop 失败后的自动降级。用户点名 Claude Code／agy／DSH headless（或同类 CLI／pane）时走 Herdr；Herdr 承接用户指定的这类模型线，与 按调用方选择的 Codex 路线分开。

每一轮在任务 workspace 里新开一个 tab 运行，命令以 `echo $? > "$D/exit-<轮次>"; exit` 结尾：这一轮结束时 tab 关闭，Herdr 发 `pane_exited`。workspace 的第一个 pane（读取 pane）留着，用来读文件。每轮的输入输出都在 `~/Projects/_status/<任务名>/`：`brief-N.md`、`result-N.*`、`events-N.jsonl`、`stderr-N.log`、`exit-N`。

**派出与追问**：一次 `execute`，改上半部分即可。追问时 `n` 加 1，填上第 1 轮返回的 `workspaceId` 和 `cwd`，`agent` 换成续接参数（见下表）。Agenvo 的 `execute` 运行在 QuickJS 里，没有 `btoa`/`Buffer`，所以正文用下面的 `b64` 编码后经 shell 解码写文件，不经 shell 引号展开。`pane.run` 的命令上限 48000 字符，正文超过约 30 KB 时拆开或精简。

```js
// ---- 改这里 ----
const h = { deviceId: "<search 的 deviceId>", instanceId: "<Herdr 实例>" };
const session = "<session.list 的名字>", home = "<实例 context 里的 home>";
const repo = "<仓库>", task = "<仓库>-<任务>", write = true, n = 1, workspaceId = "", cwd0 = "";
const brief = "<完整任务正文>";
const agent = (D) => `claude -p --dangerously-skip-permissions --model deepseek/deepseek-flash --output-format json < "${D}/brief-${n}.md" > "${D}/result-${n}.json" 2> "${D}/stderr-${n}.log"`;
// ---- 以下照抄 ----
const b64 = (s) => { const u = unescape(encodeURIComponent(s)), t = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"; let o = ""; for (let i = 0; i < u.length; i += 3) { const x = (u.charCodeAt(i) << 16) | ((u.charCodeAt(i + 1) || 0) << 8) | (u.charCodeAt(i + 2) || 0); o += t[(x >> 18) & 63] + t[(x >> 12) & 63] + (i + 1 < u.length ? t[(x >> 6) & 63] : "=") + (i + 2 < u.length ? t[x & 63] : "="); } return o; };
const v = (r) => { if (r.error) throw new Error(JSON.stringify(r.error)); return r.result?.result ?? r.result; };
let cwd = cwd0, wid = workspaceId, ws;
if (n === 1) {
  cwd = write ? `${home}/Projects/.worktrees/${repo}/${task}` : `${home}/Projects/${repo}`;
  ws = v(await call(h, write ? "worktree.create" : "workspace.create", write
    ? { session, cwd: `${home}/Projects/${repo}`, branch: `agent/${task}`, base: "origin/main", path: cwd, label: `${repo} · ${task}` }
    : { session, cwd, label: `${repo} · ${task}` }));
  wid = ws.workspace.workspace_id;
}
const tab = v(await call(h, "tab.create", { session, workspaceId: wid, cwd, label: `${task}-${n}` }));
const fresh = n === 1 && write ? "git fetch -q origin && git merge -q --ff-only origin/main && " : "";
const run = await call(h, "pane.run", { session, paneId: tab.root_pane.pane_id,
  command: `D="$HOME/Projects/_status/${task}"; mkdir -p "$D" && printf %s '${b64(brief)}' | base64 -d > "$D/brief-${n}.md" && ${fresh}${agent("$D")}; echo $? > "$D/exit-${n}"; exit` });
return { n, cwd, workspaceId: wid, readerPane: ws?.root_pane.pane_id, turnPane: tab.root_pane.pane_id, run: run.execution, error: run.error };
```

`agent` 按目标换成下面一行（第 2 轮起加续接参数）：

| agent | `agent = (D) => ...` 的命令 | 第 2 轮起加 |
| --- | --- | --- |
| Claude Code | 见上（或带 magpie 探活的完整一行） | `--resume <session_id>`，沿用新建时的 `--model` |
| agy | `agy -p "$(cat "${D}/brief-${n}.md")" --output-format json --dangerously-skip-permissions --model <agy models 的 slug> > "${D}/result-${n}.json" 2> "${D}/stderr-${n}.log"` | `--conversation <conversation_id>` |
| dsh headless | `dsh --profile headless --json - < "${D}/brief-${n}.md" > "${D}/events-${n}.jsonl" 2> "${D}/stderr-${n}.log"` | `--session-id <sessionId>` |

Claude Code 的模型只用 `deepseek/deepseek-flash`（首选）或 `minimax-code/MiniMax-M3.1-Flash`，都经本机 magpie（`ANTHROPIC_BASE_URL` 指向 `http://127.0.0.1:3425`）；不用交互式 `agent.start kind claude`。agy 只在需要终端里逐步操作、处理信任提示时用 `agent.start`/`agent.prompt`。

**等这一轮结束**：
- 推送：订阅 `runtime.changed {deviceId, instanceId: <Herdr 实例>, nativeTypes: ["pane_exited"]}`，整个实例一条常驻订阅即可；事件的 `threadId` 是轮次 pane id，按它对应任务。
- 轮询：`pane.get {session, paneId: <轮次 pane>}` 返回 `pane_not_found` 就是这一轮结束了。
- 不要用 `agent.get` 判断结束：CLI 退出后 Herdr 立即释放该 pane 的 agent，返回 `agent_not_found`。运行中它有用：Codex 轮次的 `agent_session.value` 就是 thread_id。

**读结果**：两次 `execute`（`execute` 里不能等待）。先在读取 pane 打印，再 `pane.read {session, paneId: <读取 pane>, lines: 80}`：

```js
const D = `$HOME/Projects/_status/${task}`;
await call(h, "pane.run", { session, paneId: readerPane,
  command: `clear; D="${D}"; echo "EXIT-${n}=$(cat "$D/exit-${n}" 2>/dev/null)"; head -c 300 "$D/events-${n}.jsonl" "$D/result-${n}.json" 2>/dev/null | head -2; head -c 4000 "$D/result-${n}.md" 2>/dev/null` });
```

第一行是退出码；Codex 的 thread_id 在 `events-1.jsonl` 第一行（`thread.started`），Claude Code 的 `session_id` 与 `is_error`、agy 的 `conversation_id` 与 `status` 在 `result-N.json`，dsh 的 sessionId 在 `events-1.jsonl` 第一行。退出码非 0 或结果缺失时读 `stderr-N.log` 与 events，如实报告。

**中断**：`pane.send-keys {session, paneId: <轮次 pane>, keys: ["ctrl+c"]}`。zsh 会放弃这一行剩下的命令，tab 留在提示符；下一次 `execute` 用 `pane.run` 发 `echo 130 > "$HOME/Projects/_status/<任务名>/exit-<轮次>"; exit` 收尾。

## DSH headless：登记到桌面项目

dsh headless 只在用户明确要 CLI 通道时用，桌面 Host 须在线。第一轮跑起来后，在读取 pane 里从 `events-1.jsonl` 第一行取 sessionId，经本机插件 socket 登记到桌面项目（不拿会话写锁，CLI 运行中调用安全）：

```sh
sid=$(head -1 "$HOME/Projects/_status/<任务名>/events-1.jsonl" | sed -n 's/.*"sessionId":"\([^"]*\)".*/\1/p'); curl -s --unix-socket "$HOME/.local/state/agent-fleet-dsh/native.sock" -H 'content-type: application/json' -d "{\"args\":{\"path\":\"<任务目录>\",\"sessionId\":\"$sid\"}}" http://dsh/bridge/workspace/attachSession
```

返回 `ok:true` 后告诉用户「DSH 桌面左栏项目『<title>』下」和 sessionId；失败如实报告。已有项目直接归入、不改名；新项目用目录名，与已有项目重名时加 ` · <短父路径>`。connector 方法 `workspace/attachSession` 要等 semantic-craft/agenvo#7 合并并 `release agenvo` 后才有。

最后一轮 CLI 退出、不再用 `--session-id` 追问后，才可经 DSH 实例 `session/rename {request:{sessionId, title:"[CLI] <任务名>"}}`：改名会让桌面 Host 拿住写锁，之后 CLI 续写报 `already owned by an active write handle`，追问改走 DSH 实例的 `session/prompt`。桌面先建的会话不能反过来用 CLI 续写。

## Codex 实例（读历史）

列出与读取已有线程：`thread/list`、`thread/read`。新建派活走「Codex Desktop」一节。
