import test from 'node:test';
import assert from 'node:assert/strict';
import { createCredentialAuth, credentialBindingId } from '../oauth.js';
import { CALLBACK_URL } from '../flow.js';
const binding = { serverUrl: 'https://relay.example/mcp', issuer: 'https://relay.example/', resource: 'https://relay.example/mcp', scope: 'mcp:connect' };
function fixture(extension) {
  let saved;
  const credentials = { readRecord: async () => undefined, modifyRecord: async (_key, mutate) => {
    const next = await mutate(undefined);
    // Match DSH's strict JSON admission rather than silently dropping undefined.
    assert.deepEqual(JSON.parse(JSON.stringify(next)), next); saved = next; return next;
  } };
  const controller = createCredentialAuth({ credentials, binding,
    key: `agent-fleet-agenvo-native/${credentialBindingId(binding)}`, refreshAuthorization: async () => {}, fetchFn: async () => {} });
  const auth = async (provider, options) => {
    if (options.authorizationCode) {
      provider.saveTokens({ access_token: 'synthetic', token_type: 'Bearer' }, { issuer: binding.issuer }); return 'AUTHORIZED';
    }
    provider.saveDiscoveryState({ authorizationServerUrl: binding.issuer, resourceMetadataUrl: undefined,
      authorizationServerMetadata: { issuer: binding.issuer, token_endpoint: 'https://relay.example/token', authorization_endpoint: 'https://relay.example/authorize', extension },
      resourceMetadata: undefined });
    provider.saveClientInformation({ client_id: 'synthetic', client_secret: undefined });
    provider.saveCodeVerifier('synthetic');
    provider.redirectToAuthorization(new URL(`https://relay.example/authorize?state=${provider.state()}`));
    return 'REDIRECT';
  };
  return { controller, auth, get saved() { return saved; } };
}
const metadata = { redirect_uris: [CALLBACK_URL], token_endpoint_auth_method: 'none' };
test('SDK optional undefined object fields are omitted before strict native JSON persistence', async () => {
  const extension = { absent: undefined, required: false, count: 0, values: ['a', null] };
  const f = fixture(extension);
  const pending = await f.controller.beginAuthorization({ auth: f.auth, clientMetadata: metadata, redirectUrl: CALLBACK_URL });
  await pending.finish(`${CALLBACK_URL}?${new URLSearchParams({ code: 'synthetic', state: new URL(pending.authorizationUrl).searchParams.get('state') })}`);
  assert.equal(Object.hasOwn(f.saved.payload.discovery, 'resourceMetadataUrl'), false);
  assert.equal(Object.hasOwn(f.saved.payload.clientInformation, 'client_secret'), false);
  assert.deepEqual(f.saved.payload.discovery.authorizationServerMetadata.extension, { required: false, count: 0, values: ['a', null] });
});
for (const extension of [[undefined], [NaN], new Date(0), { callback() {} }]) {
  test('non-JSON metadata is rejected instead of lossy coercion or token persistence', async () => {
    const f = fixture(extension);
    await assert.rejects(f.controller.beginAuthorization({ auth: f.auth, clientMetadata: metadata, redirectUrl: CALLBACK_URL }), /oauth_authorization_failed/);
    assert.equal(f.saved, undefined);
  });
}
