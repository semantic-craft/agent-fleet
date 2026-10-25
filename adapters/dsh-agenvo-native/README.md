# Native DSH → Agenvo caller (candidate, not loaded)

This optional namespace plugin reuses SDK 2.2 OAuth and the official DSH MCP tool projection. It creates no relay. Only the explicit human authorization command launches the system browser through the official no-shell native command helper. `apply` only uses an existing grant: it never opens a browser, registers a client or expands consent. Configuration is supplied by the host, not model arguments.

## Boundaries and evidence

- DSH rc.3 source/API fixture: SDK HTTP transport, real Cordis injection, Agent scope, official result projection and unload tested with synthetic fetch. No socket, production credential or model request. A separate real SDK fixture verifies discovery, DCR, S256 code exchange and early-401 refresh through injected synthetic fetch.
- Exactly `search` and `execute` are exposed in the mounted scope after connection. Unexpected catalogue and namespace conflicts fail connection; root scope fails closed; absent grant exposes only the human authorization command. Connection loss removes tools; the user can explicitly invoke `/agenvo-authorize` again to reconnect with the existing grant, without new DCR or replaying any task.
- ACK/unknown responses remain unchanged. There is no submit replay, native thread journal or automatic background wakeup in this raw MCP caller. Use the existing fleet harness/journal for supported durable operations; raw execute can bypass those policies.
- SDK may retry once after an HTTP 401 following refresh; generic errors/unknown outcomes are never retried here. Scope escalation throws. Cancellation of a transport call is not confirmation of remote task cancellation.
- Direct Codex same-host two-round execution was verified separately. DSH-originated and GrokBot-originated Codex tasks, official Desktop visibility, live native OAuth and restart/upgrade recovery remain **unverified**. This candidate does not promote them.

## Authorization and installation (not executed)

Before loading, verify peer versions and the actual DSH preset scope/lifecycle. The public credential service must be available in that host. Keep the plugin absent from global/preset config until approved; this repository does not autoload it.

Supply a nonsecret overlay with `serverUrl` (HTTPS `/mcp`), validated OAuth `issuer`, `resource`, and exact requested `scope`. No bearer tokens, callback codes, headers or personal endpoints belong in the repository. The host builds a credential key from the binding and the plugin name, preventing reuse across resources/issuers/scopes.

The implemented `flow.js` registers through official `ctx.authorization.registerFlow`. Only an explicit user command registers the temporary flow and calls `authorization.begin`, which starts the five-minute loopback listener at `http://127.0.0.1:13346/oauth/callback`, then SDK DCR/PKCE and a formal consent notice. Success means the official service observed a fresh credential commit; HTTP callback success alone is insufficient. Port conflict, timeout, cancellation and unload close the listener; no port fallback or automatic restart. Initial authorization uses client name **Agent Fleet DSH Native**. Pending authorization is memory-only.

The supported surface is the scoped human command `/agenvo-authorize` in DSH. Loading registers only that command. Its explicit invocation registers a transient native authorization flow, starts the bounded callback, and opens the validated issuer HTTPS URL through the official no-shell native helper on macOS. Other platforms are explicitly unsupported before any flow begins. The user personally clicks Allow. Consent URLs, callback values and errors are never written to command results/session history. Concurrent attempts for one credential key are rejected by the native authorization service. Disposal cancels the owning attempt. Existing grants reconnect without DCR. Successful authorization connects only this scope's two MCP tools; failed connection reports saved authorization separately.

The old 0.1.1 standalone Node instructions were incorrect: profile-installed packages may be symlinks, and DSH peers resolve through its in-process profile resolver. Ordinary Node cannot assume those peers are resolvable. The invalid `authorize.mjs` entry has been removed; `setup.js` is no longer shipped and remains an offline fixture, not supported production launchers. No private resolver or app archive extraction is used.

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
3. In DSH's existing authorized Creator/plugin-manager surface, use `plugin_manager` action `install_bundle`, target the inspected local package directory (or tarball), **enabled:false**. This is the official manager's supported option. Do not silently approve pending dependency build scripts, incompatible-version exemptions or alternate registries. Direct `dsh plugin --profile desktop` CLI guidance requires quitting Desktop; use the in-process manager instead and do not invoke that CLI against the running profile.
4. After explicit approval, enable the bundle through official `plugin_manager` `set_bundle`. This exposes the separate preset; it does not authorize or change the default. If the manager returns `restart-required`, stop and report before any restart.
5. Select `fleet-agenvo` explicitly in a new DSH session. Invoke `/agenvo-authorize` with no arguments using the existing command UI. The browser opens; personally inspect consent and click Allow. The callback is fixed at 127.0.0.1:13346 and bounded to five minutes. A busy port fails before DCR. Cancellation, browser launch failure or owner disposal closes the attempt. An unrelated session must not discover this command or tools.
6. Verify actual scoped tool schemas and a second unrelated session before one approved pure-text two-turn dispatch. Live OAuth, runtime loading, DSH/GrokBot-originated dispatch and Desktop visibility remain separate acceptance items.

The supporting official rc.3 source paths are `packages/credentials/authorization`, `packages/credentials/credentials-local`, `packages/boot/plugin-manager/src/{tools,types}.ts`, `packages/boot/hmr`, and `packages/preset/agent-preset-registry`. The local checkout API is verified; installed application's exact build provenance and current HMR activation are not inferred from the tag.

Extra offline integration commands (existing approved DSH checkout only):

```sh
node adapters/dsh-agenvo-native/test/flow-official.mjs /path/to/approved/dsh-source
node adapters/dsh-agenvo-native/test/setup-official.mjs /path/to/approved/dsh-source
```

These use synthetic upstream responses and isolated loopback callbacks. The setup test uses a fresh temporary credential file, never the user's file. No real client or grant is registered.

Actual host command integration: `node adapters/dsh-agenvo-native/test/action-official.mjs /path/to/approved/dsh-source`. This uses official Commands, AuthorizationService, SDK and synthetic HTTP responses; the browser opener is injected and no real grant is created.

Installation-shape regression: `node adapters/dsh-agenvo-native/test/packaged-host.mjs /path/to/approved/dsh-source`. It stages the actual package and preset, creates the same profile symlink form as the manager, reproduces ordinary Node peer-resolution failure, then loads all fixed peers and the declared plugin through the official profile resolver. Real Commands discovery is scope-isolated and unload removes it; no OAuth, browser, network or model request is made. This proves the fixed source/runtime contract, not the provenance or live UI of the signed desktop application.

## Keep the explicit caller preset; inspect status without authorizing

`/agenvo-status` is a scoped human command. It calls only the official `credentials.describeRecord` presence API and reads in-memory connection state. Its result contains configured/kind/writable and disconnected/connecting/connected/disposed. Expiry and granted scopes are **unknown**: the command does not read credential values, call `token()`, refresh, connect or start OAuth. Presence is not proof that a grant is valid. It accepts no arguments and does not change model, preset or global defaults.

A command-only session remains blank. In upstream rc.3, generic New Session can reuse it and intentionally apply the configured default when its create request omits `agentPreset`. This unloads this preset's commands/tools. Do not submit `/agenvo-authorize` as ordinary chat text if command discovery is absent; it could become a model prompt. `/agenvo-status` also leaves a session blank and is not a way to pin it.

The opt-in preparation helper always names the desired preset and caller identity:

```js
import {fleetCallerCreate, checkFleetCaller} from '../../scripts/dsh-native-caller.mjs';
// call is the existing authenticated Agenvo transport; this creates no new grant.
const identity = {sessionId: suppliedStableSessionId, workspaceId: suppliedWorkspaceId};
const descriptor = fleetCallerCreate(identity);
// Persist the one-shot submission decision in the caller's existing journal BEFORE dispatch.
// Never rerun this dispatch after a timeout, disconnect or restart with uncertain admission.
try { await call(descriptor.method, descriptor.params); } catch { /* observe same ID only */ }
const observation = await checkFleetCaller(call, {...identity, readCommands: existingNativeCommandDiscovery});
// If blocked: stop; do not retry create, switch a default, or send a model prompt.
// Ready is a point-in-time preset+command check, not a valid-grant assertion.
```

The helper constructs a request and performs readback; it is not a journal or automatic dispatcher. The caller must preserve its stable ID and submission decision using existing recovery infrastructure. Unknown admission, absent observation, or preset drift never authorizes recreation. The observation is a point-in-time check, not a lock: subsequent generic UI navigation can still reset an empty session. Use the explicit preset entry for reuse and recheck immediately before an authorized action. This patch does not change official generic New Session behavior.

Source evidence at DSH `1f81c047fc48fa190a683e469daa699857e9afe8`:
- `packages/client/ui-workspace/src/client/navigation.ts:221`: omitted preset permits any blank and omits the field from create.
- `packages/api/session-controller/src/agent.ts:460`: actual claim/reuse resolves omitted preset to default, then selects it.
- `packages/api/session-controller/tests/session-presets.host.spec.ts:238`: existing omitted-preset regression.
- `packages/api/session-controller/tests/session-list-blank.host.spec.ts:57`: command lifecycle does not clear blank.

Offline lifecycle test (existing approved source and built dependencies, no installation):

```sh
node --import /path/to/dsh/node_modules/tsx/dist/loader.mjs adapters/dsh-agenvo-native/test/preset-reuse-official.mjs /path/to/dsh
```

This executes the real upstream `ApiSessionAgentController` create/reuse and official Commands/SessionStore, with an injected synthetic preset roster that mounts/removes our actual status command. It proves the create/reuse boundary and command availability, not a full UI click or production profile reload. No credential contents, network, model, or real session is used.

Public offline planning entry: `node scripts/fleet.mjs caller-plan --session STABLE_ID --workspace WORKSPACE_ID`. It prints only the explicit create descriptor, never dispatches, reads an overlay or starts OAuth. For automated preflight use `checkFleetCaller(call, {sessionId, readCommands})`, injecting an existing supported native command-discovery function. Missing discovery support or either missing status/authorize command is blocked; a `ready` result permits only an explicitly authorized status action, not an authentication claim or blanket permission to dispatch.

The create descriptor intentionally omits `permission`; it neither requests full access nor changes global policy. The caller must inspect the existing connector’s default permission behavior and explicitly supply only its authorized policy. `observeFleetCaller` alone returns `observed/caller_preset_matches`, not ready; use `checkFleetCaller` for both scoped commands. No precheck provides a server-side lock against a later UI preset change.
