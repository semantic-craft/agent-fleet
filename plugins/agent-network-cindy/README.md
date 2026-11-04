# agent-network-cindy — Cindy × Agent Network

Cindy Ghost plugin. Host: **Cindy**; protocol: **Agent Network (anet)**. It runs as the local anet identity `cindy`:
tasks from allowed peers (own AIDs on the allow list) become Cindy tasks, and Cindy's final reply goes back as the anet
task result. Tools: `send_to_peer` (text to the configured `peerAid`), `bridge_status` (read-only).

- Source: `ghost.json`, `main.js`, `node/worker.cjs` (resident worker; stdio to `anet --id cindy mcp`, no listener).
- Runtime config (not in git): `~/Library/Application Support/CindyGlobal/agent-network-cindy/config.json`, dir 0700, file 0600:
  `{"anetBin": "<path to anet>", "anetId": "cindy", "peerAid": "<AID>", "allowPeers": ["<AID>", …]}` — `peerAid` must be in `allowPeers`.
- Pack: `python3 pack.py . dist` → `dist/agent-network-cindy-<version>.cindy`.
- Install (GUI): `open -a Cindy "$PWD/dist/agent-network-cindy-0.3.2.cindy"`, then 安装. Only one dispatch plugin per anet identity may be enabled.
- Smoke (after install): `./smoke.sh` → `A: PASS` with `CINDY_ANET_OK_20261102`. Direction B (`send_to_peer` → peer inbox `CINDY_ANET_ORIGIN_OK_20261102`): anet-delegated asks may be refused by Cindy unless user-authored; user-typed Cindy chat calling `send_to_peer` is the reliable B path.

History: 0.2.0 was `turing-cindy-dispatch` (agent-plugins PR #10, superseded). 0.3.0: tool `send_to_turing` → `send_to_peer`, config `turingAid` → `peerAid`, runtime dir → `agent-network-cindy/`. An earlier id `cindy-anet` was refused by Cindy (reserved prefix `cindy-`); the shipping id is `agent-network-cindy`.

### 0.3.1 delivery safeguards (source/package; not an installation claim)

The worker records an uncertain host create before issuing it. A timeout, restart or host error never automatically creates another run. Observation deadlines are local `unverified` states, not ANet task failures. An uncertain terminal reply is reconciled with `get_task`; observing the requested state does not verify receipt delivery or the effect. It is never automatically resent. State persistence errors stop the next side effect.

Active runs check the official task before observing the host, and send a working update at most every five minutes while a host run remains active. Official termination or a changed follow-up text requires operator reconciliation; this adapter does not claim to cancel the Cindy run or forward follow-ups. Recovery requires checking both task histories and the original host run, not starting a replacement. `delivery-state-observed` means only that the official state matches; `unverified` records are deliberately retained.

Contract evidence: installed `anet guide overview` (0.3.9), and [official MCP tools](https://docs.agentnetwork.org.cn/docs/zh-cn/latest/reference/mcp-tools.html). Automated mock tests cover uncertainty handling; real host/peer acceptance remains separate.

Discovery follows official `nextPageToken` / `page_token` pagination. Official states accept both lower-case and `TASK_STATE_*` forms. Only the exact host outputMessageId can complete a task; unavailable or oversized output requires reconciliation. Persisted state corruption fails startup closed. Runs bind to the initial official user messageId. A different latest user messageId, a missing ID, unavailable user history, or a legacy state without this binding requires operator reconciliation; identical text never proves the same request. Continuation support remains unverified.

Outgoing sends persist the host `callId` and a fresh local correlation ID before sending (the ledger field is `messageId`; it is not sent to ANet). Repeating the same callId never sends twice: a known task is read back, and a lost task ID remains unverified for operator reconciliation. A new callId is a new request, so callers must not retry an uncertain send as a new tool call. No unknown send is reported as sent. This is local duplicate suppression only; no network idempotency guarantee is claimed. Input over 32768 characters is held for reconciliation before host creation, not silently shortened. Missing state after deployment is not distinguishable from first installation; preserve and back up the runtime ledger during upgrades.

Task reconciliation requests a bounded 100-message history: the 30-minute observation window produces at most six five-minute progress messages, so adapter progress alone cannot displace the initiating user message. If external activity still pushes that message outside the window, reconciliation remains fail-closed; no text-based identity fallback is used.

### 0.3.2 exact-result pagination and bounded recovery

Cindy 0.1.124 applies a 64 KiB history-page budget even when `limit: 100` is requested. A real 33-message assessment therefore returned a cursor before its final answer. `statusJob` now follows `nextCursor` (maximum 32 pages, repeated cursors rejected), stops at the original run's exact assistant `outputMessageId`, and never substitutes the most recent assistant message. Running tasks do not fetch output history.

`reconcile_result(taskId)` is an explicit recovery operation for an existing `EXACT_OUTPUT_UNAVAILABLE` record. It rechecks the original ANet request message ID, allowed peer, nonterminal official task and exact original host run. It never creates or sends a new host run; uncertain prior deliveries are ineligible. It persists terminal-delivery uncertainty before replying, retaining the existing no-resend rule.

2026-11-04 local acceptance: 25 focused tests passed; installed main/worker matched the 0.3.2 package. A previously stuck real assessment returned through the same ANet task with `completed`, confirmed delivery and verified receipt. Cindy then originated a separate text-only reverse task; the provider answered it through official MCP and Cindy's node read the expected acknowledgement with a verified receipt. No allow/trust/inbound changes or credential creation. Identifiers and private task content remain in the consumer project's ignored handoff receipt.

### Local rollback

Both 0.3.1 and 0.3.2 packages are retained under `dist/`. If a regression requires reverting program code, reinstall the prior package through Cindy's existing plugin UI and verify the active version. Preserve the current runtime configuration and delivery ledger; never restore an older ledger snapshot merely to roll back code, because doing so can replay already-created tasks or lose uncertain-delivery records. Version 0.3.1 lacks the pagination fix and reconciliation tool, so long-output recovery remains unavailable after that rollback. Do not run two workers for the same identity.
