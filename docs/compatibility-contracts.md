# Compatibility and recovery gate API

These pure functions evaluate caller-supplied evidence. They do not discover a
machine, authenticate evidence, probe a provider, deploy software, or submit a
task. `source: observed` is a claim made by the evidence-producing adapter, not
an independent measurement by this module. Use actual runtime/catalog/header and
bounded canary evidence; do not manufacture verified receipts from a model name.

## Compatibility

```js
import { assessCompatibility, identityHash } from '../scripts/compatibility.mjs';
const decision = assessCompatibility(manifest, { now: Date.now() });
// Only status === 'verified' passes this evidence gate. It does not authorize work.
```

`compatibility.example.json` deliberately contains unknown/candidate identities
and expired simulated evidence. It must block until replaced with actual facts.
There is no permanent verified default. Each manifest has exactly these fields:

- `identity`: concrete strings for `runtime`, `adapter`, `provider`, `model`,
  `modelAlias`, `effort`, `preset`, `policy`, `harness`, `schema`. Effort, preset
  and policy are separate required fields: moving from PTC to standard or changing
  permission/policy invalidates the prior canary even if the model name is unchanged. Include pinned version/build/content identity
  in these strings. `model` identifies the resolved model; `modelAlias` records
  the alias used. Changing either requires another canary. A string is not proof
  of exact provenance. In particular, an unknown deployed gateway source must
  remain unknown, not be replaced by a nearby checkout's revision. Historical
  route profiles can remain useful records without establishing a reproducible
  verified upgrade combination.
- `capabilities`: named capabilities mapped to `verified`, `candidate`, `unknown`
  or `unsupported`; each item of nonempty `requiredCapabilities` must be verified.
- `scope`: `marker` or `read-fixture`. These do not imply general workload support.
- `canary`: `identityHash` from the observed identity, `source` (`observed` or
  `simulated`), `status: verified`, `terminal: completed`, `expiresAt` (Unix
  milliseconds), `output` and `tools`. Accepted or merely running is insufficient.
  Missing/expired expiry blocks. Choose a short reviewed validity period; a
  same-named backend change is not visible from an old receipt. Revalidate after
  observed changes and after expiry, rather than extending the date by hand.
- `output`: equal SHA-256 `expectedHash` and `actualHash`, never raw output.
- Marker `tools`: exactly `callCount: 0`, `resultCount: 0`.
- Read-fixture `tools`: `callCount: 1`, `resultCount: 1`, `name: read`, equal
  SHA-256 `expectedArgsHash`/`actualArgsHash`, equal nonempty `callId`/`resultCallId`,
  `success: true`. This scope additionally requires canary `permissionBefore`
  and `permissionAfter` both `read-only`. The existing bounded fixture harness
  must validate actual argument/result details before producing this summary;
  hashing arbitrary input is not validation. These are postconditions, not a
  sandbox or a guarantee that writes cannot occur.

`assessCompatibility(manifest, { simulation: true, now })` can return only
`simulation-passed`, never `verified`, even with observed-source metadata.
Simulated evidence is rejected by the production gate. Unknown fields in the
receipt are rejected; decisions contain only fixed status/code, not private
identities, arguments, output or error text.

## Recovery

```js
import { assessRecovery } from '../scripts/compatibility.mjs';
const decision = assessRecovery({ saved, current });
// observe-only means read the existing native request through the existing runner.
// It never means send a new prompt, change a native ID, or reset a journal.
```

`saved` contains `format: 1`, `native: {sessionId, requestId}`, `dependencies` and
`admission` (`unknown`, `accepted` or `running`). `current` contains `format: 1`,
`native`, `dependencies`, and `journalPresent: true`. Dependencies are six SHA-256
values: `task`, `context`, `journal`, `policy`, `remote`, `route`. Use the same
frozen TaskSpec/context, retained store identity, policy/version, authorized
remote instance identity and accepted route identity. The journal dependency
identifies the retained store, not its mutable cursor bytes. A path alone does
not prove store/remote identity; the caller must inspect the actual retained
journal and runtime. The helper does not read files or prove these claims.

Every dependency and original native ID must match. Missing journal, changed
policy/schema/model/remote identity or incompatible format blocks. A successful
check returns `observe-only` with `maySubmit: false`, including unknown admission.
It does not authorize replay, revive a cancelled session, remove a FileJournal
quarantine, transfer a native session to a different runtime, or bypass the
existing runner's guards. This is an advisory preflight for existing recovery,
not a second task store or dispatcher.

## Offline rehearsal

Run `node scripts/recovery-drill.mjs`. It starts an isolated temporary-HOME child
with a minimal environment, exercises only synthetic in-memory receipts, reports
fixed decisions, and removes that HOME. It has no transport or configuration
loader and makes no network calls; this is not an OS-level network sandbox.
No installed runtime, real credentials, existing journal, deployment or physical
second machine is used. The output explicitly says physical migration is not
verified. Imports have no executable side effects. Unit tests are included in the
existing `npm test` glob; no dependency installation is needed.

The rehearsal also retains a synthetic original identity/receipt snapshot, rejects
an upgraded candidate against that receipt, then rechecks the restored original
combination. Both original and restored results remain `simulation-passed`, never
production verification. This is a compatibility rollback simulation, not a binary
rollback or deployment. Actual journal durability/crash recovery is covered by the
existing FileJournal independent-process tests; this drill does not replace them.
