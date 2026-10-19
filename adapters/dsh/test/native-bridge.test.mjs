import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import http from 'node:http';
import os from 'node:os';
import path from 'node:path';
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
