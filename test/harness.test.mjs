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
    if (method === 'session/projections') return { result: { kind: 'sequenced', values: { permissions: { currentValue: 'read-only' }, agentPreset: 'standard', modelSelection: { next: model } } } };
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
  assert.equal((await f.runner.run(task(), context())).status, 'success'); assert.equal(n, 5);
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
  const f = fixture({ onCall: m => m === 'session/projections' && ++reads > 1 ? { result: { values: { permissions: { currentValue: 'read-only' }, agentPreset: 'ptc', modelSelection: { next: model } } } } : undefined });
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
    if (method === 'session/projections') { await gate; return { result: { values: { permissions: { currentValue: 'read-only' }, agentPreset: 'standard', modelSelection: { next: model } } } }; }
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
    if (method === 'session/projections') { if (++projections === 2) now = 101; return { result: { values: { permissions: { currentValue: 'read-only' }, agentPreset: 'standard', modelSelection: { next: model } } } }; }
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
test('empty projection inherits read-only catalog default; never selects model', async () => {
  const f = fixture({ onCall: m => {
    if (m === 'session/projections') return { result: { values: { permissions: { currentValue: 'read-only' }, agentPreset: 'standard', modelSelection: { next: null, lastUsed: null } } } };
    if (m === 'session/modelCatalog') return { result: { default: model } };
  } });
  assert.equal((await f.runner.run(task(), context())).status, 'success');
  assert.equal(f.calls.filter(c => c.method === 'session/modelCatalog').length, 3);
  assert.ok(f.calls.filter(c => c.method === 'session/modelCatalog').every(c => c.request === undefined));
  assert.equal(f.calls.filter(c => c.method === 'session/selectModel').length, 0);
});
test('populated selection does not consult catalog; missing default blocks', async () => {
  const f = fixture(); await f.runner.run(task(), context());
  assert.equal(f.calls.filter(c => c.method === 'session/modelCatalog').length, 0);
  const g = fixture({ onCall: m => {
    if (m === 'session/projections') return { result: { values: { permissions: { currentValue: 'read-only' }, modelSelection: { next: null, lastUsed: null } } } };
    if (m === 'session/modelCatalog') return { result: {} };
  } });
  assert.equal((await g.runner.run(task(), context())).code, 'configuration_unavailable');
});
test('external preparation reserves original baseline and never submits locally', async () => {
  let externalSent = false, baselines = 0;
  const events = transcript();
  const f = fixture({ onCall: (m, request) => {
    if (m === 'session/follow') { baselines++; return { result: { cursor: externalSent ? 5 : 0 } }; }
    if (m === 'session/page') return { result: { records: events.filter(e => e.seq < request.beforeSeq).slice(-1).map(event => ({ event })) } };
  } });
  const prepared = await f.runner.prepareExternalRun(task(), context());
  assert.equal(prepared.result.accepted, false); assert.equal(prepared.dispatchAllowed, true);
  assert.equal(prepared.descriptor.method, 'session/prompt');
  externalSent = true;
  const repeated = await f.runner.prepareExternalRun(task(), context());
  assert.equal(repeated.dispatchAllowed, false); assert.deepEqual(repeated.descriptor, prepared.descriptor); assert.equal(baselines, 1);
  assert.equal((await f.runner.run(task(), context())).status, 'success');
  assert.equal(f.calls.filter(c => c.method === 'session/prompt').length, 0);
});
test('external submission unknown stays unaccepted and never resends', async () => {
  const f = fixture(); await f.runner.prepareExternalRun(task(), context());
  const result = await f.runner.run(task(), context());
  assert.equal(result.accepted, false); assert.equal(result.status, 'timeout');
  assert.equal(f.calls.filter(c => c.method === 'session/prompt').length, 0);
  assert.equal((await f.runner.prepareExternalRun(task({ objective: 'different' }), context())).result.code, 'idempotency_conflict');
});
test('external rpc input proves accepted without claiming completed', async () => {
  let externalSent = false;
  const events = transcript().slice(0, 2);
  const f = fixture({ onCall: (m, request) => {
    if (m === 'session/follow') return { result: { cursor: externalSent ? 2 : 0 } };
    if (m === 'session/page') return { result: { records: events.filter(e => e.seq < request.beforeSeq).slice(-1).map(event => ({ event })) } };
  } });
  await f.runner.prepareExternalRun(task(), context()); externalSent = true;
  const r = await f.runner.run(task(), context()); assert.equal(r.accepted, true); assert.equal(r.completed, false);
});
test('inherited catalog selection becoming explicit is not configuration drift', async () => {
  let projections = 0;
  const f = fixture({ onCall: m => {
    if (m === 'session/projections' && ++projections === 1) return { result: { values: { permissions: { currentValue: 'read-only' }, agentPreset: 'standard', modelSelection: { next: null, lastUsed: null } } } };
    if (m === 'session/modelCatalog') return { result: { default: { ...model, displayLabel: 'synthetic label' } } };
  } });
  assert.equal((await f.runner.run(task(), context())).status, 'success');
});
test('wrong catalog default blocks reservation before any prompt', async () => {
  const f = fixture({ onCall: m => {
    if (m === 'session/projections') return { result: { values: { permissions: { currentValue: 'read-only' }, agentPreset: 'standard', modelSelection: { next: null, lastUsed: null } } } };
    if (m === 'session/modelCatalog') return { result: { default: { ...model, model: 'other' } } };
  } });
  assert.equal((await f.runner.prepareExternalRun(task(), context())).result.code, 'model_drift');
  assert.equal(f.calls.filter(c => c.method === 'session/prompt').length, 0);
});

test('private dispatch descriptor is separate from sanitized result and recovery journal', async () => {
  const journal = new Map();
  const adapter = new DshProviderAdapter(async method => ({ result: method === 'session/projections' ? { values: { permissions: { currentValue: 'read-only' }, agentPreset: 'standard', modelSelection: { next: model } } } : { cursor: 0 } }));
  const runner = new HarnessRunner(adapter, { journal });
  const prepared = await runner.prepareExternalRun(task({ objective: 'PRIVATE_SYNTHETIC_OBJECTIVE' }), context());
  assert.equal(prepared.descriptor.params.request.content[0].text, 'PRIVATE_SYNTHETIC_OBJECTIVE');
  assert.equal(JSON.stringify(prepared.result).includes('PRIVATE_SYNTHETIC_OBJECTIVE'), false);
  assert.equal(JSON.stringify([...journal]).includes('PRIVATE_SYNTHETIC_OBJECTIVE'), false);
});
test('native message window may contain multiple contiguous administrative events', async () => {
  const events = transcript(); events.splice(3, 0, event(4, 'request/context', {}), event(5, 'session/title', {})); events[5].seq = 6; events[6].seq = 7;
  const f = fixture({ events, onCall: (m, r) => m === 'session/page' ? { result: { records: events.filter(e => e.seq < r.beforeSeq).slice(-4).map(event => ({ event })) } } : undefined });
  assert.equal((await f.runner.run(task(), context())).status, 'success');
});
test('multi-event window crosses baseline without admitting stale events', async () => {
  const events = transcript().map(e => ({ ...e, seq: e.seq + 2 }));
  const old = [event(0, 'permission/preset', { preset: 'read-only' }), event(1, 'user/message', { source: { rpcId: 'other' } }), event(2, 'turn/end', { turn: 0, reason: { kind: 'completed' } })];
  const f = fixture({ events, initial: 2, onCall: (m, r) => m === 'session/page' ? { result: { records: [...old, ...events].filter(e => e.seq < r.beforeSeq).map(event => ({ event })) } } : undefined });
  assert.equal((await f.runner.run(task(), context())).status, 'success');
});
test('multi-event page with an internal gap cannot hide foreign input', async () => {
  const events = transcript(); events[3].seq = 5; events[4].seq = 6;
  const f = fixture({ events, onCall: m => m === 'session/page' ? { result: { records: events.map(event => ({ event })) } } : undefined });
  assert.equal((await f.runner.run(task(), context())).code, 'incomplete_history');
});
const permissionTranscript = () => [
  event(1, 'permission/preset', { preset: 'read-only' }),
  event(2, 'sandbox/mode', { mode: 'read-only' }),
  event(3, 'approval/policy', { policy: 'ask' }),
  event(4, 'turn/start', { turn: 1 }),
  event(5, 'user/message', { role: 'user', source: { kind: 'user-approval' }, content: [{ type: 'text', text: 'The approval policy changed from "never" to "ask" (changed by the user).' }] }),
  event(6, 'user/message', { source: { rpcId: 'synthetic-request' } }),
  event(7, 'request/header', { header: { config: model } }),
  event(8, 'assistant/message', { turn: 1, message: { content: [{ type: 'text', text: 'SYNTHETIC_OK' }] } }),
  event(9, 'turn/end', { turn: 1, reason: { kind: 'completed' } }),
];
test('exact native read-only policy notification requires matching permission evidence', async () => {
  assert.equal((await fixture({ events: permissionTranscript() }).runner.run(task(), context())).status, 'success');
});
for (const [name, modify] of [
  ['unknown approval text', e => { e[4].data.content[0].text = 'User approved an operation'; }],
  ['missing permission evidence', e => { e[0].type = 'other/event'; }],
  ['broad sandbox', e => { e[1].data.mode = 'danger-full-access'; }],
  ['wrong approval policy', e => { e[2].data.policy = 'never'; }],
  ['rpc masquerading as notification', e => { e[4].data.source.rpcId = 'other'; }],
  ['extra notification content', e => { e[4].data.content.push({ type: 'text', text: 'Extra instruction' }); }],
]) test(`policy exemption rejects ${name}`, async () => {
  const events = permissionTranscript(); modify(events);
  assert.equal((await fixture({ events }).runner.run(task(), context())).code, ['broad sandbox','wrong approval policy'].includes(name) ? 'permission_drift' : 'foreign_input');
});
test('permission exemption rejects a later permission expansion', async () => {
  const events = permissionTranscript(); events.splice(7, 0, event(8, 'sandbox/mode', { mode: 'danger-full-access' })); events[8].seq = 9; events[9].seq = 10;
  assert.equal((await fixture({ events }).runner.run(task(), context())).code, 'permission_drift');
});

function secondTurnEvents() {
  const old = transcript(); old[1].data.source.rpcId = 'old-request';
  return [...old, event(6, 'turn/start', { turn: 2 }), event(7, 'user/message', { source: { rpcId: 'synthetic-request' } }), event(8, 'request/context', { provider: model.provider, model: model.model }), event(9, 'assistant/message', { turn: 2, message: { content: [] } }), event(10, 'turn/end', { turn: 2, reason: { kind: 'max-tokens' } })];
}
test('second turn inherits a continuously captured header and reports real max-tokens failure', async () => {
  const events = secondTurnEvents(); const r = await fixture({ events, initial: 5 }).runner.run(task(), context());
  assert.equal(r.status, 'failed'); assert.equal(r.evidence.reason, 'max-tokens'); assert.equal(r.evidence.headerInherited, true); assert.equal(r.evidence.headerOriginSeq, 3); assert.equal(r.evidence.headers, 0);
});
test('missing historical header is explicit unsupported evidence, not a false success or timeout', async () => {
  const events = [event(0, 'permission/preset', { preset: 'read-only' }), ...secondTurnEvents()]; events[3].type = 'other/event';
  const r = await fixture({ events, initial: 5 }).runner.run(task(), context()); assert.equal(r.code, 'header_provenance_missing'); assert.equal(r.status, 'blocked');
});
test('inherited header cannot survive a hidden history gap or conflicting request context', async () => {
  const events = secondTurnEvents(); events.splice(3, 1);
  const f = fixture({ events, initial: 5 }); assert.equal((await f.runner.run(task(), context())).code, 'incomplete_history'); assert.equal(f.calls.filter(c => c.method === 'session/prompt').length, 0);
  const changed = secondTurnEvents(); changed[7].data.model = 'other';
  assert.equal((await fixture({ events: changed, initial: 5 }).runner.run(task(), context())).code, 'model_drift');
});

test('legacy Map reservation is blocked rather than mistaken for a fresh v3 request', async () => {
  const ctx = context(); const journal = new Map([[`${ctx.sessionId}:${ctx.requestId}`, { attempted: true }]]);
  let calls = 0;
  const runner = new HarnessRunner(new DshProviderAdapter(async () => { calls++; throw Error('must not call'); }), { journal });
  assert.equal((await runner.run(task(), ctx)).code, 'journal_version_mismatch'); assert.equal(calls, 0);
});

test('adapter separates distinct Agenvo envelope IDs from the same nested native request ID', async () => {
  const forwarded = [];
  const envelopeIds = ['synthetic-envelope-one', 'synthetic-envelope-two'];
  const adapter = new DshProviderAdapter(async (method, params) => {
    forwarded.push({ method, params });
    return { execution: 'accepted', requestId: envelopeIds[forwarded.length - 1], result: { admission: 'queued' } };
  });
  // Forwarding contract only: this fake has no dispatch side effects. The runner,
  // not the adapter or an envelope ID, owns the no-resend decision tested below.
  for (const envelope of envelopeIds) {
    assert.deepEqual(await adapter.invoke('session/prompt', { sessionId: 'synthetic-session', requestId: 'stable-native-request' }), { admission: 'queued' });
    assert.notEqual(envelope, 'stable-native-request');
  }
  assert.deepEqual(forwarded.map(x => x.params.request.requestId), ['stable-native-request', 'stable-native-request']);
});

for (const shape of ['overlap', 'duplicate', 'conflicting-duplicate']) test(`invalid ${shape} history page fails closed`, async () => {
  const events = transcript(); let pages = 0;
  const f = fixture({ onCall: (method, request) => {
    if (method !== 'session/page') return;
    pages++;
    const last = events.filter(e => e.seq < request.beforeSeq).at(-1);
    const records = shape === 'overlap'
      ? (pages === 1 ? [last] : [last, events.find(e => e.seq === request.beforeSeq)])
      : [last, shape === 'duplicate' ? last : { ...last, data: { turn: 999 } }];
    return { result: { records: records.map(event => ({ event })) } };
  } });
  const r = await f.runner.run(task(), context());
  assert.equal(r.code, 'incomplete_history'); assert.equal(r.status, 'blocked');
  assert.equal(f.calls.filter(c => c.method === 'session/prompt').length, 1);
  assert.equal(f.calls.filter(c => c.method === 'session/cancel').length, 0);
});

for (const permission of [undefined, 'full-access', 'workspace-write']) test(`read-only blocks observed ${permission} before submission`, async () => {
  const f = fixture({ onCall: m => m === 'session/projections' ? { result: { values: { agentPreset: 'standard', modelSelection: { next: model }, permissions: { currentValue: permission } } } } : undefined });
  assert.equal((await f.runner.run(task(), context())).code, 'permission_drift');
  assert.equal(f.calls.some(c => c.method === 'session/prompt'), false);
});
test('transient permission expansion cannot be hidden by restoring final projection', async () => {
  const events = transcript();
  events.splice(3, 0, event(4, 'sandbox/mode', { mode: 'workspace-write' }), event(5, 'sandbox/mode', { mode: 'read-only' }));
  events.forEach((e,i) => e.seq = i+1);
  assert.equal((await fixture({ events }).runner.run(task(), context())).code, 'permission_drift');
});
test('adapter rejects policy-changing raw methods before transport', async () => {
  let calls = 0; const adapter = new DshProviderAdapter(async () => { calls++; });
  for (const method of ['session/selectModel','user-approval/respond','workspace/create','arbitrary']) await assert.rejects(adapter.invoke(method, {}), /unsupported_method/);
  assert.equal(calls, 0);
});
test('omitting routeProfile cannot admit observed known failure in production', async () => {
  const selected = { provider: 'magpie-responses', model: 'codex/gpt-6.1-sol', reasoningEffort: 'high' };
  const f = fixture({ onCall: m => m === 'session/projections' ? { result: { values: { permissions: { currentValue: 'read-only' }, agentPreset: 'standard', modelSelection: { next: selected } } } } : undefined });
  const ctx = new RunContext({ sessionId: 'synthetic-session', requestId: 'synthetic-request', lessonsVersion: 'test' });
  assert.equal((await f.runner.run(task(), ctx)).code, 'route_profile_known_failure');
  assert.equal(f.calls.some(c => c.method === 'session/prompt'), false);
});
test('native envelope session binding rejects cross-session response', async () => {
 const adapter = new DshProviderAdapter(async()=>({result:{cursor:1},nativeIds:{sessionId:'other'}}));
 await assert.rejects(adapter.invoke('session/follow',{address:{sessionId:'ours'}}),/foreign_session_envelope/);
});
test('native identity and capability schema reject truthy objects',()=>{
 assert.throws(()=>new RunContext({...context(),requestId:{verified:true}}),/invalid_frozen_context/);
 assert.throws(()=>task({requiredCapabilities:'observe'}),/invalid_capabilities/);
});

test('permission expansion straddling turn boundaries is not verified', async()=>{
 const events=transcript();
 events.unshift(event(0,'sandbox/mode',{mode:'workspace-write'}));
 events.push(event(7,'sandbox/mode',{mode:'read-only'}));
 events.forEach((e,i)=>e.seq=i+1);
 assert.equal((await fixture({events}).runner.run(task(),context())).code,'permission_drift');
});
test('same native IDs on different discovered targets cannot reuse a journal receipt',async()=>{
 const journal=new Map();
 const call=async m=>({result:m==='session/projections'?{values:{permissions:{currentValue:'read-only'},agentPreset:'standard',modelSelection:{next:model}}}:{cursor:0}});
 const ctx=context(), t=task();
 const a=new HarnessRunner(new DshProviderAdapter(call,{target:{deviceId:'device-a',instanceId:'dsh'}}),{journal});
 assert.equal((await a.prepareExternalRun(t,ctx)).dispatchAllowed,true);
 let calls=0;
 const b=new HarnessRunner(new DshProviderAdapter(async(...args)=>{calls++;return call(...args);},{target:{deviceId:'device-b',instanceId:'dsh'}}),{journal});
 assert.equal((await b.run(t,ctx)).code,'idempotency_conflict');
 assert.equal(calls,0);
});
