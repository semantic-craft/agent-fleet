# agent-network-cindy — Cindy × Agent Network

Cindy Ghost plugin. Host: **Cindy**; protocol: **Agent Network (anet)**. It runs as the local anet identity `cindy`:
tasks from allowed peers (own AIDs on the allow list) become Cindy tasks, and Cindy's final reply goes back as the anet
task result. Tools: `send_to_peer` (text to the configured `peerAid`), `bridge_status` (read-only).

- Source: `ghost.json`, `main.js`, `node/worker.cjs` (resident worker; stdio to `anet --id cindy mcp`, no listener).
- Runtime config (not in git): `~/Library/Application Support/CindyGlobal/agent-network-cindy/config.json`, dir 0700, file 0600:
  `{"anetBin": "<path to anet>", "anetId": "cindy", "peerAid": "<AID>", "allowPeers": ["<AID>", …]}` — `peerAid` must be in `allowPeers`.
- Pack: `python3 pack.py . dist` → `dist/agent-network-cindy-<version>.cindy`.
- Install (GUI): `open -a Cindy "$PWD/dist/agent-network-cindy-0.3.0.cindy"`, then 安装. Only one dispatch plugin per anet identity may be enabled.
- Smoke (after install): `./smoke.sh` → `A: PASS` with `CINDY_ANET_OK_20261102`. Direction B (`send_to_peer` → peer inbox `CINDY_ANET_ORIGIN_OK_20261102`): anet-delegated asks may be refused by Cindy unless user-authored; user-typed Cindy chat calling `send_to_peer` is the reliable B path.

History: 0.2.0 was `turing-cindy-dispatch` (agent-plugins PR #10, superseded). 0.3.0: tool `send_to_turing` → `send_to_peer`, config `turingAid` → `peerAid`, runtime dir → `agent-network-cindy/`. An earlier id `cindy-anet` was refused by Cindy (reserved prefix `cindy-`); the shipping id is `agent-network-cindy`.
