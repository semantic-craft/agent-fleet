import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { createRequire } from 'node:module';
const require = createRequire(import.meta.url);
const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'cindy-worker-'));
process.env.AGENT_NETWORK_CINDY_DIR_TEST = dir;
fs.writeFileSync(path.join(dir, 'config.json'), JSON.stringify({ anetId: 'cindy', peerAid: 'peer', allowPeers: ['peer'] }));
const worker = require('../plugins/agent-network-cindy/node/worker.cjs');
const remote = { id: 'task', metadata: { 'anet.peer_aid': 'peer' }, history: [{ role: 'user', parts: [{ text: 'hello' }] }] };
function reset(tool, runJob) {
  for (const key of Object.keys(worker.state.tasks)) delete worker.state.tasks[key];
  worker.status.mainConnected = true;
  worker.setTestDependencies({ tool, runJob });
}
test('uncertain host create is persisted and never retried', async () => {
  let creates = 0;
  reset(async name => name === 'list_tasks' ? { tasks: [remote] } : remote, async () => { creates++; return { ok: false, error: { code: 'MAIN_TIMEOUT' } }; });
  await worker.tick(); await worker.tick();
  assert.equal(creates, 1);
  assert.equal(worker.state.tasks.task.phase, 'unverified');
  assert.equal(JSON.parse(fs.readFileSync(path.join(dir, 'state.json'))).tasks.task.phase, 'unverified');
});
test('uncertain result delivery reads state without repeating reply', async () => {
  let replies = 0; let reads = 0;
  reset(async name => {
    if (name === 'list_tasks') return { tasks: [] };
    if (name === 'reply_task') { replies++; throw Error('disconnected'); }
    reads++; return { status: { state: replies ? 'completed' : 'working' } };
  }, async () => ({ ok: true, run: { status: 'completed', outputMessageId: 'output' }, messages: [{ id: 'output', role: 'assistant', text: 'done' }] }));
  worker.state.tasks.task = { phase: 'running', acceptedAt: new Date().toISOString() };
  await worker.tick(); await worker.tick();
  assert.equal(replies, 1); assert.equal(reads, 2);
  assert.equal(worker.state.tasks.task.phase, 'delivery-state-observed');
});
test('observation timeout does not fail a running host task', async () => {
  let replies = 0;
  reset(async name => { if (name === 'reply_task') replies++; return { tasks: [] }; }, async () => ({ ok: true, run: { status: 'working' } }));
  worker.state.tasks.task = { phase: 'running', acceptedAt: new Date(0).toISOString() };
  await worker.tick();
  assert.equal(replies, 0); assert.equal(worker.state.tasks.task.phase, 'unverified');
});
test('official cancellation never triggers host status or a completion reply', async () => {
  let hostCalls = 0; let replies = 0;
  reset(async name => {
    if (name === 'list_tasks') return { tasks: [] };
    if (name === 'reply_task') replies++;
    return { status: { state: 'canceled' } };
  }, async () => { hostCalls++; });
  worker.state.tasks.task = { phase: 'running', acceptedAt: new Date().toISOString() };
  await worker.tick();
  assert.equal(hostCalls, 0); assert.equal(replies, 0);
  assert.equal(worker.state.tasks.task.error.code, 'OFFICIAL_TASK_ENDED_HOST_NOT_STOPPED');
});
test('working progress is rate limited', async () => {
  let replies = 0;
  reset(async name => { if (name === 'reply_task') replies++; return { tasks: [], status: { state: 'working' } }; }, async () => ({ ok: true, run: { status: 'working' } }));
  worker.state.tasks.task = { phase: 'running', acceptedAt: new Date().toISOString() };
  await worker.tick(); worker.state.tasks.task.lastCheck = 0; await worker.tick();
  assert.equal(replies, 1);
});
test('creating checks prefixed official cancellation before host side effects', async () => {
  let creates = 0;
  reset(async name => name === 'list_tasks' ? { tasks: [remote] } : { ...remote, status: { state: 'TASK_STATE_CANCELED' } }, async () => { creates++; });
  await worker.tick(); assert.equal(creates, 0);
  assert.equal(worker.state.tasks.task.error.code, 'OFFICIAL_TASK_ENDED_BEFORE_CREATE');
});
test('completed run never returns stale assistant output', async () => {
  let replies = 0;
  reset(async name => { if (name === 'reply_task') replies++; return { tasks: [], status: { state: 'working' } }; }, async () => ({ ok: true, run: { status: 'completed', outputMessageId: 'missing' }, messages: [{ id: 'old', role: 'assistant', text: 'old' }] }));
  worker.state.tasks.task = { phase: 'running', acceptedAt: new Date().toISOString() };
  await worker.tick(); assert.equal(replies, 0); assert.equal(worker.state.tasks.task.error.code, 'EXACT_OUTPUT_UNAVAILABLE');
});
test('submitted task discovery follows nextPageToken', async () => {
  let pages = 0;
  reset(async (name, args) => {
    if (name === 'list_tasks') { pages++; return args.page_token ? { tasks: [remote] } : { tasks: [], nextPageToken: 'next' }; }
    return remote;
  }, async () => ({ ok: false, error: { code: 'MAIN_TIMEOUT' } }));
  await worker.tick(); assert.equal(pages, 2); assert.equal(worker.state.tasks.task.phase, 'unverified');
});
test('same outgoing call id survives uncertainty without another send', async () => {
  let sends = 0;
  reset(async (name, args) => { if (name === 'send_message') { assert.equal(Object.hasOwn(args, 'message_id'), false); sends++; throw Error('disconnected'); } return {}; }, async () => {});
  const first = await worker.sendToPeer({ callId: 'outgoing-1' }, 'hello');
  const second = await worker.sendToPeer({ callId: 'outgoing-1' }, 'hello');
  assert.equal(first.sent, false); assert.equal(second.sent, false);
  assert.equal(first.messageId, second.messageId); assert.equal(sends, 1);
});
test('state write failure prevents host create', async () => {
  let creates = 0;
  reset(async name => name === 'list_tasks' ? { tasks: [remote] } : remote, async () => { creates++; });
  fs.rmSync(path.join(dir, 'state.json'), { force: true });
  fs.mkdirSync(path.join(dir, 'state.json'));
  await worker.tick(); assert.equal(creates, 0);
  fs.rmSync(dir, { recursive: true, force: true });
});
test('shipping archive contains the exact current sources', async () => {
  const { execFileSync } = await import('node:child_process');
  const root = path.resolve(import.meta.dirname, '../plugins/agent-network-cindy');
  execFileSync('python3', ['-c', `import json,pathlib,sys,zipfile
root=pathlib.Path(sys.argv[1]); version=json.loads((root/'ghost.json').read_text())['version']
with zipfile.ZipFile(root/'dist'/('agent-network-cindy-'+version+'.cindy')) as archive:
 for name in ['ghost.json','main.js','node/worker.cjs']:
  assert archive.read(name)==(root/name).read_bytes(), name
`, root]);
});
test('corrupt persisted state fails startup without replaying tasks', async () => {
  const { spawnSync } = await import('node:child_process');
  const corrupt = fs.mkdtempSync(path.join(os.tmpdir(), 'cindy-corrupt-'));
  try {
    fs.writeFileSync(path.join(corrupt, 'state.json'), '{broken');
    const r = spawnSync(process.execPath, [path.resolve(import.meta.dirname, '../plugins/agent-network-cindy/node/worker.cjs')], { env: { ...process.env, AGENT_NETWORK_CINDY_DIR_TEST: corrupt }, timeout: 3000 });
    assert.notEqual(r.status, 0); assert.equal(r.error, undefined);
    assert.equal(fs.readFileSync(path.join(corrupt, 'state.json'), 'utf8'), '{broken');
  } finally { fs.rmSync(corrupt, { recursive: true, force: true }); }
});
test('host create/send partial failure preserves task id and long input never creates', async () => {
  const vm = await import('node:vm');
  let creates = 0;
  const ctx = vm.createContext({ cindy: { node: { request: async () => ({}) }, onHostMessage() {}, tasks: {
    create: async () => { creates++; return { taskId: 'created-task', revision: 1 }; },
    send: async () => { throw Error('connection lost after send'); }
  } } });
  vm.runInContext(fs.readFileSync(path.resolve(import.meta.dirname, '../plugins/agent-network-cindy/main.js'), 'utf8'), ctx);
  const partial = await ctx.createJob({ requestKey: 'key', title: 'title', text: 'input' });
  assert.equal(partial.ok, false); assert.equal(partial.taskId, 'created-task');
  const long = await ctx.createJob({ requestKey: 'key2', title: 'title', text: 'a'.repeat(32769) });
  assert.equal(long.error.code, 'INPUT_TOO_LONG'); assert.equal(creates, 1);
});
