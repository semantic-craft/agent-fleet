// Offline: temp config, fake tasks-index.sqlite, provider catalog fixtures; never touches ZCode.app or tailscale.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { DatabaseSync } from 'node:sqlite';

const FX = path.join(import.meta.dirname, 'fixtures');
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'fleet-zcode-'));
fs.writeFileSync(path.join(tmp, 'config.json'), JSON.stringify({ host: '192.0.2.20' }));
Object.assign(process.env, { ZCODE_DISPATCH_CONFIG: path.join(tmp, 'config.json'), ZCODE_DISPATCH_DISCOVERY: '0', ZCODE_DISPATCH_PROJECTS_ROOT: path.join(tmp, 'Projects'),
  ZCODE_PROVIDER_CATALOG: path.join(FX, 'zcode-builtin.json'), ZCODE_PROVIDER_CONFIG: path.join(FX, 'provider_config.json'), ZCODE_DISPATCH_DEFAULT_MODEL: '' });
for (const k of ['DISPATCH_HOST', 'DISPATCH_PORT', 'DISPATCH_ALLOWED_IPS']) delete process.env[k];
fs.mkdirSync(path.join(tmp, 'Projects', 'demo'), { recursive: true });
fs.mkdirSync(path.join(tmp, 'Projects', 'Finance'), { recursive: true });

const { BUILTIN_DEFAULT_MODEL, resolveModel, sameModel } = await import('../models.mjs');
const A = await import('../appsession.mjs');
const C = await import('../core.mjs');
const { resolveNetConfig } = await import('../config.mjs');

function fakeDb() {
  const p = path.join(fs.mkdtempSync(path.join(tmp, 'db-')), 'tasks-index.sqlite');
  const db = new DatabaseSync(p);
  const cols = (t) => A.REQUIRED_SCHEMA[t].map((c) => `${c} TEXT`).join(', ');
  db.exec(`CREATE TABLE automations (${cols('automations')}); CREATE TABLE automation_runs (${cols('automation_runs')}); CREATE TABLE tasks (${cols('tasks')});`);
  db.close();
  return p;
}

test('models: default GLM-5.3 Flash; unknown or disabled models are hard errors; $level suffix ignored when comparing', () => {
  assert.equal(resolveModel(undefined).id, BUILTIN_DEFAULT_MODEL);
  assert.equal(resolveModel('account:bigmodel-individual-coding-plan/GLM-5.3').id, 'account:bigmodel-individual-coding-plan/GLM-5.3');
  assert.throws(() => resolveModel('account:bigmodel-individual-coding-plan/GLM-9'), /not offered/);
  assert.throws(() => resolveModel('GLM-5.3-Flash'), /not a ZCode model id/);
  assert.ok(sameModel(`${BUILTIN_DEFAULT_MODEL}$max`, BUILTIN_DEFAULT_MODEL));
});

test('config: host from config file, allowlist defaults to the host itself', () => {
  assert.deepEqual(resolveNetConfig({ status: () => null }), { host: '192.0.2.20', port: 7791, allowedIps: ['192.0.2.20'] });
  assert.deepEqual(resolveNetConfig({ env: { DISPATCH_ALLOWED_IPS: '192.0.2.1, 192.0.2.2' }, file: {}, status: () => ({ Self: { TailscaleIPs: ['192.0.2.9'] } }) }).allowedIps, ['192.0.2.1', '192.0.2.2']);
  assert.equal(resolveNetConfig({ env: {}, file: {}, status: () => null }).host, null);
});

test('prompt head and title carry source and job id once', () => {
  assert.equal(A.withSource('[Project: a] do x', 'agenvo', '0123456789abcdef'), '[Project: a] [来源: agenvo] [job: 0123456789abcdef] do x');
  assert.equal(A.withSource('[Project: a] [来源: agenvo] [job: 0123456789abcdef] do x', 'agenvo', '0123456789abcdef'), '[Project: a] [来源: agenvo] [job: 0123456789abcdef] do x');
  assert.equal(A.summaryTitle('[Project: a] do x', 'agenvo', '0123456789abcdef'), '[agenvo #0123456789abcdef] do x');
  assert.equal(A.resultText({ searchable_text: 'P\nANS' }, 'P'), 'ANS');
});

test('enqueue writes a one-shot automation with ZCode\'s model_selection shape', () => {
  const dbPath = fakeDb();
  const id = A.enqueue({ cwd: '/w', text: '[Project: x] hi', model: BUILTIN_DEFAULT_MODEL, source: 'agenvo', jobId: '0123456789abcdef', dbPath });
  const row = new DatabaseSync(dbPath).prepare('SELECT * FROM automations WHERE automation_id=?').get(id);
  assert.deepEqual(JSON.parse(row.model_selection), { providerId: 'account:bigmodel-individual-coding-plan', modelId: 'GLM-5.3-Flash', options: { reasoningLevel: 'max' } });
  assert.equal(row.prompt, '[Project: x] [来源: agenvo] [job: 0123456789abcdef] hi');
  assert.equal(row.mode, 'yolo');
});

test('runInApp: picked-up session completes with the text after the prompt; model mismatch fails loudly', async () => {
  const dbPath = fakeDb();
  const finishWith = (model, text) => setTimeout(() => {
    const db = new DatabaseSync(dbPath);
    const a = db.prepare('SELECT automation_id, prompt FROM automations ORDER BY created_at DESC LIMIT 1').get();
    db.prepare("INSERT INTO automation_runs (automation_id, outcome, session_id, created_at) VALUES (?, 'succeeded', 'sess_1', '1')").run(a.automation_id);
    db.prepare('INSERT INTO tasks (task_id, searchable_text, model) VALUES (?, ?, ?)').run('sess_1', `${a.prompt}\n${text}`, model);
    db.close();
  }, 20);
  finishWith(`${BUILTIN_DEFAULT_MODEL}$max`, 'all good');
  const seen = [];
  const ok = await A.runInApp({ cwd: '/w', text: '[Project: x] hi', model: BUILTIN_DEFAULT_MODEL, source: 'agenvo', jobId: '0123456789abcdef', pollMs: 5, dbPath, registerProject: () => {}, onSession: (s) => seen.push(s) });
  assert.equal(ok.outcome, 'succeeded'); assert.equal(ok.result, 'all good'); assert.deepEqual(seen, ['sess_1']);
  const dbPath2 = fakeDb();
  setTimeout(() => {
    const db = new DatabaseSync(dbPath2);
    const a = db.prepare('SELECT automation_id FROM automations').get();
    db.prepare("INSERT INTO automation_runs (automation_id, outcome, session_id, created_at) VALUES (?, 'running', 'sess_2', '1')").run(a.automation_id);
    db.prepare("INSERT INTO tasks (task_id, model) VALUES ('sess_2', 'account:bigmodel-individual-coding-plan/GLM-5.3')").run();
    db.close();
  }, 20);
  const bad = await A.runInApp({ cwd: '/w', text: '[Project: x] hi', model: BUILTIN_DEFAULT_MODEL, source: 'agenvo', pollMs: 5, dbPath: dbPath2, registerProject: () => {} });
  assert.equal(bad.outcome, 'failed'); assert.match(bad.error, /model mismatch/);
});

test('checkSchema flags a changed scheduler schema', () => {
  const p = path.join(fs.mkdtempSync(path.join(tmp, 's-')), 'x.sqlite');
  const db = new DatabaseSync(p); db.exec('CREATE TABLE automations (automation_id TEXT); CREATE TABLE automation_runs (automation_id TEXT); CREATE TABLE tasks (task_id TEXT)'); db.close();
  assert.throws(() => A.checkSchema(p), /schema changed \(missing: automations\.title/);
  assert.equal(A.checkSchema(fakeDb()), true);
});

test('Projects placement: registered left alone, unregistered opens the deep link, unknown shape skipped', () => {
  const f = path.join(fs.mkdtempSync(path.join(tmp, 'set-')), 'setting.json');
  fs.writeFileSync(f, JSON.stringify({ recentProjects: [{ path: '/p/a' }] }));
  const opened = [];
  assert.equal(A.ensureProjectRegistered('/p/a', { settingsPath: f, opener: (u) => opened.push(u), warn: () => {} }), 'registered');
  assert.equal(A.ensureProjectRegistered('/p/b c', { settingsPath: f, opener: (u) => opened.push(u), warn: () => {} }), 'requested');
  assert.deepEqual(opened, ['zcode://workspace/open?path=%2Fp%2Fb%20c']);
  fs.writeFileSync(f, JSON.stringify({ recentProjects: 'x' }));
  assert.equal(A.ensureProjectRegistered('/p/a', { settingsPath: f, warn: () => {} }), 'unknown-schema');
});

const cfg = (over = {}) => ({ ...C.DEFAULTS, stateDir: fs.mkdtempSync(path.join(tmp, 'st-')), appRunning: () => true, checkSchema: () => true, ...over });

test('auth is exact; validation accepts only agenvo + runner app and refuses callbacks, bad projects and tags', () => {
  const key = Buffer.from('k'.repeat(64));
  assert.ok(C.checkAuth(`Bearer ${'k'.repeat(64)}`, key)); assert.ok(!C.checkAuth(`Bearer ${'k'.repeat(63)}x`, key)); assert.ok(!C.checkAuth(undefined, key));
  const c = cfg();
  const spec = C.validateDispatch(c, { project: 'demo', text: '[Project: demo] hi', source: 'agenvo', runner: 'app', mode: 'yolo' });
  assert.equal(spec.source, 'agenvo'); assert.equal(spec.model, `${BUILTIN_DEFAULT_MODEL}$max`);
  for (const [body, re] of [
    [{ project: 'demo', text: '[Project: demo] hi', source: 'grok-bot' }, /source must be "agenvo"/],
    [{ project: 'demo', text: '[Project: demo] hi', runner: 'auto' }, /runner must be "app"/],
    [{ project: 'demo', text: '[Project: demo] hi', callback_peer: 'x' }, /not supported/],
    [{ project: 'demo', text: '[Project: other] hi' }, /must start with/],
    [{ project: '../x', text: '[Project: ../x] hi' }, /plain folder/],
    [{ project: 'Finance', text: '[Project: Finance] hi' }, /not allowed/],
    [{ project: 'demo', text: '[Project: demo] hi', job_id: 'XYZ' }, /job_id/],
  ]) assert.throws(() => C.validateDispatch(c, body), re, JSON.stringify(body));
});

test('startJob: 503 without ZCode.app (no record); completed job records runner app, session and result; job_id is idempotent', async () => {
  const off = new C.JobStore(cfg({ appRunning: () => false }));
  const spec = C.validateDispatch(off.cfg, { project: 'demo', text: '[Project: demo] hi', job_id: 'aaaaaaaaaaaaaaaa' });
  assert.throws(() => C.startJob(off, spec, {}), (e) => e.code === 503);
  assert.equal(off.get('aaaaaaaaaaaaaaaa'), null);
  const calls = [];
  const store = new C.JobStore(cfg({ runInApp: async (a) => { calls.push(a); a.onSession('sess_9'); return { outcome: 'succeeded', sessionId: 'sess_9', result: 'done', model: `${BUILTIN_DEFAULT_MODEL}$max` }; } }));
  const job = C.startJob(store, spec, {});
  assert.equal(job.state, 'running');
  await job.done;
  const saved = store.get('aaaaaaaaaaaaaaaa');
  assert.deepEqual([saved.state, saved.runner, saved.session_id, saved.result, saved.exit_code], ['completed', 'app', 'sess_9', 'done', 0]);
  assert.equal(calls[0].source, 'agenvo'); assert.equal(calls[0].jobId, 'aaaaaaaaaaaaaaaa');
  assert.equal(C.startJob(store, spec, {}).state, 'completed');
  assert.throws(() => C.startJob(store, { ...spec, text: '[Project: demo] other' }, {}), (e) => e.code === 409);
  assert.equal(C.publicJob(saved).text, undefined);
});

test('startJob: not picked up or a thrown runner is an explicit failure; restart marks running jobs failed', async () => {
  const store = new C.JobStore(cfg({ runInApp: async () => ({ outcome: 'not_picked_up', error: 'scheduler did not pick up', sessionId: null }) }));
  const j = C.startJob(store, C.validateDispatch(store.cfg, { project: 'demo', text: '[Project: demo] hi' }), {});
  await j.done;
  assert.deepEqual([store.get(j.id).state, store.get(j.id).runner, store.get(j.id).error], ['failed', null, 'scheduler did not pick up']);
  const boom = new C.JobStore(cfg({ runInApp: async () => { throw new Error('db locked'); } }));
  const b = C.startJob(boom, C.validateDispatch(boom.cfg, { project: 'demo', text: '[Project: demo] hi' }), {});
  await b.done;
  assert.equal(boom.get(b.id).error, 'db locked');
  const hang = new C.JobStore(cfg({ runInApp: () => new Promise(() => {}) }));
  const h = C.startJob(hang, C.validateDispatch(hang.cfg, { project: 'demo', text: '[Project: demo] hi' }), {});
  new C.JobStore(hang.cfg).reconcile();
  assert.equal(hang.get(h.id).state, 'failed');
});
