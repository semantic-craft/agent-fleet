import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import http from 'node:http';
import os from 'node:os';
import path from 'node:path';
import * as bridge from '../native-bridge.js';
import { createHandler, listenNativeBridge } from '../native-bridge.js';

function call(socketPath, urlPath, body) {
  return new Promise((resolve, reject) => {
    const req = http.request({ socketPath, path: urlPath, method: body ? 'POST' : 'GET', headers: { 'content-type': 'application/json' } }, async (res) => {
      let raw = '';
      for await (const c of res) raw += c;
      resolve({ status: res.statusCode, type: res.headers['content-type'], raw });
    });
    req.on('error', reject);
    req.end(body ? JSON.stringify(body) : undefined);
  });
}

test('native bridge forwards native endpoints through the Typert gateway with one owner Peer', async (t) => {
  const dir = fs.mkdtempSync(path.join('/tmp', 'dshnb-'));
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));
  const socket = path.join(dir, 'state', 'native.sock');
  const seen = [];
  const peer = { principal: { kind: 'owner' } };
  const gateway = {
    async invoke(req) {
      seen.push(req);
      if (req.method === 'prompt') throw Object.assign(new Error('session "x" not found'), { code: 'session/not-found', details: { sessionId: 'x' } });
      return { items: [{ sessionId: 's1' }] };
    },
    async stream(req) {
      seen.push(req);
      return (async function* () { yield { type: 'baseline' }; yield { type: 'projection', sessionId: 's1' }; })();
    },
  };
  // Regression: the Host creates files concurrently, so the bridge must never change the process umask,
  // not even briefly while it listens (a 0o177 umask made DSH create directories without the x bit).
  const realUmask = process.umask;
  const umaskSets = [];
  process.umask = (...a) => { if (a.length) umaskSets.push(a[0]); return realUmask.apply(process, a); };
  let close;
  try { close = await listenNativeBridge(socket, createHandler({ getGateway: () => gateway, getPeer: () => peer, logger: {} }), {}); } finally { process.umask = realUmask; }
  t.after(close);
  assert.deepEqual(umaskSets, []);
  assert.equal(fs.statSync(socket).mode & 0o777, 0o600);
  assert.equal(fs.statSync(path.dirname(socket)).mode & 0o777, 0o700);

  const health = JSON.parse((await call(socket, '/health')).raw);
  assert.equal(health.ok, true);

  const ok = JSON.parse((await call(socket, '/api/session/list', { args: { _request: {} } })).raw);
  assert.deepEqual(ok, { ok: true, value: { items: [{ sessionId: 's1' }] } });
  assert.equal(seen[0].namespace, 'session');
  assert.equal(seen[0].method, 'list');
  assert.deepEqual(seen[0].args, { _request: {} });
  assert.equal(seen[0].peer, peer);
  assert.ok(seen[0].signal instanceof AbortSignal);

  const refused = JSON.parse((await call(socket, '/api/session/prompt', { args: { request: {} } })).raw);
  assert.deepEqual(refused, { ok: false, error: { code: 'session/not-found', message: 'session "x" not found', details: { sessionId: 'x' } } });

  const stream = await call(socket, '/stream/session/control', { args: {} });
  assert.match(stream.type, /ndjson/);
  assert.deepEqual(stream.raw.trim().split('\n').map((l) => JSON.parse(l)), [
    { item: { type: 'baseline' } }, { item: { type: 'projection', sessionId: 's1' } }, { end: true },
  ]);

  assert.equal((await call(socket, '/api/session/list', { nope: 1 })).status, 400);
  assert.equal((await call(socket, '/api/../etc', { args: {} })).status, 404);
});

test('native bridge refuses to replace a socket another process still serves', async (t) => {
  const dir = fs.mkdtempSync(path.join('/tmp', 'dshnb-'));
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));
  const socket = path.join(dir, 'native.sock');
  const handler = createHandler({ getGateway: () => undefined, getPeer: () => undefined, logger: {} });
  const close = await listenNativeBridge(socket, handler, {});
  t.after(close);
  await assert.rejects(listenNativeBridge(socket, handler, {}), /already served/);
  // Without a live gateway the bridge answers 503 instead of guessing.
  assert.equal((await call(socket, '/api/session/list', { args: {} })).status, 503);
});

test('native bridge leaves the Peer to the Gateway on DSH versions without createPeer', async (t) => {
  const dir = fs.mkdtempSync(path.join('/tmp', 'dshnb-'));
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));
  const socket = path.join(dir, 'native.sock');
  const seen = [];
  const gateway = { async invoke(req) { seen.push(req); return { items: [] }; } };
  const close = await listenNativeBridge(socket, createHandler({ getGateway: () => gateway, getPeer: () => undefined, logger: {} }), {});
  t.after(close);
  const health = JSON.parse((await call(socket, '/health')).raw);
  assert.equal(health.ok, true);
  assert.equal(health.peer, 'gateway-operator');
  const ok = JSON.parse((await call(socket, '/api/session/list', { args: { _request: {} } })).raw);
  assert.deepEqual(ok, { ok: true, value: { items: [] } });
  assert.equal('peer' in seen[0], false);
});

async function workspaceFixture(t) {
  const dir = fs.mkdtempSync(path.join('/tmp', 'dshws-'));
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));
  const realPath = fs.realpathSync(dir);
  const workspaces = [];
  const registry = {
    async resolveByPath(p) { return workspaces.find((w) => w.path === fs.realpathSync(p)); },
    list() { return workspaces; },
    async readSessionHeader(id) {
      if (id !== 's1') throw new Error('session persistence holds no such session');
      return { cwd: dir };
    },
    async create(p, title) {
      const workspace = { id: 'w1', path: fs.realpathSync(p), title, sessionIds: [],
        async attachSession(id) { if (!this.sessionIds.includes(id)) this.sessionIds.push(id); } };
      workspaces.push(workspace);
      return workspace;
    },
  };
  let current = registry;
  const socket = path.join(dir, 'native.sock');
  t.after(await listenNativeBridge(socket, createHandler({ getGateway: () => undefined,
    getPeer: () => undefined, getWorkspaceRegistry: () => current, logger: {} }), {}));
  return { dir, realPath, registry, workspaces, setRegistry(value) { current = value; },
    async attach(args) { const r = await call(socket, '/bridge/workspace/attachSession', { args }); return { status: r.status, body: JSON.parse(r.raw) }; },
    async health() { return JSON.parse((await call(socket, '/health')).raw); } };
}

test('bridge attaches idempotently to an existing project without renaming it', async (t) => {
  const f = await workspaceFixture(t);
  await f.registry.create(f.dir, 'Original');
  for (let i = 0; i < 2; i++) {
    assert.deepEqual(await f.attach({ path: f.dir, sessionId: 's1', title: 'Ignored' }), {
      status: 200, body: { ok: true, value: { workspace: {
        workspaceId: 'w1', path: f.realPath, title: 'Original', sessionIds: ['s1'],
      }, created: false } },
    });
  }
  assert.equal(f.workspaces.length, 1);
});

for (const naming of ['default', 'collision', 'explicit']) {
  test(`bridge creates a project with ${naming} title`, async (t) => {
    const f = await workspaceFixture(t);
    const base = path.basename(f.realPath);
    if (naming === 'collision') f.workspaces.push({ path: '/other', title: base });
    const result = await f.attach({ path: f.dir, sessionId: 's1', ...(naming === 'explicit' ? { title: 'Chosen' } : {}) });
    assert.equal(result.status, 200);
    assert.deepEqual(result.body, { ok: true, value: { workspace: {
      workspaceId: 'w1', path: f.realPath,
      title: naming === 'explicit' ? 'Chosen' : naming === 'collision' ? `${base} · tmp` : base,
      sessionIds: ['s1'],
    }, created: true } });
  });
}

test('bridge preserves attach errors and uses its own fallback code', async (t) => {
  const f = await workspaceFixture(t);
  const w = await f.registry.create(f.dir, 'Existing');
  const message = 'session s1: its cwd resolves to /other, not this workspace';
  w.attachSession = async () => { throw new Error(message); };
  assert.deepEqual(await f.attach({ path: f.dir, sessionId: 's1' }), {
    status: 200, body: { ok: false, error: { code: 'bridge/attach-failed', message, details: {} } },
  });
  w.attachSession = async () => { throw Object.assign(new Error('native failure'), { code: 'session/missing', details: { sessionId: 's1' } }); };
  assert.deepEqual((await f.attach({ path: f.dir, sessionId: 's1' })).body.error,
    { code: 'session/missing', message: 'native failure', details: { sessionId: 's1' } });
});

test('bridge preflight rejects mismatched cwd and unknown sessions before creating a project', async (t) => {
  const f = await workspaceFixture(t);
  const other = path.join(f.dir, 'other');
  fs.mkdirSync(other);
  const mismatch = await f.attach({ path: other, sessionId: 's1' });
  assert.equal(mismatch.status, 200);
  assert.equal(mismatch.body.ok, false);
  assert.equal(mismatch.body.error.code, 'bridge/attach-failed');
  assert.ok(mismatch.body.error.message.includes(f.realPath));
  assert.ok(mismatch.body.error.message.includes(fs.realpathSync(other)));
  const unknown = await f.attach({ path: f.dir, sessionId: 'unknown' });
  assert.equal(unknown.body.error.message, 'session persistence holds no such session');
  assert.equal(f.workspaces.length, 0);
});

test('bridge accepts cwd symlinks and supports registries without header preflight', async (t) => {
  const f = await workspaceFixture(t);
  const alias = path.join(f.dir, 'alias');
  fs.symlinkSync(f.dir, alias);
  assert.equal((await f.attach({ path: alias, sessionId: 's1' })).body.ok, true);
  delete f.registry.readSessionHeader;
  assert.equal((await f.attach({ path: f.dir, sessionId: 's1' })).body.ok, true);
});

test('bridge validates arguments before accessing the registry', async (t) => {
  const f = await workspaceFixture(t);
  f.setRegistry(undefined);
  for (const args of [{ sessionId: 's1' }, { path: 'relative', sessionId: 's1' },
    { path: f.dir }, { path: f.dir, sessionId: ' ' }, { path: f.dir, sessionId: 's1', title: '' },
    { path: f.dir, sessionId: 's1', title: 42 }, null, []]) {
    const result = await f.attach(args);
    assert.equal(result.status, 400);
    assert.equal(typeof result.body.error, 'string');
  }
});

test('bridge reads registry availability per call and reports it in health', async (t) => {
  const f = await workspaceFixture(t);
  assert.equal((await f.health()).workspaceRegistry, true);
  f.setRegistry(undefined);
  assert.equal((await f.health()).workspaceRegistry, false);
  assert.deepEqual(await f.attach({ path: f.dir, sessionId: 's1' }), {
    status: 503, body: { error: 'DSH Host workspace registry is not available' },
  });
});

test('projectTitle uses basename or a shortened parent on collision', () => {
  const title = bridge.projectTitle;
  assert.equal(title('/private/tmp/turing-koe-p1/koe', [], '/home/test'), 'koe');
  assert.equal(title('/private/tmp/turing-koe-p1/koe', ['koe'], '/home/test'), 'koe · tmp/turing-koe-p1');
  assert.equal(title('/home/test/work/koe', ['koe'], '/home/test'), 'koe · ~/work');
  assert.equal(title('/home/test/koe', ['koe'], '/home/test'), 'koe · ~');
  assert.equal(title('/home/test-other/koe', ['koe'], '/home/test'), 'koe · home/test-other');
});
