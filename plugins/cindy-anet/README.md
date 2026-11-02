# cindy-anet — Cindy × Agent Network

Cindy Ghost plugin. Host: **Cindy**; protocol: **Agent Network (anet)**. It runs as the local anet identity `cindy`:
tasks from allowed peers (own AIDs on the allow list) become Cindy tasks, and Cindy's final reply goes back as the anet
task result. Tools: `send_to_peer` (text to the configured `peerAid`), `bridge_status` (read-only).

- Source: `ghost.json`, `main.js`, `node/worker.cjs` (resident worker; stdio to `anet --id cindy mcp`, no listener).
- Runtime config (not in git): `~/Library/Application Support/CindyGlobal/cindy-anet/config.json`, dir 0700, file 0600:
  `{"anetBin": "<path to anet>", "anetId": "cindy", "peerAid": "<AID>", "allowPeers": ["<AID>", …]}` — `peerAid` must be in `allowPeers`.
- Pack: `python3 pack.py . dist` → `dist/cindy-anet-<version>.cindy`.
- Install (GUI): `open -a Cindy "$PWD/dist/cindy-anet-0.3.0.cindy"`, then 安装. Only one dispatch plugin per anet identity may be enabled.
- Smoke (after install): `./smoke.sh` → `A: PASS` and `B: PASS` with markers `CINDY_ANET_OK_20261102` / `CINDY_ANET_ORIGIN_OK_20261102`.

History: 0.2.0 was `turing-cindy-dispatch` (agent-plugins PR #10, superseded). 0.3.0 renamed id, tool `send_to_turing` → `send_to_peer`,
config key `turingAid` → `peerAid`, runtime dir `turing-cindy-dispatch/` → `cindy-anet/`.
