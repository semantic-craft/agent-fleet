# Native DSH → Agenvo caller (candidate, not loaded)

This optional namespace plugin reuses SDK 2.2 OAuth and the official DSH MCP tool projection. It creates no relay or child process. `apply` only uses an existing grant: it never opens a browser, registers a client or expands consent. Configuration is supplied by the host, not model arguments.

## Boundaries and evidence

- DSH rc.3 source/API fixture: SDK HTTP transport, real Cordis injection, Agent scope, official result projection and unload tested with synthetic fetch. No socket, production credential or model request. A separate real SDK fixture verifies discovery, DCR, S256 code exchange and early-401 refresh through injected synthetic fetch.
- Exactly `search` and `execute` are exposed in the mounted scope. Unexpected catalogue, namespace conflict, absent grant and root scope fail closed. Connection loss removes tools; reconnect requires explicit remount.
- ACK/unknown responses remain unchanged. There is no submit replay, native thread journal or automatic background wakeup in this raw MCP caller. Use the existing fleet harness/journal for supported durable operations; raw execute can bypass those policies.
- SDK may retry once after an HTTP 401 following refresh; generic errors/unknown outcomes are never retried here. Scope escalation throws. Cancellation of a transport call is not confirmation of remote task cancellation.
- Direct Codex same-host two-round execution was verified separately. DSH-originated and GrokBot-originated Codex tasks, official Desktop visibility, live native OAuth and restart/upgrade recovery remain **unverified**. This candidate does not promote them.

## Authorization and installation (not executed)

Before loading, verify peer versions and the actual DSH preset scope/lifecycle. The public credential service must be available in that host. Keep the plugin absent from global/preset config until approved; this repository does not autoload it.

Supply a nonsecret overlay with `serverUrl` (HTTPS `/mcp`), validated OAuth `issuer`, `resource`, and exact requested `scope`. No bearer tokens, callback codes, headers or personal endpoints belong in the repository. The host builds a credential key from the binding and the plugin name, preventing reuse across resources/issuers/scopes.

The host-only `createHostAuthorization(ctx, config)` creates an authorization controller. Explicit `beginAuthorization` uses SDK `auth` and public-client metadata (`token_endpoint_auth_method: none`, authorization_code + refresh_token grants, approved loopback redirect). It returns an authorization URL for the user's formal consent and a one-use `finish(callbackUrl)` handler. The embedding host must own the approved callback listener and close it on finish/cancel/timeout; this module does not start a listener or register a UI endpoint. Pending authorization is memory-only. Do not expose this controller as an Agent tool. Startup with missing credentials fails instead of silently authorizing.

**New approval boundary:** this is a new native DSH client identity, with credentials stored as a plugin-owned grant in the configured DSH `credentials` provider, rather than the external bridge's cache. The provider's storage location, approved callback/consent UI, and scope must be confirmed before live use. Do not import dot, GrokBot or mcp-remote credentials. Plugin scope/only-two-tools is not server-side instance authorization. Agenvo grants cover the relay's approved instances; fixed server expiry cannot be extended by client refresh.

The earlier fixed mcp-remote 0.1.38 bridge was installed and started once, registered a client and opened consent; it was stopped on user pause. Its dedicated cache exists without an observed token file, and no DSH plugin was loaded. The extra transport installation was subsequently moved to Trash at the user’s request; its former installation path is absent. The independent OAuth registration cache remains preserved, with no token file observed; remote grant state is unknown and no revocation was attempted. The bridge is an alternative candidate, not an active fallback; never automatically switch identity/transport on failure.

## Repeatable local verification

```sh
npm test
# Optional existing SDK fixture; omitted env is reported as a skip, never downloaded.
DSH_MCP_CLIENT_TEST_MODULE=file:///path/to/existing/sdk/dist/index.mjs node --test adapters/dsh-agenvo-native/test/oauth-sdk.test.mjs
node adapters/dsh-agenvo-native/test/official-client.mjs /path/to/approved/dsh-source
```

The second command uses **existing** rc.3 built libraries and SDK dependencies with synthetic fetch only. It does not build a clean DSH distribution or prove installed desktop provenance. No third-party install script is invoked.

## Migration, upgrade and rollback

Keep configuration nonsecret and portable; reauthorize on a new machine through its chosen DSH credential provider. Do not copy pending authorization codes or another client's cache. A record identity/schema mismatch blocks; unknown in-flight task outcomes require native readback, never resubmission. Verify SDK/credential API contracts and offline negative tests before a candidate update, then separately approve a bounded live consent/discovery/two-turn canary. Model/desktop evidence must not be inferred from OAuth success.

Rollback means unload this scoped plugin (removes its tools), retain task/native IDs for reconciliation, and revoke its grant through Agenvo's management UI if retiring it. Removing local credentials alone does not revoke the server grant or cancel remote tasks. Do not restart shared services or replace their configuration as part of rollback.

## Sources and local changes

See `references.json`. No community source was copied; implementation uses public SDK/DSH APIs. Community patterns informed the design. Unlike the referenced expiry-only refresh guard, an early 401 forces locked refresh even before expiry; this is protected by negative tests. Future upgrades must preserve that test and the scope/disposal/unknown-result tests.
