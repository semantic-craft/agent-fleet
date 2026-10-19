#!/usr/bin/env node
/**
 * ZCode receiver for the Agenvo `dispatch` connector: GET /health, POST /v1/jobs, GET /v1/jobs/<id>.
 * Plain HTTP on the configured host (never 0.0.0.0), source-IP allowlist plus a Bearer key.
 * Deployed with `fleet.mjs release zcode`; see adapters/zcode/README.md.
 */
import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';
import { DEFAULTS, HttpError, JobStore, checkAuth, checkCompat, loadKey, normalizeIp, publicJob, startJob, validateDispatch } from './core.mjs';

const cfg = { ...DEFAULTS };
const logDir = path.join(cfg.stateDir, 'logs');
fs.mkdirSync(logDir, { recursive: true, mode: 0o700 });
const logFile = path.join(logDir, 'dispatch.log');
const log = (lvl, msg) => {
  try { if (fs.statSync(logFile).size > 2e6) fs.renameSync(logFile, logFile + '.1'); } catch {}
  fs.appendFileSync(logFile, `${new Date().toISOString()} ${lvl} ${msg}\n`, { mode: 0o600 });
  if (lvl !== 'INFO') process.stderr.write(`[zcode] ${lvl} ${msg}\n`);
};
const logger = { info: (m) => log('INFO', m), warn: (m) => log('WARN', m), error: (m) => log('ERROR', m) };

const checks = checkCompat(cfg);
for (const e of checks.errors) logger.error(`compat check FAILED: ${e}`);
for (const w of checks.warnings) logger.warn(`compat check: ${w}`);
logger.info(`compat check ${checks.ok ? 'ok' : 'FAILED'} default_model=${checks.defaultModel}`);
let key;
try { key = loadKey(cfg.keyFile); } catch (e) { logger.error(`cannot start without a key: ${e.message}`); process.exit(1); }
const store = new JobStore(cfg);
store.reconcile();

const send = (res, code, obj) => { res.writeHead(code, { 'content-type': 'application/json' }); res.end(JSON.stringify(obj)); };
const readBody = (req, limit) => new Promise((resolve, reject) => {
  let raw = '';
  req.setEncoding('utf8');
  req.on('data', (d) => { raw += d; if (Buffer.byteLength(raw, 'utf8') > limit) { reject(new HttpError(413, 'body too large')); req.destroy(); } });
  req.on('end', () => { try { resolve(JSON.parse(raw)); } catch { reject(new HttpError(400, 'invalid JSON')); } });
});
const health = () => {
  const c = checkCompat(cfg);
  const app = cfg.appRunning();
  return { ok: c.ok, running: store.running.size, runner: app ? 'zcode-app-session' : 'zcode-app-unavailable', app_available: app, strict_runner: 'app',
    default_model: c.defaultModel, checks: { errors: c.errors, warnings: c.warnings } };
};

const server = http.createServer(async (req, res) => {
  const ip = normalizeIp(req.socket.remoteAddress);
  const url = new URL(req.url, 'http://x');
  if (!cfg.allowedIps.includes(ip) && ip !== '127.0.0.1') { logger.warn(`forbidden ip ${ip} ${req.method} ${url.pathname}`); return send(res, 403, { error: 'forbidden' }); }
  if (url.pathname === '/health' && req.method === 'GET') return send(res, 200, health());
  if (!checkAuth(req.headers.authorization, key)) { logger.warn(`auth failed from ${ip}`); return send(res, 401, { error: 'unauthorized' }); }
  try {
    if (req.method === 'POST' && url.pathname === '/v1/jobs') {
      const job = startJob(store, validateDispatch(cfg, await readBody(req, 100000)), logger);
      logger.info(`job ${job.id} accepted project=${job.project} model=${job.model}`);
      return send(res, 202, publicJob(job));
    }
    const m = /^\/v1\/jobs\/([0-9a-f]{16})$/.exec(url.pathname);
    if (req.method === 'GET' && m) {
      const job = store.get(m[1]);
      if (!job) throw new HttpError(404, 'no such job');
      return send(res, 200, publicJob(job));
    }
    send(res, 404, { error: 'not found' });
  } catch (e) {
    if (e.code !== 404) logger.warn(`rejected: ${e.message}`);
    send(res, Number.isInteger(e.code) && e.code >= 400 && e.code <= 599 ? e.code : 500, { error: e.message });
  }
});
if (!cfg.host) {
  logger.error('no bind host (set DISPATCH_HOST or "host" in ~/.config/zcode-dispatch/config.json, or bring tailscale up); exiting');
  setTimeout(() => process.exit(1), 30000);
} else server.listen(cfg.port, cfg.host, () => logger.info(`listening ${cfg.host}:${cfg.port} allowed=${cfg.allowedIps.join(',')}`));
server.on('error', (e) => { logger.error(`listen error ${e.code}`); setTimeout(() => process.exit(1), 30000); });
for (const s of ['SIGTERM', 'SIGINT']) process.on(s, () => { logger.info('stopping'); server.close(); process.exit(0); });
