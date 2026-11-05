import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import vm from 'node:vm';
import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);
const ROOT = path.resolve(import.meta.dirname, '..');
const MAIN = path.join(ROOT, 'plugins/agent-network-cindy/main.js');
const WORKER = path.join(ROOT, 'plugins/agent-network-cindy/node/worker.cjs');
const PEER = 'aid-peer';
const ROUTE = { agentKind: 'codex', providerId: 'magpie-a822ecb8', model: 'zcode/GLM-5.3-Flash', effort: 'max', fastMode: false };

process.env.AGENT_NETWORK_CINDY_TEST = '1';

const envelope = (route = ROUTE, text = 'build it') =>
  'ANET_CINDY_ROUTE_V1 ' + JSON.stringify(route) + '\n' + text;
const official = (id, text) => ({
  id, metadata: { 'anet.peer_aid': PEER }, status: { state: 'TASK_STATE_WORKING' },
  history: [{ messageId: 'request-1', role: 'ROLE_USER', parts: [{ text }] }],
});

function persistedRecord(h) {
  const state = JSON.parse(fs.readFileSync(path.join(h.dir, 'state.json'), 'utf8'));
  return Object.values(state.tasks)[0];
}

async function loadMain(tasks) {
  const context = vm.createContext({
    cindy: { tasks, node: { async request() { return { ok: true }; } }, onHostMessage() {} },
    console,
  });
  vm.runInContext(fs.readFileSync(MAIN, 'utf8'), context, { filename: 'main.js' });
  return context;
}

function setupWorker({ source, officialTask, getTask, createResult, prepareResult, sendResult, runResult,
  statusResult, replyError } = {}) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'cindy-model-route-'));
  fs.writeFileSync(path.join(dir, 'config.json'), JSON.stringify({
    anetId: 'cindy', peerAid: PEER, allowPeers: [PEER], anetBin: '/unused/anet',
  }));
  fs.writeFileSync(path.join(dir, 'state.json'), JSON.stringify({ version: 1, outgoing: {}, tasks: {} }));
  process.env.AGENT_NETWORK_CINDY_DIR_TEST = dir;
  delete require.cache[WORKER];
  const worker = require(WORKER);
  const calls = { create: [], prepare: [], send: [], status: [], replies: [] };
  worker.setTestDependencies({
    async tool(name, args) {
      if (name === 'list_tasks') return { tasks: source ? [officialTask ?? official('anet-task', source)] : [] };
      if (name === 'get_task') {
        const task = typeof getTask === 'function' ? getTask(args.task_id) : getTask;
        return { task: task ?? officialTask ?? official('anet-task', source ?? 'request 1') };
      }
      if (name === 'reply_task') {
        calls.replies.push(args);
        if (replyError) throw replyError;
        return {};
      }
      throw new Error('unexpected tool ' + name);
    },
    async runJob(job) {
      if (job.kind === 'create') {
        calls.create.push({ ...job });
        return createResult ?? { ok: true, taskId: 'cindy-task', runId: 'run1' };
      }
      if (job.kind === 'prepare-continuation') {
        calls.prepare.push({ ...job });
        return prepareResult ?? { ok: true, taskId: 'cindy-task', status: 'active', revision: 8 };
      }
      if (job.kind === 'send-continuation') {
        calls.send.push({ ...job });
        return sendResult ?? { ok: true, run: { taskId: 'cindy-task', runId: 'run2', status: 'queued' } };
      }
      if (job.kind === 'status') {
        calls.status.push({ ...job });
        const result = typeof statusResult === 'function' ? statusResult(job, calls.status.length) : statusResult;
        if (result !== undefined || typeof statusResult === 'function') return result;
        return result ?? runResult ?? { ok: true, run: { taskId: 'cindy-task', runId: job.runId, status: 'working' } };
      }
      throw new Error('unexpected job ' + job.kind);
    },
  });
  worker.status.mainConnected = true;
  return { worker, calls, dir,
    get rec() { return Object.values(worker.state.tasks)[0]; } };
}

test('main sends an explicit route and rejects resolved mismatch before first send', async () => {
  const calls = [];
  const context = await loadMain({
    async create(args) { calls.push(args); return { taskId: 'host-task', revision: 4, resolvedConfig: { ...ROUTE, model: 'other-model' } }; },
    async send() { throw new Error('send must not run'); },
  });
  const result = await context.handleJob({ kind: 'create', requestKey: 'key', title: 'title',
    text: 'build it', route: ROUTE });
  assert.equal(calls.length, 1);
  assert.deepEqual(calls[0].route, ROUTE);
  assert.equal(result.ok, false);
  assert.equal(result.taskId, 'host-task');
  assert.equal(result.error.code, 'MODEL_ROUTE_MISMATCH');
  assert.equal(result.beforeSend, true);
});

test('main preserves explicit IDs when accepted route evidence is missing after send', async () => {
  const context = await loadMain({
    async create() { return { taskId: 'host-task', revision: 4, resolvedConfig: ROUTE }; },
    async send() { return { taskId: 'host-task', runId: 'host-run', status: 'queued' }; },
  });
  const result = await context.handleJob({ kind: 'create', requestKey: 'key', title: 'title',
    text: 'build it', route: ROUTE });
  assert.equal(result.ok, false);
  assert.equal(result.taskId, 'host-task');
  assert.equal(result.runId, 'host-run');
  assert.equal(result.error.code, 'MODEL_ROUTE_UNVERIFIED');
  assert.equal(result.afterSend, true);
});

test('main validates explicit route on continuation prepare and send receipt', async () => {
  let getReads = 0; let sendCalls = 0;
  const context = await loadMain({
    async get() { getReads++; return { taskId: 'host-task', status: 'active', revision: 8, resolvedConfig: ROUTE }; },
    async send() { sendCalls++; return { taskId: 'host-task', runId: 'run2', status: 'queued', acceptedConfig: { ...ROUTE, fastMode: true } }; },
  });
  const prepared = await context.handleJob({ kind: 'prepare-continuation', taskId: 'host-task', route: ROUTE });
  assert.equal(prepared.ok, true);
  assert.equal(prepared.revision, 8);
  const sent = await context.handleJob({ kind: 'send-continuation', taskId: 'host-task',
    requestKey: 'key', expectedRevision: 8, text: 'follow', route: ROUTE });
  assert.equal(getReads, 2);
  assert.equal(sendCalls, 1);
  assert.equal(sent.ok, false);
  assert.equal(sent.runId, 'run2');
  assert.equal(sent.afterSend, true);
  assert.equal(sent.error.code, 'MODEL_ROUTE_UNVERIFIED');
});

test('main sends current runtime values to the host API and accepts only matching evidence', async () => {
  const requested = { ...ROUTE, model: 'zcode/GLM-5.3-Flash', effort: 'xhigh', fastMode: 'ultrafast' };
  const calls = [];
  const context = await loadMain({
    async create(args) {
      calls.push(args);
      return { taskId: 'host-task', revision: 5, resolvedConfig: requested };
    },
    async send(args) {
      calls.push(args);
      return { taskId: 'host-task', runId: 'host-run', status: 'queued', acceptedConfig: requested };
    },
  });
  const result = await context.handleJob({ kind: 'create', requestKey: 'key', title: 'title',
    text: 'build it', route: requested });
  assert.equal(result.ok, true);
  assert.equal(calls.length, 2);
  assert.deepEqual(calls[0].route, requested);
  assert.equal('route' in calls[1], false);
  assert.equal(calls[1].taskId, 'host-task');
});

test('a fresh persisted continuation send is distinct from its exact replay', async () => {
  const officialTask = { ...official('anet-task', envelope()), history: [
    { messageId: 'request-1', role: 'ROLE_USER', parts: [{ text: envelope() }] },
    { messageId: 'follow', role: 'ROLE_USER', parts: [{ text: 'follow' }] },
  ] };
  const h = setupWorker({
    source: envelope(), officialTask,
    sendResult: { ok: false, error: { code: 'SEND_TIMEOUT' } },
    runResult: { ok: true, run: { taskId: 'cindy-task', runId: 'run1', status: 'completed', outputMessageId: 'out-run1' },
      messages: [{ id: 'out-run1', role: 'assistant', text: 'first' }] },
  });
  h.worker.state.tasks['anet-task'] = {
    phase: 'running', peer: PEER, requestMessageId: 'request-1', requestText: envelope(),
    officialTaskId: 'anet-task', cindyTaskId: 'cindy-task', runId: 'run1', route: ROUTE,
    acceptedAt: new Date().toISOString(), lastCheck: 0, lastProgressAt: Date.now(),
    queue: [{ messageId: 'follow', text: 'follow' }], runs: {}, completedRequests: ['request-1'],
  };
  await h.worker.tick();
  const firstSend = h.calls.send[0];
  assert.equal(h.calls.send.length, 1);
  assert.equal(h.calls.send[0].replay, false);
  h.rec.lastCheck = 0;
  await h.worker.tick();
  assert.equal(h.calls.send.length, 2);
  assert.equal(h.calls.send[1].replay, true);
  assert.deepEqual(h.calls.send[1], { ...firstSend, replay: true });
});

test('main validates every current host effort value and ultrafast without aliases', () => {
  const worker = setupWorker({ source: 'ignored' }).worker;
  for (const effort of ['minimal', 'low', 'medium', 'high', 'xhigh', 'max', 'ultra']) {
    assert.equal(worker.parseRouteEnvelope(envelope({ ...ROUTE, effort })).error, undefined);
  }
  for (const fastMode of [true, false, 'ultrafast']) {
    assert.equal(worker.parseRouteEnvelope(envelope({ ...ROUTE, fastMode })).error, undefined);
  }
  for (const route of [{ ...ROUTE, effort: 'maximum' }, { ...ROUTE, fastMode: 'fast' }]) {
    assert.equal(worker.parseRouteEnvelope(envelope(route)).error, 'MODEL_ROUTE_INVALID');
  }
});

test('reserved route prefix with unknown version or malformed separator rejects before create', () => {
  const h = setupWorker({ source: 'ignored' });
  const cases = [
    'ANET_CINDY_ROUTE_V2 ' + JSON.stringify(ROUTE) + '\nrequest',
    'ANET_CINDY_ROUTE_V999 ' + JSON.stringify(ROUTE) + '\nrequest',
    'ANET_CINDY_ROUTE_V1{' + JSON.stringify(ROUTE).slice(1) + '\nrequest',
    'ANET_CINDY_ROUTE_V1  ' + JSON.stringify(ROUTE) + '\nrequest',
  ];
  for (const source of cases) assert.equal(h.worker.parseRouteEnvelope(source).error, 'MODEL_ROUTE_INVALID');
});

test('host dedup recovers an exact replay accepted before a separate config revision change', async () => {
  const taskState = { taskId: 'host-task', status: 'active', revision: 8, resolvedConfig: ROUTE };
  let currentConfig = ROUTE;
  const sent = [];
  const executed = new Set();
  const context = await loadMain({
    async get() { return { ...taskState, resolvedConfig: currentConfig }; },
    async send(params) {
      sent.push(params);
      if (!executed.has(params.requestKey)) {
        executed.add(params.requestKey);
        taskState.revision = 9;
        currentConfig = { ...ROUTE, model: 'changed-model' };
        throw new Error('accepted reply lost');
      }
      return { taskId: 'host-task', runId: 'original-run', status: 'queued', acceptedConfig: ROUTE };
    },
  });
  const job = { kind: 'send-continuation', taskId: 'host-task', requestKey: 'stable-key',
    expectedRevision: 8, text: 'exact follow', route: ROUTE };
  const lost = await context.handleJob({ ...job, replay: false });
  assert.equal(lost.ok, false);
  assert.equal(sent.length, 1);
  const recovered = await context.handleJob({ ...job, replay: true });
  assert.equal(recovered.ok, true);
  assert.equal(sent.length, 2);
  assert.deepEqual(sent[1], sent[0]);
  assert.equal(recovered.run.runId, 'original-run');
  assert.equal(executed.size, 1);
});

test('legacy routeless pending send accepts missing sourceText and route', async () => {
  const officialTask = { ...official('anet-task', 'request 1'), history: [
    { messageId: 'request-1', role: 'ROLE_USER', parts: [{ text: 'request 1' }] },
    { messageId: 'follow', role: 'ROLE_USER', parts: [{ text: 'follow' }] },
  ] };
  const h = setupWorker({
    source: 'request 1', officialTask,
    runResult: { ok: true, run: { taskId: 'cindy-task', runId: 'run1', status: 'completed', outputMessageId: 'out-run1' },
      messages: [{ id: 'out-run1', role: 'assistant', text: 'first' }] },
  });
  h.worker.state.tasks['anet-task'] = {
    phase: 'running', peer: PEER, requestMessageId: 'request-1', requestText: 'request 1',
    officialTaskId: 'anet-task', cindyTaskId: 'cindy-task', runId: 'run1',
    acceptedAt: new Date().toISOString(), lastCheck: 0, lastProgressAt: Date.now(),
    queue: [{ messageId: 'follow', text: 'follow' }],
    pendingSend: { messageId: 'follow', text: 'follow', taskId: 'cindy-task',
      requestKey: 'stable-key', expectedRevision: 8 },
    runs: {}, completedRequests: ['request-1'],
  };
  await h.worker.tick();
  assert.equal(h.calls.prepare.length, 0);
  assert.equal(h.calls.send.length, 1);
  assert.equal(h.calls.send[0].replay, true);
  assert.equal('route' in h.calls.send[0], false);
});

test('valid route envelope passes through once, strips task text, and persists route', async () => {
  const h = setupWorker({ source: envelope(), createResult: {
    ok: true, taskId: 'cindy-task', runId: 'run1',
  } });
  await h.worker.tick();
  assert.deepEqual(h.calls.create[0].route, ROUTE);
  assert.equal(h.calls.create[0].text, 'build it');
  assert.deepEqual(h.rec.route, ROUTE);
  const persisted = JSON.parse(fs.readFileSync(path.join(h.dir, 'state.json'), 'utf8')).tasks['anet-task'];
  assert.equal(persisted.phase, 'running');
  assert.deepEqual(persisted.route, ROUTE);
  assert.equal(persisted.requestText, envelope());
  const configBefore = fs.readFileSync(path.join(h.dir, 'config.json'), 'utf8');
  await h.worker.tick();
  assert.equal(fs.readFileSync(path.join(h.dir, 'config.json'), 'utf8'), configBefore);
});

test('no-envelope requests keep the legacy default path', async () => {
  const h = setupWorker({ source: 'plain request', createResult: {
    ok: true, taskId: 'cindy-task', runId: 'run1',
  } });
  await h.worker.tick();
  assert.equal(h.calls.create.length, 1);
  assert.equal(h.calls.create[0].text, 'plain request');
  assert.equal('route' in h.calls.create[0], false);
  assert.equal('route' in h.rec, false);
});

test('malformed, partial, unknown, and changed route envelopes fail closed', () => {
  const worker = setupWorker({ source: 'ignored' }).worker;
  const cases = [
    'ANET_CINDY_ROUTE_V1 not-json\nrequest',
    'ANET_CINDY_ROUTE_V1 {"agentKind":"codex"}\nrequest',
    envelope({ ...ROUTE, provider: 'guessed' }),
    envelope({ ...ROUTE, agentKind: 'claude' }),
    envelope({ ...ROUTE, model: '' }),
    envelope({ ...ROUTE, fastMode: 'false' }),
  ];
  for (const source of cases) {
    const parsed = worker.parseRouteEnvelope(source);
    assert.equal(parsed.error, 'MODEL_ROUTE_INVALID');
  }
});

test('main rejects a malformed status route without accepting run evidence', async () => {
  const context = await loadMain({
    async getRun() { return { taskId: 'host-task', runId: 'run1', status: 'queued', acceptedConfig: ROUTE }; },
  });
  const status = await context.handleJob({ kind: 'status', taskId: 'host-task', runId: 'run1',
    route: { ...ROUTE, extra: true } });
  assert.equal(status.ok, false);
  assert.equal(status.error.code, 'MODEL_ROUTE_INVALID');
});

test('malformed initial envelope sends one failed reply without creating a host task', async () => {
  const h = setupWorker({ source: 'ANET_CINDY_ROUTE_V1 {"agentKind":"codex"}\nrequest' });
  await h.worker.tick();
  assert.deepEqual(h.calls.create, []);
  assert.equal(h.calls.replies.length, 1);
  assert.equal(h.calls.replies[0].state, 'failed');
  assert.match(h.calls.replies[0].text, /MODEL_ROUTE_INVALID/);
  assert.equal(h.rec.phase, 'failed');
  assert.ok(h.rec.pendingReply);
});

test('continuation route drift is rejected before send with one failed reply', async () => {
  const officialTask = { ...official('anet-task', envelope()), history: [
    { messageId: 'request-1', role: 'ROLE_USER', parts: [{ text: envelope() }] },
    { messageId: 'follow', role: 'ROLE_USER', parts: [{ text: 'plain follow' }] },
  ] };
  const h = setupWorker({
    source: envelope(),
    officialTask,
    prepareResult: { ok: false, taskId: 'cindy-task', error: { code: 'MODEL_ROUTE_MISMATCH' }, beforeSend: true },
    runResult: { ok: true, run: { taskId: 'cindy-task', runId: 'run1', status: 'completed', outputMessageId: 'out-run1' },
      messages: [{ id: 'out-run1', role: 'assistant', text: 'first' }] },
  });
  h.worker.state.tasks['anet-task'] = {
    phase: 'running', peer: PEER, requestMessageId: 'request-1', requestText: envelope(),
    officialTaskId: 'anet-task', cindyTaskId: 'cindy-task', runId: 'run1', route: ROUTE,
    acceptedAt: new Date().toISOString(), lastCheck: 0, lastProgressAt: Date.now(),
    queue: [{ messageId: 'follow', text: 'plain follow' }], runs: {}, completedRequests: ['request-1'],
  };
  await h.worker.tick();
  assert.equal(h.calls.prepare.length, 1);
  assert.deepEqual(h.calls.prepare[0].route, ROUTE);
  assert.deepEqual(h.calls.send, []);
  assert.equal(h.calls.replies.length, 1);
  assert.equal(h.calls.replies[0].state, 'failed');
  assert.equal(h.rec.phase, 'failed');
});

test('same saved route continuation accepts a repeated envelope and strips it', async () => {
  const source = envelope(ROUTE, 'same model follow');
  const officialTask = { ...official('anet-task', envelope()), history: [
    { messageId: 'request-1', role: 'ROLE_USER', parts: [{ text: envelope() }] },
    { messageId: 'follow', role: 'ROLE_USER', parts: [{ text: source }] },
  ] };
  const h = setupWorker({ source: envelope(), officialTask, runResult: {
    ok: true, run: { taskId: 'cindy-task', runId: 'run1', status: 'completed', outputMessageId: 'out-run1' },
    messages: [{ id: 'out-run1', role: 'assistant', text: 'first' }] } });
  h.worker.state.tasks['anet-task'] = {
    phase: 'running', peer: PEER, requestMessageId: 'request-1', requestText: envelope(),
    officialTaskId: 'anet-task', cindyTaskId: 'cindy-task', runId: 'run1', route: ROUTE,
    acceptedAt: new Date().toISOString(), lastCheck: 0, lastProgressAt: Date.now(),
    queue: [{ messageId: 'follow', text: source }],
    runs: { 'request-1:run1': { messageId: 'request-1', runId: 'run1', text: 'first' } },
    completedRequests: ['request-1'],
  };
  await h.worker.tick();
  assert.equal(h.calls.prepare.length, 1);
  assert.deepEqual(h.calls.prepare[0].route, ROUTE);
  assert.equal(h.calls.send.length, 1);
  assert.deepEqual(h.calls.send[0].route, ROUTE);
  assert.equal(h.calls.send[0].text, 'same model follow');
  assert.equal(h.rec.requestText, source);
  assert.equal(h.rec.runId, 'run2');
  assert.equal(h.rec.pendingSend.runId, 'run2');
});

test('restart retains saved route and validates an accepted continuation without resending', async () => {
  const h = setupWorker({ source: envelope() });
  h.worker.state.tasks['anet-task'] = {
    phase: 'running', peer: PEER, requestMessageId: 'follow', requestText: envelope(ROUTE, 'same model follow'),
    officialTaskId: 'anet-task', cindyTaskId: 'cindy-task', runId: 'run2', route: ROUTE,
    acceptedAt: new Date().toISOString(), lastCheck: 0, lastProgressAt: Date.now(), queue: [],
    pendingSend: { messageId: 'follow', text: 'same model follow', sourceText: envelope(ROUTE, 'same model follow'),
      route: ROUTE, taskId: 'cindy-task', requestKey: 'stable-key', expectedRevision: 8, runId: 'run2' },
    runs: {}, completedRequests: ['request-1'],
  };
  await h.worker.tick();
  assert.deepEqual(h.calls.prepare, []);
  assert.deepEqual(h.calls.send, []);
  assert.deepEqual(h.rec.pendingSend.route, ROUTE);
  assert.deepEqual(h.rec.route, ROUTE);
  assert.equal(h.rec.pendingSend.runId, 'run2');
});

test('explicit continuation accepted-route mismatch retains run ID and never resends', async () => {
  const officialTask = { ...official('anet-task', envelope()), history: [
    { messageId: 'request-1', role: 'ROLE_USER', parts: [{ text: envelope() }] },
    { messageId: 'follow', role: 'ROLE_USER', parts: [{ text: 'plain follow' }] },
  ] };
  const h = setupWorker({
    source: envelope(),
    officialTask,
    sendResult: { ok: false, runId: 'uncertain-run', error: { code: 'MODEL_ROUTE_UNVERIFIED' }, afterSend: true },
    runResult: { ok: true, run: { taskId: 'cindy-task', runId: 'run1', status: 'completed', outputMessageId: 'out-run1' },
      messages: [{ id: 'out-run1', role: 'assistant', text: 'first' }] },
  });
  h.worker.state.tasks['anet-task'] = {
    phase: 'running', peer: PEER, requestMessageId: 'request-1', requestText: envelope(),
    officialTaskId: 'anet-task', cindyTaskId: 'cindy-task', runId: 'run1', route: ROUTE,
    acceptedAt: new Date().toISOString(), lastCheck: 0, lastProgressAt: Date.now(),
    queue: [{ messageId: 'follow', text: 'plain follow' }], runs: {}, completedRequests: ['request-1'],
  };
  await h.worker.tick();
  assert.equal(h.calls.send.length, 1);
  assert.equal(h.calls.replies.length, 1);
  assert.equal(h.calls.replies[0].state, 'working');
  assert.match(h.calls.replies[0].text, /not confirmed/);
  assert.equal(h.rec.uncertaintyNoticeAttempt.outcome, 'unknown');
  assert.equal(h.rec.uncertaintyNotice.outcome, 'delivered');
  assert.deepEqual(persistedRecord(h).uncertaintyNotice, h.rec.uncertaintyNotice);
  assert.equal(h.rec.runId, 'run1');
  assert.equal(h.rec.pendingSend.runId, 'uncertain-run');
  assert.equal(h.rec.phase, 'unverified');
  assert.equal(h.rec.error.code, 'MODEL_ROUTE_UNVERIFIED');
  h.rec.lastCheck = 0;
  await h.worker.tick();
  assert.equal(h.calls.send.length, 1);
});

test('status route mismatch leaves running and reports visible uncertainty once', async () => {
  const h = setupWorker({
    source: envelope(),
    runResult: { ok: false, runError: { code: 'MODEL_ROUTE_MISMATCH' } },
  });
  h.worker.state.tasks['anet-task'] = {
    phase: 'running', peer: PEER, requestMessageId: 'request-1', requestText: envelope(),
    officialTaskId: 'anet-task', cindyTaskId: 'cindy-task', runId: 'run1', route: ROUTE,
    acceptedAt: new Date().toISOString(), lastCheck: 0, lastProgressAt: Date.now(), queue: [],
  };
  await h.worker.tick();
  assert.equal(h.calls.status.length, 1);
  assert.equal(h.calls.replies.length, 1);
  assert.equal(h.calls.replies[0].state, 'working');
  assert.match(h.calls.replies[0].text, /execution and result are not confirmed/);
  assert.equal(h.rec.phase, 'unverified');
  assert.equal(h.rec.error.code, 'MODEL_ROUTE_MISMATCH');
  assert.equal(h.rec.cindyTaskId, 'cindy-task');
  assert.equal(h.rec.runId, 'run1');
  assert.equal(h.rec.uncertaintyNoticeAttempt.outcome, 'unknown');
  assert.equal(h.rec.uncertaintyNotice.outcome, 'delivered');
  h.worker.status.mainConnected = true;
  await h.worker.tick();
  assert.equal(h.calls.status.length, 1);
  assert.equal(h.calls.replies.length, 1);
});

test('null and undefined host status skip safely and later tasks still run', async () => {
  const h = setupWorker({
    getTask: aid => official(aid, envelope()),
    getTask: aid => official(aid, envelope()),
    statusResult: (job, callNumber) => callNumber === 1 ? null : {
      ok: false, runError: { code: 'MODEL_ROUTE_MISMATCH' },
    },
  });
  const base = {
    peer: PEER, requestMessageId: 'request-1', requestText: envelope(), route: ROUTE,
    cindyTaskId: 'cindy-task', runId: 'run1', acceptedAt: new Date().toISOString(),
    lastCheck: 0, lastProgressAt: Date.now(), queue: [],
  };
  h.worker.state.tasks['task-a'] = { ...base, phase: 'running', officialTaskId: 'task-a' };
  h.worker.state.tasks['task-b'] = { ...base, phase: 'running', officialTaskId: 'task-b' };
  assert.equal(h.worker.status.mainConnected, true);
  assert.equal(Object.keys(h.worker.state.tasks).length, 2);
  await h.worker.tick();
  assert.equal(h.worker.status.lastError, null);
  assert.equal(h.worker.state.tasks['task-a'].phase, 'running');
  assert.equal(h.worker.state.tasks['task-b'].phase, 'unverified');
  assert.equal(h.worker.state.tasks['task-b'].error.code, 'MODEL_ROUTE_MISMATCH');
  assert.equal(h.calls.replies.length, 1);
  h.worker.status.mainConnected = true;
  await h.worker.tick();
  assert.equal(h.calls.replies.length, 1);
});

test('uncertain reply delivery stays unknown without notification retry', async () => {
  const h = setupWorker({
    source: envelope(),
    getTask: aid => ({ ...official(aid, envelope()), history: [
      { messageId: 'request-1', role: 'ROLE_USER', parts: [{ text: envelope() }] },
      { messageId: 'follow', role: 'ROLE_USER', parts: [{ text: 'plain follow' }] },
    ] }),
    sendResult: { ok: false, runId: 'uncertain-run', error: { code: 'MODEL_ROUTE_UNVERIFIED' }, afterSend: true },
    runResult: { ok: true, run: { taskId: 'cindy-task', runId: 'run1', status: 'completed', outputMessageId: 'out-run1' },
      messages: [{ id: 'out-run1', role: 'assistant', text: 'first' }] },
    replyError: new Error('reply outcome unknown'),
  });
  h.worker.state.tasks['anet-task'] = {
    phase: 'running', peer: PEER, requestMessageId: 'request-1', requestText: envelope(),
    officialTaskId: 'anet-task', cindyTaskId: 'cindy-task', runId: 'run1', route: ROUTE,
    acceptedAt: new Date().toISOString(), lastCheck: 0, lastProgressAt: Date.now(),
    queue: [{ messageId: 'follow', text: 'plain follow' }], runs: {}, completedRequests: ['request-1'],
  };
  assert.equal(h.worker.status.mainConnected, true);
  assert.equal(Object.keys(h.worker.state.tasks).length, 1);
  await h.worker.tick();
  assert.equal(h.calls.send.length, 1);
  assert.equal(h.calls.replies.length, 1);
  assert.equal(h.calls.replies.length, 1);
  assert.equal(h.worker.status.lastError, 'reply outcome unknown');
  assert.equal(h.rec.uncertaintyNoticeAttempt.outcome, 'unknown');
  assert.equal(h.rec.uncertaintyNotice, undefined);
  assert.equal(h.rec.phase, 'unverified');
  assert.equal(h.rec.pendingSend.runId, 'uncertain-run');
  h.worker.status.mainConnected = true;
  await h.worker.tick();
  assert.equal(h.calls.send.length, 1);
  assert.equal(h.calls.replies.length, 1);
});

test('fresh route unavailable create rejection replies deterministically before host send', async () => {
  const h = setupWorker({ source: envelope(), createResult: {
    ok: false, taskId: 'host-task', error: { code: 'MODEL_ROUTE_UNAVAILABLE' }, beforeSend: true,
  } });
  await h.worker.tick();
  assert.equal(h.calls.create.length, 1);
  assert.equal(h.rec.cindyTaskId, 'host-task');
  assert.equal(h.calls.replies.length, 1);
  assert.equal(h.calls.replies[0].state, 'failed');
  assert.match(h.calls.replies[0].text, /Rejected before sending this request/);
  assert.equal(h.rec.phase, 'failed');
});

test('initial accepted-route mismatch after send reports uncertainty once', async () => {
  const h = setupWorker({ source: envelope(), createResult: {
    ok: false, taskId: 'host-task', runId: 'host-run',
    error: { code: 'MODEL_ROUTE_UNVERIFIED' }, afterSend: true,
  } });
  await h.worker.tick();
  assert.equal(h.calls.create.length, 1);
  assert.equal(h.calls.replies.length, 1);
  assert.equal(h.calls.replies[0].state, 'working');
  assert.match(h.calls.replies[0].text, /execution and result are not confirmed/);
  assert.equal(h.rec.cindyTaskId, 'host-task');
  assert.equal(h.rec.runId, 'host-run');
  assert.equal(h.rec.phase, 'unverified');
  assert.equal(h.rec.error.code, 'MODEL_ROUTE_UNVERIFIED');
  await h.worker.tick();
  assert.equal(h.calls.create.length, 1);
  assert.equal(h.calls.replies.length, 1);
});
