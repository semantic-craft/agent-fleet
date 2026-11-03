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
  m.ready = rpc(m, 'initialize', { protocolVersion: '2025-03-26', capabilities: {}, clientInfo: { name: 'agent-network-cindy', version: '0.3.1' } }, 30000)
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
function pickOutput(st) {
  const msgs = Array.isArray(st.messages) ? st.messages : [];
  const outId = st.run && st.run.outputMessageId;
  if (!outId) return '';
  const message = msgs.find(x => x.id === outId && x.role === 'assistant');
  return message && typeof message.text === 'string' ? message.text : '';
}
const officialState = (task) => String(task.status && task.status.state || '').replace(/^TASK_STATE_/, '').toLowerCase().replaceAll('_', '-');
const OFFICIAL_TERMINAL = new Set(['completed', 'failed', 'rejected', 'canceled']);
const TERMINAL = new Set(['completed', 'failed', 'cancelled', 'interrupted']);

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
      if (rec.phase === 'delivery-unverified') {
        // Read official state after uncertain delivery. Never repeat a terminal mutation.
        const remote = await tool('get_task', { task_id: aid, history_length: TASK_HISTORY_LENGTH });
        const task = remote.task || remote;
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
        Object.assign(rec, { phase: 'unverified', requestText: text, error: { code: 'CREATE_OUTCOME_UNKNOWN' } }); saveState();
        const r = await runJob({ kind: 'create', requestKey: 'anet-' + aid.replace(/[^A-Za-z0-9_-]/g, '').slice(0, 100), title: ('Agent Network: ' + text.replace(/\s+/g, ' ')).slice(0, 100), text }, 90000);
        if (r && r.ok) {
          Object.assign(rec, { phase: 'running', cindyTaskId: r.taskId, runId: r.runId, acceptedAt: new Date().toISOString() }); saveState();
          await tool('reply_task', { task_id: aid, state: 'working', text: 'Accepted by Cindy; working on it.' }).catch((e) => log('working reply failed', e.message));
        } else {
          const code = (r && r.error && r.error.code) || 'UNKNOWN';
          // A host error may follow a successful create or send. Do not retry blindly.
          Object.assign(rec, { phase: 'unverified', error: { code }, cindyTaskId: r && r.taskId || null }); saveState();
        }
      } else if (rec.phase === 'running') {
        if (!status.mainConnected) continue;
        if (rec.lastCheck && Date.now() - rec.lastCheck < STATUS_EVERY_MS) continue;
        rec.lastCheck = Date.now();
        const officialResult = await tool('get_task', { task_id: aid, history_length: TASK_HISTORY_LENGTH });
        const official = officialResult.task || officialResult;
        if (OFFICIAL_TERMINAL.has(officialState(official))) {
          Object.assign(rec, { phase: 'unverified', error: { code: 'OFFICIAL_TASK_ENDED_HOST_NOT_STOPPED' }, officialState: official.status.state }); saveState(); continue;
        }
        if (!rec.requestMessageId || lastUserMessageId(official) !== rec.requestMessageId || !lastUserText(official).trim()) {
          Object.assign(rec, { phase: 'unverified', error: { code: 'FOLLOWUP_REQUIRES_OPERATOR' } }); saveState(); continue;
        }
        const st = await runJob({ kind: 'status', taskId: rec.cindyTaskId, runId: rec.runId }, 30000);
        if (!st || !st.ok || st.runError || !st.run || typeof st.run.status !== 'string') continue;
        const runStatus = st.run && st.run.status;
        rec.runStatus = runStatus || null;
        if (TERMINAL.has(runStatus)) {
          const out = pickOutput(st);
          if (runStatus === 'completed' && (!out || out.length > 60000)) {
            Object.assign(rec, { phase: 'unverified', error: { code: 'EXACT_OUTPUT_UNAVAILABLE' } }); saveState(); continue;
          }
          const ok = runStatus === 'completed' && out;
          Object.assign(rec, { phase: 'delivery-unverified', pendingReply: { task_id: aid, state: ok ? 'completed' : 'failed', text: ok ? out : ('Cindy run ended: ' + runStatus) } }); saveState();
          await tool('reply_task', rec.pendingReply);
          Object.assign(rec, { phase: ok ? 'replied' : 'failed', endedAt: new Date().toISOString() }); saveState();
        } else if (Date.now() - Date.parse(rec.acceptedAt) > MAX_RUN_MS) {
          // Observation deadline is not evidence that the host failed.
          Object.assign(rec, { phase: 'unverified', error: { code: 'RUN_TIMEOUT' } }); saveState();
        } else if (['running', 'working', 'queued', 'pending'].includes(runStatus) && (!rec.lastProgressAt || Date.now() - rec.lastProgressAt > 5 * 60 * 1000)) {
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
module.exports = { tick, state, status, sendToPeer, setTestDependencies(deps) { tool = deps.tool; runJob = deps.runJob; startMcp = () => {}; } };
