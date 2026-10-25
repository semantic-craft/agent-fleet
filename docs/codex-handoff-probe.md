# Codex handoff probe candidate (not deployed)

Scope: the required external DSH/GrokBot → Agenvo → Codex Desktop route.
It does not gate dot’s available, authorized native platform task route; that route
cannot replace external callers’ needs. See [caller routing](caller-routing.md).

This is offline preparation for one explicitly authorized canary, not a working
Desktop handoff route. Keep the [PR64 admission gate](codex-desktop-handoff.md).
The desired UX remains Desktop visibility followed by agent `turn/start` on the
same thread. A source patch, a successful unsubscribe, or elapsed time is not
writer ownership evidence.

## Why a scoped connector is necessary but insufficient

The pinned Agenvo adapter forces full access at start/resume/turn and auto-approves
permission requests. Its event watcher resumes loaded threads. Merely attaching a
second, restricted connector to the **same production app-server** does not remove
that first connector's authority: the original watcher may resume the test thread
and change its permissions. Do not point this candidate at that shared endpoint.
No restart, subscription removal, or production setting change is authorized here.

The candidate parameters affect only an opt-in probe instance. Default instances
keep their previous behavior. The probe accepts a single predesignated thread ID,
not an arbitrary list, and cannot create threads. This intentionally leaves the
bootstrap step explicit: a future approved native client must create exactly one
empty read-only thread in an isolated native environment, record its ID, and then
close its subscription using a supported operation. Unknown creation must be read
back, never repeated. No bootstrap client or test thread was run in this work.

The native 0.164.0 generated protocol contains `thread/unsubscribe`. The current
Agenvo method catalog did not expose it. The [official protocol documentation](https://learn.chatgpt.com/docs/app-server#unsubscribe-from-a-loaded-thread)
says it removes the **current connection's subscription**; only after the last
subscriber and 30 minutes without activity does the server unload and emit
`thread/status/changed` (`notLoaded`) and `thread/closed`. Neither the reply nor a
30-minute timer proves Desktop acquired the writer. `thread/attachmentOwner/list`
is reverse lookup of saved attachment identity, not execution writer ownership.

## Source and offline reproduction

Replay the small patch in `patches/codex-handoff-probe/` on its exact manifest base
in a clean isolated checkout of the recorded repository. Preserve the Apache-2.0
license. Run `git apply --check` before `git apply`; if it fails, stop and rebase
with review. Do not force application or apply to a running release directory.

Dependency materialization is separate from runtime execution. Use the locked
package tree with lifecycle scripts disabled; offline cache failure is an explicit
block, not permission to fetch or execute an installer. The manifest and recorded
validation distinguish focused adapter mocks, build/type checks, and unrun native
or Desktop tests. No test here establishes live ownership or a new-machine result.

Validated on the manifest candidate: 14 focused probe/protocol/config tests and
8 existing Codex attach/process fixture tests; TypeScript check and Codex package
build passed. Dependencies came from cache with `npm ci --offline --ignore-scripts`.
The focused probe uses an injected in-memory transport; existing fixture tests are
local protocol mocks, not production native/desktop acceptance. Full upstream
suite, system tests, real native ownership, and Desktop UI were not run.

From the patched checkout (with cached locked dependencies):

```sh
npm ci --offline --ignore-scripts
node --import tsx --test tests/codex-handoff-probe.test.ts tests/protocol.test.ts tests/config.test.ts
node --import tsx --test tests/codex-attach.test.ts tests/codex-process.test.ts
npm run types
node node_modules/typescript/bin/tsc --noEmit
npm run build --workspace @agenvo/codex-app-server
```

The generic connector gains an optional per-config execution descriptor resolver;
without it the existing static policy/default is unchanged. The probe reports
`read-only-handoff-probe` and a separate capability revision. Its thread ID is part
of the approval fingerprint. These metadata changes are not deployment or approval.

## Proposed runtime boundary — approval required, not a run instruction

Before any launch, resolve these placeholders from approved discovery/overlay:

- `CANDIDATE`: built CLI from the reviewed patch, not the installed production CLI.
- `PROBE_CONFIG`: new temporary, private Agenvo configuration directory; never copy
  production credentials, use their directory, or reuse a running connector identity.
- `PROBE_HOME`, `PROBE_CWD`, `PROBE_SOCKET`: isolated native state/workspace/Unix
  socket with no production connector watcher attached; no new public listener.
- `PROBE_THREAD`: the single confirmed empty read-only bootstrap thread above.
- `APPROVED_RELAY`: the existing approved relay, only if a temporary test device /
  instance is explicitly paired and approved for this experiment.

The proposed instance definition uses the new explicit option (do not execute now):

```sh
AGENVO_CONFIG_DIR="$PROBE_CONFIG" node "$CANDIDATE" instance add \
  --id handoff-probe --home "$PROBE_HOME" --cwd "$PROBE_CWD" \
  --endpoint "unix://$PROBE_SOCKET" --handoff-probe-thread-id "$PROBE_THREAD"
```

This would persist a new local instance definition and changes its approval
fingerprint. It cannot reuse production approval. The exact relay pairing command
and its fingerprint must be reviewed after isolation is established, before any
`connect` or approval action. No `service install` is part of the proposal.

The eventual foreground process is:

```sh
AGENVO_CONFIG_DIR="$PROBE_CONFIG" node "$CANDIDATE" run
```

This starts one outbound connector process, reads the temporary pairing identity,
and attaches to the configured native socket. It does not start/stop native Codex,
register a launch service, or itself create a thread. Configuring the instance,
pairing it, and granting relay access are separate mutations and have **not** been
performed. A fresh test identity requires explicit pairing/instance approval; an
existing OAuth grant must not be silently expanded. Local-only adapter mock tests
need no pairing and cannot substitute for the final Agenvo route.

A future approval must name the isolated native endpoint, temporary identity and
scope, authorized Desktop connection mechanism, one thread, model/effort, and test
window. There is currently no verified supported way in this work to attach the
installed Desktop to a separate test `CODEX_HOME` without changing its live state.
Do not invent an alternate profile switch, use private IPC, or repoint the normal
Desktop. Consequently **no live launch is ready for approval yet**: first establish
that supported isolation/attachment path. A production shared-endpoint canary
would require separate coordination of every watcher and is not this proposal.

## Acceptance and stop conditions

1. Verify native client and Desktop support the approved isolation path. Verify no
   other connector can resume the test thread. Read-only permission must remain in
   native request/header evidence, not just in a prompt.
2. Explicitly resume the predesignated test thread only when needed. Unsubscribe
   that connection and observe authoritative unload/closed state; disconnect or a
   timeout leaves ownership unknown. The probe must not auto-resume on reconnect.
3. Open only the test thread via a normal supported Desktop entry. Capture only
   test UI. Visibility and history readback are separate from writer authority.
   Require a documented authoritative ownership/attach acknowledgment; absence of
   an error banner is insufficient. If no such evidence exists, stop as unverified.
4. Only with the preceding native/desktop contract established, submit one unique
   marker via Agenvo `turn/start`, no tools, read-only. Correlate device, instance,
   thread and turn; read exact output/terminal state. Confirm Desktop remains able
   to take over by a supported acknowledgment, not an extra model request.
5. Any observed permission prompt, foreign-thread event, unknown write or changed
   scope requires the operator to stop the experiment. The adapter filters foreign
   packets and refuses native requests; filtering is not an automatic experiment
   controller or proof that no foreign packet reached the socket. Read back the
   original IDs; do not retry input, auto-grant,
   resume on reconnect, or fall back to exec / user Send.

Stop the **probe foreground process only** with its normal SIGINT. Closing it does
not stop native turns or immediately unload the thread. If a test turn remains
active, observe it; interrupt only its confirmed turn ID if separately authorized.
After confirmed termination, revoke only the temporary test pairing/instance under
its approved cleanup plan; keep the evidence. Never disconnect the production
identity, kill a shared native server, or delete shared state to make cleanup pass.
