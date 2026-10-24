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
