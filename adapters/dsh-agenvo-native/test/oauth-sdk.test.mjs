import test from 'node:test';
import assert from 'node:assert/strict';
import { createCredentialAuth, credentialBindingId } from '../oauth.js';

// Optional fixed SDK checkout supplied by the offline integration runner.
// No package download, discovery of local credentials, or real fetch is allowed.
const sdkModule = process.env.DSH_MCP_CLIENT_TEST_MODULE;
for (const includeIssuer of [true, false]) test(`SDK 2.2 discovery/DCR/PKCE and issuer binding (callback issuer=${includeIssuer})`, { skip: !sdkModule }, async () => {
  const { auth, refreshAuthorization } = await import(sdkModule);
  const binding = { serverUrl: 'https://relay.example/mcp', issuer: 'https://relay.example/', resource: 'https://relay.example/mcp', scope: 'mcp:connect' };
  const key = `agent-fleet-agenvo-native/${credentialBindingId(binding)}`;
  let record, writes = 0, exchange = 0, refresh = 0, registration = 0, challenge;
  const credentials = {
    readRecord: async () => structuredClone(record),
    modifyRecord: async (_key, mutate) => {
      const next = await mutate(structuredClone(record));
      if (next !== undefined) { record = structuredClone(next); writes++; }
      return structuredClone(record);
    },
  };
  const reply = value => new Response(JSON.stringify(value), { headers: { 'content-type': 'application/json' } });
  const fetchFn = async (input, init = {}) => {
    const target = new URL(typeof input === 'string' || input instanceof URL ? input : input.url);
    assert.equal(target.origin, 'https://relay.example');
    if (target.pathname.includes('.well-known/oauth-protected-resource')) return reply({ resource: binding.resource, authorization_servers: [binding.issuer], scopes_supported: [binding.scope] });
    if (target.pathname.includes('.well-known/oauth-authorization-server')) return reply({
      issuer: binding.issuer, authorization_endpoint: 'https://relay.example/authorize', token_endpoint: 'https://relay.example/token',
      registration_endpoint: 'https://relay.example/register', response_types_supported: ['code'], grant_types_supported: ['authorization_code', 'refresh_token'],
      token_endpoint_auth_methods_supported: ['none'], code_challenge_methods_supported: ['S256'], authorization_response_iss_parameter_supported: true,
    });
    if (target.pathname === '/register') {
      registration++; const metadata = JSON.parse(init.body);
      assert.equal(metadata.token_endpoint_auth_method, 'none');
      return reply({ ...metadata, client_id: 'synthetic-public-client' });
    }
    if (target.pathname === '/token') {
      const body = new URLSearchParams(init.body);
      assert.equal(body.get('client_id'), 'synthetic-public-client');
      assert.equal(body.get('resource'), binding.resource);
      assert.equal(new Headers(init.headers).get('authorization'), null);
      if (body.get('grant_type') === 'authorization_code') {
        exchange++;
        assert.equal(body.get('code'), 'synthetic-code');
        const { createHash } = await import('node:crypto');
        assert.equal(createHash('sha256').update(body.get('code_verifier')).digest('base64url'), challenge);
        return reply({ access_token: 'synthetic-access', refresh_token: 'synthetic-refresh', token_type: 'Bearer', expires_in: 3600, scope: binding.scope });
      }
      assert.equal(body.get('grant_type'), 'refresh_token');
      assert.equal(body.get('refresh_token'), 'synthetic-refresh'); refresh++;
      // SDK and module must preserve the old refresh token when no rotation is returned.
      return reply({ access_token: 'synthetic-renewed', token_type: 'Bearer', expires_in: 3600, scope: binding.scope });
    }
    assert.fail('unexpected synthetic endpoint');
  };
  const service = createCredentialAuth({ credentials, key, binding, refreshAuthorization, fetchFn, now: () => 1000 });
  const redirectUrl = 'http://127.0.0.1:8080/callback';
  const pending = await service.beginAuthorization({ auth, redirectUrl,
    clientMetadata: { client_name: 'synthetic-client', redirect_uris: [redirectUrl], response_types: ['code'], grant_types: ['authorization_code', 'refresh_token'], token_endpoint_auth_method: 'none' } });
  const authorization = new URL(pending.authorizationUrl);
  assert.equal(authorization.searchParams.get('code_challenge_method'), 'S256');
  challenge = authorization.searchParams.get('code_challenge');
  assert.equal(writes, 0); assert.equal(registration, 1);
  const returned = `${redirectUrl}?${new URLSearchParams({ code: 'synthetic-code', state: authorization.searchParams.get('state'), ...(includeIssuer ? { iss: binding.issuer } : {}) })}`;
  if (!includeIssuer) {
    await assert.rejects(pending.finish(returned), /oauth_authorization_failed/);
    assert.equal(exchange, 0); assert.equal(writes, 0); return;
  }
  await pending.finish(returned);
  assert.equal(exchange, 1); assert.equal(writes, 1);
  assert.equal(await service.authProvider.token(), 'synthetic-access');
  await service.authProvider.onUnauthorized({ serverUrl: new URL(binding.serverUrl), response: new Response(null, { status: 401 }), fetchFn });
  assert.equal(refresh, 1); assert.equal(writes, 2);
  assert.equal(await service.authProvider.token(), 'synthetic-renewed');
  assert.equal(record.payload.tokens.refresh_token, 'synthetic-refresh');
});
