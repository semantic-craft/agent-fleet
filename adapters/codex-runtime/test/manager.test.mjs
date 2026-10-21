import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { OwnedRuntimeManager } from '../manager.mjs';
import { fakeFactory } from './fake-worker.mjs';
import { until } from './support.mjs';

async function setup(t, overrides = {}) {
  const root = await mkdtemp(join(tmpdir(), 'owned-manager-'));
  const factory = fakeFactory();
  const options = { stateDir: root, createWorker: factory.createWorker, ...overrides };
  const manager = new OwnedRuntimeManager(options);
  t.after(async () => {
    // Stop only fixture activity before deleting its metadata directory. Unknown
    // workers are intentionally retained by production logic, never force-closed.
    for (const w of factory.workers) if (!w.eofCount) w.emit('fault', new Error('fixture cleanup'));
    await rm(root, { recursive: true, force: true });
  });
  const { thread } = await manager.call('thread/start', {});
  return { manager, factory, options, thread, worker: factory.workers[0] };
}

test('inflight acceptance and idle status cannot release the exact accepted turn', async t => {
  const { manager, worker, thread } = await setup(t);
  let allow;
  worker.intercept = async method => {
    if (method === 'turn/start') await new Promise(resolve => { allow = resolve; });
  };
  const accepted = manager.call('turn/start', { threadId: thread.id, input: [{ type: 'text', text: 'fixture' }] });
  await until(() => allow);
  await assert.rejects(manager.release(thread.id), /busy|unknown/);
  assert.equal(worker.eofCount, 0);
  allow(); const { turn } = await accepted;
  assert.equal(worker.thread().status.type, 'idle');
  await assert.rejects(manager.release(thread.id), /busy|unknown/);
  assert.equal(worker.eofCount, 0);
  worker.complete(turn.id);
  await until(() => manager.status()[0].state, v => v !== 'checking' && v !== 'closing');
  if (!worker.eofCount) await manager.release(thread.id);
  assert.equal(worker.eofCount, 1);
});

test('pending native interaction retains owned worker even after idle and turn completion', async t => {
  const { manager, worker, thread } = await setup(t);
  const packets = []; manager.on('packet', p => packets.push(p));
  const { turn } = await manager.call('turn/start', { threadId: thread.id, input: [] });
  worker.emitPacket('item/tool/requestUserInput', { threadId: thread.id, turnId: turn.id, questions: [] }, 7);
  worker.complete(turn.id);
  await assert.rejects(manager.release(thread.id), /busy|unknown/);
  assert.equal(worker.eofCount, 0);
  const question = packets.find(p => p.method === 'item/tool/requestUserInput');
  assert.notEqual(question.id, 7, 'native request ids are namespaced');
  manager.reply(question.id, { result: { answers: {} } });
  assert.deepEqual(worker.replies.at(-1), { id: 7, result: { answers: {} } });
  await manager.release(thread.id);
  assert.equal(worker.eofCount, 1);
});

test('native request resolution clears a pending interaction without inventing an answer', async t => {
  const { manager, worker, thread } = await setup(t);
  const packets = []; manager.on('packet', packet => packets.push(packet));
  worker.emitPacket('item/tool/requestUserInput', { threadId: thread.id, questions: [] }, 'request-1');
  await assert.rejects(manager.release(thread.id), /busy|unknown/);
  worker.emitPacket('serverRequest/resolved', { threadId: thread.id, requestId: 'request-1' });
  assert.equal(packets.find(p => p.method === 'serverRequest/resolved').params.requestId,
    packets.find(p => p.method === 'item/tool/requestUserInput').id);
  await manager.release(thread.id);
  assert.equal(worker.eofCount, 1);
  assert.equal(worker.replies.length, 0);
});

for (const kind of ['background', 'queue']) test(`${kind} work blocks close, including a nonempty later page`, async t => {
  const { manager, worker, thread } = await setup(t);
  const method = kind === 'background' ? 'thread/backgroundTerminals/list' : 'thread/queue/list';
  worker.intercept = async (m, p) => m === method ? (p.cursor ? { data: [{ id: 'still-running' }], nextCursor: null } : { data: [], nextCursor: 'next' }) : undefined;
  await assert.rejects(manager.release(thread.id), /background|queued/);
  assert.equal(worker.eofCount, 0);
  worker.intercept = undefined;
  await manager.release(thread.id);
  assert.equal(worker.eofCount, 1);
});

test('all loaded threads are checked; a running child prevents root idle close', async t => {
  const { manager, worker, thread } = await setup(t);
  worker.loaded.add('child'); worker.thread('child').status.type = 'active';
  await assert.rejects(manager.release(thread.id), /idle/);
  assert.equal(worker.eofCount, 0);
  worker.thread('child').status.type = 'idle';
  await manager.release(thread.id);
  assert.equal(worker.eofCount, 1);
});

test('new input is rejected while close check is fenced, without forwarding input', async t => {
  const { manager, worker, thread } = await setup(t);
  let proceed;
  worker.intercept = async method => {
    if (method === 'thread/loaded/list') await new Promise(resolve => { proceed = resolve; });
  };
  const closing = manager.release(thread.id);
  await until(() => proceed);
  await assert.rejects(manager.call('turn/start', { threadId: thread.id, input: [] }), /checking|input not sent/);
  assert.equal(worker.calls.filter(c => c.method === 'turn/start').length, 0);
  proceed(); await closing;
  assert.equal(worker.eofCount, 1);
});

test('unclassified native events fail closed, including after manager restart', async t => {
  const { manager, factory, worker, thread, options } = await setup(t);
  worker.emitPacket('future/unknownActivity', { threadId: thread.id });
  await assert.rejects(manager.release(thread.id), /busy|unknown/);
  assert.equal(worker.eofCount, 0);
  const restored = new OwnedRuntimeManager(options);
  assert.equal(restored.status()[0].state, 'unknown');
  await assert.rejects(restored.call('thread/resume', { threadId: thread.id }), /unknown/);
  assert.equal(factory.workers.length, 1, 'restart must not replay or silently replace unknown owner');
});

test('released thread mapping survives restart, with explicit resume and no replayed input', async t => {
  const { manager, factory, thread, options } = await setup(t);
  await manager.release(thread.id);
  const restored = new OwnedRuntimeManager(options);
  assert.equal(restored.status()[0].state, 'released');
  const resumed = await restored.call('thread/resume', { threadId: thread.id });
  assert.equal(resumed.thread.id, thread.id);
  assert.equal(factory.workers.length, 2);
  assert.equal(factory.workers.flatMap(w => w.calls).filter(c => c.method === 'turn/start').length, 0);
  await restored.release(thread.id);
});

test('lost native acknowledgement is unknown and never replayed or closed', async t => {
  const { manager, factory, worker, thread, options } = await setup(t);
  worker.intercept = async method => { if (method === 'turn/start') throw new Error('execution_unknown: response lost'); };
  await assert.rejects(manager.call('turn/start', { threadId: thread.id, input: [] }), /unknown/);
  await assert.rejects(manager.release(thread.id), /busy|unknown/);
  assert.equal(worker.eofCount, 0);
  const restored = new OwnedRuntimeManager(options);
  await assert.rejects(restored.call('thread/resume', { threadId: thread.id }), /unknown/);
  assert.equal(factory.workers.length, 1);
  assert.equal(worker.calls.filter(c => c.method === 'turn/start').length, 1);
});

test('unroutable process APIs are rejected without forwarding or killing the worker', async t => {
  const { manager, worker, thread } = await setup(t);
  await assert.rejects(manager.call('process/spawn', { processHandle: 'unscoped', command: ['unused'] }), /unsupported/);
  assert.equal(worker.calls.some(c => c.method === 'process/spawn'), false);
  assert.equal(worker.eofCount, 0);
  await manager.release(thread.id);
});

test('subagent notification registers child beyond loaded list and prevents premature close', async t => {
  const { manager, worker, thread } = await setup(t);
  worker.thread('unloaded-child').status.type = 'active';
  worker.emitPacket('item/completed', { threadId: thread.id, item: { id: 'spawn', type: 'collabAgentToolCall', tool: 'spawnAgent', receiverThreadIds: ['unloaded-child'], agentsStates: {}, status: 'completed' } });
  await assert.rejects(manager.release(thread.id), /idle|busy|unknown/);
  assert.equal(worker.eofCount, 0);
  worker.thread('unloaded-child').status.type = 'idle';
  await manager.release(thread.id);
  assert.equal(worker.eofCount, 1);
});

test('background completion is rechecked and eventually releases without new input', { timeout: 8000 }, async t => {
  const { manager, worker, thread } = await setup(t);
  const { turn } = await manager.call('turn/start', { threadId: thread.id, input: [] });
  worker.background = [{ processId: 'bg' }];
  worker.complete(turn.id);
  await until(() => manager.status()[0].reason, value => /background/.test(value ?? ''));
  assert.equal(worker.eofCount, 0);
  worker.background = [];
  await until(() => worker.eofCount, value => value === 1, 6500);
  assert.equal(manager.status()[0].state, 'released');
});

test('missing accepted turn in native history fails closed despite terminal event', async t => {
  const { manager, worker, thread } = await setup(t);
  const { turn } = await manager.call('turn/start', { threadId: thread.id, input: [] });
  worker.intercept = async method => method === 'thread/turns/list' ? { data: [], nextCursor: null } : undefined;
  worker.complete(turn.id);
  await until(() => manager.status()[0].state, state => state !== 'checking');
  await assert.rejects(manager.release(thread.id), /accepted|history|turn/);
  assert.equal(worker.eofCount, 0);
});

test('abnormal EOF exit cannot be reported as released', async t => {
  const { manager, worker, thread } = await setup(t);
  worker.eof = async () => { worker.eofCount++; return { code: 1, signal: null }; };
  await assert.rejects(manager.release(thread.id), /normally/);
  assert.equal(manager.status()[0].state, 'unknown');
  assert.equal(worker.eofCount, 1);
});

test('concurrent resume loser creates no empty owned worker', async t => {
  const { manager, factory, thread, options } = await setup(t);
  await manager.release(thread.id);
  let allow;
  const restored = new OwnedRuntimeManager({ ...options, createWorker: async args => {
    await new Promise(resolve => { allow = resolve; });
    return factory.createWorker(args);
  } });
  t.after(() => clearInterval(restored.timer));
  const first = restored.call('thread/resume', { threadId: thread.id });
  await until(() => allow);
  await assert.rejects(restored.call('thread/resume', { threadId: thread.id }), /concurrent resume/);
  assert.equal(factory.workers.length, 1, 'loser did not spawn an empty process');
  allow(); await first;
  assert.equal(factory.workers.length, 2);
  assert.equal(restored.status().filter(u => u.state === 'open').length, 1);
  await restored.release(thread.id);
});

test('collab receiver owned by another live runtime does not change routing or close identity', async t => {
  const { manager, factory, worker: sender, thread } = await setup(t);
  const { thread: other } = await manager.call('thread/start', {});
  const receiver = factory.workers[1], packets = [];
  manager.on('packet', p => packets.push(p));
  sender.emitPacket('item/completed', { threadId: thread.id,
    item: { id: 'send', type: 'collabAgentToolCall', tool: 'sendInput', receiverThreadIds: [other.id], agentsStates: {}, status: 'completed' } });
  const turn = await manager.call('turn/start', { threadId: other.id, input: [] });
  assert.equal(sender.calls.some(c => c.method === 'turn/start'), false);
  assert.equal(receiver.calls.filter(c => c.method === 'turn/start').length, 1);
  await assert.rejects(manager.release(thread.id), /idle|terminal/);
  assert.equal(sender.eofCount, 0);
  receiver.complete(turn.turn.id, other.id);
  await until(() => manager.status()[1].state, state => state !== 'checking' && state !== 'closing');
  if (!receiver.eofCount) await manager.release(other.id);
  await manager.release(thread.id);
  const senderId = manager.status()[0].id;
  assert.deepEqual(packets.filter(p => p.method === 'thread/closed' && p.params.managedRuntime.id === senderId).map(p => p.params.threadId), [thread.id]);
});

test('metadata ENOSPC during native completion retains worker and blocks fresh input', async t => {
  let fail = false;
  const snapshots = [];
  const { manager, worker, thread } = await setup(t, { persist(snapshot) {
    if (fail) throw Object.assign(new Error('No space left on device'), { code: 'ENOSPC' });
    snapshots.push(structuredClone(snapshot));
  } });
  const { turn } = await manager.call('turn/start', { threadId: thread.id, input: [] });
  assert.ok(snapshots.at(-1).units.some(u => u.turns.some(([id]) => id.endsWith(turn.id))));
  fail = true;
  assert.doesNotThrow(() => worker.complete(turn.id));
  assert.equal(manager.status()[0].state, 'unknown');
  assert.equal(worker.eofCount, 0);
  await assert.rejects(manager.call('turn/start', { threadId: thread.id, input: [] }), /metadata unavailable/);
  assert.equal(worker.calls.filter(c => c.method === 'turn/start').length, 1);
  assert.equal(worker.eofCount, 0);
});
