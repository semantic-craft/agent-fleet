# DSH session-only selection candidate

Status: source candidate only; **not deployed**, no live model request performed.
The installed runtime keeps its old behavior until an explicitly approved runtime
and connector release is installed. This change does not modify Magpie or presets.

## Rebuild inputs and boundaries

| Source | Fixed base | Patch |
| --- | --- | --- |
| `https://github.com/deepseek-ai/deepseek-harness` | `1f81c047fc48fa190a683e469daa699857e9afe8` (official rc.3 tag) | `patches/dsh-session-only/runtime.patch` |
| `https://github.com/semantic-craft/agenvo` | `cd10fb92e1da731b307b85f8e94e8db8b6b8808a` | `patches/dsh-session-only/agenvo.patch` |

Use separate clean checkouts at these exact revisions. From each checkout run
`git apply --check <absolute-patch-path>`, then `git apply <absolute-patch-path>`.
A mismatch is a rebase/review requirement, never a reason to force application.
Retain upstream LICENSE (MIT for DSH, Apache-2.0 for Agenvo); copies accompany the
patches. These small diffs are not vendored runtime dependencies. No new fork or
upstream PR is required to replay locally. On upgrade, retest the patch against
the new source and capability contract; do not infer support from a version string.

The runtime reuses provider/model validation and per-session admission
serialization. The old API keeps its background global-default save. The new
`session/selectModelForSession` never calls that save and returns
`{selected, scope:'session', status:'applied', seq}`. A running turn's already
assembled request stays unchanged; selection applies to a later assembly, not a
promise to replace an in-flight model. Invalid selections must leave no selection
event. This API has no request-id deduplication or exclusive-owner lease.

The existing selection event participates in the normal session log/restore
mechanism. Sequence assignment and cached application **do not prove disk flush**.
After crash/restart read back the session and verify the actual next request
header; do not replay an uncertain mutation. This patch does not introduce a
persistence barrier or claim durable restoration.

Agenvo advertises the optional native operation only after live
`session/modelCatalog` returns `capabilities.selectModelForSession === true`, and
rechecks before each invocation. Missing/false capability is unsupported. A lost
write response remains unknown. Legacy global-saving selection is never used as
fallback. The capability read is not an atomic runtime-generation lease: a restart
between probe and write can yield a native rejection/unknown, not success.

## Explicit caller entry

Reuse the existing authorized Agenvo `call` binding for the intended instance.
No new OAuth grant, private endpoint, credential, or service is created by this
module. The caller must own the session exclusively and authorize this selection.

```js
import { selectSessionModel } from './scripts/session-model-selection.mjs';
const result = await selectSessionModel(call, {
  sessionId: ownedSessionId,
  selection: { provider: chosenProvider, model: chosenModel, reasoningEffort: chosenEffort },
  exclusiveSession: true,
});
if (result.status !== 'applied') {
  // Stop. unknown means read the original session; never retry or use selectModel.
  throw new Error(result.code);
}
// Use the existing HarnessRunner with the expected model/effort/preset and stable
// native request ID. Verify its actual request header and task acceptance.
```

`call(method, params)` returns the explicit Agenvo native receipt (not a hidden
execute result). `exclusiveSession` is an operator assertion, not a lock. Local
caller validation requires explicit effort; the runtime itself may normalize
omitted effort. Unsupported or invalid inputs cause no write. Applied is not task
completion, header verification, model compatibility, or approval to run tools.
Route profile boundaries remain unchanged: Sol PTC marker success does not imply
its failed read-only tool route works.

## Development and release gate

Use locked dependencies and disable lifecycle scripts during dependency materialization.
Do not launch either built runtime as an offline test. DSH may need explicit
reviewed native build steps for its host suite; dependency install alone is not
permission to execute arbitrary postinstall scripts.

DSH commands (tested Node 24.22.1, official pnpm 11.28.5):

```sh
pnpm install --frozen-lockfile --ignore-scripts
node node_modules/vitest/vitest.mjs run \
  packages/api/session-controller/tests/session-models.host.spec.ts \
  packages/api/session-controller/tests/session-cold.host.spec.ts \
  packages/api/session-controller/tests/agent.host.spec.ts
node --import tsx/esm scripts/generate-typert.ts
node --import tsx/esm scripts/build.ts
```

Acquire the exact official package-manager version separately if absent, verifying
registry integrity; do not silently use a different system shim or change lock.
For the tested isolated build, `npm_execpath` points explicitly at the temporary
pnpm 11.28.5 `bin/pnpm.mjs`, with temporary empty HOME, minimal PATH/TMPDIR,
`CI=true`, `COREPACK_ENABLE_NETWORK=0`, `npm_config_ignore_scripts=true` and no
inherited credentials. Dependency acquisition is separate from test execution.
On macOS, the test/generator wrapper was `sandbox-exec -p '(version 1)(allow
default)(deny network*)'`; build permits only local Unix IPC additionally via
`(allow network* (local unix-socket))` for tsx, still denying INET. This is a
local test sandbox, not a change to system network settings.

Agenvo commands: `npm ci --ignore-scripts`,
`node_modules/.bin/tsx --test tests/dsh.test.ts`, `npm run check`, `npm run build`.
Full `npm test` additionally needs its Lody SQLite native test dependency; do not
silently report the complete suite green when only DSH tests passed.
Fleet commands: `node --test test/session-model-selection.test.mjs`, `npm test`.

Before deployment, obtain approval for the exact DSH artifact/signing/distribution
and connector release and idle restart window. Preserve existing package and
configuration, inspect migrations, and never overwrite the official signed app
with an unsigned source candidate. In the approved canary, prove unchanged global
default, two independent sessions, actual next-request headers, unknown-result
handling and restart recovery; then run the cloud Agenvo marker/tool acceptance
for only approved profiles. Roll back the connector release and runtime package
only if their data formats remain compatible; otherwise stop for an explicit
recovery plan. No service install/restart or live canary was performed here.

## Recorded offline results

- DSH fixed rc.3 candidate: 67/67 focused host tests; 9/9 repository build stages
  (native, host/client compilation and bundles, Desktop/Web bundles and aggregate
  checks); Typert generation 30 artifacts; diff whitespace check passed.
- The new header test feeds assembly output into a manually appended header and
  reads it back. It does **not** prove complete agent-loop automatic header
  generation or any real model request. Cross-session/serialized selection,
  validation failure, managed child-session refusal and legacy-save behavior are
  covered by synthetic host fixtures.
- Agenvo candidate: DSH Unix-socket fixture tests 9/9, type check and build passed.
  Full unit suite: 100 passed, 5 failed, 2 skipped. All five failures are Lody
  fixtures missing `better-sqlite3` native bindings after `--ignore-scripts`;
  no complete-suite success is claimed. Adapter/system/live tests not run.
- Both patches pass `git apply --cached --check` against their exact bases using
  temporary indexes; original indexes and parallel worktrees are unchanged.
- Runtime/connector candidates have only offline evidence. Installed signed
  source provenance, deployment, crash-durable acknowledgment, physical new-host
  migration and fresh cloud behavior remain separate gates.
