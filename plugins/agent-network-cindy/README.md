# agent-network-cindy — Cindy × Agent Network

Cindy Ghost plugin. Host: **Cindy**; protocol: **Agent Network (anet)**. It runs as the local anet identity `cindy`:
tasks from allowed peers (own AIDs on the allow list) become Cindy tasks, and Cindy's final reply goes back as the anet
task result. Tools: `send_to_peer` (text to the configured `peerAid`), `bridge_status` (read-only).

- Source: `ghost.json`, `main.js`, `node/worker.cjs` (resident worker; stdio to `anet --id cindy mcp`, no listener).
- Runtime config (not in git): `~/Library/Application Support/CindyGlobal/agent-network-cindy/config.json`, dir 0700, file 0600:
  `{"anetBin": "<path to anet>", "anetId": "cindy", "peerAid": "<AID>", "allowPeers": ["<AID>", …]}` — `peerAid` must be in `allowPeers`.
- Pack: `python3 pack.py . dist` → `dist/agent-network-cindy-<version>.cindy`.
- Install (GUI): `open -a Cindy "$PWD/dist/agent-network-cindy-0.3.1.cindy"`, then 安装. Only one dispatch plugin per anet identity may be enabled.
- Smoke (after install): `./smoke.sh` → `A: PASS` with `CINDY_ANET_OK_20261102`. Direction B (`send_to_peer` → peer inbox `CINDY_ANET_ORIGIN_OK_20261102`): anet-delegated asks may be refused by Cindy unless user-authored; user-typed Cindy chat calling `send_to_peer` is the reliable B path.

History: 0.2.0 was `turing-cindy-dispatch` (agent-plugins PR #10, superseded). 0.3.0: tool `send_to_turing` → `send_to_peer`, config `turingAid` → `peerAid`, runtime dir → `agent-network-cindy/`. An earlier id `cindy-anet` was refused by Cindy (reserved prefix `cindy-`); the shipping id is `agent-network-cindy`.

### 0.3.1 delivery safeguards (source/package; not an installation claim)

The worker records an uncertain host create before issuing it. A timeout, restart or host error never automatically creates another run. Observation deadlines are local `unverified` states, not ANet task failures. An uncertain terminal reply is reconciled with `get_task`; observing the requested state does not verify receipt delivery or the effect. It is never automatically resent. State persistence errors stop the next side effect.

Active runs check the official task before observing the host, and send a working update at most every five minutes while a host run remains active. Official termination or a changed follow-up text requires operator reconciliation; this adapter does not claim to cancel the Cindy run or forward follow-ups. Recovery requires checking both task histories and the original host run, not starting a replacement. `delivery-state-observed` means only that the official state matches; `unverified` records are deliberately retained.

Contract evidence: installed `anet guide overview` (0.3.9), and [official MCP tools](https://docs.agentnetwork.org.cn/docs/zh-cn/latest/reference/mcp-tools.html). Automated mock tests cover uncertainty handling; real host/peer acceptance remains separate.

Discovery follows official `nextPageToken` / `page_token` pagination. Official states accept both lower-case and `TASK_STATE_*` forms. Only the exact host outputMessageId can complete a task; unavailable or oversized output requires reconciliation. Persisted state corruption fails startup closed. Runs bind to the initial official user messageId. A different latest user messageId, a missing ID, unavailable user history, or a legacy state without this binding requires operator reconciliation; identical text never proves the same request. Continuation support remains unverified.

Outgoing sends persist the host `callId` and a fresh local correlation ID before sending (the ledger field is `messageId`; it is not sent to ANet). Repeating the same callId never sends twice: a known task is read back, and a lost task ID remains unverified for operator reconciliation. A new callId is a new request, so callers must not retry an uncertain send as a new tool call. No unknown send is reported as sent. This is local duplicate suppression only; no network idempotency guarantee is claimed. Input over 32768 characters is held for reconciliation before host creation, not silently shortened. Missing state after deployment is not distinguishable from first installation; preserve and back up the runtime ledger during upgrades.

Task reconciliation requests a bounded 100-message history: the 30-minute observation window produces at most six five-minute progress messages, so adapter progress alone cannot displace the initiating user message. If external activity still pushes that message outside the window, reconciliation remains fail-closed; no text-based identity fallback is used.
