import test from 'node:test';
import assert from 'node:assert/strict';
import { TaskSpec, RunContext, DshProviderAdapter, HarnessRunner, BaseHarness, ReadOnlyVerificationHarness, selectHarness } from '../scripts/harness.mjs';
const model = { provider: 'test-provider', model: 'test-model', reasoningEffort: 'medium' };
const context = () => new RunContext({ sessionId: 'synthetic-session', requestId: 'synthetic-request', lessonsVersion: 'reviewed-lessons-v1', expected: { ...model, preset: 'standard' } });
const task = (extra = {}) => new TaskSpec({ objective: 'Return SYNTHETIC_OK', acceptance: { kind: 'exact-text', text: 'SYNTHETIC_OK' }, budget: { maxPolls: 2, pollMs: 0 }, ...extra });
const event = (seq, type, data) => ({ seq, type, data });
const transcript = () => [
  event(1, 'turn/start', { turn: 1 }),
  event(2, 'user/message', { source: { rpcId: 'synthetic-request', kind: 'rpc' } }),
  event(3, 'request/header', { header: { config: model } }),
  event(4, 'assistant/message', { turn: 1, message: { content: [{ type: 'text', text: 'SYNTHETIC_OK' }] } }),
  event(5, 'turn/end', { turn: 1, reason: { kind: 'completed' } }),
];
function fixture({ events = transcript(), onCall, sendError = false, initial = 0 } = {}) {
  let sent = false;
  const calls = [];
  const call = async (method, { request }) => {
    calls.push({ method, request });
    const override = await onCall?.(method, request, calls);
    if (override !== undefined) return override;
    if (method === 'session/projections') return { result: { kind: 'sequenced', values: { agentPreset: 'standard', modelSelection: { next: model } } } };
    if (method === 'session/prompt') { sent = true; if (sendError) throw Error('private-secret'); return { execution: 'accepted', result: {} }; }
    if (method === 'session/follow') return { result: { cursor: sent ? events.at(-1)?.seq ?? initial : initial, records: [] } };
    if (method === 'session/page') return { result: { records: events.filter(e => e.seq < request.beforeSeq).slice(-1).map(event => ({ event })) } };
    throw Error('unexpected endpoint');
  };
  return { calls, runner: new HarnessRunner(new DshProviderAdapter(call), { sleep: async () => {} }) };
}
test('matching completed turn is goal verified; duplicate request cached without replay', async () => {
  const f = fixture(); const r = await f.runner.run(task(), context());
  assert.equal(r.status, 'success'); assert.equal(r.completed, true); assert.equal(r.evidence.turn, 1);
  assert.equal(JSON.stringify(r).includes('SYNTHETIC_OK'), false);
  assert.deepEqual(await f.runner.run(task(), context()), r);
  assert.equal(f.calls.filter(c => c.method === 'session/prompt').length, 1);
  assert.equal((await f.runner.run(task({ objective: 'Changed' }), context())).code, 'idempotency_conflict');
});
test('unknown mutation is not retried; same rpcId resumes readback', async () => {
  const f = fixture({ sendError: true });
  assert.equal((await f.runner.run(task({ sideEffects: 'write' }), context())).status, 'unknown');
  assert.equal((await f.runner.run(task({ sideEffects: 'write' }), context())).status, 'success');
  assert.equal(f.calls.filter(c => c.method === 'session/prompt').length, 1);
});
for (const status of [429, 503]) test(`read ${status} retries boundedly`, async () => {
  let n = 0;
  const f = fixture({ onCall: m => m === 'session/projections' && ++n <= 2 ? { error: { status } } : undefined });
  assert.equal((await f.runner.run(task(), context())).status, 'success'); assert.equal(n, 4);
});
test('exhausted read retries do not submit', async () => {
  const f = fixture({ onCall: () => ({ error: { status: 503, message: 'secret' } }) });
  const r = await f.runner.run(task(), context()); assert.equal(r.code, 'read_unavailable'); assert.equal(f.calls.length, 3); assert.ok(!JSON.stringify(r).includes('secret'));
});
for (const [name, edit, code] of [
  ['foreign input', e => e.splice(3, 0, event(3.5, 'user/message', { source: { rpcId: 'other' } })), 'incomplete_history'],
  ['foreign integer input', e => { e.splice(3, 0, event(4, 'user/message', { source: { rpcId: 'other' } })); e[4].seq = 5; e[5].seq = 6; }, 'foreign_input'],
  ['wrong header model', e => { e[2].data.header.config = { ...model, model: 'other' }; }, 'model_drift'],
]) test(name, async () => { const events = transcript(); edit(events); assert.equal((await fixture({ events }).runner.run(task(), context())).code, code); });
test('wrong turn cannot be completed by another end', async () => {
  const events = transcript(); events[4].data.turn = 2;
  assert.equal((await fixture({ events }).runner.run(task(), context())).status, 'timeout');
});
test('accepted queue without own rpcId is not completed', async () => {
  const events = transcript(); events[1].data.source.rpcId = 'other';
  const r = await fixture({ events }).runner.run(task(), context()); assert.equal(r.accepted, true); assert.equal(r.completed, false); assert.equal(r.status, 'timeout');
});
test('goal failure differs from native completed', async () => {
  const r = await fixture().runner.run(task({ acceptance: { kind: 'exact-text', text: 'OTHER' } }), context());
  assert.equal(r.status, 'failed'); assert.equal(r.completed, true);
});
test('native cancellation requires matching terminal; observer abort never claims queue cancellation', async () => {
  const events = transcript(); events[4].data.reason.kind = 'cancelled';
  assert.equal((await fixture({ events }).runner.run(task(), context())).status, 'cancelled');
  const controller = new AbortController(); controller.abort(); const f = fixture();
  assert.equal((await f.runner.run(task(), context(), { signal: controller.signal })).status, 'unknown'); assert.equal(f.calls.length, 0);
});
test('transport timeout returns unknown mutation and cannot replay', async () => {
  let prompts = 0;
  const f = fixture({ onCall: m => { if (m === 'session/prompt') { prompts++; return new Promise(() => {}); } } });
  const t = task({ budget: { timeoutMs: 10, maxPolls: 0 } });
  assert.equal((await f.runner.run(t, context())).status, 'unknown');
  await f.runner.run(t, context()); assert.equal(prompts, 1);
});
test('configuration/preset checked before and after turn', async () => {
  let reads = 0;
  const f = fixture({ onCall: m => m === 'session/projections' && ++reads > 1 ? { result: { values: { agentPreset: 'ptc', modelSelection: { next: model } } } } : undefined });
  assert.equal((await f.runner.run(task(), context())).code, 'preset_drift');
  const ctx = new RunContext({ ...context(), expected: { ...model, model: 'different' } });
  const g = fixture(); assert.equal((await g.runner.run(task(), ctx)).code, 'model_drift'); assert.equal(g.calls.length, 1);
});
test('capability requests fail explicitly; hard tool allowlist is not promised', async () => {
  assert.equal((await fixture().runner.run(task({ requiredCapabilities: ['enforce-tool-allowlist'] }), context())).code, 'unsupported_capability');
});
test('specialist requires observed tools and inherits answer acceptance', async () => {
  const h = new ReadOnlyVerificationHarness(); assert.ok(h instanceof BaseHarness);
  assert.equal((await fixture().runner.run(task(), context(), { harness: h })).status, 'failed');
  const events = transcript(); events.splice(3, 0, event(4, 'tool/end', {})); events[4].seq = 5; events[5].seq = 6;
  assert.equal((await fixture({ events }).runner.run(task(), context(), { harness: h })).status, 'success');
});
test('frozen review versions and heldout separated; fallback cannot replay production/write work', () => {
  assert.throws(() => new RunContext({ ...context(), harnessVersion: 'unreviewed' }));
  assert.throws(() => { context().expected.model = 'other'; });
  const failed = { status: 'failed' };
  assert.throws(() => selectHarness({ task: task(), context: context(), baseResult: failed }));
  const evaluation = new RunContext({ ...context(), phase: 'heldout' });
  assert.ok(selectHarness({ task: task(), context: evaluation, baseResult: failed, specialist: true }).harness instanceof ReadOnlyVerificationHarness);
  assert.throws(() => selectHarness({ task: task({ sideEffects: 'write' }), context: evaluation, baseResult: failed }));
  const success = { status: 'success' }; assert.equal(selectHarness({ task: task(), context: context(), baseResult: success }).reuse, success);
});
test('incomplete oversized history cannot manufacture completion', async () => {
  const f = fixture({ onCall: m => m === 'session/page' ? { error: { code: 'result_too_large' } } : undefined });
  assert.equal((await f.runner.run(task(), context())).code, 'read_unavailable');
});
test('explicit cancellation submits once for matched running turn; queued input stays queued', async () => {
  const events = transcript().slice(0, 3); let cancels = 0;
  const f = fixture({ events, onCall: m => { if (m === 'session/cancel') { cancels++; events.push(event(4, 'turn/end', { turn: 1, reason: { kind: 'cancelled' } })); return { result: {} }; } } });
  assert.equal((await f.runner.run(task(), context())).status, 'timeout');
  assert.equal((await f.runner.run(task(), context(), { requestCancellation: true, exclusiveSession: true })).status, 'cancelled'); assert.equal(cancels, 1);
  const queued = fixture({ events: transcript().slice(0, 2) });
  await queued.runner.run(task(), context());
  assert.equal((await queued.runner.run(task(), context(), { requestCancellation: true, exclusiveSession: true })).code, 'queued_cancellation_unsupported');
  assert.equal(queued.calls.filter(c => c.method === 'session/cancel').length, 0);
});
test('unknown cancellation cannot be repeated', async () => {
  let cancels = 0;
  const f = fixture({ events: transcript().slice(0, 3), onCall: m => { if (m === 'session/cancel') { cancels++; throw Error('lost response'); } } });
  await f.runner.run(task(), context());
  assert.equal((await f.runner.run(task(), context(), { requestCancellation: true, exclusiveSession: true })).code, 'cancellation_outcome_unknown');
  await f.runner.run(task(), context(), { requestCancellation: true, exclusiveSession: true }); assert.equal(cancels, 1);
});
test('stream loss after acceptance is unknown and resumes without side effect replay', async () => {
  let unavailable = true;
  const f = fixture({ onCall: m => { if (m === 'session/page' && unavailable) throw Error('disconnect'); } });
  assert.equal((await f.runner.run(task(), context())).status, 'unknown'); unavailable = false;
  assert.equal((await f.runner.run(task(), context())).status, 'success');
  assert.equal(f.calls.filter(c => c.method === 'session/prompt').length, 1);
});
test('old terminal before submission baseline is ignored', async () => {
  const f = fixture({ initial: 5 });
  assert.equal((await f.runner.run(task(), context())).status, 'timeout');
});
test('shared journal prevents concurrent duplicate submissions across runner instances', async () => {
  const journal = new Map(); let release;
  const gate = new Promise(r => { release = r; }); let prompts = 0;
  const call = async method => {
    if (method === 'session/projections') { await gate; return { result: { values: { agentPreset: 'standard', modelSelection: { next: model } } } }; }
    if (method === 'session/follow') return { result: { cursor: 0 } };
    if (method === 'session/prompt') { prompts++; return { result: {} }; }
  };
  const a = new HarnessRunner(new DshProviderAdapter(call), { journal, sleep: async () => {} });
  const b = new HarnessRunner(new DshProviderAdapter(call), { journal, sleep: async () => {} });
  const pending = a.run(task(), context());
  assert.equal((await b.run(task(), context())).code, 'observation_in_progress'); release(); await pending;
  assert.equal(prompts, 1);
});
test('late configuration completion cannot claim success beyond deadline', async () => {
  let now = 0, projections = 0;
  const events = transcript(); let sent = false;
  const call = async (method, { request }) => {
    if (method === 'session/projections') { if (++projections === 2) now = 101; return { result: { values: { agentPreset: 'standard', modelSelection: { next: model } } } }; }
    if (method === 'session/prompt') { sent = true; return { result: {} }; }
    if (method === 'session/follow') return { result: { cursor: sent ? 5 : 0 } };
    return { result: { records: events.filter(e => e.seq < request.beforeSeq).slice(-1).map(event => ({ event })) } };
  };
  const runner = new HarnessRunner(new DshProviderAdapter(call), { now: () => now });
  const r = await runner.run(task({ budget: { timeoutMs: 100 } }), context()); assert.equal(r.status, 'timeout'); assert.equal(r.completed, false);
});
test('spoofed constructor cannot bypass frozen policy selection', async () => {
  class Spoof extends BaseHarness { verify() { return true; } }
  Object.defineProperty(Spoof.prototype, 'constructor', { get() { return BaseHarness; } });
  assert.equal((await fixture().runner.run(task(), context(), { harness: new Spoof() })).code, 'unreviewed_policy');
  assert.throws(() => { BaseHarness.prototype.verify = () => true; });
});

test('cancellation refuses foreign input before own in the active turn', async () => {
  const events = [event(1, 'turn/start', { turn: 1 }), event(2, 'user/message', { source: { rpcId: 'other' } }), event(3, 'user/message', { source: { rpcId: 'synthetic-request' } }), event(4, 'request/header', { header: { config: model } })];
  const f = fixture({ events }); await f.runner.run(task(), context());
  assert.equal((await f.runner.run(task(), context(), { requestCancellation: true, exclusiveSession: true })).code, 'cancellation_identity_ambiguous');
  assert.equal(f.calls.filter(c => c.method === 'session/cancel').length, 0);
});

test('missing sequence cannot hide a foreign input', async () => {
  const events = transcript(); events[3].seq = 5; events[4].seq = 6;
  assert.equal((await fixture({ events }).runner.run(task(), context())).code, 'incomplete_history');
});
test('session-wide cancellation requires exclusive caller ownership', async () => {
  const f = fixture({ events: transcript().slice(0, 3) }); await f.runner.run(task(), context());
  assert.equal((await f.runner.run(task(), context(), { requestCancellation: true })).code, 'exclusive_session_required');
});
