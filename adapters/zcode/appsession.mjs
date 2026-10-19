/**
 * Visible ZCode.app session runner: hands a job to the running app through its own automation
 * scheduler (tasks-index.sqlite `automations`, one one-shot row). The app's scheduler (polls every
 * 20 s, BEGIN IMMEDIATE claims) forwards it to the Host, which creates a normal sidebar session in the
 * project, on the desktop login, with model_selection {providerId, modelId, options.reasoningLevel}
 * (the shape ZCode itself writes), mode yolo. After pickup the session's tasks.model is compared with
 * the requested id; a mismatch fails the job. There is no CLI or headless fallback (ADR-0004).
 *
 * Source refs (zai-org/ZCode@aac4755): desktop/src/scheduler/index.ts, desktop/src/host/index.ts
 * dispatchCronRun, services/src/session/automationRepo.ts create(). Credentials are never read.
 */
import { DatabaseSync } from 'node:sqlite';
import { execFileSync } from 'node:child_process';
import { randomUUID } from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { parseModel, sameModel } from './models.mjs';

export const DB_PATH = () => process.env.ZCODE_TASKS_DB || path.join(os.homedir(), '.zcode', 'v2', 'tasks-index.sqlite');
export const SETTINGS_PATH = () => process.env.ZCODE_SETTINGS || path.join(os.homedir(), '.zcode', 'v2', 'setting.json');
const MODES = new Set(['default', 'yolo', 'guarded', 'plan', 'edit', 'build']);
const JOB_ID_RE = /^[0-9a-f]{16}$/;

export function appRunning() {
  try { execFileSync('/usr/bin/pgrep', ['-x', 'ZCode'], { stdio: 'ignore' }); return true; } catch { return false; }
}

function open(dbPath = DB_PATH()) {
  const db = new DatabaseSync(dbPath);
  db.exec('PRAGMA busy_timeout = 5000');
  return db;
}

/** "[Project: X] body" -> "[Project: X] [来源: src] [job: id] body"; the prompt head is also the session title. */
export function withSource(text, source, jobId) {
  let out = String(text ?? '');
  if (source && !/^\s*\[Project: [^\]]+\]\s*\[来源:/.test(out)) out = out.replace(/^(\s*\[Project: [^\]\n]+\])\s*/, `$1 [来源: ${source}] `);
  if (JOB_ID_RE.test(jobId ?? '') && !/\[job:\s*[0-9a-f]{16}\]/i.test(out)) {
    out = /^\s*\[Project: [^\]]+\]\s*\[来源:[^\]]+\]/.test(out)
      ? out.replace(/^(\s*\[Project: [^\]\n]+\]\s*\[来源:[^\]\n]+\])\s*/, `$1 [job: ${jobId}] `)
      : /^\s*\[Project: [^\]]+\]/.test(out) ? out.replace(/^(\s*\[Project: [^\]\n]+\])\s*/, `$1 [job: ${jobId}] `) : `[job: ${jobId}] ${out}`;
  }
  return out;
}

/** Automation row title: "[source #job] first 40 chars". */
export function summaryTitle(text, source, jobId) {
  const body = String(text || '').replace(/^\s*\[Project: [^\]\n]+\]\s*(\[来源[^\]]*\]\s*)?(\[job:\s*[0-9a-f]{16}\]\s*)?/i, '').replace(/\s+/g, ' ').trim();
  const tag = JOB_ID_RE.test(jobId ?? '') ? `${source || 'dispatch'} #${jobId}` : `${source || 'dispatch'}`;
  return `[${tag}] ${body.slice(0, 40)}${body.length > 40 ? '…' : ''}`;
}

export function enqueue({ cwd, text, mode = 'yolo', model, source, jobId, now = Date.now(), dbPath }) {
  if (!MODES.has(mode)) throw new Error(`mode ${mode} not allowed`);
  const id = `automation-zcode-dispatch-${randomUUID()}`;
  const d = new Date(now);
  const cron = `${d.getMinutes()} ${d.getHours()} ${d.getDate()} ${d.getMonth() + 1} *`;
  const sel = model ? (({ providerId, modelId, reasoningLevel }) => JSON.stringify({ providerId, modelId, options: { reasoningLevel } }))(parseModel(model)) : 'null';
  const db = open(dbPath);
  try {
    db.exec('BEGIN IMMEDIATE');
    try {
      db.prepare(`INSERT INTO automations (automation_id,title,cron_expr,prompt,model,provider,model_selection,
        workspace_key,workspace_path,location_kind,recurring,max_runs,enabled,lifecycle_status,next_run_at,
        running,dispatch_status,dispatch_attempts,mode,created_at,updated_at)
        VALUES (?,?,?,?,NULL,NULL,?,?,?,'local',0,1,1,'active',?,0,'idle',0,?,?,?)`)
        .run(id, summaryTitle(text, source, jobId), cron, withSource(text, source, jobId), sel, cwd, cwd, now, mode, now, now);
      db.exec('COMMIT');
    } catch (e) { db.exec('ROLLBACK'); throw e; }
  } finally { db.close(); }
  return id;
}

export function poll(id, dbPath) {
  const db = open(dbPath);
  try {
    const a = db.prepare('SELECT lifecycle_status,dispatch_status,last_error,dispatch_attempts FROM automations WHERE automation_id=?').get(id);
    const r = db.prepare('SELECT dispatch_status,outcome,session_id,error,model_selection FROM automation_runs WHERE automation_id=? ORDER BY created_at DESC LIMIT 1').get(id);
    const task = r?.session_id ? db.prepare('SELECT title,task_status,searchable_text,model FROM tasks WHERE task_id=?').get(r.session_id) ?? null : null;
    return { automation: a ?? null, run: r ?? null, task };
  } finally { db.close(); }
}

/** Drop our finished one-shot row (the app keeps at most 20 automations); the session itself stays. */
export function cleanup(id, dbPath) {
  if (!id.startsWith('automation-zcode-dispatch-')) return;
  const db = open(dbPath);
  try { db.prepare("DELETE FROM automations WHERE automation_id=? AND running=0 AND lifecycle_status!='active'").run(id); } finally { db.close(); }
}

export function resultText(task, prompt) {
  if (!task?.searchable_text) return null;
  let s = task.searchable_text;
  const i = s.indexOf(prompt.trim());
  if (i >= 0) s = s.slice(i + prompt.trim().length);
  return s.trim() || null;
}

/**
 * ZCode's Projects view lists only workspaces in setting.json `recentProjects`, so register the folder
 * through ZCode's own deep link `zcode://workspace/open?path=<abs>`; ZCode asks once per folder.
 * An unrecognised settings shape is warned about and skipped (the session lands under Automation).
 */
export function projectRegistered(cwd, settingsPath = SETTINGS_PATH()) {
  let rp;
  try { rp = JSON.parse(fs.readFileSync(settingsPath, 'utf8')).recentProjects; } catch { return null; }
  if (!Array.isArray(rp)) return null;
  const paths = rp.map((e) => (typeof e === 'string' ? e : typeof e?.path === 'string' ? e.path : null));
  if (paths.some((p) => p === null)) return null;
  return paths.some((p) => path.resolve(p) === path.resolve(cwd));
}
export const projectDeepLink = (cwd) => `zcode://workspace/open?path=${encodeURIComponent(path.resolve(cwd))}`;
export function ensureProjectRegistered(cwd, { settingsPath, opener = (url) => execFileSync('/usr/bin/open', [url], { stdio: 'ignore' }), warn = (m) => process.stderr.write(m + '\n') } = {}) {
  const r = projectRegistered(cwd, settingsPath);
  if (r === true) return 'registered';
  if (r === null) { warn('[zcode] setting.json recentProjects shape not recognised; skipping Projects registration'); return 'unknown-schema'; }
  try { opener(projectDeepLink(cwd)); } catch (e) { warn(`[zcode] could not open the ZCode deep link (${e.message})`); return 'open-failed'; }
  return 'requested';
}

/** Run to completion: {automationId, sessionId, model, outcome, result, error}. */
export async function runInApp({ cwd, text, mode, model, source, jobId, timeoutSeconds = 1800, pickupSeconds = 90, pollMs = 1000, dbPath, onSession = () => {}, registerProject = ensureProjectRegistered }) {
  registerProject(cwd);
  const id = enqueue({ cwd, text, mode, model, source, jobId, dbPath });
  const prompt = withSource(text, source, jobId);
  const t0 = Date.now();
  let sid = null;
  for (;;) {
    await new Promise((r) => setTimeout(r, pollMs));
    const s = poll(id, dbPath);
    if (s.run?.session_id && s.run.session_id !== sid) { sid = s.run.session_id; onSession(sid); }
    if (model && s.task?.model && !sameModel(s.task.model, model))
      return { automationId: id, sessionId: sid, model: s.task.model, outcome: 'failed', result: null,
        error: `model mismatch: requested ${model}, ZCode session ${sid} reports ${s.task.model} (session left running in ZCode.app)` };
    if (s.run?.outcome && s.run.outcome !== 'running') {
      cleanup(id, dbPath);
      const ok = s.run.outcome === 'succeeded';
      return { automationId: id, sessionId: sid, model: s.task?.model ?? null, outcome: s.run.outcome, result: resultText(s.task, prompt),
        error: ok ? null : s.run.error || s.automation?.last_error || s.run.outcome };
    }
    const age = (Date.now() - t0) / 1000;
    if (!s.run && age > pickupSeconds) {
      const db = open(dbPath);
      try { db.prepare('DELETE FROM automations WHERE automation_id=? AND running=0 AND dispatch_attempts=0 AND NOT EXISTS (SELECT 1 FROM automation_runs WHERE automation_id=?)').run(id, id); } finally { db.close(); }
      return { automationId: id, sessionId: null, outcome: 'not_picked_up', result: null, error: 'ZCode.app scheduler did not pick up the job within ' + pickupSeconds + ' s' };
    }
    if (age > timeoutSeconds) return { automationId: id, sessionId: sid, outcome: 'timeout', result: null, error: 'timeout (session keeps running in ZCode.app)' };
  }
}

/** The scheduler internals this runner reads and writes (ZCode.app 3.15.x layout). */
export const REQUIRED_SCHEMA = {
  automations: ['automation_id', 'title', 'cron_expr', 'prompt', 'model', 'provider', 'model_selection', 'workspace_key', 'workspace_path',
    'location_kind', 'recurring', 'max_runs', 'enabled', 'lifecycle_status', 'next_run_at', 'running', 'dispatch_status', 'dispatch_attempts',
    'mode', 'created_at', 'updated_at', 'last_error'],
  automation_runs: ['automation_id', 'dispatch_status', 'outcome', 'session_id', 'error', 'created_at', 'model_selection'],
  tasks: ['task_id', 'title', 'task_status', 'searchable_text', 'model'],
};
export function checkSchema(dbPath = DB_PATH()) {
  if (!fs.existsSync(dbPath)) throw new Error(`ZCode tasks index not found at ${dbPath}`);
  const db = open(dbPath);
  try {
    const missing = [];
    for (const [t, cols] of Object.entries(REQUIRED_SCHEMA)) {
      const have = new Set(db.prepare(`PRAGMA table_info(${t})`).all().map((r) => r.name));
      if (!have.size) { missing.push(`table ${t}`); continue; }
      for (const c of cols) if (!have.has(c)) missing.push(`${t}.${c}`);
    }
    if (missing.length) throw new Error(`ZCode.app scheduler schema changed (missing: ${missing.join(', ')}); update adapters/zcode/appsession.mjs`);
    return true;
  } finally { db.close(); }
}
