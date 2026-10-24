import test from 'node:test';
import assert from 'node:assert/strict';
import { ROUTE_PROFILES } from '../scripts/route-profiles.mjs';
import { TaskSpec, RunContext, HarnessRunner, DshProviderAdapter, ReadOnlyVerificationHarness } from '../scripts/harness.mjs';
const id = 'sol-low-ptc-marker-v1';
const expected = ROUTE_PROFILES[id].expected;
const model = Object.fromEntries(Object.entries(expected).filter(([k]) => k !== 'preset'));
const task = extra => new TaskSpec({ objective: 'Return only SYNTHETIC_OK without using tools.', acceptance: { kind: 'exact-text', text: 'SYNTHETIC_OK' }, budget: { maxPolls: 1, pollMs: 0 }, ...extra });
const context = extra => new RunContext({ requestId: 'synthetic-request', sessionId: 'synthetic-session', lessonsVersion: 'synthetic-v1', expected, routeProfile: id, ...extra });
function fixture({ preset = 'ptc', selected = model, header = model, tool = false, unknown = false } = {}) {
  let sent = false; const calls = [];
  const events = [
    { type: 'turn/start', data: { turn: 1 } },
    { type: 'user/message', data: { source: { rpcId: 'synthetic-request' } } },
    { type: 'request/header', data: { header: { config: header } } },
    ...(tool ? [{ type: 'tool/call', data: { turn: 1 } }] : []),
    { type: 'assistant/message', data: { turn: 1, message: { content: [{ type: 'text', text: 'SYNTHETIC_OK' }] } } },
    { type: 'turn/end', data: { turn: 1, reason: { kind: 'completed' } } },
  ].map((event, i) => ({ seq: i + 1, ...event }));
  const call = async (method, { request }) => {
    calls.push(method);
    if (method === 'session/projections') return { result: { values: { agentPreset: preset, modelSelection: { next: selected } } } };
    if (method === 'session/follow') return { result: { cursor: sent ? events.length : 0 } };
    if (method === 'session/page') return { result: { records: events.filter(e => e.seq < request.beforeSeq).map(event => ({ event })) } };
    if (method === 'session/prompt') { sent = true; if (unknown) throw Error('synthetic-lost-response'); return { result: {} }; }
    throw Error('unexpected method');
  };
  return { calls, runner: new HarnessRunner(new DshProviderAdapter(call), { sleep: async () => {} }) };
}
test('profile records and selection are frozen', () => {
  assert.ok(Object.isFrozen(ROUTE_PROFILES)); assert.ok(Object.isFrozen(ROUTE_PROFILES[id])); assert.ok(Object.isFrozen(expected)); assert.equal(ROUTE_PROFILES['sol-high-standard-v1'].expected.reasoningEffort, 'high');
});
test('explicit marker profile uses existing runner and verifies matching runtime evidence', async () => {
  const f = fixture(); const result = await f.runner.run(task(), context());
  assert.equal(result.status, 'success'); assert.equal(result.evidence.toolEvents, 0);
  assert.ok(f.calls.every(m => ['session/projections', 'session/follow', 'session/page', 'session/prompt'].includes(m)));
});
for (const [profile, code] of [['sol-low-standard-v1', 'route_profile_known_failure'], ['sol-high-standard-v1', 'route_profile_known_failure'], ['unknown-v1', 'route_profile_unknown']]) test(`${profile} blocks with zero transport`, async () => {
  const f = fixture(); const result = await f.runner.run(task(), context({ routeProfile: profile }));
  assert.equal(result.code, code); assert.equal(result.status, 'blocked'); assert.deepEqual(f.calls, []);
});
for (const extra of [{ expected: {} }, { expected: { ...expected, preset: 'standard' } }]) test('profile requires explicit matching selection expectation', async () => {
  const f = fixture(); assert.equal((await f.runner.run(task(), context(extra))).code, 'route_profile_selection_required'); assert.deepEqual(f.calls, []);
});
for (const extra of [{ sideEffects: 'write' }, { acceptance: { kind: 'nonempty-text' } }]) test('marker verification does not authorize a broader task scope', async () => {
  const f = fixture(); assert.equal((await f.runner.run(task(extra), context())).code, 'route_profile_scope_unsupported'); assert.deepEqual(f.calls, []);
});
test('tool-requiring specialist is incompatible with marker-only evidence', async () => {
  const f = fixture(); assert.equal((await f.runner.run(task(), context(), { harness: new ReadOnlyVerificationHarness() })).code, 'route_profile_scope_unsupported'); assert.deepEqual(f.calls, []);
});
for (const [opts, code] of [[{ preset: 'standard' }, 'preset_drift'], [{ selected: { ...model, reasoningEffort: 'high' } }, 'model_drift']]) test('runtime selection mismatch blocks before submission without changing settings', async () => {
  const f = fixture(opts); assert.equal((await f.runner.run(task(), context())).code, code); assert.deepEqual(f.calls, ['session/projections']);
});
test('profile still checks actual request header', async () => {
  const f = fixture({ header: { ...model, reasoningEffort: 'high' } }); assert.equal((await f.runner.run(task(), context())).code, 'model_drift'); assert.equal(f.calls.filter(m => m === 'session/prompt').length, 1);
});
test('observed tool execution fails marker profile even with the correct marker', async () => {
  const f = fixture({ tool: true }); const result = await f.runner.run(task(), context());
  assert.equal(result.status, 'failed'); assert.equal(result.code, 'terminal_not_verified'); assert.equal(result.evidence.toolEvents, 1);
});
test('profile changes cannot reuse a reserved native request fingerprint', async () => {
  const f = fixture({ unknown: true }); assert.equal((await f.runner.run(task(), context())).status, 'unknown'); const before = f.calls.length;
  assert.equal((await f.runner.run(task(), context({ routeProfile: undefined }))).code, 'idempotency_conflict'); assert.equal(f.calls.length, before);
  assert.equal((await f.runner.run(task(), context())).status, 'success'); assert.equal(f.calls.filter(m => m === 'session/prompt').length, 1);
});
test('external preparation enforces profile without submitting or changing the session', async () => {
  const f = fixture(); const prepared = await f.runner.prepareExternalRun(task(), context());
  assert.equal(prepared.result.status, 'prepared'); assert.equal(prepared.dispatchAllowed, true);
  assert.equal(prepared.descriptor.method, 'session/prompt'); assert.deepEqual(f.calls, ['session/projections', 'session/follow']);
  const blocked = fixture(); const rejected = await blocked.runner.prepareExternalRun(task(), context({ routeProfile: 'sol-low-standard-v1' }));
  assert.equal(rejected.dispatchAllowed, false); assert.equal(rejected.descriptor, undefined); assert.deepEqual(blocked.calls, []);
});

for (const tool of [false, true]) test(`high PTC profile validates matching high header and zero tool events (${tool})`, async () => {
  const high = ROUTE_PROFILES['sol-high-ptc-marker-v1'].expected;
  assert.equal(high.reasoningEffort, 'high'); assert.equal(high.preset, 'ptc');
  const selection = { ...model, reasoningEffort: 'high' };
  const f = fixture({ selected: selection, header: selection, tool });
  const result = await f.runner.run(task(), context({ expected: high, routeProfile: 'sol-high-ptc-marker-v1' }));
  assert.equal(result.status, tool ? 'failed' : 'success');
  assert.equal(result.evidence.toolEvents, tool ? 1 : 0);
});
test('high PTC profile rejects low runtime before sending and never selects a model', async () => {
  const f = fixture();
  const result = await f.runner.run(task(), context({ expected: ROUTE_PROFILES['sol-high-ptc-marker-v1'].expected, routeProfile: 'sol-high-ptc-marker-v1' }));
  assert.equal(result.code, 'model_drift'); assert.deepEqual(f.calls, ['session/projections']);
});
