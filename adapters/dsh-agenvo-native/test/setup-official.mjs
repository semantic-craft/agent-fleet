// Explicit offline integration entry: existing fixed DSH source root as argv[2].
// Real SDK/Cordis/AuthorizationService, in-memory credentials, synthetic upstream,
// ephemeral loopback HTTP only. Never reads an installed app or user credentials.
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { pathToFileURL } from 'node:url';
import { resolve, join } from 'node:path';
import { createServer, request } from 'node:http';
import { createHash } from 'node:crypto';
import { CALLBACK_URL } from '../flow.js';
import { credentialBindingId } from '../oauth.js';
import { authorizeWithHost } from '../setup.js';
import { mkdtemp, stat, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
const root = resolve(process.argv[2]);
const require = createRequire(join(root, 'packages/mcp/mcp-client/package.json'));
const sdk = await import(pathToFileURL(require.resolve('@modelcontextprotocol/client')));
const { Context } = await import(pathToFileURL(require.resolve('@deepseek-ai/cordis')));
const { default: Authorization } = await import(pathToFileURL(join(root, 'packages/credentials/authorization/lib/index.js')));
const { credentialKey } = await import(pathToFileURL(join(root, 'packages/credentials/credentials/lib/index.js')));
const binding = { serverUrl: 'https://relay.example/mcp', issuer: 'https://relay.example/', resource: 'https://relay.example/mcp', scope: 'mcp:connect' };
const key = credentialKey('agent-fleet-agenvo-native', credentialBindingId(binding));
const { LocalCredentialProvider } = await import(pathToFileURL(join(root, 'packages/credentials/credentials-local/lib/index.js')));
let challenge, exchange = 0;
const reply = value => new Response(JSON.stringify(value), { headers: { 'content-type': 'application/json' } });
const fetchFn = async (input, init = {}) => {
  const target = new URL(input);
  assert.equal(target.origin, 'https://relay.example');
  if (target.pathname.includes('.well-known/oauth-protected-resource')) return reply({ resource: binding.resource, authorization_servers: [binding.issuer] });
  if (target.pathname.includes('.well-known/oauth-authorization-server')) return reply({
    issuer: binding.issuer, authorization_endpoint: 'https://relay.example/authorize', token_endpoint: 'https://relay.example/token', registration_endpoint: 'https://relay.example/register',
    response_types_supported: ['code'], grant_types_supported: ['authorization_code', 'refresh_token'], token_endpoint_auth_methods_supported: ['none'], code_challenge_methods_supported: ['S256'],
  });
  if (target.pathname === '/register') return reply({ ...JSON.parse(init.body), client_id: 'synthetic-client' });
  assert.equal(target.pathname, '/token');
  const body = new URLSearchParams(init.body);
  assert.equal(body.get('grant_type'), 'authorization_code');
  assert.equal(createHash('sha256').update(body.get('code_verifier')).digest('base64url'), challenge);
  exchange++;
  return reply({ access_token: 'synthetic-access', refresh_token: 'synthetic-refresh', token_type: 'Bearer', expires_in: 3600, scope: binding.scope });
};
let server;
const createServerImpl = handler => {
  server = createServer(handler);
  const listen = server.listen.bind(server);
  server.listen = (options, callback) => {
    assert.deepEqual(options, { host: '127.0.0.1', port: 13346, exclusive: true });
    // Production stays fixed. The isolated fixture avoids reserving its real port.
    return listen({ ...options, port: 0 }, callback);
  };
  return server;
};
function send(path) {
  return new Promise((resolve, reject) => {
    const r = request({ host: '127.0.0.1', port: server.address().port, path, headers: { host: '127.0.0.1:13346' } }, response => {
      let body = ''; response.setEncoding('utf8'); response.on('data', data => { body += data; });
      response.on('end', () => resolve({ status: response.statusCode, body }));
    });
    r.on('error', reject); r.end();
  });
}
const folder = await mkdtemp(join(tmpdir(), 'fleet-authorization-fixture-'));
const credentialsFile = join(folder, 'credentials.yaml');
const unrelatedKey = credentialKey('synthetic-other-owner', 'fixture');
const unrelated = { kind: 'grant', payload: { synthetic: 'unrelated-value' } };
async function openCredentials() {
  const ctx = new Context();
  await ctx.plugin(LocalCredentialProvider, { path: credentialsFile, watch: false });
  return ctx;
}
try {
  const seed = await openCredentials();
  try { await seed.credentials.modifyRecord(unrelatedKey, async () => unrelated); }
  finally { await seed.fiber.dispose(); }
  assert.equal((await stat(credentialsFile)).mode & 0o777, 0o600);
  const noticed = Promise.withResolvers();
  const operation = authorizeWithHost({ binding, credentialsFile, notify: noticed.resolve }, {
    Context, LocalCredentialProvider, AuthorizationService: Authorization, credentialKey,
    auth: sdk.auth, refreshAuthorization: sdk.refreshAuthorization, fetchFn, createServerImpl,
  });
  operation.catch(() => {});
  const notice = await noticed.promise;
  const authorization = new URL(notice.url);
  challenge = authorization.searchParams.get('code_challenge');
  assert.ok(challenge);
  const response = await send(`/oauth/callback?${new URLSearchParams({ code: 'synthetic-code', state: authorization.searchParams.get('state'), iss: binding.issuer })}`);
  assert.equal(response.status, 200); assert.doesNotMatch(response.body, /synthetic|code=|state=/);
  assert.deepEqual(await operation, { status: 'authorized' });
  assert.equal(exchange, 1); assert.equal(server.listening, false); assert.equal(server.address(), null);
  const readback = await openCredentials();
  try {
    assert.equal((await readback.credentials.describeRecord(key)).configured, true);
    assert.equal((await readback.credentials.readRecord(key)).payload.binding.serverUrl, binding.serverUrl);
    assert.deepEqual(await readback.credentials.readRecord(unrelatedKey), unrelated);
  } finally { await readback.fiber.dispose(); }
  assert.equal((await stat(credentialsFile)).mode & 0o777, 0o600);
  console.log('official setup + local credentials + SDK + callback: authorized, unrelated record preserved, mode 0600, closed');
} finally { server?.closeAllConnections(); await rm(folder, { recursive: true, force: true }); }
