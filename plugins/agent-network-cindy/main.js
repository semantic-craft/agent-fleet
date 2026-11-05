// agent-network-cindy (Cindy × Agent Network): browser-sandbox brain (pattern follows official cindy-ping:
// resident node worker -> node-notification -> main.js -> cindy.tasks.* -> cindy.node.request back).
async function node(method, params, timeoutMs) {
  try {
    const r = await cindy.node.request({ method, params: params || {}, timeoutMs: timeoutMs || 30000 });
    return r || { ok: false, message: 'empty response' };
  } catch (e) { return { ok: false, message: String((e && e.message) || e) }; }
}
function errInfo(e) {
  return { code: (e && e.code) ? String(e.code) : 'INTERNAL', message: String((e && e.message) || e).slice(0, 500) };
}
function bindingError() {
  const error = new Error('TASK_BINDING_INVALID');
  error.code = 'TASK_BINDING_INVALID';
  return error;
}
const ROUTE_FIELDS = ['agentKind', 'providerId', 'model', 'effort', 'fastMode'];
const AGENT_KINDS = new Set(['cc', 'codex', 'pi']);
const EFFORTS = new Set(['minimal', 'low', 'medium', 'high', 'xhigh', 'max', 'ultra']);
function routeError(code, message, beforeSend) {
  const error = new Error(message);
  error.code = code;
  error.beforeSend = beforeSend === true;
  return error;
}
function validateRoute(route) {
  if (!route || typeof route !== 'object' || Array.isArray(route)) throw routeError('MODEL_ROUTE_INVALID', 'explicit route must be an object', true);
  const keys = Object.keys(route);
  if (keys.length !== ROUTE_FIELDS.length || ROUTE_FIELDS.some(key => !keys.includes(key))) {
    throw routeError('MODEL_ROUTE_INVALID', 'explicit route must contain exactly agentKind, providerId, model, effort, fastMode', true);
  }
  if (!AGENT_KINDS.has(route.agentKind) || !EFFORTS.has(route.effort) ||
      typeof route.providerId !== 'string' || !route.providerId.trim() ||
      typeof route.model !== 'string' || !route.model.trim() ||
      (route.fastMode !== true && route.fastMode !== false && route.fastMode !== 'ultrafast')) {
    throw routeError('MODEL_ROUTE_INVALID', 'explicit route contains an unsupported value', true);
  }
  return route;
}
function routeMatches(route, config) {
  return !!config && typeof config === 'object' && !Array.isArray(config) &&
    ROUTE_FIELDS.every(key => config[key] === route[key]);
}
function requireRoute(route, config, phase) {
  if (!routeMatches(route, config)) {
    throw routeError('MODEL_ROUTE_MISMATCH', 'explicit Cindy route did not match ' + phase, phase === 'task resolvedConfig');
  }
}
function partText(c) {
  if (typeof c === 'string') return c;
  if (Array.isArray(c)) return c.map(partText).filter(Boolean).join('\n');
  if (c && typeof c === 'object') {
    if (typeof c.text === 'string') return c.text;
    if (c.content !== undefined) return partText(c.content);
  }
  return '';
}
async function createJob(job) {
  if (typeof job.text !== 'string' || job.text.length > 32768) return { ok: false, error: { code: 'INPUT_TOO_LONG', message: 'Input exceeds host adapter limit; nothing created.' } };
  const route = job.route === undefined ? undefined : validateRoute(job.route);
  const task = await cindy.tasks.create({
    requestKey: job.requestKey + ':create', title: job.title,
    ...(route ? { route } : {})
  });
  if (!task || typeof task.taskId !== 'string' || !task.taskId || !Number.isSafeInteger(task.revision)) {
    return { ok: false, taskId: task && task.taskId || null, requestKey: job.requestKey, error: errInfo(bindingError()) };
  }
  if (route) {
    try { requireRoute(route, task.resolvedConfig, 'task resolvedConfig'); }
    catch (e) { return { ok: false, taskId: task.taskId, requestKey: job.requestKey, error: errInfo(e), beforeSend: e.beforeSend === true }; }
  }
  let run;
  try { run = await cindy.tasks.send({ taskId: task.taskId, expectedRevision: task.revision,
    requestKey: job.requestKey + ':send', text: job.text }); }
  catch (e) { return { ok: false, taskId: task.taskId, requestKey: job.requestKey, error: errInfo(e) }; }
  if (!run || run.taskId !== task.taskId || typeof run.runId !== 'string' || !run.runId) {
    return { ok: false, taskId: task.taskId, requestKey: job.requestKey, error: errInfo(bindingError()) };
  }
  if (route && !routeMatches(route, run.acceptedConfig)) {
    return { ok: false, taskId: task.taskId, runId: run.runId, requestKey: job.requestKey,
      error: errInfo(routeError('MODEL_ROUTE_UNVERIFIED', 'explicit Cindy route acceptance is missing or mismatched', false)), afterSend: true };
  }
  return { ok: true, taskId: task.taskId, runId: run.runId, runStatus: run.status || null };
}
async function statusJob(job) {
  const out = { ok: true, taskId: job.taskId, runId: job.runId || null };
  const route = job.route === undefined ? undefined : validateRoute(job.route);
  if (job.runId) {
    try {
      const run = await cindy.tasks.getRun({ runId: job.runId });
      if (!run || run.taskId !== job.taskId || run.runId !== job.runId) throw bindingError();
      if (route && !routeMatches(route, run.acceptedConfig)) {
        throw routeError('MODEL_ROUTE_MISMATCH', 'explicit Cindy route status evidence is missing or mismatched');
      }
      out.run = run;
    } catch (e) { out.runError = errInfo(e); }
  }
  out.messages = [];
  const outputId = out.run && out.run.outputMessageId;
  if (out.run && out.run.status === 'completed' && typeof outputId === 'string' && outputId) {
    try {
      let after;
      const cursors = new Set();
      for (let pageNumber = 0; pageNumber < 32; pageNumber++) {
        const page = await cindy.tasks.readMessages({ taskId: job.taskId, limit: 100, ...(after ? { after } : {}) });
        const msgs = (page && (page.messages || page.items)) || [];
        const exact = msgs.find(m => (m.id || m.messageId) === outputId && m.role === 'assistant');
        if (exact) {
          out.messages = [{ id: outputId, role: 'assistant', text: partText(exact.text !== undefined ? exact.text : exact.content) }];
          break;
        }
        const next = page && page.nextCursor;
        if (!next) break;
        if (typeof next !== 'string' || cursors.has(next)) throw new Error('Invalid or repeated message cursor');
        cursors.add(next);
        after = next;
        if (pageNumber === 31) throw new Error('Message pagination limit reached');
      }
    } catch (e) { out.messagesError = errInfo(e); }
  }
  return out;
}
async function prepareContinuation(job) {
  const task = await cindy.tasks.get({ taskId: job.taskId });
  if (!task || task.taskId !== job.taskId || task.status !== 'active' || !Number.isSafeInteger(task.revision)) {
    const error = new Error('TASK_BINDING_INVALID');
    error.code = 'TASK_BINDING_INVALID';
    throw error;
  }
  const route = job.route === undefined ? undefined : validateRoute(job.route);
  if (route) {
    try { requireRoute(route, task.resolvedConfig, 'task resolvedConfig'); }
    catch (e) { throw e; }
  }
  return { ok: true, taskId: task.taskId, status: task.status, revision: task.revision,
    ...(route ? { route } : {}) };
}
async function sendContinuation(job) {
  if (!job || typeof job.taskId !== 'string' || !job.taskId ||
      typeof job.requestKey !== 'string' || !job.requestKey ||
      !Number.isSafeInteger(job.expectedRevision) || typeof job.text !== 'string') throw bindingError();
  const route = job.route === undefined ? undefined : validateRoute(job.route);
  if (!job.replay) {
    const task = await cindy.tasks.get({ taskId: job.taskId });
    if (!task || task.taskId !== job.taskId || task.status !== 'active' || task.revision !== job.expectedRevision) {
      throw routeError('TASK_BINDING_INVALID', 'continuation task binding changed', true);
    }
    if (route) requireRoute(route, task.resolvedConfig, 'task resolvedConfig');
  }
  const run = await cindy.tasks.send({ taskId: job.taskId, requestKey: job.requestKey,
    expectedRevision: job.expectedRevision, text: job.text });
  if (!run || run.taskId !== job.taskId || typeof run.runId !== 'string' || !run.runId) throw bindingError();
  if (route && !routeMatches(route, run.acceptedConfig)) {
    const error = routeError('MODEL_ROUTE_UNVERIFIED', 'explicit Cindy route continuation acceptance is missing or mismatched', false);
    error.afterSend = true;
    error.runId = run.runId;
    throw error;
  }
  return { ok: true, run: { taskId: run && run.taskId, runId: run && run.runId, status: run && run.status } };
}
async function handleJob(job) {
  try {
    if (job.kind === 'create') return await createJob(job);
    if (job.kind === 'status') return await statusJob(job);
    if (job.kind === 'prepare-continuation') return await prepareContinuation(job);
    if (job.kind === 'send-continuation') return await sendContinuation(job);
    return { ok: false, error: { code: 'UNKNOWN_JOB', message: 'unknown job kind' } };
  } catch (e) {
    return { ok: false, ...(e && e.runId ? { runId: e.runId } : {}),
      error: errInfo(e), beforeSend: e && e.beforeSend === true, afterSend: e && e.afterSend === true };
  }
}
async function toolCall(msg) {
  let r;
  if (msg.tool === 'send_to_peer') {
    const text = String((msg.args && msg.args.text) || '').trim();
    if (!text || text.length > 8000) {
      await cindy.send({ type: 'tool-result', callId: msg.callId, ok: false, errorCode: 'INVALID_TEXT', message: 'text must be 1-8000 characters' });
      return;
    }
    r = await node('anet/send_to_peer', { text, callId: msg.callId }, 60000);
  } else if (msg.tool === 'reconcile_result') {
    r = await node('anet/reconcile_result', { taskId: msg.args && msg.args.taskId }, 60000);
  } else if (msg.tool === 'bridge_status') {
    r = await node('bridge/status', {}, 15000);
  } else return;
  if (!r.ok) {
    await cindy.send({ type: 'tool-result', callId: msg.callId, ok: false, errorCode: 'BRIDGE_FAILED',
      message: 'anet bridge failed: ' + String(r.message || '').slice(0, 300) + ' — check that the anet node `cindy` is running (anet --id cindy up).' });
    return;
  }
  await cindy.send({ type: 'tool-result', callId: msg.callId, ok: true, result: r.result });
}
cindy.onHostMessage(async (msg) => {
  if (!msg) return;
  if (msg.type === 'tool-call') { await toolCall(msg); return; }
  if (msg.type === 'event' && msg.name === 'node-notification' && msg.method === 'dispatch/job') {
    const job = msg.params || {};
    const result = await handleJob(job);
    await node('dispatch/done', { jobId: job.jobId, result }, 15000);
    return;
  }
  if (msg.type === 'event' && msg.name === 'node-status' && msg.state === 'running') {
    await node('dispatch/hello', {}, 15000);
  }
});
node('dispatch/hello', {}, 15000);
