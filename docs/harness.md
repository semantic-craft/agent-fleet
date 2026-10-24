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
match the expected model. It reads bounded single-record pages to associate the
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

For final delivery, use existing `scripts/receipt.py` with a project-local JSON
payload containing the sanitized RunResult and a stable result revision key;
acknowledge only after actual delivery. That immutable result receipt is separate
from mutable transport recovery state. Overlay loading and project selection
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
suite 66/66; existing Python receipt tests 2/2; private guard passed with no overlay.
Independent review added and rechecked late-deadline, policy-constructor spoof,
foreign-input cancellation, and sequence-gap regressions. Python-generated
bytecode initially tripped the private guard because it embeds source paths;
remove generated bytecode and run receipt tests with `PYTHONDONTWRITEBYTECODE=1`
or `python3 -B`. No live model requests, production configuration changes,
deployment, or push were part of this validation.
