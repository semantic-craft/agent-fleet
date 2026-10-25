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
import { registerHostAuthorization } from '../action.js';
import { createCredentialAuth, credentialBindingId } from '../oauth.js';
const root = resolve(process.argv[2]);
const require = createRequire(join(root, 'packages/mcp/mcp-client/package.json'));
const sdk = await import(pathToFileURL(require.resolve('@modelcontextprotocol/client')));
const { Context } = await import(pathToFileURL(require.resolve('@deepseek-ai/cordis')));
const { default: Authorization } = await import(pathToFileURL(join(root, 'packages/credentials/authorization/lib/index.js')));
const { default: Credentials, credentialKey } = await import(pathToFileURL(join(root, 'packages/credentials/credentials/lib/index.js')));
const {default:Commands}=await import(pathToFileURL(join(root,'packages/interaction/commands/lib/index.js')));
const {createScope}=await import(pathToFileURL(join(root,'packages/core/scope/lib/index.js')));
const {default:Sessions,SessionId}=await import(pathToFileURL(join(root,'packages/core/session/lib/index.js')));
const binding = { serverUrl: 'https://relay.example/mcp', issuer: 'https://relay.example/', resource: 'https://relay.example/mcp', scope: 'mcp:connect' };
const key = credentialKey('agent-fleet-agenvo-native', credentialBindingId(binding));
let record, writes = 0, issueEvent = true, challenge, exchange = 0;
class MemoryCredentials extends Credentials {
  async readRecord() { return structuredClone(record); }
  async describeRecord() { return { configured: record !== undefined, writable: true, kind: 'grant' }; }
  async modifyRecord(_key, mutate) {
    const next = await mutate(structuredClone(record));
    if (next !== undefined) { record = structuredClone(next); writes++; if (issueEvent) this.ctx.emit('credentials/record-updated', key); }
    return structuredClone(record);
  }
}
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
const ctx = new Context();
await ctx.plugin(MemoryCredentials); await ctx.plugin(Authorization); await ctx.plugin(Sessions); await ctx.plugin(Commands);
const agent={session:ctx.sessions.create(SessionId('synthetic-agent'))}; agent.id=agent.session.id;
const other={session:ctx.sessions.create(SessionId('synthetic-other'))}; other.id=other.session.id;
let scope;await ctx.plugin(Object.assign(inner=>{scope=createScope(inner,agent);},{inject:['commands','authorization']}));
let noticed,connections=0,failBrowser=false;
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
const controller = createCredentialAuth({ credentials: ctx.credentials, key, binding, refreshAuthorization: sdk.refreshAuthorization, fetchFn });
const metadata = { client_name: 'synthetic-client', redirect_uris: [CALLBACK_URL], response_types: ['code'], grant_types: ['authorization_code', 'refresh_token'], token_endpoint_auth_method: 'none' };
function send(path) {
  return new Promise((resolve, reject) => {
    const r = request({ host: '127.0.0.1', port: server.address().port, path, headers: { host: '127.0.0.1:13346' } }, response => {
      let body = ''; response.setEncoding('utf8'); response.on('data', data => { body += data; });
      response.on('end', () => resolve({ status: response.statusCode, body }));
    });
    r.on('error', reject); r.end();
  });
}
async function waitNotice(run) {
 let timer;
 try {
  return await Promise.race([
   noticed.promise,
   run.then(()=>{throw Error('synthetic_authorization_ended_before_notice');}),
   new Promise((_,reject)=>{timer=setTimeout(()=>reject(Error('synthetic_notice_timeout')),5000);}),
  ]);
 } finally {clearTimeout(timer);}
}
const dispose=registerHostAuthorization(scope.ctx,{key,oauthController:controller,auth:sdk.auth,createServerImpl,
 openBrowser:async url=>{if(failBrowser)throw Error('synthetic-browser-failure');noticed.resolve({url});},connect:async()=>{connections++;}});
try {
 assert.equal(server,undefined); assert.equal(ctx.authorization.list().length,0);
 assert.equal(ctx.commands.list(agent)[0].name,'agenvo-authorize');assert.equal(ctx.commands.list(other).length,0);
 for(const emit of [true,false]) {
  record=undefined;issueEvent=emit;noticed=Promise.withResolvers();
  const run=ctx.commands.execute(agent,'/agenvo-authorize',[],new AbortController().signal);
  const notice=await waitNotice(run);const authorizationUrl=new URL(notice.url);challenge=authorizationUrl.searchParams.get('code_challenge');
  const response=await send(`/oauth/callback?${new URLSearchParams({code:'synthetic-code',state:authorizationUrl.searchParams.get('state'),iss:binding.issuer})}`);
  assert.equal(response.status,200);assert.equal((await run).result.kind,emit?'success':'error');
  assert.equal(server.listening,false);assert.equal(ctx.authorization.list().length,0);
 }
 assert.equal(connections,1);assert.equal(exchange,2);assert.equal(writes,2);
 record=undefined;failBrowser=true;
 assert.equal((await ctx.commands.execute(agent,'/agenvo-authorize',[],new AbortController().signal)).result.kind,'error');
 await new Promise(resolve=>setImmediate(resolve));assert.equal(server.listening,false);assert.equal(writes,2);assert.equal(ctx.authorization.list().length,0);failBrowser=false;

 record=undefined;noticed=Promise.withResolvers();
 let otherScope;await ctx.plugin(Object.assign(inner=>{otherScope=createScope(inner,other);},{inject:['commands','authorization']}));
 const disposeOther=registerHostAuthorization(otherScope.ctx,{key,oauthController:controller,auth:sdk.auth,createServerImpl,openBrowser:async url=>{if(failBrowser)throw Error('synthetic-browser-failure');noticed.resolve({url});},connect:async()=>{connections++;}});
 const run=ctx.commands.execute(agent,'/agenvo-authorize',[],new AbortController().signal);
 await waitNotice(run);
 assert.equal((await ctx.commands.execute(other,'/agenvo-authorize',[],new AbortController().signal)).result.kind,'error');
 assert.equal(ctx.authorization.list().length,1); // losing scope did not remove winning flow
 dispose();assert.equal((await run).result.kind,'error');
 await new Promise(resolve=>setImmediate(resolve));assert.equal(server.listening,false);
 assert.equal(ctx.commands.list(agent).length,0);assert.equal(ctx.authorization.list().length,0);
 // The surviving scope can start a fresh attempt after first owner disposal.
 noticed=Promise.withResolvers();const retry=ctx.commands.execute(other,'/agenvo-authorize',[],new AbortController().signal);
 await waitNotice(retry);disposeOther();assert.equal((await retry).result.kind,'error');
 await new Promise(resolve=>setImmediate(resolve));assert.equal(server.listening,false);assert.equal(ctx.authorization.list().length,0);

 const history=JSON.stringify(agent.session.snapshotEvents());assert.doesNotMatch(history,/state=|synthetic-code|synthetic-access|synthetic-refresh|https:/);
 console.log('actual Commands + Authorization + SDK: success, missing commit, disposal passed; no consent URL in session history');
} finally {dispose();await ctx.fiber.dispose();}
