import test from 'node:test';
import assert from 'node:assert/strict';
import { EventEmitter } from 'node:events';
import { registerAuthorizationFlow, CALLBACK_URL } from '../flow.js';

const metadata = { redirect_uris: [CALLBACK_URL], token_endpoint_auth_method: 'none' };
const tick = () => new Promise(resolve => setImmediate(resolve));
function harness({ begin, finish, listenError, timeoutMs = 1000 } = {}) {
  let flow, server, begins = 0, finishes = 0, cancels = 0, unregistered = 0;
  const notices = [], controller = new AbortController();
  const pending = { authorizationUrl: 'https://relay.example/authorize?state=synthetic-state',
    cancel() { cancels++; }, async finish(url) { finishes++; return finish ? finish(url) : { status: 'authorized' }; } };
  const options = {
    authorization: { registerFlow(value) { flow = value; return () => { unregistered++; }; } }, key: 'synthetic/key',
    oauthController: { async beginAuthorization(options) { begins++; assert.equal(server.listening, true); return begin ? begin(options, pending) : pending; } },
    auth() {}, clientMetadata: metadata, timeoutMs,
    createServerImpl(handler) {
      server = new EventEmitter(); server.listening = false; server.closed = 0; server.destroyed = 0;
      server.listen = (options, callback) => {
        assert.deepEqual(options, { host: '127.0.0.1', port: 13346, exclusive: true });
        queueMicrotask(() => { if (listenError) server.emit('error', new Error('private address error')); else { server.listening = true; callback(); } });
      };
      server.close = callback => { server.closed++; server.listening = false; queueMicrotask(callback); };
      server.closeAllConnections = () => { server.destroyed++; };
      server.request = (overrides = {}) => new Promise(resolve => {
        const req = { method: 'GET', url: '/oauth/callback?code=synthetic-code&state=synthetic-state', headers: { host: '127.0.0.1:13346' }, socket: { remoteAddress: '127.0.0.1' }, ...overrides };
        const res = { writeHead(status, headers) { this.status = status; this.headers = headers; }, end(body, callback) { resolve({ status: this.status, headers: this.headers, body }); callback?.(); } };
        handler(req, res);
      });
      return server;
    },
  };
  const dispose = registerAuthorizationFlow(options);
  return { options, pending, dispose, controller, notices,
    run: () => flow.run({ method: 'oauth', signal: controller.signal, notify: notice => notices.push(notice) }),
    get server() { return server; }, get begins() { return begins; }, get finishes() { return finishes; },
    get cancels() { return cancels; }, get unregistered() { return unregistered; },
  };
}
test('registration alone does not listen or initiate authorization', () => {
  const h = harness(); assert.equal(h.server, undefined); assert.equal(h.begins, 0); h.dispose(); h.dispose(); assert.equal(h.unregistered, 1);
});
test('listener exists before explicit DCR; notify uses formal notice; completion closes it', async () => {
  const h = harness(); const running = h.run(); await tick();
  assert.equal(h.begins, 1); assert.equal(h.notices[0].url, h.pending.authorizationUrl);
  const response = await h.server.request(); await running;
  assert.equal(response.status, 200); assert.equal(response.headers['Cache-Control'], 'no-store');
  assert.doesNotMatch(response.body, /synthetic-code|synthetic-state|https:/);
  assert.equal(h.finishes, 1); assert.equal(h.server.closed, 1); h.dispose();
});
test('port conflict fails before DCR and never claims success', async () => {
  const h = harness({ listenError: true }); await assert.rejects(h.run(), /oauth_flow_/);
  assert.equal(h.begins, 0); assert.equal(h.server.closed, 1); h.dispose();
});
test('SDK setup failure is scrubbed and closes listener', async () => {
  const h = harness({ begin: async () => { throw new Error('private-token'); } });
  await assert.rejects(h.run(), e => e.message === 'oauth_flow_failed' && !e.cause);
  assert.equal(h.server.closed, 1); h.dispose();
});
test('invalid callback state failure returns static error and closes attempt', async () => {
  const h = harness({ finish: async () => { throw new Error('private-state-token'); } });
  const running = h.run(); const failed = assert.rejects(running, /oauth_flow_failed/); await tick();
  const response = await h.server.request(); await failed;
  assert.equal(response.status, 400); assert.doesNotMatch(response.body, /private|synthetic/);
  assert.equal(h.server.closed, 1); h.dispose();
});
test('method/host/path/foreign address/origin/oversized URL are refused without consuming callback', async () => {
  const h = harness(); const running = h.run(); await tick();
  for (const request of [{ method: 'POST' }, { headers: { host: 'evil.example' } }, { url: '/other' },
    { socket: { remoteAddress: '192.0.2.1' } }, { headers: { host: '127.0.0.1:13346', origin: 'https://evil.example' } },
    { url: `/${'a'.repeat(8192)}` }, { url: '//evil.example/oauth/callback' }]) {
    assert.ok((await h.server.request(request)).status >= 400);
  }
  assert.equal(h.finishes, 0); await h.server.request(); await running; h.dispose();
});
test('simultaneous callback is fenced to one finish', async () => {
  const release = Promise.withResolvers(); const h = harness({ finish: async () => { await release.promise; return { status: 'authorized' }; } });
  const running = h.run(); await tick(); const first = h.server.request(); await tick();
  assert.equal((await h.server.request()).status, 409); assert.equal(h.finishes, 1);
  release.resolve(); await first; await running; h.dispose();
});
test('abort before run performs no listen or DCR', async () => {
  const h = harness(); h.controller.abort(); await assert.rejects(h.run(), /oauth_flow_cancelled/);
  assert.equal(h.server, undefined); assert.equal(h.begins, 0); h.dispose();
});
test('five-minute maximum is enforced; timeout closes listener and cancels pending flow', async () => {
  assert.throws(() => registerAuthorizationFlow({ ...harness().options, timeoutMs: 300001 }), /configuration_invalid/);
  const h = harness({ timeoutMs: 10 }); await assert.rejects(h.run(), /oauth_flow_cancelled_or_expired/);
  assert.equal(h.server.closed, 1); assert.ok(h.cancels > 0); h.dispose();
});
test('plugin disposal aborts callback wait and withdraws the registered flow', async () => {
  const h = harness(); const failed = assert.rejects(h.run(), /oauth_flow_cancelled_or_expired/); await tick();
  h.dispose(); await failed; assert.equal(h.server.closed, 1); assert.equal(h.unregistered, 1);
});
test('abort during discovery closes immediately and invalidates a late pending flow', async () => {
  const release = Promise.withResolvers(); const h = harness({ begin: async (_options, pending) => { await release.promise; return pending; } });
  const failed = assert.rejects(h.run(), /oauth_flow_cancelled_or_expired/); await tick(); h.controller.abort(); await failed;
  assert.equal(h.server.closed, 1); release.resolve(); await tick(); assert.equal(h.cancels, 1); h.dispose();
});
