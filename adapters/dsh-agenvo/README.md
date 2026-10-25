# DSH as an Agenvo caller (external bridge candidate; paused)

This optional namespace plugin composes the official DSH MCP client with an explicitly approved `mcp-remote` stdio OAuth transport. It exposes Agenvo's existing `search` and `execute`, just as other MCP callers do. It does not create a relay, Codex child process, dispatcher, new network service, credential provider or copy of the inbound `agent-fleet-dsh` bridge. Magpie remains model inference.

## Contract and limits

Mount only on the intended DSH Agent/preset scope. `apply` refuses an unscoped Host context; the scope primitive itself is opaque and does not certify that the installer selected the intended Agent. The installer must inspect actual `tools.schemas(agent)` and verify isolation with a second scope. The official MCP client owns ToolRuntime registration, tool execution policy, child-fiber disposal and transport shutdown. Root-wide enabling is not supported. This is a native MCP caller, **not** a restricted four-tool facade: authorized `execute` retains native Agenvo methods and cannot be advertised as a sandbox or durable idempotent dispatcher.

No persistent run journal is added. Existing `scripts/file-journal.mjs` and DSH Harness stay unchanged; their DSH-specific event validator is not a Codex validator. Future bounded automation must reuse storage primitives and supply the correct native adapter, rather than treating raw MCP calls as journalled. Native IDs must be saved by the caller; Agenvo envelope request IDs are not persistent idempotency keys. Unknown writes require native readback with original identity, never blind resubmission. Missing thread identity after unknown creation is blocked pending reconciliation, not permission to create another thread. Cancellation acceptance is not terminal cancellation. This plugin does not auto-retry calls, steer or cancel; unknown is returned unchanged.

Agenvo ACK is not completion. Follow native status/history through a separate execute call; do not keep execute polling until the model finishes. Same-thread continuation uses the exact returned thread and native turn identities. Notifications can wake a consumer but do not substitute for native readback. **DSH background wakeup/subscription integration is pending**; explicit observe is not proof of push delivery. Official Codex Desktop visibility, DSH→Codex and GrokBot→Codex end-to-end acceptance remain pending.

## Exact prerequisite and approval

Agenvo's standard client requires dynamic OAuth registration, S256 PKCE, Streamable HTTP and refresh. The inspected official client configuration does not wire authProvider; this does not mean the SDK or DSH ecosystem lacks OAuth. This optional external bridge uses reviewed `mcp-remote v0.1.38`; the [native candidate](../dsh-agenvo-native/README.md) instead uses the existing SDK and DSH credential service. Do not use on-demand `npx`, floating versions, shared client caches or copied tokens.

If explicitly resuming this alternative, its installation/load requirements are:

1. Install the reviewed transport into a versioned local location with its dependency lock retained; record package integrity and actual script/dependency hashes. No existing release is installed by this repository change.
2. Create one **dedicated DSH-only 0700 OAuth directory** and reserve an approved fixed loopback callback port (1024–65535). Config validation reads only path metadata, never credentials. Use canonical absolute paths; no symlink cache. Changing callback port can force transport client re-registration, so do not silently change it.
3. Load this module in one selected Agent/preset scope. This spawns the stdio proxy and may open the browser/loopback callback on first authentication. On Agenvo's own page the user signs in and clicks Allow. The administrator key is never supplied to the agent or configuration.
4. The real grant scope is **all currently approved instances and future approvals**, not just Codex. DSH tool scoping does not narrow that grant. Access tokens last 15 minutes; grants up to 30 days per inspected Agenvo version. User consent is required for this new persistent DSH client. No new connector pairing or Cloudflare/Tailscale/network change is needed.

Status: fixed 0.1.38 transport was installed with scripts disabled, started once for client registration and browser consent, then stopped on user pause. The extra installation was subsequently moved to Trash at the user’s request; the old path is absent and no public executable symlink pointed there. Its separate cache remains, with no observed token file; remote grant completion is not confirmed or revoked. No DSH plugin was loaded. Do not resume from the old approval without confirming the selected approach. Existing Dot grants and inbound connector authorization cannot be reused as implicit approval.

## Load after approval

The installed DSH loader can load this package by an approved absolute package path or configured package name. It intentionally has no automatic Host bundle patch: add it only to the selected preset's `agent.cordis.yml`, using the loader's standard name/config shape. Verify the installed runtime's preset loading path before modifying it. Example non-secret config (replace all placeholders using discovery):

```yaml
- name: /absolute/approved/agent-fleet/adapters/dsh-agenvo
  config:
    relayUrl: https://relay.example.com/mcp
    nodePath: /absolute/reviewed/node
    proxyPath: /absolute/reviewed/mcp-remote/dist/proxy.js
    oauthDirectory: /absolute/private/dsh-agenvo-oauth
    callbackPort: 13345 # example only; approve an unused fixed port
```

The module builds: `node proxy.js <relay>/mcp <port> --transport http-only --host 127.0.0.1`, with only `MCP_REMOTE_CONFIG_DIR` added to the official client's scrubbed environment. `failOnStartupError=true`; automatic client reconnection is disabled for this initial candidate. Startup/auth errors propagate, never silently register an empty working client. Transport token refresh is its standard behavior, not a new authorization implementation.

## Offline acceptance and migration

`npm test` runs config/composition negative tests. For a separately approved source checkout with dependencies **already built**, run:

```sh
node adapters/dsh-agenvo/test/official-client.mjs /absolute/dsh-source
```

This launches only our stdio fixture, not mcp-remote, OAuth, a model, or Desktop. It uses the real official MCP client, Cordis scope and ToolRuntime registry: search/execute appear only in the owner scope, ACK/unknown are preserved, observe does not resubmit, disposal removes registrations. Tool execution in this fixture directly invokes the official tool definition; it is not live Agent approval-policy acceptance. The fixture's fixed HTTPS argument is ignored by its server: it has no network code. Source `rc.3` checkout was used; local built library provenance is recorded in the repository’s `test/official-client-evidence.json` by hashes (not a package-install attestation), not assumed identical merely from its version string.

On a new machine rebuild/reinstall reviewed dependencies, recreate the private directory, and perform fresh user authorization; do not copy grants by default. Upgrade candidates must pass offline tests then an explicitly approved discovery/read-only canary, real Agent scope/permission isolation, same-thread continuation, unknown-result recovery and unload/restart. Keep prior package/config paths for rollback; unloading the scope disposes the child transport but does not revoke the OAuth grant. Revoke the DSH client through Agenvo's normal admin UI if abandoning it; do not revoke another client's grant. Cache deletion is a separate user decision. No production release command was added.
