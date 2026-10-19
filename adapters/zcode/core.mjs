/**
 * ZCode receiver core: validation, Bearer auth, job store, and the visible-session runner.
 * Callers are the Agenvo `dispatch` connector only (source "agenvo", runner "app"): every job is a
 * visible ZCode.app session or an explicit failure, and results are polled via GET /v1/jobs/<id>.
 */
import { randomBytes, timingSafeEqual } from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { appRunning, checkSchema, runInApp } from './appsession.mjs';
import { denyProjectsRe, projectsRoot, resolveNetConfig } from './config.mjs';
import { defaultModel, resolveModel } from './models.mjs';

const HOME = os.homedir();
export const DEFAULTS = {
  get host() { return resolveNetConfig().host; },
  get port() { return resolveNetConfig().port; },
  get allowedIps() { return resolveNetConfig().allowedIps; },
  keyFile: path.join(HOME, '.config', 'zcode-dispatch', 'key'),
  stateDir: path.join(HOME, '.local', 'state', 'zcode-dispatch'),
  get projectsRoot() { return projectsRoot(); },
  get defaultModel() { return defaultModel(); },
  mode: 'yolo',
  maxConcurrent: 2,
  defaultTimeoutSeconds: 1800,
  maxTimeoutSeconds: 7200,
  maxTextChars: 20000,
  maxResultChars: 100000,
  appRunning,
  checkSchema: () => checkSchema(),
  runInApp,
};

const PROJECT_RE = /^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$/;
const TAG_RE = /^\s*\[Project: ([^\]\n]+)\]/;
const JOB_RE = /^[0-9a-f]{16}$/;
const MODES = ['plan', 'edit', 'build', 'guarded', 'yolo'];

export class HttpError extends Error { constructor(code, msg) { super(msg); this.code = code; } }

export function loadKey(file) {
  const st = fs.statSync(file);
  if ((st.mode & 0o077) !== 0) throw new Error(`key file ${file} must be chmod 600`);
  const key = fs.readFileSync(file, 'utf8').trim();
  if (key.length < 32) throw new Error('key too short');
  return Buffer.from(key);
}
export function checkAuth(header, key) {
  const m = /^Bearer (\S+)$/.exec(header || '');
  if (!m) return false;
  const got = Buffer.from(m[1]);
  return got.length === key.length && timingSafeEqual(got, key);
}
export const normalizeIp = (ip) => (ip || '').replace(/^::ffff:/, '');

export function projectDir(cfg, project) {
  if (typeof project !== 'string' || !PROJECT_RE.test(project)) throw new HttpError(400, 'project must be a plain folder name under ~/Projects');
  if (denyProjectsRe().test(project)) throw new HttpError(403, 'project not allowed');
  const root = fs.realpathSync(cfg.projectsRoot);
  let cwd;
  try { cwd = fs.realpathSync(path.join(root, project)); } catch { throw new HttpError(404, `no such project folder ~/Projects/${project}`); }
  if (path.dirname(cwd) !== root || !fs.statSync(cwd).isDirectory() || denyProjectsRe().test(cwd)) throw new HttpError(403, 'project folder must be a direct child of ~/Projects');
  return cwd;
}

export function validateDispatch(cfg, body) {
  if (!body || typeof body !== 'object') throw new HttpError(400, 'JSON object body required');
  const { project, text } = body;
  const cwd = projectDir(cfg, project);
  if (typeof text !== 'string' || !text.trim() || text.length > cfg.maxTextChars) throw new HttpError(400, `text required (max ${cfg.maxTextChars} chars)`);
  if (TAG_RE.exec(text)?.[1] !== project) throw new HttpError(400, `text must start with "[Project: ${project}]"`);
  const timeoutSeconds = body.timeout_seconds ?? cfg.defaultTimeoutSeconds;
  if (!Number.isInteger(timeoutSeconds) || timeoutSeconds < 10 || timeoutSeconds > cfg.maxTimeoutSeconds) throw new HttpError(400, `timeout_seconds must be 10..${cfg.maxTimeoutSeconds}`);
  const mode = body.mode ?? cfg.mode;
  if (!MODES.includes(mode)) throw new HttpError(400, `mode must be one of ${MODES.join(', ')}`);
  const source = body.source ?? 'agenvo';
  if (source !== 'agenvo') throw new HttpError(400, 'source must be "agenvo" (the only caller is the Agenvo dispatch connector)');
  if ((body.runner ?? 'app') !== 'app') throw new HttpError(400, 'runner must be "app" (visible ZCode.app session; there is no CLI/headless path)');
  for (const k of ['callback_peer', 'callback_url']) if (body[k] != null) throw new HttpError(400, `${k} is not supported; poll GET /v1/jobs/<id>`);
  const jobId = body.job_id == null ? undefined : String(body.job_id);
  if (jobId !== undefined && !JOB_RE.test(jobId)) throw new HttpError(400, 'job_id must be 16 lowercase hex chars');
  if (body.model != null && (typeof body.model !== 'string' || !body.model.trim())) throw new HttpError(400, 'model must be a ZCode model id string');
  let model;
  try { model = resolveModel(body.model, { available: cfg.availableModels?.() }).selection; } catch (e) { throw new HttpError(400, e.message); }
  return { project, cwd, text, timeoutSeconds, mode, source, jobId, model };
}

export class JobStore {
  constructor(cfg) {
    this.cfg = cfg;
    this.dir = path.join(cfg.stateDir, 'jobs');
    fs.mkdirSync(this.dir, { recursive: true, mode: 0o700 });
    this.running = new Set();
  }
  file(id) { return path.join(this.dir, `${id}.json`); }
  save(job) { const t = this.file(job.id) + '.tmp'; fs.writeFileSync(t, JSON.stringify(job, null, 2), { mode: 0o600 }); fs.renameSync(t, this.file(job.id)); }
  get(id) {
    if (!JOB_RE.test(id || '')) return null;
    try { return JSON.parse(fs.readFileSync(this.file(id), 'utf8')); } catch (e) { if (e.code === 'ENOENT') return null; throw new HttpError(409, 'stored job unreadable; reconcile before retry'); }
  }
  /** A restart loses the in-process poller: mark leftovers failed (the ZCode session itself keeps running). */
  reconcile() {
    for (const n of fs.readdirSync(this.dir)) {
      if (!n.endsWith('.json')) continue;
      const job = this.get(n.slice(0, -5));
      if (job?.state === 'running') { Object.assign(job, { state: 'failed', error: 'receiver restarted while job was running', finished_at: new Date().toISOString() }); this.save(job); }
    }
  }
}

/** Accept a job and run it as a visible ZCode.app session; resolves `job.done` when finished. */
export function startJob(store, spec, logger = console) {
  const cfg = store.cfg;
  if (spec.jobId) {
    const existing = store.get(spec.jobId);
    if (existing) {
      if (existing.project !== spec.project || existing.text !== spec.text) throw new HttpError(409, 'job_id already used for a different request');
      return existing;
    }
  }
  if (store.running.size >= cfg.maxConcurrent) throw new HttpError(429, `busy: ${store.running.size} jobs running`);
  if (!cfg.appRunning()) throw new HttpError(503, 'ZCode.app is not running (runner=app has no CLI/headless fallback)');
  try { cfg.checkSchema(); } catch (e) { throw new HttpError(503, e.message); }
  const id = spec.jobId || randomBytes(8).toString('hex');
  const job = { id, project: spec.project, cwd: spec.cwd, mode: spec.mode, source: spec.source, model: spec.model, timeout_seconds: spec.timeoutSeconds,
    runner_requested: 'app', runner: null, state: 'running', created_at: new Date().toISOString(), text: spec.text };
  store.save(job);
  store.running.add(id);
  const done = cfg.runInApp({ cwd: spec.cwd, text: spec.text, mode: spec.mode, model: spec.model, source: spec.source, jobId: id, timeoutSeconds: spec.timeoutSeconds,
    onSession: (sid) => { job.session_id = sid; job.runner = 'app'; store.save(job); } })
    .catch((e) => ({ outcome: 'failed', error: e.message, result: null, sessionId: job.session_id ?? null }))
    .then((r) => {
      store.running.delete(id);
      const ok = r.outcome === 'succeeded';
      Object.assign(job, { state: ok ? 'completed' : 'failed', runner: r.sessionId ? 'app' : null, session_id: r.sessionId ?? job.session_id ?? null,
        ...(r.model ? { session_model: r.model } : {}), exit_code: ok ? 0 : 1, finished_at: new Date().toISOString(),
        result: r.result == null ? null : String(r.result).slice(0, cfg.maxResultChars), error: ok ? null : String(r.error ?? r.outcome).slice(0, 2000) });
      store.save(job);
      logger.info?.(`job ${id} ${job.state}${ok ? '' : ` error=${JSON.stringify(job.error.slice(0, 300))}`}`);
      return job;
    });
  Object.defineProperty(job, 'done', { value: done, enumerable: false });
  return job;
}

export function publicJob(job) { const { text, cwd, ...rest } = job; return { ...rest, text_preview: text.slice(0, 200) }; }

/** Startup compatibility check -> {ok, errors[], warnings[]}; logged at start and served on /health. */
export function checkCompat(cfg) {
  const errors = []; const warnings = [];
  try { loadKey(cfg.keyFile); } catch (e) { errors.push(`key: ${e.code === 'ENOENT' ? `missing ${cfg.keyFile} (openssl rand -hex 32 > it; chmod 600)` : e.message}`); }
  if (!cfg.host) errors.push('host: no bind host (set "host" in ~/.config/zcode-dispatch/config.json or DISPATCH_HOST; tailscale discovery found none)');
  if (!cfg.allowedIps?.length) errors.push('allowedIps: empty, every caller is refused');
  try { fs.accessSync(cfg.projectsRoot); } catch { errors.push(`projectsRoot: ${cfg.projectsRoot} does not exist`); }
  try { cfg.checkSchema(); } catch (e) { warnings.push(`zcode: ${e.message}`); }
  try { const m = resolveModel(undefined, { available: cfg.availableModels?.() }); if (m.id !== cfg.defaultModel) warnings.push(`model: default resolved to ${m.id}`); }
  catch (e) { errors.push(`model: ${e.message.replace(/^\[zcode-dispatch\] /, '')}`); }
  return { ok: errors.length === 0, defaultModel: cfg.defaultModel, errors, warnings };
}
