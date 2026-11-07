'use strict';
// agent-network-cindy (Cindy × Agent Network) resident Node worker.
// Bridges the local Agent Network node `cindy` (anet --id cindy mcp, stdio) to Cindy tasks.
// No network listener. stdout = JSON-RPC to Cindy host only; logs -> stderr.
const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');
const crypto = require('node:crypto');
const readline = require('node:readline');
const { spawn } = require('node:child_process');

const HOME = process.env.HOME || os.homedir();
const DIR = process.env.AGENT_NETWORK_CINDY_DIR_TEST || path.join(HOME, 'Library', 'Application Support', 'CindyGlobal', 'agent-network-cindy');
const CONFIG_FILE = path.join(DIR, 'config.json');
const STATE_FILE = path.join(DIR, 'state.json');
const STATUS_FILE = path.join(DIR, 'status.json');
const POLL_MS = 10000;
const STATUS_EVERY_MS = 15000;
const MAX_RUN_MS = 30 * 60 * 1000;
// 30-minute observation window emits at most six 5-minute progress messages.
// Keep a bounded 100-message window so our own progress cannot hide the request.
const TASK_HISTORY_LENGTH = 100;
const ROUTE_PREFIX = 'ANET_CINDY_ROUTE_V1 ';
const ROUTE_FIELDS = ['agentKind', 'providerId', 'model', 'effort', 'fastMode'];
const AGENT_KINDS = new Set(['cc', 'codex', 'pi']);
const EFFORTS = new Set(['minimal', 'low', 'medium', 'high', 'xhigh', 'max', 'ultra']);

const log = (...a) => { try { process.stderr.write('[agent-network-cindy] ' + a.join(' ') + '\n'); } catch {} };
const write = (m) => process.stdout.write(JSON.stringify(m) + '\n');

// ---------- config / state ----------
function readJson(f, d) { try { return JSON.parse(fs.readFileSync(f, 'utf8')); } catch { return d; } }
function writeJson(f, obj) {
  try { fs.mkdirSync(DIR, { recursive: true, mode: 0o700 }); const t = f + '.tmp'; fs.writeFileSync(t, JSON.stringify(obj, null, 2), { mode: 0o600 }); fs.renameSync(t, f); }
  catch (e) { log('write failed', path.basename(f), e.message); throw e; }
}
let config = null;
function loadConfig() {
  const c = readJson(CONFIG_FILE, null);
  if (!c || typeof c.anetId !== 'string' || !/^[a-z0-9-]{1,32}$/.test(c.anetId) || !Array.isArray(c.allowPeers) || typeof c.peerAid !== 'string' || !c.allowPeers.includes(c.peerAid)) { config = null; return; }
  config = { anetBin: typeof c.anetBin === 'string' ? c.anetBin : path.join(HOME, '.local', 'bin', 'anet'), anetId: c.anetId,
    allowPeers: c.allowPeers.filter((x) => typeof x === 'string'), peerAid: c.peerAid };
}
let state;
try { state = JSON.parse(fs.readFileSync(STATE_FILE, 'utf8')); } catch (e) {
  if (e.code !== 'ENOENT') throw e;
  state = { version: 1, tasks: {} };
}
if (!state || state.version !== 1 || !state.tasks || typeof state.tasks !== 'object' || Array.isArray(state.tasks)) throw new Error('Invalid persisted task state');
if (!state.outgoing) state.outgoing = {};
if (typeof state.outgoing !== 'object' || Array.isArray(state.outgoing)) throw new Error('Invalid outgoing ledger');
const saveState = () => writeJson(STATE_FILE, state);
const status = { startedAt: new Date().toISOString(), mcp: 'down', mainConnected: false, lastPollAt: null, lastError: null, configured: false };
const saveStatus = () => {
  const counts = {}; for (const t of Object.values(state.tasks)) counts[t.phase] = (counts[t.phase] || 0) + 1;
  writeJson(STATUS_FILE, { ...status, configured: !!config, anetId: config ? config.anetId : null, counts, updatedAt: new Date().toISOString() });
};

// ---------- MCP client: anet --id <id> mcp ----------
let mcp = null;
function startMcp() {
  if (!config || mcp) return;
  const env = {}; for (const k of ['PATH', 'HOME', 'LANG', 'TMPDIR']) if (process.env[k]) env[k] = process.env[k];
  env.ANET_DATA_DIR = process.env.ANET_DATA_DIR || path.join(HOME, '.anet');
  const child = spawn(config.anetBin, ['--id', config.anetId, 'mcp'], { env, stdio: ['pipe', 'pipe', 'pipe'] });
  const m = { child, nextId: 0, waits: new Map(), ready: null, buf: '' };
  mcp = m;
  child.stdout.on('data', (d) => {
    m.buf += d; let i;
    while ((i = m.buf.indexOf('\n')) >= 0) {
      const line = m.buf.slice(0, i); m.buf = m.buf.slice(i + 1);
      let msg; try { msg = JSON.parse(line); } catch { continue; }
      const w = m.waits.get(msg.id); if (w) { m.waits.delete(msg.id); clearTimeout(w.t); w.resolve(msg); }
    }
  });
  child.stderr.on('data', (d) => log('anet-mcp:', String(d).trim().slice(0, 300)));
  const down = (why) => {
    if (mcp !== m) return; mcp = null; status.mcp = 'down'; status.lastError = 'anet mcp ' + why; saveStatus();
    for (const w of m.waits.values()) { clearTimeout(w.t); w.resolve({ error: { message: 'anet mcp exited' } }); }
    m.waits.clear();
  };
  child.on('exit', (code) => down('exited ' + code));
  child.on('error', (e) => down('error ' + e.message));
  m.ready = rpc(m, 'initialize', { protocolVersion: '2025-03-26', capabilities: {}, clientInfo: { name: 'agent-network-cindy', version: '0.3.4' } }, 30000)
    .then((r) => { if (r.error) throw new Error(r.error.message); child.stdin.write(JSON.stringify({ jsonrpc: '2.0', method: 'notifications/initialized' }) + '\n'); status.mcp = 'up'; saveStatus(); });
  m.ready.catch((e) => { log('mcp init failed', e.message); try { child.kill(); } catch {} });
}
function rpc(m, method, params, timeoutMs) {
  return new Promise((resolve) => {
    const id = ++m.nextId;
    const t = setTimeout(() => { m.waits.delete(id); resolve({ error: { message: 'timeout ' + method } }); }, timeoutMs || 60000);
    m.waits.set(id, { resolve, t });
    try { m.child.stdin.write(JSON.stringify({ jsonrpc: '2.0', id, method, params }) + '\n'); } catch (e) { clearTimeout(t); m.waits.delete(id); resolve({ error: { message: e.message } }); }
  });
}
async function tool(name, args, timeoutMs) {
  startMcp(); const m = mcp; if (!m) throw new Error('anet mcp not available');
  await m.ready;
  const r = await rpc(m, 'tools/call', { name, arguments: args }, timeoutMs || 60000);
  if (r.error) throw new Error(r.error.message || 'mcp error');
  const res = r.result || {};
  const text = (res.content || []).filter((c) => c && c.type === 'text').map((c) => c.text).join('\n');
  if (res.isError) throw new Error(text.slice(0, 500) || 'tool error');
  try { return JSON.parse(text); } catch { return { text }; }
}

// ---------- jobs for main.js (node-notification -> dispatch/done) ----------
const jobs = new Map(); // jobId -> {job, resolve, sentAt}
function runJob(job, timeoutMs) {
  return new Promise((resolve) => {
    const jobId = crypto.randomUUID();
    const entry = { job: { ...job, jobId }, resolve, sentAt: 0 };
    entry.timer = setTimeout(() => { jobs.delete(jobId); resolve({ ok: false, error: { code: 'MAIN_TIMEOUT', message: 'Cindy plugin brain did not answer' } }); }, timeoutMs || 60000);
    jobs.set(jobId, entry);
    if (status.mainConnected) { entry.sentAt = Date.now(); write({ jsonrpc: '2.0', method: 'dispatch/job', params: entry.job }); }
  });
}
function flushJobs() { for (const e of jobs.values()) if (!e.sentAt) { e.sentAt = Date.now(); write({ jsonrpc: '2.0', method: 'dispatch/job', params: e.job }); } }

// ---------- anet task helpers ----------
function latestUserMessage(task) {
  const history = Array.isArray(task.history) ? task.history : [];
  for (let i = history.length - 1; i >= 0; i--) {
    const message = history[i];
    if (message.role === 'ROLE_USER' || message.role === 'user') {
      const parts = Array.isArray(message.parts) ? message.parts : [];
      return {
        id: typeof message.messageId === 'string' && message.messageId ? message.messageId : null,
        text: parts.map(p => p && typeof p.text === 'string' ? p.text : '').filter(Boolean).join('\n')
      };
    }
  }
  return { id: null, text: '' };
}
const lastUserMessageId = task => latestUserMessage(task).id;
const lastUserText = task => latestUserMessage(task).text;
function historyFor(task) {
  return Array.isArray(task.history) ? task.history : [];
}
function userText(message) {
  const parts = Array.isArray(message.parts) ? message.parts : null;
  if (!parts || !parts.length || parts.some(part => !part || typeof part !== 'object' || Array.isArray(part) ||
      Object.keys(part).some(key => key !== 'text') || typeof part.text !== 'string' || !part.text.length)) return null;
  return parts.map(part => part.text).join('\n');
}
function routeKeyCount(routeText, key) {
  return (routeText.match(new RegExp('"' + key + '"\\s*:', 'g')) || []).length;
}
function validateRoute(route) {
  if (!route || typeof route !== 'object' || Array.isArray(route)) return null;
  const keys = Object.keys(route);
  if (keys.length !== ROUTE_FIELDS.length || ROUTE_FIELDS.some(key => !keys.includes(key))) return null;
  if (!AGENT_KINDS.has(route.agentKind) || !EFFORTS.has(route.effort) ||
      typeof route.providerId !== 'string' || !route.providerId.trim() ||
      typeof route.model !== 'string' || !route.model.trim() ||
      (route.fastMode !== true && route.fastMode !== false && route.fastMode !== 'ultrafast')) return null;
  return route;
}
function routeMatches(left, right) {
  if (left == null && right == null) return true;
  return !!left && !!right && typeof left === 'object' && !Array.isArray(left) &&
    typeof right === 'object' && !Array.isArray(right) &&
    ROUTE_FIELDS.every(key => left[key] === right[key]);
}
function routeCompatible(left, right) {
  return left === right || routeMatches(left, right);
}
function parseRouteEnvelope(sourceText) {
  if (typeof sourceText !== 'string' || !sourceText.length) return { error: 'MODEL_ROUTE_INVALID', text: sourceText, route: null };
  const newline = sourceText.indexOf('\n');
  const firstLine = newline < 0 ? sourceText : sourceText.slice(0, newline);
  if (firstLine.startsWith('ANET_CINDY_ROUTE_')) {
    if (!firstLine.startsWith(ROUTE_PREFIX) ||
        /^\s/.test(firstLine.slice(ROUTE_PREFIX.length))) {
      return { error: 'MODEL_ROUTE_INVALID', text: sourceText, route: null };
    }
  } else return { text: sourceText, route: null };
  const routeText = firstLine.slice(ROUTE_PREFIX.length);
  let route;
  try { route = JSON.parse(routeText); } catch { return { error: 'MODEL_ROUTE_INVALID', text: sourceText, route: null }; }
  if (typeof route !== 'object' || !route || Array.isArray(route) ||
      ROUTE_FIELDS.some(key => routeKeyCount(routeText, key) !== 1) ||
      Object.keys(route).length !== ROUTE_FIELDS.length) {
    return { error: 'MODEL_ROUTE_INVALID', text: sourceText, route: null };
  }
  route = validateRoute(route);
  if (!route) return { error: 'MODEL_ROUTE_INVALID', text: sourceText, route: null };
  const text = newline < 0 ? '' : sourceText.slice(newline + 1);
  if (!text.trim()) return { error: 'MODEL_ROUTE_INVALID', text: sourceText, route: null };
  return { text, route };
}
function taskTextFromAnchor(rec, sourceText) {
  const parsed = parseRouteEnvelope(sourceText);
  if (parsed.error) return null;
  if (parsed.route && !rec.route) return null;
  if (parsed.route && !routeMatches(parsed.route, rec.route)) return null;
  if (!parsed.route && rec.route && sourceText !== parsed.text) return null;
  return parsed.text;
}
async function failRouteDeterministically(aid, rec, code, message) {
  Object.assign(rec, { phase: 'delivery-unverified', error: { code }, pendingSend: null,
    pendingReply: { task_id: aid, state: 'failed', text: 'Rejected before sending this request: ' + message } });
  saveState();
  try { await tool('reply_task', rec.pendingReply); rec.phase = 'failed'; }
  catch { rec.phase = 'delivery-unverified'; }
  saveState();
}
function validateHistory(task, anchorId, anchorText) {
  const history = historyFor(task);
  const seen = new Set();
  let anchorIndex = -1;
  for (let index = 0; index < history.length; index++) {
    const message = history[index];
    if (!message || typeof message !== 'object' || Array.isArray(message)) return { error: 'MALFORMED_HISTORY' };
    const id = typeof message.messageId === 'string' && message.messageId ? message.messageId : null;
    const isUser = message.role === 'ROLE_USER' || message.role === 'user';
    if (isUser) {
      if ('attachments' in message) return { error: 'MALFORMED_USER_MESSAGE' };
      if (!id) return { error: 'MISSING_MESSAGE_ID' };
      if (seen.has(id)) return { error: 'DUPLICATE_MESSAGE_ID' };
      seen.add(id);
      const text = userText(message);
      if (text === null) return { error: 'MALFORMED_USER_MESSAGE' };
      if (text.length > 32768) return { error: 'INPUT_TOO_LONG' };
      if (id === anchorId) {
        if (anchorIndex >= 0) return { error: 'DUPLICATE_MESSAGE_ID' };
        anchorIndex = index;
        if (text !== anchorText) return { error: 'ANCHOR_TEXT_CHANGED' };
      }
    } else if (id) {
      if (seen.has(id)) return { error: 'DUPLICATE_MESSAGE_ID' };
      seen.add(id);
    }
  }
  if (anchorIndex < 0) return { error: 'ANCHOR_NOT_IN_BOUNDED_HISTORY' };
  const suffix = [];
  for (let index = anchorIndex + 1; index < history.length; index++) {
    const message = history[index];
    if (message.role !== 'ROLE_USER' && message.role !== 'user') continue;
    const text = userText(message);
    if (text === null || !text.trim()) return { error: 'MALFORMED_USER_MESSAGE' };
    suffix.push({ messageId: message.messageId, text });
  }
  return { suffix };
}
const requestKeyFor = (taskId, messageId) =>
  'anet-followup-' + crypto.createHash('sha256').update('agent-network-cindy\0' + taskId + '\0' + messageId, 'utf8').digest('hex');
const outputKey = (messageId, runId) => messageId + ':' + runId;
const CONTINUATION_REJECTIONS = new Set(['IDEMPOTENCY_CONFLICT', 'REVISION_CONFLICT', 'TASK_BINDING_INVALID',
  'TASK_NOT_FOUND', 'TASK_BUSY', 'PERMISSION_DENIED']);
function storeOutput(rec, messageId, runId, text) {
  if (messageId !== rec.requestMessageId || runId !== rec.runId) throw new Error('inconsistent output binding');
  if (!rec.runs || typeof rec.runs !== 'object' || Array.isArray(rec.runs)) rec.runs = {};
  if (!Array.isArray(rec.completedRequests)) rec.completedRequests = [];
  const key = outputKey(messageId, runId);
  const prior = rec.runs[key];
  if (prior && (!prior || prior.messageId !== messageId || prior.runId !== runId || prior.text !== text)) {
    throw new Error('inconsistent persisted output');
  }
  rec.runs[key] = { messageId, runId, text, at: prior && prior.at || new Date().toISOString() };
  if (!rec.completedRequests.includes(messageId)) rec.completedRequests.push(messageId);
}
function composedOutput(rec) {
  const runs = rec.runs && typeof rec.runs === 'object' ? rec.runs : {};
  const outputs = (Array.isArray(rec.completedRequests) ? rec.completedRequests : [])
    .map(messageId => Object.values(runs).find(run => run.messageId === messageId))
    .filter(Boolean);
  if (outputs.length === 1) return outputs[0].text;
  return outputs.map((run, index) => '[Cindy request ' + (index + 1) + ': ' + run.messageId + ']\n' + run.text).join('\n\n');
}
function failureReport(rec, runStatus) {
  const outputs = composedOutput(rec);
  if (!outputs) return 'Cindy run ended: ' + runStatus;
  return '[Cindy: run ended as ' + runStatus + '; exact prior outputs follow]\n\n' + outputs;
}
async function reportRouteUncertainty(rec, aid, code) {
  Object.assign(rec, { phase: 'unverified', error: { code },
    uncertaintyNoticeAttempt: { at: new Date().toISOString(), outcome: 'unknown' } });
  saveState();
  await tool('reply_task', { task_id: aid, state: 'working',
    text: 'Cindy route verification failed after a host call. The execution and result are not confirmed; this request will not automatically retry.' });
  Object.assign(rec, { uncertaintyNotice: { at: rec.uncertaintyNoticeAttempt.at,
    outcome: 'delivered', deliveredAt: new Date().toISOString() } });
  saveState();
}

function pendingSendMatchesQueue(rec, send) {
  const next = Array.isArray(rec.queue) && rec.queue[0];
  const expectedText = next ? taskTextFromAnchor(rec, next.text) : null;
  return !!send && typeof send === 'object' && !Array.isArray(send) &&
    send.taskId === rec.cindyTaskId && !!next &&
    send.messageId === next.messageId && send.text === expectedText &&
    (send.sourceText === next.text || !rec.route && send.sourceText === undefined) &&
    routeCompatible(send.route, rec.route) &&
    typeof send.requestKey === 'string' && !!send.requestKey &&
    Number.isSafeInteger(send.expectedRevision) &&
    (!('runId' in send) || typeof send.runId === 'string' && !!send.runId);
}
function queueSuffixIsAppendOnly(previousQueue, suffix) {
  const prior = Array.isArray(previousQueue) ? previousQueue : [];
  return suffix.length >= prior.length && prior.every((message, index) => {
    const next = suffix[index];
    return message && next && message.messageId === next.messageId && message.text === next.text;
  });
}
function acceptedPendingSendMatches(rec, send) {
  const expectedText = typeof rec.requestText === 'string' ? taskTextFromAnchor(rec, rec.requestText) : null;
  return !!send && typeof send === 'object' && !Array.isArray(send) &&
    send.taskId === rec.cindyTaskId && send.runId === rec.runId &&
    send.messageId === rec.requestMessageId && send.text === expectedText &&
    (send.sourceText === rec.requestText || !rec.route && send.sourceText === undefined) &&
    routeCompatible(send.route, rec.route) &&
    typeof send.requestKey === 'string' && !!send.requestKey &&
    Number.isSafeInteger(send.expectedRevision);
}
function commitContinuation(rec, send, response) {
  if (!response || !response.ok || !response.run || response.run.taskId !== send.taskId ||
      typeof response.run.runId !== 'string' || !response.run.runId) return false;
  Object.assign(rec, { requestMessageId: send.messageId, requestText: send.sourceText || send.text, runId: response.run.runId,
    acceptedAt: new Date().toISOString(), lastCheck: 0, lastProgressAt: null, error: null,
    pendingSend: { ...send, runId: response.run.runId } });
  rec.queue.shift();
  saveState();
  return true;
}
async function dispatchContinuation(rec, aid) {
  if (!rec.queue || !rec.queue.length) return;
  if (rec.route && !validateRoute(rec.route)) {
    Object.assign(rec, { phase: 'unverified', error: { code: 'MODEL_ROUTE_STATE_INVALID' } }); saveState();
    return;
  }
  if (!rec.route && rec.requestText && parseRouteEnvelope(rec.requestText).route) {
    Object.assign(rec, { phase: 'unverified', error: { code: 'MODEL_ROUTE_STATE_MISSING' } }); saveState();
    return;
  }
  const replay = !!rec.pendingSend;
  let send = rec.pendingSend;
  const nextRoute = rec.queue.length ? parseRouteEnvelope(rec.queue[0].text) : null;
  if (nextRoute?.error) {
    await failRouteDeterministically(rec.officialTaskId, rec, nextRoute.error,
      'the follow-up explicit route envelope is malformed, partial, or contains unsupported values.');
    return;
  }
  if (nextRoute?.route && (!rec.route || !routeMatches(nextRoute.route, rec.route))) {
    await failRouteDeterministically(rec.officialTaskId, rec, 'MODEL_ROUTE_CHANGED',
      'the follow-up attempted to change the saved explicit Cindy route.');
    return;
  }
  if (send && (!pendingSendMatchesQueue(rec, send) || send.runId && send.runId !== rec.runId)) {
    Object.assign(rec, { phase: 'unverified', error: { code: 'FOLLOWUP_STATE_INCONSISTENT' } }); saveState();
    return;
  }
  if (!send) {
    const next = rec.queue[0];
    const prepared = await runJob({ kind: 'prepare-continuation', taskId: rec.cindyTaskId,
      ...(rec.route ? { route: rec.route } : {}) }, 30000);
    if (!prepared || !prepared.ok || prepared.taskId !== rec.cindyTaskId || prepared.status !== 'active' ||
        !Number.isSafeInteger(prepared.revision)) {
      const preparedCode = prepared?.error?.code;
      if (prepared?.beforeSend && preparedCode === 'MODEL_ROUTE_MISMATCH') {
        await failRouteDeterministically(aid || rec.officialTaskId, rec, preparedCode,
          'saved Cindy route no longer matches the host task (MODEL_ROUTE_MISMATCH).');
        return;
      }
      Object.assign(rec, { phase: 'unverified',
        error: { code: preparedCode || 'FOLLOWUP_PREPARE_FAILED' } });
      saveState();
      return;
    }
    send = { messageId: next.messageId, text: next.text, sourceText: next.text, taskId: rec.cindyTaskId,
      requestKey: requestKeyFor(rec.cindyTaskId, next.messageId), expectedRevision: prepared.revision };
    const taskText = taskTextFromAnchor(rec, send.text);
    if (!taskText) {
      Object.assign(rec, { phase: 'unverified', error: { code: 'MODEL_ROUTE_CHANGED' } }); saveState();
      return;
    }
    send.text = taskText;
    if (rec.route) send.route = rec.route;
    rec.pendingSend = send;
    saveState();
  }
  const response = await runJob({ kind: 'send-continuation', taskId: send.taskId, requestKey: send.requestKey,
    expectedRevision: send.expectedRevision, text: send.text,
    replay,
    ...(send.route ? { route: send.route } : {}) }, 45000);
  if (!commitContinuation(rec, send, response)) {
    const code = (response && response.error && response.error.code) || 'FOLLOWUP_SEND_OUTCOME_UNKNOWN';
    if (response?.afterSend) {
      Object.assign(rec, { phase: 'unverified', error: { code },
        pendingSend: { ...send, runId: response.runId || send.runId || null } });
      saveState();
      await reportRouteUncertainty(rec, aid || rec.officialTaskId, code);
      return;
    }
    if (response?.beforeSend && code === 'MODEL_ROUTE_MISMATCH') {
      await failRouteDeterministically(aid || rec.officialTaskId, rec, code,
        'saved Cindy route no longer matches the host task (MODEL_ROUTE_MISMATCH).');
      return;
    }
    const knownRejection = CONTINUATION_REJECTIONS.has(code);
    Object.assign(rec, { phase: knownRejection ? 'unverified' : 'running',
      error: { code }, pendingSend: send });
    saveState();
  }
}
async function repairLegacyFrozen(aid, rec) {
  if (rec.phase !== 'unverified' || rec.error?.code !== 'FOLLOWUP_REQUIRES_OPERATOR' ||
      !rec.cindyTaskId || !rec.runId || !rec.requestMessageId || typeof rec.requestText !== 'string' ||
      !config.allowPeers.includes(rec.peer) || rec.pendingSend) return;
  const remote = await tool('get_task', { task_id: aid, history_length: TASK_HISTORY_LENGTH });
  const task = remote.task || remote;
  const peerValid = rec.peer === task.metadata?.['anet.peer_aid'] && config.allowPeers.includes(rec.peer);
  const validated = task.id === aid && !OFFICIAL_TERMINAL.has(officialState(task)) ?
    validateHistory(task, rec.requestMessageId, rec.requestText) : { error: 'OFFICIAL_OR_ANCHOR_UNVERIFIED' };
  if (!peerValid || validated.error) return;
  if (!rec.runs || typeof rec.runs !== 'object' || Array.isArray(rec.runs)) rec.runs = {};
  if (!Array.isArray(rec.completedRequests)) rec.completedRequests = [rec.requestMessageId];
  Object.assign(rec, { phase: 'running', queue: validated.suffix, pendingSend: null, error: null,
    lastCheck: 0, lastProgressAt: null });
  saveState();
}
function pickOutput(st) {
  const msgs = Array.isArray(st.messages) ? st.messages : [];
  const outId = st.run && st.run.outputMessageId;
  if (!outId) return '';
  const message = msgs.find(x => x.id === outId && x.role === 'assistant');
  return message && typeof message.text === 'string' ? message.text : '';
}
const officialState = (task) => String(task?.status?.state || '').replace(/^TASK_STATE_/, '').toLowerCase().replaceAll('_', '-');
const OFFICIAL_TERMINAL = new Set(['completed', 'failed', 'rejected', 'canceled']);
const TERMINAL = new Set(['completed', 'failed', 'cancelled', 'interrupted']);
const ACTIVE_RUN_STATUSES = new Set(['running', 'working', 'queued', 'pending', 'reconciling', 'stopping']);
const KNOWN_RUN_STATUSES = new Set([...ACTIVE_RUN_STATUSES, ...TERMINAL]);

let busy = false;
async function tick() {
  if (busy) return; busy = true;
  try {
    loadConfig();
    if (!config) { status.lastError = 'not configured: ' + CONFIG_FILE; saveStatus(); return; }
    startMcp();
    // 1) new tasks sent to the cindy identity
    const list = { tasks: [] };
    let pageToken; const seenTokens = new Set();
    do {
      const page = await tool('list_tasks', { role: 'provider', state: 'submitted', page_size: 100, history_length: 5, ...(pageToken ? { page_token: pageToken } : {}) }, 60000);
      list.tasks.push(...(page.tasks || []));
      pageToken = page.nextPageToken;
      if (pageToken && seenTokens.has(pageToken)) throw new Error('Repeated list_tasks page token');
      if (pageToken) seenTokens.add(pageToken);
    } while (pageToken);
    status.lastPollAt = new Date().toISOString(); status.lastError = null;
    for (const t of (list.tasks || [])) {
      if (!t || !t.id || state.tasks[t.id]) continue;
      const peer = t.metadata && t.metadata['anet.peer_aid'];
      if (!config.allowPeers.includes(peer)) {
        state.tasks[t.id] = { phase: 'delivery-unverified', peer, pendingReply: { task_id: t.id, state: 'rejected', text: 'This Cindy bridge only takes tasks from its configured peers.' }, at: new Date().toISOString() }; saveState();
        await tool('reply_task', state.tasks[t.id].pendingReply);
        state.tasks[t.id].phase = 'rejected'; saveState();
        continue;
      }
      const text = lastUserText(t);
      if (!text.trim()) {
        state.tasks[t.id] = { phase: 'unverified', peer, requestMessageId: lastUserMessageId(t), error: { code: 'LATEST_USER_TEXT_UNAVAILABLE' } }; saveState(); continue;
      }
      state.tasks[t.id] = { phase: 'creating', peer, requestMessageId: lastUserMessageId(t), at: new Date().toISOString() }; saveState();
    }
    // 2) advance known tasks
    for (const [aid, rec] of Object.entries(state.tasks)) {
      await repairLegacyFrozen(aid, rec);
      if (rec.phase === 'delivery-unverified') {
        let task;
        if (rec.pendingReply && ['completed', 'failed'].includes(rec.pendingReply.state)) {
          const official = await tool('get_task', { task_id: aid, history_length: TASK_HISTORY_LENGTH });
          task = official.task || official;
          const current = officialState(task);
          if (OFFICIAL_TERMINAL.has(current)) {
            if (current === rec.pendingReply.state) Object.assign(rec, { phase: 'delivery-state-observed', endedAt: new Date().toISOString() });
            else Object.assign(rec, { phase: 'unverified', error: { code: 'OFFICIAL_STATE_CONFLICT' }, officialState: task.status.state });
            saveState(); continue;
          }
        } else {
          const remote = await tool('get_task', { task_id: aid, history_length: TASK_HISTORY_LENGTH });
          task = remote.task || remote;
        }
        // Read official state after uncertain delivery. Never repeat a terminal mutation.
        if (officialState(task) === rec.pendingReply.state) {
          Object.assign(rec, { phase: 'delivery-state-observed', endedAt: new Date().toISOString() }); saveState();
        }
      } else if (rec.phase === 'creating') {
        if (!status.mainConnected) continue;
        const t = await tool('get_task', { task_id: aid, history_length: TASK_HISTORY_LENGTH });
        const task = t.task || t;
        if (OFFICIAL_TERMINAL.has(officialState(task))) {
          Object.assign(rec, { phase: 'unverified', error: { code: 'OFFICIAL_TASK_ENDED_BEFORE_CREATE' } }); saveState(); continue;
        }
        if (!rec.requestMessageId || lastUserMessageId(task) !== rec.requestMessageId) {
          Object.assign(rec, { phase: 'unverified', error: { code: 'REQUEST_MESSAGE_CHANGED_OR_UNAVAILABLE' } }); saveState(); continue;
        }
        const text = lastUserText(task);
        if (!text.trim()) {
          Object.assign(rec, { phase: 'unverified', error: { code: 'LATEST_USER_TEXT_UNAVAILABLE' } }); saveState(); continue;
        }
        if (text.length > 32768) {
          Object.assign(rec, { phase: 'unverified', error: { code: 'INPUT_TOO_LONG' } }); saveState(); continue;
        }
        // Persist uncertainty BEFORE the host side effect; restart must not duplicate it.
        const envelope = parseRouteEnvelope(text);
        if (envelope.error) {
          Object.assign(rec, { requestText: text, error: { code: envelope.error } });
          await failRouteDeterministically(aid, rec, envelope.error,
            'the explicit route envelope is malformed, partial, or contains unsupported values (' + envelope.error + ').');
          continue;
        }
        Object.assign(rec, { phase: 'unverified', requestText: text,
          ...(envelope.route ? { route: envelope.route } : {}), error: { code: 'CREATE_OUTCOME_UNKNOWN' } }); saveState();
        const r = await runJob({ kind: 'create', requestKey: 'anet-' + aid.replace(/[^A-Za-z0-9_-]/g, '').slice(0, 100), title: ('Agent Network: ' + text.replace(/\s+/g, ' ')).slice(0, 100), text: envelope.text,
          ...(envelope.route ? { route: envelope.route } : {}) }, 90000);
        if (r && r.ok) {
          Object.assign(rec, { phase: 'running', officialTaskId: aid, cindyTaskId: r.taskId, runId: r.runId, acceptedAt: new Date().toISOString(), error: null }); saveState();
          await tool('reply_task', { task_id: aid, state: 'working',
            text: 'Accepted by \u007b\u007bPERSON_bee7cl5q\u007d\u007d; working on it.' }).catch((e) => log('working reply failed', e.message));
        } else {
          const code = (r && r.error && r.error.code) || 'UNKNOWN';
          if (r?.beforeSend && r.error?.code?.startsWith('MODEL_ROUTE_')) {
            Object.assign(rec, { requestText: text, cindyTaskId: r.taskId || null,
              ...(r.runId ? { runId: r.runId } : {}) });
            await failRouteDeterministically(aid, rec, code,
              'the host task route does not match the requested explicit Cindy route.');
            continue;
          }
          if (r?.afterSend && r.error?.code?.startsWith('MODEL_ROUTE_')) {
            Object.assign(rec, { phase: 'unverified', error: { code },
              cindyTaskId: r.taskId || null,
              ...(r.runId ? { runId: r.runId } : {}) });
            await reportRouteUncertainty(rec, aid, code);
            continue;
          }
          // A host error may follow a successful create or send. Do not retry blindly.
          Object.assign(rec, { phase: 'unverified', error: { code }, cindyTaskId: r && r.taskId || null,
            ...(r && typeof r.runId === 'string' && r.runId ? { runId: r.runId } : {}) }); saveState();
        }
      } else if (rec.phase === 'running') {
        if (!status.mainConnected) continue;
        if (rec.lastCheck && Date.now() - rec.lastCheck < STATUS_EVERY_MS) continue;
        rec.lastCheck = Date.now();
        if (rec.route && !validateRoute(rec.route)) {
          Object.assign(rec, { phase: 'unverified', error: { code: 'MODEL_ROUTE_STATE_INVALID' } }); saveState(); continue;
        }
        if (!rec.route && rec.requestText && parseRouteEnvelope(rec.requestText).route) {
          Object.assign(rec, { phase: 'unverified', error: { code: 'MODEL_ROUTE_STATE_MISSING' } }); saveState(); continue;
        }
        const officialResult = await tool('get_task', { task_id: aid, history_length: TASK_HISTORY_LENGTH });
        const official = officialResult.task || officialResult;
        if (OFFICIAL_TERMINAL.has(officialState(official))) {
          Object.assign(rec, { phase: 'unverified', error: { code: 'OFFICIAL_TASK_ENDED_HOST_NOT_STOPPED' }, officialState: official.status.state }); saveState(); continue;
        }
        const peerValid = official.id === aid && rec.peer === official.metadata?.['anet.peer_aid'] &&
          config.allowPeers.includes(rec.peer);
        const validated = rec.requestMessageId && typeof rec.requestText === 'string' ?
          validateHistory(official, rec.requestMessageId, rec.requestText) :
          { error: 'ANCHOR_NOT_IN_BOUNDED_HISTORY' };
        if (!peerValid || validated.error) {
          Object.assign(rec, { phase: 'unverified',
            error: { code: peerValid ? validated.error : 'PEER_OR_TASK_BINDING_CHANGED' } }); saveState(); continue;
        }
        const previousQueue = Array.isArray(rec.queue) ? rec.queue : [];
        if (!queueSuffixIsAppendOnly(previousQueue, validated.suffix)) {
          Object.assign(rec, { phase: 'unverified', error: { code: 'FOLLOWUP_QUEUE_HISTORY_CHANGED' } });
          saveState(); continue;
        }
        if (previousQueue.length !== validated.suffix.length) rec.queue = validated.suffix;
        saveState();
        const st = await runJob({ kind: 'status', taskId: rec.cindyTaskId, runId: rec.runId,
          ...(rec.route ? { route: rec.route } : {}) }, 30000);
        if (st?.runError?.code?.startsWith('MODEL_ROUTE_')) {
          await reportRouteUncertainty(rec, aid, st.runError.code); continue;
        }
        if (!st || !st.ok || st.runError || !st.run) continue;
        if (st.run.taskId !== rec.cindyTaskId || st.run.runId !== rec.runId) {
          Object.assign(rec, { phase: 'unverified', error: { code: 'HOST_RUN_BINDING_INVALID' } }); saveState(); continue;
        }
        const runStatus = st.run.status;
        if (!KNOWN_RUN_STATUSES.has(runStatus)) {
          Object.assign(rec, { phase: 'unverified', error: { code: 'UNKNOWN_RUN_STATUS' } }); saveState(); continue;
        }
        rec.runStatus = runStatus || null;
        if (TERMINAL.has(runStatus)) {
          const out = pickOutput(st);
          if (runStatus === 'completed' && (!out || out.length > 60000)) {
            Object.assign(rec, { phase: 'unverified', error: { code: 'EXACT_OUTPUT_UNAVAILABLE' } }); saveState(); continue;
          }
          const ok = runStatus === 'completed' && out;
          if (ok) {
            storeOutput(rec, rec.requestMessageId, rec.runId, out);
            saveState();
          }
          if (rec.pendingSend?.runId === rec.runId) {
            if (!acceptedPendingSendMatches(rec, rec.pendingSend)) {
              Object.assign(rec, { phase: 'unverified', error: { code: 'FOLLOWUP_STATE_INCONSISTENT' } }); saveState(); continue;
            }
            rec.pendingSend = null;
            saveState();
          }
          if (runStatus === 'completed' && rec.queue && rec.queue.length) {
        await dispatchContinuation(rec, aid);
            continue;
          }
          if (runStatus === 'completed') {
            const refreshed = await tool('get_task', { task_id: aid, history_length: TASK_HISTORY_LENGTH });
            const latest = refreshed.task || refreshed;
            if (OFFICIAL_TERMINAL.has(officialState(latest))) {
              Object.assign(rec, { phase: 'unverified', error: { code: 'OFFICIAL_TASK_ENDED_HOST_NOT_STOPPED' },
                officialState: latest.status.state }); saveState(); continue;
            }
            const latestValidated = validateHistory(latest, rec.requestMessageId, rec.requestText);
            const latestPeerValid = latest.id === aid && rec.peer === latest.metadata?.['anet.peer_aid'] &&
              config.allowPeers.includes(rec.peer);
            if (!latestPeerValid || latestValidated.error) {
              Object.assign(rec, { phase: 'unverified',
                error: { code: latestPeerValid ? latestValidated.error : 'PEER_OR_TASK_BINDING_CHANGED' } }); saveState(); continue;
            }
            if (latestValidated.suffix.length) {
              rec.queue = latestValidated.suffix;
              saveState();
              await dispatchContinuation(rec, aid);
              continue;
            }
          }
          const finalText = ok ? composedOutput(rec) : failureReport(rec, runStatus);
          if (finalText.length > 60000) {
            Object.assign(rec, { phase: 'unverified', error: { code: 'EXACT_OUTPUT_TOO_LARGE' } }); saveState(); continue;
          }
          if (rec.pendingSend) {
            Object.assign(rec, { phase: 'unverified', error: { code: 'FOLLOWUP_SEND_UNRESOLVED' } });
            saveState(); continue;
          }
          Object.assign(rec, { phase: 'delivery-unverified', pendingReply: { task_id: aid, state: ok ? 'completed' : 'failed', text: finalText } }); saveState();
          await tool('reply_task', rec.pendingReply);
          Object.assign(rec, { phase: ok ? 'replied' : 'failed', endedAt: new Date().toISOString() }); saveState();
        } else if (Date.now() - Date.parse(rec.acceptedAt) > MAX_RUN_MS) {
          // Observation deadline is not evidence that the host failed.
          Object.assign(rec, { phase: 'unverified', error: { code: 'RUN_TIMEOUT' } }); saveState();
        } else if (ACTIVE_RUN_STATUSES.has(runStatus) && (!rec.lastProgressAt || Date.now() - rec.lastProgressAt > 5 * 60 * 1000)) {
          rec.lastProgressAt = Date.now(); saveState();
          await tool('reply_task', { task_id: aid, state: 'working', text: 'Cindy host run remains active; result not yet verified.' });
        }
      }
    }
  } catch (e) {
    status.lastError = String(e.message || e).slice(0, 300); log('tick error', status.lastError);
  } finally { saveStatus(); busy = false; }
}

async function sendToPeer(p, text) {
  loadConfig(); if (!config) throw new Error('bridge not configured');
      const callId = typeof p.callId === 'string' ? p.callId : '';
      if (!callId || callId.length > 256) throw new Error('stable host callId is required');
      const key = crypto.createHash('sha256').update(callId).digest('hex');
      const prior = state.outgoing[key];
      if (prior) {
        if (prior.text !== text || prior.peerAid !== config.peerAid) throw new Error('callId reused for different content');
        if (prior.anetTaskId) {
          const observed = await tool('get_task', { task_id: prior.anetTaskId });
          return ({ sent: prior.phase === 'accepted', outcome: prior.phase, anetTaskId: prior.anetTaskId, observed });
        }
        return ({ sent: false, outcome: 'unverified', messageId: prior.messageId, message: 'Prior send outcome is unknown; inspect official tasks. Do not resend.' });
      }
      const rec = state.outgoing[key] = { phase: 'unverified', text, peerAid: config.peerAid, messageId: crypto.randomUUID() };
      saveState();
      try {
        const r = await tool('send_message', { to: config.peerAid, text, return_immediately: true }, 45000);
        const task = r.task || r;
        if (typeof task.id !== 'string' || !task.id) return ({ sent: false, outcome: 'unverified', messageId: rec.messageId });
        Object.assign(rec, { phase: 'accepted', anetTaskId: task.id }); saveState();
        return ({ sent: true, outcome: 'accepted', anetTaskId: task.id, state: officialState(task) });
      } catch (e) { return ({ sent: false, outcome: 'unverified', messageId: rec.messageId, message: String(e.message || e).slice(0, 300) }); }
}

async function reconcileResult(aid) {
  loadConfig();
  if (!config || typeof aid !== 'string') throw new Error('configured bridge and task id required');
  const rec = state.tasks[aid];
  if (!rec || rec.phase !== 'unverified' || rec.error?.code !== 'EXACT_OUTPUT_UNAVAILABLE' ||
      !rec.cindyTaskId || !rec.runId || !rec.requestMessageId ||
      !config.allowPeers.includes(rec.peer) || rec.pendingSend) {
    return { reconciled: false, reason: 'not-eligible', taskId: aid };
  }
  if (busy || !status.mainConnected) return { reconciled: false, reason: 'bridge-busy-or-disconnected', taskId: aid };
  busy = true;
  try {
    const remote = await tool('get_task', { task_id: aid, history_length: TASK_HISTORY_LENGTH });
    const task = remote.task || remote;
    if (OFFICIAL_TERMINAL.has(officialState(task)) || task.id !== aid ||
        rec.peer !== task.metadata?.['anet.peer_aid'] || !config.allowPeers.includes(rec.peer) ||
        lastUserMessageId(task) !== rec.requestMessageId || !lastUserText(task).trim()) {
      return { reconciled: false, reason: 'official-task-ended-or-request-changed', taskId: aid };
    }
    const st = await runJob({ kind: 'status', taskId: rec.cindyTaskId, runId: rec.runId }, 30000);
    if (!st?.ok || st.runError || st.messagesError || st.run?.taskId !== rec.cindyTaskId ||
        st.run?.runId !== rec.runId || st.run?.status !== 'completed') {
      return { reconciled: false, reason: 'host-output-not-ready', taskId: aid };
    }
    const text = pickOutput(st);
    if (!text || text.length > 60000) return { reconciled: false, reason: 'exact-output-unavailable', taskId: aid };
    Object.assign(rec, { phase: 'delivery-unverified', pendingReply: { task_id: aid, state: 'completed', text } });
    saveState();
    await tool('reply_task', rec.pendingReply);
    Object.assign(rec, { phase: 'replied', error: null, reconciledAt: new Date().toISOString(), endedAt: new Date().toISOString() });
    saveState();
    return { reconciled: true, taskId: aid, state: 'completed' };
  } finally { busy = false; saveStatus(); }
}

// ---------- stdio JSON-RPC from Cindy host (main.js) ----------
if (!process.env.AGENT_NETWORK_CINDY_TEST) readline.createInterface({ input: process.stdin }).on('line', async (line) => {
  let req; try { req = JSON.parse(line); } catch { write({ jsonrpc: '2.0', id: null, error: { code: -32700, message: 'Parse error' } }); return; }
  if (!req || req.id === undefined || req.id === null) return;
  const p = req.params || {};
  const ok = (result) => write({ jsonrpc: '2.0', id: req.id, result });
  const fail = (message) => write({ jsonrpc: '2.0', id: req.id, error: { code: -32000, message } });
  try {
    if (req.method === 'dispatch/hello') { status.mainConnected = true; saveStatus(); flushJobs(); return ok({ ok: true }); }
    if (req.method === 'dispatch/done') {
      status.mainConnected = true;
      const e = jobs.get(p.jobId); if (e) { jobs.delete(p.jobId); clearTimeout(e.timer); e.resolve(p.result); }
      return ok({ ok: true });
    }
    if (req.method === 'bridge/status') { loadConfig(); saveStatus(); return ok(readJson(STATUS_FILE, {})); }
    if (req.method === 'anet/reconcile_result') return ok(await reconcileResult(p.taskId));
    if (req.method === 'anet/send_to_peer') {
      loadConfig(); if (!config) return fail('bridge not configured');
      const text = String(p.text || '').trim(); if (!text || text.length > 8000) return fail('text must be 1-8000 characters');
      return ok(await sendToPeer(p, text));
    }
    return write({ jsonrpc: '2.0', id: req.id, error: { code: -32601, message: 'Method not found' } });
  } catch (e) { return fail(String(e.message || e).slice(0, 300)); }
});
if (!process.env.AGENT_NETWORK_CINDY_TEST) process.stdin.on('end', () => { try { if (mcp) mcp.child.kill(); } catch {} process.exit(0); });

if (!process.env.AGENT_NETWORK_CINDY_TEST) {
  loadConfig(); saveStatus();
  setInterval(tick, POLL_MS);
  setTimeout(tick, 1500);
  log('started; config', config ? 'ok' : 'missing');
}
module.exports = { tick, state, status, parseRouteEnvelope, sendToPeer, reconcileResult,
  setTestDependencies(deps) { tool = deps.tool; runJob = deps.runJob; startMcp = () => {}; } };
