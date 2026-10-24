import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, readdirSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { ROUTE_PROFILES } from '../scripts/route-profiles.mjs';
import { createReadOnlyFixtureTask, RunContext, HarnessRunner, DshProviderAdapter, ReadOnlyVerificationHarness } from '../scripts/harness.mjs';
import { FileJournal } from '../scripts/file-journal.mjs';
const routeProfile = 'minimax-low-standard-read-fixture-v1';
const expected = ROUTE_PROFILES[routeProfile].expected;
const model = Object.fromEntries(['provider', 'model', 'reasoningEffort'].map(k => [k, expected[k]]));
const filePath = '/synthetic/private-fixture.txt'; const expectedLine = 'SYNTHETIC_PRIVATE_FIRST_LINE';
const task = options => createReadOnlyFixtureTask({ filePath, expectedLine, budget: { maxPolls: 1, pollMs: 0 }, ...options });
const context = () => new RunContext({ requestId: 'synthetic-request', sessionId: 'synthetic-session', lessonsVersion: 'reviewed-fixture-v1', expected, routeProfile });
const options = () => ({ harness: new ReadOnlyVerificationHarness() });
function fixture({ mutate = () => {}, permission = 'read-only', afterPermission = permission, inherited = false, unknown = false } = {}) {
  const calls = []; let sent = false; let projections = 0;
  const before = inherited ? [
    { type: 'turn/start', data: { turn: 1 } },
    { type: 'request/header', data: { header: { config: model } } },
    { type: 'turn/end', data: { turn: 1, reason: { kind: 'completed' } } },
  ] : [];
  const turn = inherited ? 2 : 1;
  const callData = { turn, step: 1, callId: 'synthetic-call', name: 'read', arguments: JSON.stringify({ file_path: filePath, offset: 1, limit: 1 }) };
  const resultData = { turn, step: 1, message: { role: 'tool', toolCallId: 'synthetic-call', source: { kind: 'tool', callId: 'synthetic-call' }, isError: false,
    content: [{ type: 'text', text: `<path>${filePath}</path>\n<type>file</type>\n<content>\n1: ${expectedLine}\n\n(End of file - total 1 lines)\n</content>` }] } };
  const current = [
    { type: 'turn/start', data: { turn } },
    { type: 'user/message', data: { source: { rpcId: 'synthetic-request' } } },
    ...(!inherited ? [{ type: 'request/header', data: { header: { config: model } } }] : []),
    { type: 'tool/call', data: callData }, { type: 'tool/result', data: resultData },
    { type: 'assistant/message', data: { turn, message: { content: [{ type: 'text', text: expectedLine }] } } },
    { type: 'turn/end', data: { turn, reason: { kind: 'completed' } } },
  ];
  mutate(callData, resultData, current);
  const events = [...before, ...current].map((e, i) => ({ ...e, seq: i + 1 }));
  const transport = async (method, { request }) => {
    calls.push(method);
    if (method === 'session/projections') return { result: { values: { agentPreset: 'standard', modelSelection: { next: model }, permissions: { currentValue: ++projections === 1 ? permission : afterPermission } } } };
    if (method === 'session/follow') return { result: { cursor: sent ? events.length : before.length } };
    if (method === 'session/page') return { result: { records: events.filter(e => e.seq < request.beforeSeq).map(event => ({ event })) } };
    if (method === 'session/prompt') { sent = true; if (unknown) throw Error('synthetic-response-loss'); return { result: {} }; }
    throw Error('unexpected mutation');
  };
  return { calls, runner: journal => new HarnessRunner(new DshProviderAdapter(transport), { journal, sleep: async () => {} }) };
}
test('fixture factory requires a nonempty single-line expectation and explicit path', () => {
  for (const extra of [{ filePath: '' }, { expectedLine: 'a\nb' }, { expectedLine: '' }]) assert.throws(() => task(extra));
});
for (const inherited of [false, true]) test(`one exact successful read verifies with inherited header=${inherited}`, async () => {
  const f = fixture({ inherited }); const result = await f.runner().run(task(), context(), options());
  assert.equal(result.status, 'success'); assert.equal(result.evidence.toolEvents, 2); assert.equal(result.evidence.headerInherited, inherited);
  assert.ok(f.calls.every(m => ['session/projections', 'session/follow', 'session/page', 'session/prompt'].includes(m)));
  for (const privateValue of [filePath, expectedLine, 'file_path', 'synthetic-call', 'toolContractMatched']) assert.ok(!JSON.stringify(result).includes(privateValue));
});
const tampering = [
  ['tools before this request input', (_, r, events) => { events.splice(1, 0, ...events.splice(3, 2)); }],
  ['wrong tool', c => { c.name = 'write'; }],
  ['wrong path', c => { c.arguments = JSON.stringify({ file_path: '/synthetic/other', offset: 1, limit: 1 }); }],
  ['wrong offset', c => { c.arguments = JSON.stringify({ file_path: filePath, offset: 2, limit: 1 }); }],
  ['wrong limit', c => { c.arguments = JSON.stringify({ file_path: filePath, offset: 1, limit: 2 }); }],
  ['extra argument', c => { c.arguments = JSON.stringify({ file_path: filePath, offset: 1, limit: 1, extra: true }); }],
  ['malformed arguments', c => { c.arguments = '{'; }],
  ['wrong call turn', c => { c.turn = 99; }],
  ['error result', (_, r) => { r.message.isError = true; }],
  ['missing success flag', (_, r) => { delete r.message.isError; }],
  ['wrong result turn', (_, r) => { r.turn = 99; }],
  ['wrong result step', (_, r) => { r.step = 99; }],
  ['wrong call id', (_, r) => { r.message.toolCallId = 'foreign'; }],
  ['wrong source id', (_, r) => { r.message.source.callId = 'foreign'; }],
  ['wrong read content', (_, r) => { r.message.content[0].text = '1: WRONG'; }],
  ['extra numbered line', (_, r) => { r.message.content[0].text += '\n2: EXTRA'; }],
  ['extra content block', (_, r) => { r.message.content.push({ type: 'text', text: 'extra' }); }],
  ['extra tool event', (_, r, events) => { events.splice(4, 0, { type: 'tool/result', data: structuredClone(r) }); }],
  ['result before call', (_, r, events) => { [events[3], events[4]] = [events[4], events[3]]; }],
];
for (const [name, mutate] of tampering) test(`${name} cannot pass on the final answer alone`, async () => {
  const f = fixture({ mutate }); const result = await f.runner().run(task(), context(), options());
  assert.equal(result.status, 'failed'); assert.equal(result.code, 'terminal_not_verified');
});
for (const permission of [undefined, 'workspace-write', 'danger-full-access']) test(`native permission ${permission} blocks before prompt`, async () => {
  const f = fixture({ permission: permission ?? null }); const r = await f.runner().run(task(), context(), options());
  assert.equal(r.code, 'permission_drift'); assert.deepEqual(f.calls, ['session/projections']);
});
test('permission drift after tool completion is blocked, not successful', async () => {
  const f = fixture({ afterPermission: 'workspace-write' }); const r = await f.runner().run(task(), context(), options());
  assert.equal(r.code, 'permission_drift'); assert.equal(f.calls.filter(m => m === 'session/prompt').length, 1);
});
test('fixture profile rejects BaseHarness before transport', async () => {
  const f = fixture(); assert.equal((await f.runner().run(task(), context())).code, 'route_profile_scope_unsupported'); assert.deepEqual(f.calls, []);
});
test('durable unknown admission revalidates tool contract without resubmit or raw persistence', async t => {
  const directory = mkdtempSync(join(tmpdir(), 'fixture-journal-')); t.after(() => rmSync(directory, { recursive: true, force: true }));
  const f = fixture({ unknown: true, inherited: true });
  assert.equal((await f.runner(new FileJournal(directory)).run(task(), context(), options())).status, 'unknown');
  const changed = await f.runner(new FileJournal(directory)).run(task({ filePath: '/synthetic/changed' }), context(), options());
  assert.equal(changed.code, 'idempotency_conflict');
  const result = await f.runner(new FileJournal(directory)).run(task(), context(), options()); assert.equal(result.status, 'success');
  assert.equal(f.calls.filter(m => m === 'session/prompt').length, 1);
  const text = readdirSync(directory).filter(n => n.endsWith('.json')).map(n => readFileSync(join(directory, n), 'utf8')).join('');
  for (const privateValue of [filePath, expectedLine, 'file_path', 'synthetic-call', 'toolContractMatched']) assert.ok(!text.includes(privateValue));
});
