# plugins/

Self-made dispatch plugins: one directory per host plugin, each connecting a host app to Agent Network (see
[docs/anet.md](../docs/anet.md)). Source, `pack.py` and `smoke.sh` live in the plugin directory; the built pack goes under
`<plugin>/dist/` and is committed so an install does not need a build. Smoke evidence goes under `<plugin>/evidence/`
(gitignored). Runtime config with AIDs stays outside git.

| Dir | Host | Protocol | Status |
| --- | --- | --- | --- |
| [agent-network-cindy](agent-network-cindy/README.md) | Cindy | Agent Network (`anet --id cindy mcp`) | current (0.3.0) |

`agent-plugins` is only the marketplace/install root for other plugins; multi-agent dispatch plugins live here.
