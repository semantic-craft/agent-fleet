// cindy-anet (Cindy × Agent Network): browser-sandbox brain (pattern follows official cindy-ping:
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
  const task = await cindy.tasks.create({ requestKey: job.requestKey + ':create', title: job.title });
  const run = await cindy.tasks.send({ taskId: task.taskId, expectedRevision: task.revision,
    requestKey: job.requestKey + ':send', text: job.text });
  return { ok: true, taskId: task.taskId, runId: (run && run.runId) || null, runStatus: (run && run.status) || null };
}
async function statusJob(job) {
  const out = { ok: true, taskId: job.taskId, runId: job.runId || null };
  if (job.runId) {
    try { out.run = await cindy.tasks.getRun({ runId: job.runId }); } catch (e) { out.runError = errInfo(e); }
  }
  try {
    const page = await cindy.tasks.readMessages({ taskId: job.taskId, limit: 100 });
    const msgs = (page && (page.messages || page.items)) || [];
    out.messages = msgs.slice(-20).map(m => ({ id: m.id || m.messageId || null, role: m.role || null,
      text: partText(m.text !== undefined ? m.text : m.content).slice(0, 16000) }));
  } catch (e) { out.messagesError = errInfo(e); }
  return out;
}
async function handleJob(job) {
  try {
    if (job.kind === 'create') return await createJob(job);
    if (job.kind === 'status') return await statusJob(job);
    return { ok: false, error: { code: 'UNKNOWN_JOB', message: 'unknown job kind' } };
  } catch (e) { return { ok: false, error: errInfo(e) }; }
}
async function toolCall(msg) {
  let r;
  if (msg.tool === 'send_to_peer') {
    const text = String((msg.args && msg.args.text) || '').trim();
    if (!text || text.length > 8000) {
      await cindy.send({ type: 'tool-result', callId: msg.callId, ok: false, errorCode: 'INVALID_TEXT', message: 'text must be 1-8000 characters' });
      return;
    }
    r = await node('anet/send_to_peer', { text }, 60000);
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
