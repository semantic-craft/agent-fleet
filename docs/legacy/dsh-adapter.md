# DSH 桌面插件（已退役）

`adapters/dsh`（`agent-fleet-dsh`）和 `fleet.mjs release dsh` 已从本仓库删除。

它只做一件事：在 DSH 桌面 Host 里把 Typert 端点转到 `~/.local/state/agent-fleet-dsh/native.sock`，给 Agenvo 的 `dsh` connector 用。见 [ADR-0002](0002-minimal-agenvo-fork.md)。

现行派活不走这条 socket。官方接入是 `anet agents wire` 与 `anet mcp`，见 [anet.md](../anet.md)。本仓没有第二个消费者：Cindy 插件、`skills/delegate-via-anet` 和 `fleet.mjs check` 都不调用它。`release` 的组件表里也只剩这一项，所以安装脚本一并删了。

不要重新安装。机器上若仍有 `~/.local/share/agent-fleet/releases/dsh/` 或桌面 profile 里的 `agent-fleet-dsh` 链接，那是仓外残留，由外循环处理，不在本仓恢复发布命令。
