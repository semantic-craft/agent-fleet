# Agent Network：当前派活传输（2026-11-02）

只引用官方文档：<https://docs.agentnetwork.org.cn>。本页只记本机用法，不另造层。

## 现状

- 版本：官方 anet **0.3.9**（[llms.txt](https://docs.agentnetwork.org.cn/llms.txt)：Current release 0.3.9）。
- 本机身份：`default`、`turing`、`cindy`，一机多身份，各自 `~/.anet/ids/<name>`（`default` 用 `~/.anet`）。见 [Identity](https://docs.agentnetwork.org.cn/docs/en/latest/topics/identity.html)、[CLI：Identity and data directory](https://docs.agentnetwork.org.cn/docs/en/latest/reference/cli.html)。
- 入站：三个身份都是 `inbound policy closed`（默认值），只接 allow／trust 名单上的对端。见 [Delegation：inbound policy](https://docs.agentnetwork.org.cn/docs/en/latest/topics/delegation.html)、[Config：inbound.policy](https://docs.agentnetwork.org.cn/docs/en/latest/reference/config.html)。
- 名单：只 allow 自有 AID（`turing` ⇄ `cindy` 互相 allow）。不加外人，不放宽 policy。
- 派活：`anet --id turing delegate <AID> "…" --watch`。`--watch` 在 completed 时退出 0，failed／rejected／canceled 退出 1，input-required 退出 3（[CLI：Exit status](https://docs.agentnetwork.org.cn/docs/en/latest/reference/cli.html)）。参数只收完整 AID。
- 宿主插件：Cindy 侧是 [plugins/agent-network-cindy](../plugins/agent-network-cindy/README.md)，经 `anet --id cindy mcp`（stdio）用官方 MCP 工具 `list_tasks`／`get_task`／`reply_task`／`send_message`（[MCP tools](https://docs.agentnetwork.org.cn/docs/en/latest/reference/mcp-tools.html)）。

## 教训

- **peers allow／trust、放宽 inbound 要在终端确认**（[CLI：Confirmation on the terminal](https://docs.agentnetwork.org.cn/docs/en/latest/reference/cli.html)）。非 TTY 调用返回 `tty_required`。agent 代跑时须开 PTY 并回 `yes`，且只对用户已批准的自有 AID 这样做。
- **`ANET_HOME` 决定身份根目录**（默认 `~/.anet`，[Config：Environment variables](https://docs.agentnetwork.org.cn/docs/en/latest/reference/config.html)）。shell 里残留错的 `ANET_HOME` 会让 `--id` 指向别的身份树。本机命令写成 `env -u ANET_HOME ANET_DATA_DIR=$HOME/.anet ~/.local/bin/anet --id <name> …`。
- **同一身份只挂一个常驻处理者。** 两个插件／worker 轮询同一身份会重复接单；换插件先停旧的再 smoke。
