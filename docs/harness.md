# Bounded DSH harness

`scripts/harness.mjs` adds caller-side run validation to the existing Agenvo/native
endpoint path. It has no network client, credentials, dispatcher, daemon, queue, or
model router. `fleet.mjs` supplies the existing authenticated `call` function.
Provider/model selection stays outside this module. In particular,
`session/selectModel` can persist a global default and is never called here.

## Contracts

- `TaskSpec`: objective, explicit exact-text or nonempty-text acceptance, declared
  side effects, caller observation budget, and required capabilities.
- `RunContext`: stable request/session IDs, reviewed harness version, frozen lessons
  revision, evaluation phase (`discovery`, `heldout`, or `production`), expected
  provider/model/effort/preset. Lessons are metadata, not runtime-editable prompts.
- `DshProviderAdapter(call)`: unwraps Agenvo execute envelopes and forwards only
  the existing native endpoint requests. It does not choose a provider.
- `HarnessRunner`: owns transport, retry limits, correlation, journal and evidence.
  `BaseHarness` checks the declared answer acceptance. The one subclass,
  `ReadOnlyVerificationHarness`, additionally requests bounded read-only
  inspection and requires observed tool events. It cannot enforce runtime tool
  permissions or prove those tools were read-only; use a sandbox or independent
  tool audit for that stronger requirement.
- `RunResult`: separates accepted, native completed, and goal verified. It returns
  only IDs, fixed diagnostic codes, sequence/turn metadata, output length/hash and
  version metadata. Raw prompts, replies, provider errors and authentication are
  never included. `nonempty-text` is only a smoke-check criterion, not proof of a
  general business goal.

The runner checks projections before submission and after the matching turn,
including model selection and preset drift; every observed request header must
match the expected model. It reads bounded message-window pages to associate the
original RPC ID, native turn, model header, reply and terminal. Foreign input,
ambiguous turns, missing/oversized history and wrong model fail closed. This
reader requires contiguous event sequences; a runtime with legitimate gaps must
provide a completeness contract before being supported. This is
poll/readback acceptance, not push-event acceptance.

Read-only calls retry 429/5xx with bounded backoff. Mutation calls never retry.
Marking a submission attempted precedes sending it; an unknown outcome requires
reading the original request ID, never a replacement request. Re-running with
the same journal resumes that observation. Conflicting task/context reuse fails.
Cancellation is explicit (`run(..., {requestCancellation: true, exclusiveSession: true})`) after an earlier
submission: only a positively matched running turn may receive one cancel call.
The caller must own the isolated session exclusively: native cancel addresses a
session, not an atomic expected-turn token. Observation alone cannot eliminate a
concurrent user starting another turn between inspection and cancel. The runner
still waits for a matching cancelled terminal. Native cancellation
cannot remove queued input; aborting observation does not cancel native work.
A caller deadline bounds observation, not model runtime or token spend. An
unresolved transport may still complete after observer timeout.

## Journal and receipts

The default Map is process-local. It is useful for tests and a single CLI run,
**not a durable exactly-once guarantee**. Long-lived callers must supply a
synchronous `get`/`set` journal that persists each attempted marker before return
and serialize access to the same session/request across processes. Shared runners
in one process share a journal ownership guard. Preserve the journal on crashes;
never reconstruct a new request ID to hide an unknown write outcome. The library
does not create a second task ledger or claim cross-process locking.

For final delivery, pass the sanitized RunResult to the caller’s existing receipt
mechanism with a stable result revision key; acknowledge only after actual delivery.
Immutable result receipts are separate from mutable transport recovery state.
Overlay loading and project selection
continue to use `scripts/lib.mjs`; no host values enter this library.

## Frozen policy selection and evaluation

`selectHarness` chooses a complete reviewed class or reuses a successful Base
result. It does not execute ancestor agents. Retrying Base with a specialist is
allowed only in discovery/heldout evaluation of read-only or sandbox tasks;
production and real write replays are rejected. Callers must use distinct stable
IDs for distinct evaluation runs and keep discovery traces separate from heldout
assessment. There is no automated learning/evolution API or performance claim.
Subclasses receive only task/evidence, never the transport or journal. Unsupported
hard tool allowlists, hard token budgets, and other capabilities fail explicitly.

## Validation and limits

`node --test test/harness.test.mjs` uses synthetic transcripts and injected calls;
`npm test` includes it in the existing suite. Run tests with a temporary HOME and
no live overlay. Coverage includes transient reads, unknown mutations, disconnect
recovery, wrong input/turn/model/preset, cancellation, fallback, concurrency and
idempotency. These tests do not establish a real DSH tool-loop or cloud acceptance.
That requires separate authorized Dot → Agenvo → DSH → provider evidence with the
actual preset/model and a bounded read-only tool invocation. Do not use
`fleet.mjs check --dispatch` as an offline test: it creates real requests.

Offline implementation validation (2026-10-24): harness tests 28/28; complete Node
suite 66/66 on the development base; private guard passed with no overlay.
Independent review added and rechecked late-deadline, policy-constructor spoof,
foreign-input cancellation, and sequence-gap regressions. Python-generated
bytecode initially tripped the private guard because it embeds source paths;
remove generated bytecode and run receipt tests with `PYTHONDONTWRITEBYTECODE=1`
or `python3 -B`. No live model requests, production configuration changes,
deployment, or push were part of this validation.

For cloud-owned submission, call `runner.prepareExternalRun(task, context)` first.
It checks configuration and saves the original cursor and attempted marker in the
same caller-owned journal before returning `{result, dispatchAllowed, descriptor}`. `result` is the
sanitized RunResult; the separate private descriptor is the `session/prompt` call.
Unlike the separate sanitized RunResult, this dispatch descriptor intentionally
contains the caller's objective; keep it private and do not append it to result
receipts. Send it externally only when `dispatchAllowed` is true. A repeated
prepare returns the same descriptor with `dispatchAllowed: false`, retaining the
original baseline. `run` then only observes the reserved RPC ID and never submits
it locally. Unknown external delivery stays unaccepted until its RPC input is
observed. A completed entry returns `{result, dispatchAllowed: false}` with its terminal
result and no descriptor. Failed preparation similarly returns no descriptor.
Only `prepared.result` is eligible for result receipts or generic logging.

Fresh sessions with both `modelSelection.next` and `lastUsed` explicitly null
inherit `session/modelCatalog.default`. Only in that case does the adapter call
this read-only endpoint with empty named arguments `{}` (no `request` wrapper).
It never calls `session/selectModel`. Missing defaults fail closed; selection and
preset drift remain checked on subsequent observations.

This extension uses harness version `bounded-dsh-v2`; old frozen contexts must
not silently opt into changed recovery behavior.

Extension validation: harness tests 47/47, complete Node suite 85/85 and private
guard passed. Independent review checked external reservation idempotency and
catalog inheritance; its privacy feedback separated the private descriptor from
RunResult. Freeze the actual module revision before preparing a request. If a
reservation is already in flight when policy versions change, keep its original
reader and journal until completion; never erase the journal, refresh the cursor,
or create a replacement request merely to satisfy a new version fingerprint.
An explicitly audited legacy reader may observe only already-reserved requests;
it must not gain a submission or cancellation path.

Live integration exposed an important native contract: `maxMessages: 1` counts
message windows, each of which may include multiple administrative events. The
reader now validates every sequence in the returned window, including window
boundaries, and trims only events at or before the reserved baseline. Synthetic
regressions reproduce that observed shape; they do not themselves establish the
real request's eventual success.

Native permission changes may inject a `user/message` whose source is
`user-approval`. This is not a blanket trusted source. The narrow read-only
transition exemption requires the exact single-text never-to-ask notification,
role user, source with only that kind, before our RPC input, plus the observed
consecutive read-only preset/read-only sandbox/ask policy events and no later
permission change. Other approval messages remain foreign input. The generation
path was inspected as inert source in `dsh-user-approval/lib/index.js`,
`setPolicy`, source SHA-256
`6a5ad1ba3c3dc3abbbae52e6c8c4463970ee0c1c8988cebb0d5cf2acd17ee90b`.
No upstream package code was executed.

Authorized live integration subsequently verified one bounded read-only fixture
loop through cloud prompt/follow → Agenvo → DSH → Magpie, using Argon at medium
effort with the PTC preset. The matching turn completed with one tool call/result
and the exact synthetic fixture marker; native permissions were read-only. This
was the existing reserved v1 run observed with an audited read-only compatibility
reader, retaining its original RPC ID, cursor and journal. It establishes that
specific route and tool-loop, not standard-preset or other-provider acceptance,
not push delivery, and not a blanket production rollout of this module. Private
request identifiers and raw trace evidence remain in the owning task's records.

## Fleet entrypoint integration

`fleet.mjs` directly imports `dshTurn` from `scripts/dsh-check.mjs`; the tested
wrapper is the actual DSH branch used by `check --dispatch`, not a separate demo.
Importing the wrapper has no CLI, OAuth, or dispatch side effects. Its injected
Agenvo-envelope tests cover an existing session and workspace/session creation
through prompt and matching terminal. Max-token failure, errors, and completed
turns with empty output all remain unsuccessful. Creation errors are sanitized.

The complete CLI OAuth entrypoint was deliberately not executed: it registers a
new grant, which is outside this validation's permission scope. These changes are
local commits in the isolated worktree, not deployed or applied to the primary
checkout. The frozen v2 harness module and existing in-flight reservations are
unchanged by this extraction.

Wrapper validation: 11/11 injected-envelope integration tests; complete Node suite
96/96 and private guard passed with a temporary HOME and no live overlay. This
exercises the exported production wrapper, not the full CLI authentication flow.

The wrapper extraction was independently reviewed by a non-implementing agent,
which reran all 11 wrapper tests successfully. A separate authorized cloud run
also verified Argon medium with the standard preset: a harmless text task under
read-only permissions completed with zero tool executions and matching harness
readback. That run used an independently installed parallel build, not a gateway
switch performed by this wrapper work. It included tool declarations; exact
13-schema wire comparison remains a separate evidence check. This does not imply
the isolated worktree changes have been deployed.

## Optional durable recovery (v3)

`FileJournal` in `scripts/file-journal.mjs` is an optional local recovery adapter.
The default remains a process-local Map. Audit found no mutable runstore on main;
the separate result/delivery receipt implementation is immutable, so it is not
used as a transport journal and was not imported into this change.

```js
import { FileJournal } from './scripts/file-journal.mjs';
import { HarnessRunner, DshProviderAdapter, TaskSpec, RunContext } from './scripts/harness.mjs';

// Caller explicitly supplies an already-existing, owner-only directory and an
// already-authorized transport. There is no default global storage or login.
const journal = new FileJournal(existingPrivateDirectory);
const runner = new HarnessRunner(new DshProviderAdapter(existingCall), { journal });
const task = new TaskSpec({
  objective: originalTaskText,
  acceptance: { kind: 'exact-text', text: expectedAnswer },
  sideEffects: 'read-only',
});
const context = new RunContext({
  sessionId, requestId: stableRequestId, lessonsVersion: 'reviewed-lessons-v1',
  expected: { provider, model, reasoningEffort, preset },
});
async function dispatchOrObserve() {
  const prepared = await runner.prepareExternalRun(task, context);
  // Never call run() after failed preparation: an unreserved run may submit.
  if (prepared.result.status !== 'prepared') return prepared.result;
  if (prepared.dispatchAllowed) {
    // Exactly one send. The private descriptor contains the task; do not log it.
    try { await existingCall(prepared.descriptor.method, prepared.descriptor.params); }
    catch { /* Unknown admission: retain the original reservation and observe. */ }
  }
  // dispatchAllowed=false MUST NOT send again, even after an earlier timeout.
  // After restart reconstruct the SAME task/context/policy and directory.
  return runner.run(task, context); // reserved request: readback only
}
const result = await dispatchOrObserve(); // log/receipt only this sanitized result
```

The stable `(sessionId, requestId)` pair is the run identity; there is no extra ID
registry. One hashed file per explicitly addressed session contains only request
IDs, review versions, hashes, sequence/provenance metadata and sanitized terminal
evidence. No directory scan, task body, answer text, reasoning text, credentials,
or generic payload field is supported. Unknown fields are rejected, not silently
persisted. Caller-supplied identifiers/model labels must be non-sensitive metadata.
The file has a 2 MiB bound; no automatic pruning or state migration occurs.

A small Python 3 stdlib helper (`journal-lock.py`, isolated mode, no packages)
holds a POSIX `fcntl` lock for the session while the runner works. Different
requests cannot use that same session concurrently. Every read/write verifies
root and lock inode identity; the helper also verifies identity on a pre-mutation
handshake. Missing Python, unsupported platform, helper loss, unsafe paths, lock
contention or ambiguous storage return blocked; there is no unlocked fallback.
The caller must provide an existing 0700 directory on a local POSIX filesystem;
record and lock files use 0600. Network filesystems and multi-host locking are
unsupported. The helper is temporary per runner call, not an installed service.

An update writes a same-directory exclusive temporary file, fsyncs it, renames it
atomically, then fsyncs the directory. Attempted/cancel-attempted markers must
complete that sequence before transport mutation. The session file derives
pending ownership from an attempted run without a confirmed terminal. Thus an OS
lock released by process death **does not release remote task ownership**: a new
request in that session is blocked until the old run is observed terminal or an
operator explicitly resolves it using native evidence. The library exposes no
clear/forget/reset shortcut. Same-ID recovery observes first and never blindly
resends. A crash after the durable marker but before sending may leave a request
that never reached DSH; that ambiguity is intentionally retained, not retried.
Queued cancellation remains unsupported, and runtime cancellation still requires
exclusive native-session ownership outside this library's local-process lock.

On crash, a leftover temporary file is ignored; the canonical file is either its
prior complete state or the renamed complete state. Corrupt/truncated records,
unknown format versions, or incompatible harness versions block before transport.
Do not delete, move, replace or restore the directory to bypass a pending run.
Manual repair must preserve original IDs/baseline and independently establish the
remote outcome. Local locks cannot enforce ownership against unrelated native
clients or another directory/host. Filesystem fsync/atomic-rename guarantees are
required; process-kill tests are not proof against storage hardware failure.

v3 freezes this storage/correlation contract. Existing v2 contexts and reservations
must continue using their original frozen observer; they are not silently migrated.

## Header inheritance

The inspected DSH `dsh-agent-loop` source (`buildRequest`, lines 1214–1240,
SHA-256 `cf57e3ecd487ac5c2553455aaa5cef2c6fb41e6bd61be5aa455675d1d7193033`)
emits a header only on initial/resume, config change or new request series. A
second turn can therefore have a real terminal without a fresh request/header.
Preparation now reads bounded, contiguous history to the latest header and stores
only provider/model/effort plus header origin sequence and baseline coverage.
Observation may inherit that proven header while requiring continuous events,
unchanged model/preset projections before and after, and compatible request/context.
Evidence marks inherited headers explicitly. Missing provenance or an unreadable
history window is blocked rather than fabricated; a matched max-tokens terminal
with proven inherited header is failed, not an observation timeout. This does not
claim a fresh tool-schema snapshot or expanded cancellation support for headerless
running turns. Such a cancellation returns `headerless_cancellation_unsupported`
without sending cancel or claiming that the run is merely queued.

v3 offline validation: 128/128 complete repository tests and private guard passed
with a temporary HOME and no live overlay. This includes 51 harness tests and 20
durability tests, with independent-process SIGKILL/restart, same-session different
request blocking, pre-rename interruption, fsync-before-mutation ordering, helper
loss, lock/root inode replacement, corruption/version rejection, unknown cancel
recovery, inherited-header restart and catalog effort `off`. No live request,
CLI OAuth grant, service install or shared-checkout receipt change was involved.

## Native request identity versus transport correlation

There are two distinct `requestId` fields. Agenvo's dispatch result envelope has
its own transport correlation ID; the DSH prompt parameter is
`params.request.requestId`. Only the latter is `RunContext.requestId`, the durable
`(sessionId, requestId)` key and the expected `user/message.source.rpcId` in native
history. `DshProviderAdapter` unwraps `envelope.result`; it neither adopts the
outer ID nor substitutes it into the nested prompt. A new connection or dispatch
may have a different envelope ID without changing the native run identity.

Connection-local relay deduplication is not a durable admission guarantee. After
unknown admission, reconstruct the same task/context/journal and observe the old
native request. Never interpret a new transport ID, a reconnect, or an accepted
envelope as permission to submit again. The independent-process reconnect test
uses fresh envelope IDs on the second transport and asserts that only projections,
follow and page are called; the total prompt count remains one and no envelope ID
is persisted in the journal. The separate adapter test demonstrates forwarding
identity only; it does not claim that calling the adapter directly twice deduplicates.

`dsh-check.mjs` is intentionally a smoke-check wrapper: every `dshTurn` invocation
creates a fresh random native request and an in-memory runner. Re-running the CLI
is a new check, not recovery of a previous unknown run. Callers needing reconnect
or crash recovery must use the durable runner/preparation flow with the original
stable pair, rather than invoke the smoke-check wrapper again. This audit and its
regressions change no relay or production dispatch behavior.

### Cancellation quarantines the session

Native `session/cancel` has no turn-specific fence. A timeout or even a later terminal event cannot prove that a delayed cancellation will not affect the next turn. Therefore any durable `cancelAttempted` record permanently blocks **different native request IDs in that session**, including after the owner naturally completes or is cancelled (`journal_session_cancelled_owner`). The original request may still be read and recovered; no cancellation is replayed. Use a new native session for subsequent work.

This protection coordinates callers using the same journal; it is not a remote fence and cannot control other clients. Do not delete records or switch journal directories to reuse a quarantined session. There is no automatic reset or migration. Existing records already carry `cancelAttempted`, so no new persistent field is required.

Transport source boundary: the Agenvo relay review pinned revision
[`a89415e557c415faf5626354f8e87233eb69d877`](https://github.com/Xuanwo/agenvo/blob/a89415e557c415faf5626354f8e87233eb69d877/packages/relay/src/core.ts#L438) found
per-dispatch envelope UUIDs, a 10-second dispatch confirmation window, and
connection-local deduplication without event replay. These transport properties
are not a durable execution identity or an exactly-once guarantee. Only the
nested native request ID is held stable by this harness. The offline reconnect
regression changes the envelope IDs and connection identity while retaining the
native ID and journal; recovery reads the existing turn without another prompt.

Companion sources at the same revision are
[connector main](https://github.com/Xuanwo/agenvo/blob/a89415e557c415faf5626354f8e87233eb69d877/packages/connector/src/main.ts)
and [event semantics](https://github.com/Xuanwo/agenvo/blob/a89415e557c415faf5626354f8e87233eb69d877/docs/events.md).

Validation of this increment: 138 local tests and the private guard pass with a
temporary HOME and no live overlay. This includes isolated-process reconnect,
post-admission accepted/final file-fsync failure, persistent cancellation
quarantine, and overlapping/duplicate/conflicting history page regressions.
It does not exercise a real reconnect or cancel, directory-fsync failure after
rename, cancel ACK with no terminal, or an old observer's deferred reply arriving
during a second observer's recovery. No service, credential, relay, or model
configuration changed.

## Explicit route acceptance profiles

`scripts/route-profiles.mjs` contains frozen, versioned evidence snapshots. These
are caller policies, separate from provider routing and harness strategy. They
are not runtime health discovery, a model benchmark, or a setting change:

| Profile | Evidence and allowed scope |
| --- | --- |
| `sol-low-ptc-marker-v1` | Cloud prompt/follow round trip with Sol/low/PTC returned the exact marker, with one `run_code` declaration and zero tool executions. Only an exact-text, read-only marker check is admitted. Tool execution and general workloads remain unverified. |
| `sol-high-ptc-marker-v1` | A separate cloud Sol/high/PTC marker round trip completed with one `run_code` declaration and zero tool executions. The same marker-only restrictions apply; general workloads and tool execution remain unverified. |
| `sol-high-standard-v1` | A fresh standard/high marker request with 345 tool declarations and zero tool executions also ended with empty max-tokens output. Explicit use blocks before transport; raising effort did not resolve this observed failure. |
| `sol-low-standard-v1` | On the tested deployment, standard-mode requests ended with empty max-tokens output. Explicit use of this profile is blocked before transport; this is not a universal limitation of Sol or standard mode. |

The high test used a fresh session, so history and context differ from prior low tests; it does not isolate a unique root cause. A separate high/PTC marker check subsequently passed; this does not establish that tool count alone caused the standard failures.

The evidence was recorded on 2026-10-24 with DSH 0.2.1-rc.3 and pi-ai 1.1.0.
The exact deployed Magpie source tree was not proven. Revalidate after upgrades
and introduce a new profile version when evidence or acceptance changes; do not
silently rewrite the meaning of an existing reservation. Profile ID is part of
the existing context fingerprint. A reserved native request cannot drop or switch
profiles to resume under different acceptance rules. Existing callers without a
profile retain their previous behavior; the new profiles do not globally ban Sol
standard requests or automatically select a fallback.

For a routine compatibility marker check, start with an **existing session that
the user has explicitly selected** and the existing authorized `call` transport:

```js
import { ROUTE_PROFILES } from './scripts/route-profiles.mjs';
import { TaskSpec, RunContext, DshProviderAdapter, HarnessRunner } from './scripts/harness.mjs';

// The caller supplies existingCall, existingSessionId, stableNativeRequestId,
// marker, and a retained journal (FileJournal for process restart recovery).
const routeProfile = 'sol-low-ptc-marker-v1'; // or explicitly 'sol-high-ptc-marker-v1'
const task = new TaskSpec({
  objective: `Reply only with ${marker}. Do not use tools.`,
  acceptance: { kind: 'exact-text', text: marker },
  sideEffects: 'read-only',
});
const context = new RunContext({
  sessionId: existingSessionId, requestId: stableNativeRequestId,
  lessonsVersion: 'reviewed-marker-check-v1', routeProfile,
  expected: ROUTE_PROFILES[routeProfile].expected,
});
const runner = new HarnessRunner(new DshProviderAdapter(existingCall), { journal });
const result = await runner.run(task, context);
```

This copies only **expectations**, not settings. The runner reads native
projections and checks the actual request header. If the session is not already
the selected profile’s Sol/effort/PTC expectations, it blocks without a prompt. It never calls `session/selectModel` or
changes preset/defaults; native model selection can also save the global default,
so the caller must handle that separately under explicit authorization. For cloud
external submission, use the existing `prepareExternalRun` flow with the same
profile and context; its dispatch guard still applies.

Zero tool events is a **postcondition**, not a tool execution prevention mechanism.
A prompt saying not to use tools and `sideEffects: read-only` are not enforceable
native permissions. The caller must have authorization and suitable trusted
native execution restrictions before sending. PTC's `run_code` declaration does
not establish sandbox safety. If a tool event is observed, even a correct marker
is `failed / terminal_not_verified`; this cannot undo an already executed tool.
The profile must not be used as authorization for real work or write operations.
