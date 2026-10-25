import test from 'node:test';
import assert from 'node:assert/strict';
import { credentialBindingId, createCredentialAuth } from '../oauth.js';

const binding = { serverUrl: 'https://relay.example/mcp', issuer: 'https://relay.example/', resource: 'https://relay.example/mcp', scope: 'mcp:connect' };
const key = `agent-fleet-agenvo-native/${credentialBindingId(binding)}`;
const discovery = { authorizationServerUrl: binding.issuer, authorizationServerMetadata: {
  issuer: binding.issuer, token_endpoint: 'https://relay.example/token', authorization_endpoint: 'https://relay.example/authorize',
}, resourceMetadata: { resource: binding.resource } };
function record({ expiresAt = 999999, generation = 1, refresh = 'refresh-old' } = {}) {
  return { kind: 'grant', payload: { format: 1, binding, generation, expiresAt,
    discovery, clientInformation: { client_id: 'synthetic-client' },
    tokens: { access_token: `access-${generation}`, refresh_token: refresh, token_type: 'Bearer', scope: binding.scope },
  } };
}
function store(initial) {
  let value = structuredClone(initial), queue = Promise.resolve(), writes = 0;
  return {
    async readRecord(k) { assert.equal(k, key); return structuredClone(value); },
    modifyRecord(k, mutate) {
      assert.equal(k, key);
      const op = queue.then(async () => {
        const next = await mutate(structuredClone(value));
        if (next !== undefined) { value = structuredClone(next); writes++; }
        return structuredClone(value);
      });
      queue = op.catch(() => {});
      return op;
    },
    get value() { return structuredClone(value); }, get writes() { return writes; },
  };
}
const fetchFn = async () => { throw new Error('unexpected fetch'); };
function setup(initial = record(), refreshAuthorization = async () => ({ access_token: 'new-access', token_type: 'Bearer', expires_in: 60 }), extra = {}) {
  const credentials = store(initial);
  const service = createCredentialAuth({ credentials, key, binding, refreshAuthorization, fetchFn, now: () => 100, refreshSkewMs: 0, ...extra });
  return { credentials, ...service };
}
const unauthorized = () => ({ response: new Response(null, { status: 401 }), serverUrl: new URL(binding.serverUrl), fetchFn });

test('construction and token lookup never initiate authorization or refresh a fresh token', async () => {
  let calls = 0;
  const s = setup(record(), async () => { calls++; });
  assert.equal(await s.authProvider.token(), 'access-1'); assert.equal(calls, 0); assert.equal(s.credentials.writes, 0);
});
test('early 401 forces refresh despite future expiry; omitted refresh token is preserved', async () => {
  let seen;
  const s = setup(record(), async (issuer, options) => { seen = { issuer, options }; return { access_token: 'new-access', token_type: 'bearer', expires_in: 60 }; });
  await s.authProvider.onUnauthorized(unauthorized());
  assert.equal(await s.authProvider.token(), 'new-access');
  assert.equal(seen.options.refreshToken, 'refresh-old'); assert.equal(seen.options.fetchFn, fetchFn);
  assert.equal(s.credentials.value.payload.tokens.refresh_token, 'refresh-old');
  assert.equal(s.credentials.value.payload.generation, 2);
});
test('concurrent 401s serialize under native record lock using the latest rotating refresh token', async () => {
  const seen = [];
  const credentials = store(record());
  const refreshAuthorization = async (_issuer, { refreshToken }) => {
    seen.push(refreshToken); await new Promise(resolve => setImmediate(resolve));
    return { access_token: `access-${seen.length + 1}`, refresh_token: `rotated-${seen.length}`, token_type: 'Bearer', expires_in: 60 };
  };
  const args = { credentials, key, binding, refreshAuthorization, fetchFn, now: () => 100 };
  const a = createCredentialAuth(args), b = createCredentialAuth(args);
  await Promise.all([a.authProvider.onUnauthorized(unauthorized()), b.authProvider.onUnauthorized(unauthorized())]);
  assert.deepEqual(seen, ['refresh-old', 'rotated-1']); assert.equal(credentials.value.payload.tokens.refresh_token, 'rotated-2');
});
test('concurrent expiry reads recheck locked record and refresh only once', async () => {
  let calls = 0;
  const s = setup(record({ expiresAt: 0 }), async () => { calls++; return { access_token: 'new-access', token_type: 'Bearer', expires_in: 60 }; });
  assert.deepEqual(await Promise.all([s.authProvider.token(), s.authProvider.token()]), ['new-access', 'new-access']);
  assert.equal(calls, 1);
});
test('refresh errors are scrubbed and leave the existing record unchanged', async () => {
  const s = setup(record(), async () => { throw new Error('private-refresh-token-body'); });
  const before = s.credentials.value;
  await assert.rejects(s.authProvider.onUnauthorized(unauthorized()), e => e.message === 'oauth_refresh_failed' && !e.cause);
  assert.deepEqual(s.credentials.value, before);
});
for (const change of [p => { p.binding = { ...binding, scope: 'other' }; }, p => { p.discovery.authorizationServerMetadata.issuer = 'https://other.example/'; }, p => { p.tokens.token_type = 'DPoP'; }]) {
  test('mismatched stored identity or unsupported token type refuses without rotation', async () => {
    const input = structuredClone(record()); change(input.payload); let calls = 0;
    const s = setup(input, async () => { calls++; });
    await assert.rejects(s.authProvider.token(), /oauth_authorization_unavailable/); assert.equal(calls, 0);
  });
}
test('foreign unauthorized context cannot trigger refresh', async () => {
  let calls = 0; const s = setup(record(), async () => { calls++; });
  await assert.rejects(s.authProvider.onUnauthorized({ ...unauthorized(), serverUrl: new URL('https://other.example/mcp') }), /context_mismatch/);
  assert.equal(calls, 0);
});
test('missing credentials never trigger DCR or browser fallback', async () => {
  const credentials = store(undefined);
  const s = createCredentialAuth({ credentials, key, binding, refreshAuthorization: async () => { assert.fail(); }, fetchFn });
  await assert.rejects(s.authProvider.token(), /oauth_authorization_unavailable/);
  assert.equal(credentials.writes, 0);
});
test('invalid refresh payload/scope cannot replace the existing record', async () => {
  const s = setup(record(), async () => ({ access_token: 'wrong', token_type: 'Bearer', scope: 'admin' }));
  await assert.rejects(s.authProvider.onUnauthorized(unauthorized()), /oauth_refresh_failed/);
  assert.equal(s.credentials.writes, 0);
});
const redirectUrl = 'http://127.0.0.1:8080/callback';
const clientMetadata = { redirect_uris: [redirectUrl], client_name: 'synthetic-client', grant_types: ['authorization_code', 'refresh_token'], response_types: ['code'], token_endpoint_auth_method: 'none' };
function initial({ clock = () => 100, beforeSave } = {}) {
  const credentials = store(undefined); let exchanges = 0;
  const service = createCredentialAuth({ credentials, key, binding, refreshAuthorization: async () => { assert.fail(); }, fetchFn, now: clock });
  const auth = async (p, options) => {
    assert.equal(options.fetchFn, fetchFn);
    if (!options.authorizationCode) {
      await p.saveDiscoveryState(discovery);
      await p.saveClientInformation({ client_id: 'synthetic-client' }, { issuer: binding.issuer });
      await p.saveCodeVerifier('synthetic-pkce-verifier');
      await p.redirectToAuthorization(new URL(`https://relay.example/authorize?state=${await p.state()}`));
      return 'REDIRECT';
    }
    exchanges++; assert.equal(await p.codeVerifier(), 'synthetic-pkce-verifier');
    await beforeSave?.(credentials);
    await p.saveTokens({ access_token: 'initial-access', refresh_token: 'initial-refresh', token_type: 'Bearer', expires_in: 60 }, { issuer: binding.issuer });
    return 'AUTHORIZED';
  };
  return { ...service, credentials, auth, get exchanges() { return exchanges; } };
}
function callback(pending, changes = {}) {
  const state = new URL(pending.authorizationUrl).searchParams.get('state');
  return `${redirectUrl}?${new URLSearchParams({ state, code: 'synthetic-code', iss: binding.issuer, ...changes })}`;
}
test('explicit SDK initial flow stages DCR/PKCE and commits only validated one-use callback', async () => {
  const s = initial();
  const pending = await s.beginAuthorization({ auth: s.auth, clientMetadata, redirectUrl });
  assert.equal(s.credentials.writes, 0);
  assert.deepEqual(await pending.finish(callback(pending)), { status: 'authorized' });
  assert.equal(s.credentials.writes, 1); assert.equal(await s.authProvider.token(), 'initial-access');
  await assert.rejects(pending.finish(callback(pending)), /oauth_authorization_expired/);
  assert.equal(s.exchanges, 1);
});
for (const changes of [{ state: 'wrong' }, { iss: 'https://other.example/' }, { code: '' }]) {
  test('invalid callback is consumed without code exchange or credential write', async () => {
    const s = initial(); const pending = await s.beginAuthorization({ auth: s.auth, clientMetadata, redirectUrl });
    await assert.rejects(pending.finish(callback(pending, changes)), /oauth_authorization_failed/);
    await assert.rejects(pending.finish(callback(pending)), /oauth_authorization_expired/);
    assert.equal(s.exchanges, 0); assert.equal(s.credentials.writes, 0);
  });
}
test('authorization timeout and cancel cannot exchange or persist', async () => {
  let time = 100; const s = initial({ clock: () => time });
  const p = await s.beginAuthorization({ auth: s.auth, clientMetadata, redirectUrl, timeoutMs: 10 });
  time = 111; await assert.rejects(p.finish(callback(p)), /oauth_authorization_expired/);
  const q = await s.beginAuthorization({ auth: s.auth, clientMetadata, redirectUrl }); q.cancel();
  await assert.rejects(q.finish(callback(q)), /oauth_authorization_expired/); assert.equal(s.exchanges, 0);
});
test('concurrent authorization cannot overwrite an independently committed grant', async () => {
  const s = initial({ beforeSave: credentials => credentials.modifyRecord(key, async () => record({ generation: 8 })) });
  const p = await s.beginAuthorization({ auth: s.auth, clientMetadata, redirectUrl });
  await assert.rejects(p.finish(callback(p)), /oauth_authorization_failed/);
  assert.equal(s.credentials.value.payload.generation, 8);
});
test('record key is bound to endpoint/issuer/resource/scope; no token or personal value in its address', () => {
  assert.match(credentialBindingId(binding), /^grant-[a-f0-9]{64}$/);
  assert.notEqual(credentialBindingId(binding), credentialBindingId({ ...binding, scope: 'other' }));
  assert.throws(() => createCredentialAuth({ credentials: store(), key: 'wrong', binding, refreshAuthorization() {}, fetchFn }), /oauth_invalid_configuration/);
});
test('cancel during code exchange prevents the late token response from being committed', async () => {
  let release, entered;
  const waiting = new Promise(resolve => { entered = resolve; });
  const gate = new Promise(resolve => { release = resolve; });
  const s = initial({ beforeSave: async () => { entered(); await gate; } });
  const pending = await s.beginAuthorization({ auth: s.auth, clientMetadata, redirectUrl });
  const finishing = pending.finish(callback(pending));
  await waiting; pending.cancel(); release();
  await assert.rejects(finishing, /oauth_authorization_failed/);
  assert.equal(s.credentials.writes, 0);
});
test('public-client-only policy rejects secret auth before SDK discovery', async () => {
  const s = initial(); let called = false;
  await assert.rejects(s.beginAuthorization({ auth: async () => { called = true; }, clientMetadata: { ...clientMetadata, token_endpoint_auth_method: 'client_secret_basic' }, redirectUrl }), /oauth_redirect_mismatch/);
  assert.equal(called, false);
});
test('credential persistence failure after refresh is scrubbed; no raw tokens escape in errors', async () => {
  const credentials = store(record());
  const original = credentials.modifyRecord;
  credentials.modifyRecord = async (k, mutate) => original(k, async current => {
    await mutate(current); throw new Error('synthetic-secret-disk-error');
  });
  const service = createCredentialAuth({ credentials, key, binding, fetchFn,
    refreshAuthorization: async () => ({ access_token: 'synthetic-secret-access', token_type: 'Bearer' }) });
  await assert.rejects(service.authProvider.onUnauthorized(unauthorized()), e => e.message === 'oauth_refresh_failed' && !e.cause);
  assert.equal(credentials.writes, 0);
});
