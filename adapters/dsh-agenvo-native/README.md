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

The implemented `flow.js` registers through official `ctx.authorization.registerFlow`. Only an explicit `authorization.begin` starts the five-minute loopback listener at `http://127.0.0.1:13346/oauth/callback`, then SDK DCR/PKCE and a formal consent notice. Success means the official service observed a fresh credential commit; HTTP callback success alone is insufficient. Port conflict, timeout, cancellation and unload close the listener; no port fallback or automatic restart. Initial authorization uses client name **Agent Fleet DSH Native**. Pending authorization is memory-only.

`authorize.mjs` is a one-shot human surface built with the same official Cordis, LocalCredentialProvider and AuthorizationService. It is not a permanent proxy and is never run by installation or plugin startup. It prints only the intended formal consent URL and final status, never the callback/code/token. It accesses only the explicitly supplied DSH credential document through the official service, preserving other records under its lock. Run it only after approval. The browser must complete formal Allow; there is no approval bypass.

**New approval boundary:** this is a new native DSH client identity, with credentials stored as a plugin-owned grant in the configured DSH `credentials` provider, rather than the external bridge's cache. The default provider stores `$DSH_HOME/.credentials.yaml` (normally `~/.dsh/.credentials.yaml`) at owner-only 0600, with atomic replacement and cross-process locking. This is a local file, not Keychain encryption. Only a new `agent-fleet-agenvo-native/grant-<binding-hash>` record is added; existing records must remain unchanged. Confirm the actual path and protection before live use. Do not import dot, GrokBot or mcp-remote credentials. Plugin scope/only-two-tools is not server-side instance authorization. Agenvo grants cover the relay's approved instances; fixed server expiry cannot be extended by client refresh.

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

## Concrete install/authorize/load sequence (requires explicit live approval)

1. Prepare a nonsecret binding JSON with `serverUrl`, `issuer`, `resource`, `scope`. Agenvo's inspected Cloudflare source declares **runtime:approved**, not an invented per-instance scope. For a root relay, issuer is the relay origin and resource/serverUrl are its `/mcp` URL. Real discovery must agree or authorization fails.
2. `node prepare.mjs /path/binding.json /NEW/staging-directory` stages an immutable local bundle. `npm pack --ignore-scripts` in that directory produces the tarball. No install hooks exist. The generated patch inserts only `preset-fleet-agenvo`; it does not change `selectedDefault` or replace standard/PTC configurations.
3. In DSH's existing authorized Creator/plugin-manager surface, use `plugin_manager` action `install_bundle`, target the inspected local tarball, **enabled:false**. This is the official manager's supported option. Do not silently approve pending dependency build scripts, incompatible-version exemptions or alternate registries. Direct `dsh plugin --profile desktop` CLI guidance requires quitting Desktop; use the in-process manager instead and do not invoke that CLI against the running profile.
4. Run the installed package's `authorize.mjs` with absolute binding-file and confirmed credentials-file paths. Dependencies resolve from that installed package; no app-server/relay/connector restart is involved. The one-shot process opens only the fixed temporary loopback listener. The user opens the printed formal consent URL and clicks Allow. A busy port fails before DCR; cancel/expiry exits and closes it.
5. Only after `authorized`, call the official `plugin_manager` action `set_bundle` with target `agent-fleet-agenvo-native`, enabled:true. With current HMR support it adds the separate preset; create a new explicitly selected `fleet-agenvo` test session. Existing preset generations remain attached to existing sessions. Check the actual manager result: if `restart-required`, **stop** and report its exact reason before any restart. Never assume source HMR capability proves live HMR is enabled.
6. Verify actual scoped tool schemas and a second unrelated session before one approved pure-text two-turn dispatch. Live OAuth, runtime loading, DSH/GrokBot-originated dispatch and Desktop visibility remain separate acceptance items.

The supporting official rc.3 source paths are `packages/credentials/authorization`, `packages/credentials/credentials-local`, `packages/boot/plugin-manager/src/{tools,types}.ts`, `packages/boot/hmr`, and `packages/preset/agent-preset-registry`. The local checkout API is verified; installed application's exact build provenance and current HMR activation are not inferred from the tag.

Extra offline integration commands (existing approved DSH checkout only):

```sh
node adapters/dsh-agenvo-native/test/flow-official.mjs /path/to/approved/dsh-source
node adapters/dsh-agenvo-native/test/setup-official.mjs /path/to/approved/dsh-source
```

These use synthetic upstream responses and isolated loopback callbacks. The setup test uses a fresh temporary credential file, never the user's file. No real client or grant is registered.
