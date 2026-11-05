import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import vm from 'node:vm';
import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);
const ROOT = path.resolve(import.meta.dirname, '..');
const WORKER = path.join(ROOT, 'plugins/agent-network-cindy/node/worker.cjs');
const PEER = 'aid-peer';

process.env.AGENT_NETWORK_CINDY_TEST = '1';

function user(id, text, extra = {}) {
  return { messageId: id, role: 'ROLE_USER', parts: [{ text }], ...extra };
}
function task(id, users, options = {}) {
  const metadata = options.metadata === undefined ? { 'anet.peer_aid': options.peer ?? PEER } : options.metadata;
  return {
    id,
    metadata,
    status: { state: options.state ?? 'TASK_STATE_WORKING' },
    history: [...users],
  };
}
function setup({ official, rec } = {}) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'anet-cindy-followup-'));
  fs.writeFileSync(path.join(dir, 'config.json'), JSON.stringify({
    anetId: 'cindy', peerAid: PEER, allowPeers: [PEER], anetBin: '/unused/anet',
  }));
  const initialOfficial = typeof official === 'function' ? official() : official;
  const officialId = initialOfficial?.id;
  const initialState = { version: 1, outgoing: {}, tasks: officialId && rec ? { [officialId]: rec } : {} };
  fs.writeFileSync(path.join(dir, 'state.json'), JSON.stringify(initialState));
  process.env.AGENT_NETWORK_CINDY_DIR_TEST = dir;
  delete require.cache[WORKER];
  const worker = require(WORKER);
  const calls = { listTasks: [], prepare: [], send: [], status: [], replies: [] };
  const receipts = new Map();
  const defaults = { run1: { status: 'running' }, run2: { status: 'queued' }, run3: { status: 'queued' } };
  let runs = defaults;
  let failures = [];
  const latest = () => (typeof official === 'function' ? official() : official);
  worker.setTestDependencies({
    async tool(name, args) {
      if (name === 'list_tasks') {
        calls.listTasks.push(args);
        return { tasks: official && rec ? [] : [{ ...latest(), history: latest().history.slice() }] };
      }
      if (name === 'get_task') return { task: { ...latest(), history: latest().history.slice() } };
      if (name === 'reply_task') { calls.replies.push(args); return {}; }
      if (name === 'send_message') return { id: 'outgoing-task' };
      throw new Error('unexpected tool ' + name);
    },
    async runJob(job) {
      if (job.kind === 'create') return { ok: true, taskId: 'cindy-task', runId: 'run1' };
      if (job.kind === 'status') {
        calls.status.push(job);
        const run = runs[job.runId];
        return run ? { ok: true, run: { taskId: 'cindy-task', runId: job.runId, ...run,
          outputMessageId: 'out-' + job.runId }, messages: [{ id: 'out-' + job.runId,
          role: 'assistant', text: 'output-' + job.runId }] } : { ok: true, run };
      }
      if (job.kind === 'prepare-continuation') {
        calls.prepare.push(job);
        return { ok: true, taskId: job.taskId, status: 'active', revision: 8 };
      }
      if (job.kind === 'send-continuation') {
        calls.send.push({ ...job });
        if (!receipts.has(job.requestKey)) receipts.set(job.requestKey,
          { ok: true, run: { taskId: job.taskId, runId: 'run' + (calls.send.length + 1), status: 'queued' } });
        if (failures.length) return failures.shift();
        return receipts.get(job.requestKey);
      }
      throw new Error('unexpected job ' + job.kind);
    },
  });
  worker.status.mainConnected = true;
  return {
    worker, calls, dir,
    get rec() { return Object.values(worker.state.tasks)[0]; },
    setRuns(next) { runs = { ...defaults, ...next }; },
    failNextSend(result) { failures.push(result); },
  };
}
function runningRec(messageId = 'req1', text = 'request 1') {
  return {
    phase: 'running', peer: PEER, requestMessageId: messageId, requestText: text,
    cindyTaskId: 'cindy-task', runId: 'run1', acceptedAt: new Date().toISOString(),
    lastCheck: 0, lastProgressAt: Date.now(), queue: [], runs: {}, completedRequests: [messageId],
  };
}
async function tick(worker) {
  await worker.tick();
  assert.equal(worker.status.busy !== true, true);
}
async function loadMain(tasks) {
  const context = vm.createContext({
    cindy: { tasks, node: { async request() { return { ok: true }; } }, onHostMessage() {} },
    console,
  });
  vm.runInContext(fs.readFileSync(path.join(ROOT, 'plugins/agent-network-cindy/main.js'), 'utf8'),
    context, { filename: 'main.js' });
  return context;
}

test('terminal delivery uncertainty observes WORKING without resending reply', async () => {
  const official = task('anet-task', [user('req1', 'request 1')]);
  const h = setup({ official, rec: { ...runningRec(), phase: 'delivery-unverified',
    pendingReply: { task_id: 'anet-task', state: 'completed', text: 'previously sent' } } });
  h.setRuns({ run1: { status: 'completed' } });
  await tick(h.worker);
  assert.equal(h.calls.replies.length, 0);
  assert.equal(h.rec.phase, 'delivery-unverified');
});

test('lost followup response after changed revision replays exact receipt parameters', async () => {
  const official = task('anet-task', [user('req1', 'request 1'), user('follow', 'follow')]);
  const rec = { ...runningRec(), pendingSend: { messageId: 'follow', text: 'follow', taskId: 'cindy-task',
    requestKey: 'stable-key', expectedRevision: 7 } };
  rec.queue = [{ messageId: 'follow', text: 'follow' }];
  const h = setup({ official: () => official, rec });
  h.setRuns({ run1: { status: 'completed' } });
  h.failNextSend({ ok: false, error: { code: 'MAIN_TIMEOUT', message: 'lost response' } });
  await tick(h.worker);
  assert.equal(h.calls.prepare.length, 0);
  assert.equal(h.calls.send.length, 1);
  assert.equal(h.rec.pendingSend.runId, undefined);
  assert.equal(h.rec.runId, 'run1');
  h.rec.lastCheck = 0;
  await tick(h.worker);
  assert.deepEqual(h.calls.send[1], h.calls.send[0]);
  assert.equal(h.calls.prepare.length, 0);
  assert.equal(h.calls.send.length, 2);
  assert.equal(h.rec.runId, 'run2');
  assert.deepEqual(h.rec.queue, []);
  assert.equal(h.rec.pendingSend.runId, 'run2');
});

test('restart observes an accepted continuation without another host send', async () => {
  const official = task('anet-task', [user('req1', 'request 1'), user('follow', 'follow')]);
  const rec = { ...runningRec(), requestMessageId: 'follow', requestText: 'follow', runId: 'run2',
    queue: [], pendingSend: { messageId: 'follow', text: 'follow', taskId: 'cindy-task',
      requestKey: 'stable-key', expectedRevision: 7, runId: 'run2' } };
  const h = setup({ official, rec });
  await tick(h.worker);
  assert.deepEqual(h.calls.send, []);
  assert.deepEqual(h.calls.prepare, []);
  assert.equal(h.rec.pendingSend.runId, 'run2');
});

test('accepted continuation receipt must match the settled request', async () => {
  const official = task('anet-task', [user('req1', 'request 1'), user('follow', 'follow')]);
  const rec = { ...runningRec(), requestMessageId: 'follow', requestText: 'follow', runId: 'run2', queue: [],
    pendingSend: { messageId: 'other', text: 'follow', taskId: 'cindy-task', requestKey: 'stable-key',
      expectedRevision: 7, runId: 'run2' } };
  const h = setup({ official, rec });
  h.setRuns({ run2: { status: 'completed' } });
  await tick(h.worker);
  assert.equal(h.rec.phase, 'unverified');
  assert.equal(h.rec.error.code, 'FOLLOWUP_STATE_INCONSISTENT');
  assert.equal(h.rec.pendingSend.messageId, 'other');
});

test('pending send must match the owned task and queued first message', async () => {
  const cases = [
    { ...runningRec(), pendingSend: { taskId: 'other', messageId: 'follow', text: 'follow',
      requestKey: 'stable-key', expectedRevision: 7 } },
    { ...runningRec(), pendingSend: { taskId: 'cindy-task', messageId: 'other', text: 'follow',
      requestKey: 'stable-key', expectedRevision: 7 } },
    { ...runningRec(), pendingSend: { taskId: 'cindy-task', messageId: 'follow', text: 'changed',
      requestKey: 'stable-key', expectedRevision: Number.NaN } },
  ];
  for (const rec of cases) {
    rec.queue = [{ messageId: 'follow', text: 'follow' }];
    const h = setup({ official: task('anet-task', [user('req1', 'request 1'), user('follow', 'follow')]), rec });
    h.setRuns({ run1: { status: 'completed' } });
    await tick(h.worker);
    assert.equal(h.rec.phase, 'unverified');
    assert.equal(h.rec.error.code, 'FOLLOWUP_STATE_INCONSISTENT');
    assert.deepEqual(h.calls.prepare, []);
    assert.deepEqual(h.calls.send, []);
  }
});

test('unknown pending send cannot finalize when its queued followup disappears', async () => {
  let history = [user('req1', 'request 1'), user('follow', 'follow')];
  const official = () => task('anet-task', history);
  const rec = { ...runningRec(), queue: [{ messageId: 'follow', text: 'follow' }],
    pendingSend: { messageId: 'follow', text: 'follow', taskId: 'cindy-task',
      requestKey: 'stable-key', expectedRevision: 8 } };
  const h = setup({ official, rec });
  h.setRuns({ run1: { status: 'completed' } });
  h.failNextSend({ ok: false, error: { code: 'FOLLOWUP_SEND_OUTCOME_UNKNOWN' } });
  await tick(h.worker);
  assert.equal(h.rec.phase, 'running');
  assert.equal(h.calls.send.length, 1);
  assert.ok(h.rec.pendingSend);

  history = [user('req1', 'request 1')];
  h.rec.lastCheck = 0;
  await tick(h.worker);
  assert.equal(h.rec.phase, 'unverified');
  assert.equal(h.rec.error.code, 'FOLLOWUP_QUEUE_HISTORY_CHANGED');
  assert.deepEqual(h.rec.queue, [{ messageId: 'follow', text: 'follow' }]);
  assert.deepEqual(h.rec.pendingSend, { messageId: 'follow', text: 'follow', taskId: 'cindy-task',
    requestKey: 'stable-key', expectedRevision: 8 });
  assert.equal(h.calls.send.length, 1);
  assert.equal(h.calls.replies.length, 0);
});

test('unresolved pending send blocks finalization even when queue is empty', async () => {
  const official = task('anet-task', [user('req1', 'request 1')]);
  const rec = { ...runningRec(), queue: [], pendingSend: { messageId: 'follow', text: 'follow',
    taskId: 'cindy-task', requestKey: 'stable-key', expectedRevision: 8 } };
  const h = setup({ official, rec });
  h.setRuns({ run1: { status: 'completed' } });
  await tick(h.worker);
  assert.equal(h.rec.phase, 'unverified');
  assert.equal(h.rec.error.code, 'FOLLOWUP_SEND_UNRESOLVED');
  assert.ok(h.rec.pendingSend);
  assert.deepEqual(h.calls.send, []);
  assert.equal(h.calls.replies.length, 0);
});

test('history refresh rejects truncation, reordering, and text edits', async () => {
  const cases = [
    {
      official: task('anet-task', [user('req1', 'request 1')]),
      queue: [{ messageId: 'f1', text: 'first' }, { messageId: 'f2', text: 'second' }],
    },
    {
      official: task('anet-task', [user('req1', 'request 1'), user('f2', 'second'), user('f1', 'first')]),
      queue: [{ messageId: 'f1', text: 'first' }, { messageId: 'f2', text: 'second' }],
    },
    {
      official: task('anet-task', [user('req1', 'request 1'), user('f1', 'edited')]),
      queue: [{ messageId: 'f1', text: 'first' }],
    },
  ];
  for (const { official, queue } of cases) {
    const rec = { ...runningRec(), queue: structuredClone(queue),
      pendingSend: { messageId: queue[0].messageId, text: queue[0].text, taskId: 'cindy-task',
        requestKey: 'stable-key', expectedRevision: 8 } };
    const h = setup({ official, rec });
    h.setRuns({ run1: { status: 'completed' } });
    await tick(h.worker);
    assert.equal(h.rec.phase, 'unverified');
    assert.equal(h.rec.error.code, 'FOLLOWUP_QUEUE_HISTORY_CHANGED');
    assert.deepEqual(h.rec.queue, queue);
    assert.deepEqual(h.calls.send, []);
    assert.equal(h.calls.replies.length, 0);
  }
});

test('append-only history growth remains safe after an unknown send', async () => {
  let history = [user('req1', 'request 1'), user('follow', 'follow')];
  const official = () => task('anet-task', history);
  const rec = { ...runningRec(), queue: [{ messageId: 'follow', text: 'follow' }],
    pendingSend: { messageId: 'follow', text: 'follow', taskId: 'cindy-task',
      requestKey: 'stable-key', expectedRevision: 8 } };
  const h = setup({ official, rec });
  h.setRuns({ run1: { status: 'completed' } });
  h.failNextSend({ ok: false, error: { code: 'FOLLOWUP_SEND_OUTCOME_UNKNOWN' } });
  await tick(h.worker);
  assert.equal(h.rec.phase, 'running');
  assert.equal(h.calls.send.length, 1);

  history.push(user('newer', 'new message'));
  h.rec.lastCheck = 0;
  await tick(h.worker);
  assert.equal(h.calls.prepare.length, 0);
  assert.equal(h.calls.send.length, 2);
  assert.equal(h.calls.send[1].requestKey, h.calls.send[0].requestKey);
  assert.equal(h.calls.send[1].expectedRevision, h.calls.send[0].expectedRevision);
  assert.equal(h.calls.send[1].text, 'follow');
  assert.deepEqual(h.rec.queue.map(item => item.messageId), ['newer']);
  assert.equal(h.rec.pendingSend.runId, 'run2');
});

test('peer metadata and task ownership are required before continuation', async () => {
  const cases = [
    [task('anet-task', [user('req1', 'request 1')], { metadata: {} }), 'PEER_OR_TASK_BINDING_CHANGED'],
    [task('anet-task', [user('req1', 'request 1')], { peer: 'aid-other' }), 'PEER_OR_TASK_BINDING_CHANGED'],
  ];
  for (const [official, code] of cases) {
    const h = setup({ official, rec: runningRec() });
    await tick(h.worker);
    assert.equal(h.rec.phase, 'unverified');
    assert.equal(h.rec.error.code, code);
    assert.deepEqual(h.calls.send, []);
  }
  const mismatched = task('other-task', [user('req1', 'request 1')]);
  const record = runningRec();
  const h = setup({ official: mismatched, rec: record });
  delete h.worker.state.tasks['other-task'];
  h.worker.state.tasks['anet-task'] = record;
  await tick(h.worker);
  assert.equal(h.rec.phase, 'unverified');
  assert.equal(h.rec.error.code, 'PEER_OR_TASK_BINDING_CHANGED');
});

test('unknown and mismatched host run bindings fail closed', async () => {
  const official = task('anet-task', [user('req1', 'request 1')]);
  const h = setup({ official, rec: runningRec() });
  h.setRuns({ run1: { status: 'mysterious' } });
  await tick(h.worker);
  assert.equal(h.rec.phase, 'unverified');
  assert.equal(h.rec.error.code, 'UNKNOWN_RUN_STATUS');
  assert.deepEqual(h.calls.send, []);

  const h2 = setup({ official, rec: runningRec() });
  h2.setRuns({ run1: { status: 'completed' } });
  h2.worker.setTestDependencies({
    async tool(name) {
      if (name === 'list_tasks') return { tasks: [] };
      if (name === 'get_task') return { task: official };
      if (name === 'reply_task') return {};
      throw new Error('unexpected tool');
    },
    async runJob() { return { ok: true, run: { taskId: 'other-host', runId: 'run1', status: 'completed' } }; },
  });
  await tick(h2.worker);
  assert.equal(h2.rec.error.code, 'HOST_RUN_BINDING_INVALID');
  assert.deepEqual(h2.calls.send, []);
});

test('host cancellation after completed prior outputs reports exact prior output once', async () => {
  let official = task('anet-task', [user('req1', 'request 1'), user('follow', 'follow')]);
  const h = setup({ official: () => official, rec: runningRec() });
  h.setRuns({ run1: { status: 'completed' }, run2: { status: 'completed' } });
  await tick(h.worker);
  assert.equal(h.rec.runId, 'run2');
  h.setRuns({ run2: { status: 'cancelled' } });
  await tick(h.worker);
  assert.equal(h.rec.phase, 'failed');
  assert.match(h.calls.replies.at(-1).text, /exact prior outputs follow/);
  assert.match(h.calls.replies.at(-1).text, /output-run1/);
  assert.equal(official.status.state, 'TASK_STATE_WORKING');
});

test('ordered followups remain on one owned host task with exact outputs', async () => {
  let official = task('anet-task', [user('req1', 'request 1'), user('f1', 'follow 1'), user('f2', 'follow 2')]);
  const h = setup({ official: () => official, rec: runningRec() });
  h.setRuns({ run1: { status: 'completed' }, run2: { status: 'completed' }, run3: { status: 'completed' } });
  await tick(h.worker);
  assert.deepEqual(h.calls.send.map(job => job.text), ['follow 1']);
  assert.equal(h.rec.cindyTaskId, 'cindy-task');
  official = task('anet-task', [user('req1', 'request 1'), user('f1', 'follow 1'), user('f2', 'follow 2')]);
  await tick(h.worker);
  assert.deepEqual(h.calls.send.map(job => job.text), ['follow 1', 'follow 2']);
  await tick(h.worker);
  assert.equal(h.rec.phase, 'replied');
  const reply = h.calls.replies.find(item => item.state === 'completed');
  assert.match(reply.text, /\[Cindy request 1: req1\]\noutput-run1/);
  assert.match(reply.text, /\[Cindy request 3: f2\]\noutput-run3/);
});

test('legacy frozen followup repairs only through exact owned history', async () => {
  const official = task('anet-task', [user('req1', 'request 1'), user('follow', 'new request')]);
  const rec = { ...runningRec(), phase: 'unverified', error: { code: 'FOLLOWUP_REQUIRES_OPERATOR' },
    runs: null, completedRequests: null, pendingSend: null };
  const h = setup({ official, rec });
  await tick(h.worker);
  assert.equal(h.rec.phase, 'running');
  assert.deepEqual(h.rec.queue.map(item => item.messageId), ['follow']);
  assert.deepEqual(h.calls.send, []);
});

test('main adapter rejects status and send receipt binding mismatches', async () => {
  const context = await loadMain({
    async get(args) { return { taskId: args.taskId, status: 'active', revision: 7, resolvedConfig: {} }; },
    async getRun(args) { return { taskId: 'other', runId: args.runId, status: 'completed' }; },
    async send(args) { return { taskId: 'other', runId: 'run2', status: 'queued' }; },
  });
  const status = await context.handleJob({ kind: 'status', taskId: 'owned', runId: 'run1' });
  assert.equal(status.ok, true);
  assert.equal(status.runError.code, 'TASK_BINDING_INVALID');
  assert.equal(status.messages.length, 0);
  const sent = await context.handleJob({ kind: 'send-continuation', taskId: 'owned', requestKey: 'key',
    expectedRevision: 7, text: 'follow' });
  assert.equal(sent.ok, false);
  assert.equal(sent.error.code, 'TASK_BINDING_INVALID');
});

test('main adapter passes exact continuation parameters and validates receipts', async () => {
  const sentParams = [];
  const context = await loadMain({
    async get() { return { taskId: 'cindy-task', status: 'active', revision: 7 }; },
    async send(args) {
      sentParams.push(args);
      return { taskId: 'cindy-task', runId: 'run2', status: 'queued' };
    },
  });
  const prepared = await context.handleJob({ kind: 'prepare-continuation', taskId: 'cindy-task' });
  assert.equal(prepared.revision, 7);
  const sent = await context.handleJob({ kind: 'send-continuation', taskId: 'cindy-task',
    requestKey: 'key', expectedRevision: 7, text: 'follow' });
  assert.equal(sentParams.length, 1);
  assert.equal(sentParams[0].taskId, 'cindy-task');
  assert.equal(sentParams[0].requestKey, 'key');
  assert.equal(sentParams[0].expectedRevision, 7);
  assert.equal(sentParams[0].text, 'follow');
  assert.equal(sent.run.taskId, 'cindy-task');
  assert.equal(sent.run.runId, 'run2');
});
