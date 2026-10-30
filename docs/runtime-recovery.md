# Runtime recovery and control-plane diagnosis

远程任务控制面是 CommHub MCP。本页诊断 connector 与原生运行时：沿用调用方已有的授权客户端，不把模型回复当成派活健康。Magpie 模型路由与可选的 Magpie MCP 转发是另一路依赖。一次可用的搜索或模型回复不能证明控制面或 runtime 可用。不要为了绕过边缘规则改道、复制其他客户端凭据，或把重新授权当成诊断捷径。

## Diagnose before changing anything

1. Discover the instance. `online` describes its connector, while `available` describes the native runtime. Preserve both values and the request receipt.
2. For DSH, read the configured native bridge `/health`. A missing socket means the bridge endpoint is absent, not that OAuth is invalid. Use normal desktop app state to distinguish a stopped application from an already-running app whose plugin failed. A normal start of a stopped, already-installed app may restore it without a release or restart. Do not restart an active app before assessing unrelated work and approval.
3. Recheck `/health`, discovery, and one native read after recovery. In an existing dedicated Fleet caller, `/agenvo-status` reports non-secret authorization/connection metadata; `connected` is not a completed-task receipt. Do not read credentials or start another OAuth flow.
4. For an MCP HTTP error, retain only status, content type, sanitized error detail and edge Ray ID. A Cloudflare `Server` header alone does not prove Cloudflare Access. Browser-signature rejection and OAuth errors are distinct. Use existing authorized security-event read access, scoped to the observed Ray IDs. Do not spoof fingerprints, rotate credentials, or change security rules to make a probe pass. Any proposed rule change needs its exact scope and separate approval.

## Verify the actual caller and return path

Use one approved pure-text test and an explicit model/provider/effort. The source DSH agent must call its own existing Agenvo tools, create an isolated target thread, read the first terminal result, and continue the same native thread for the second nonce. The caller returns both results in its original conversation. Independently read the target's complete turn items and match IDs, terminal status, exact text and absence of model tool items. Zero target tool items does not mean zero host hooks or source-agent tool calls.

Unknown admission is observe-only: reconcile the original native identity before any retry. A relay envelope request ID is not a durable native idempotency key. Do not intentionally replay a real write to demonstrate deduplication; use the existing FileJournal/conformance regressions. Read final results before unsubscribing a target thread. Unsubscribe does not establish Desktop visibility or absence of writer conflict.

## Proactive events are a separate acceptance gate

A connection `resync_required` event followed by a successful read proves recovery only. Polling and source-agent synchronous readback do not prove background wakeup. Leave existing subscriptions and legacy test sessions untouched. If the current subscription is restricted to another test session, first obtain approval for a narrowly scoped new subscription (or an explicitly agreed scope change), specifying the source instance/session, destination conversation, event kind, duration and teardown. A completion test must correlate its new nonce, native turn and actual delivered event. Deduplicate by event ID and native turn; duplicate/resync events must never submit work. A polling-only client cannot promise proactive completion delivery.

## Bounded observation, 2026-10-26

- A stopped DSH desktop app was started normally; the unchanged native bridge then returned healthy and discovery became available. No plugin release, network change, OAuth grant or shared-service restart was needed.
- Existing DSH native OAuth connected. DSH (`magpie / antigravity/gemini-4-argon / medium`, observed `openai-completions`) initiated two Codex turns on one new thread. Both returned their exact distinct pure-text nonces and completed; persisted full history contained only user/agent messages. Target readback reported `magpie / gpt-6-astra / medium`. The original DSH turn returned the receipt and completed. Final read preceded successful unsubscribe.
- Independently, local Magpie MCP forwarding and a bounded unauthenticated upstream probe returned a Cloudflare browser-signature 403; the account's HTTP analytics confirmed `block` for all three recorded Ray IDs. The exact rule identity was not established and no rule changed. Successful native-client execution does not repair or clear the forwarded route's failure.
- A pre-existing callback was scoped to a different legacy session. No subscription or legacy input changed. Fresh proactive completion, GrokBot origin and Desktop UI remained unverified.

Operational IDs and local paths stay in the task's private receipt, not this reusable guide. This observation is not an evergreen guarantee across upgrades.
