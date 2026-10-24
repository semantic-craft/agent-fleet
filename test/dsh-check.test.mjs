import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dshTurn } from '../scripts/dsh-check.mjs';
function fake({ reason = 'completed', answer = 'Synthetic project', override } = {}) {
  const calls = []; let requestId;
  const config = { provider: 'synthetic', model: 'synthetic', reasoningEffort: 'low' };
  const events = () => [
    { seq: 1, type: 'turn/start', data: { turn: 1 } },
    { seq: 2, type: 'user/message', data: { source: { rpcId: requestId } } },
    { seq: 3, type: 'request/header', data: { header: { config } } },
    { seq: 4, type: 'assistant/message', data: { turn: 1, message: { content: [{ type: 'text', text: answer }] } } },
    { seq: 5, type: 'turn/end', data: { turn: 1, reason: { kind: reason } } },
  ];
  const call = async (method, params) => {
    calls.push({ method, params });
    const replacement = override?.(method);
    if (replacement !== undefined) return replacement;
    if (method === 'workspace/create') return { execution: 'accepted', result: { workspace: { workspaceId: 'synthetic-workspace' } } };
    if (method === 'session/create') return { execution: 'accepted', nativeIds: { sessionId: 'synthetic-created' }, result: { sessionId: 'synthetic-created' } };
    if (method === 'session/projections') return { result: { values: { agentPreset: 'standard', modelSelection: { next: config } } } };
    if (method === 'session/follow') return { result: { cursor: requestId ? 5 : 0 } };
    if (method === 'session/prompt') { requestId = params.request.requestId; return { execution: 'accepted', result: {} }; }
    if (method === 'session/page') return { result: { records: events().filter(e => e.seq < params.request.beforeSeq).map(event => ({ event })) } };
    throw Error('unexpected endpoint');
  };
  return { call, calls };
}
test('fleet imports the tested wrapper without duplicate implementation', () => {
  const source = readFileSync(new URL('../scripts/fleet.mjs', import.meta.url), 'utf8');
  assert.match(source, /import \{ dshTurn \} from "\.\/dsh-check\.mjs"/);
  assert.doesNotMatch(source, /function dshTurn\(/);
});
test('existing-session fleet wrapper uses the harness and no creation/auth paths', async () => {
  const f = fake(); const r = await dshTurn(f.call, { sessionId: 'synthetic-existing' });
  assert.deepEqual(r, { ok: true, sessionId: 'synthetic-existing', text: 'success: verified' });
  assert.equal(f.calls.filter(c => c.method === 'session/prompt').length, 1);
  assert.ok(f.calls.every(c => c.method.startsWith('session/')));
});
test('new-session wrapper creates workspace/session then waits for its own terminal', async () => {
  const f = fake(); const r = await dshTurn(f.call, { root: '/synthetic-projects', project: 'fixture' });
  assert.equal(r.ok, true); assert.equal(r.sessionId, 'synthetic-created');
  assert.deepEqual(f.calls.slice(0, 2).map(c => c.method), ['workspace/create', 'session/create']);
  assert.equal(f.calls[0].params.request.path, '/synthetic-projects/fixture');
});
for (const reason of ['max-tokens', 'error', 'completed']) test(`${reason} with empty output is not successful`, async () => {
  const f = fake({ reason, answer: '' }); const r = await dshTurn(f.call, { sessionId: 'synthetic-existing' });
  assert.equal(r.ok, false); assert.match(r.text, /failed: terminal_not_verified/);
});
for (const endpoint of ['workspace/create', 'session/create', 'session/prompt', 'session/follow']) test(`wrapped ${endpoint} errors are safe and never success`, async () => {
  const f = fake({ override: m => m === endpoint ? { error: { code: 'synthetic_failure', message: 'SECRET_FIXTURE_ERROR' } } : undefined });
  const r = await dshTurn(f.call, { root: '/synthetic-projects', project: 'fixture' });
  assert.equal(r.ok, false); assert.ok(!JSON.stringify(r).includes('SECRET_FIXTURE_ERROR'));
  assert.ok(f.calls.filter(c => c.method === 'session/prompt').length <= 1);
});
test('thrown creation error is sanitized, and missing root makes no call', async () => {
  const f = fake({ override: () => { throw Error('SECRET_FIXTURE_ERROR'); } });
  const r = await dshTurn(f.call, { root: '/synthetic-projects', project: 'fixture' });
  assert.deepEqual(r, { ok: false, sessionId: undefined, text: 'check_failed' });
  const g = fake(); assert.equal((await dshTurn(g.call, {})).ok, false); assert.equal(g.calls.length, 0);
});

test('separate dshTurn smoke checks create fresh native requests, not resumable CLI runs', async () => {
  const a = fake(), b = fake();
  assert.equal((await dshTurn(a.call, { sessionId: 'synthetic-existing' })).ok, true);
  assert.equal((await dshTurn(b.call, { sessionId: 'synthetic-existing' })).ok, true);
  const ids = [a, b].map(f => f.calls.find(x => x.method === 'session/prompt').params.request.requestId);
  assert.ok(ids.every(id => /^fleet-check-[a-f0-9]{24}$/.test(id))); assert.notEqual(ids[0], ids[1]);
});
