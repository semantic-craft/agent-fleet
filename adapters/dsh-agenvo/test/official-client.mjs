// Explicit offline integration: node .../official-client.mjs /absolute/approved/dsh-source
// Reuses existing dependencies. Does not install or start DSH, OAuth or mcp-remote.
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {pathToFileURL,fileURLToPath} from 'node:url';
import {resolve,join} from 'node:path';
import {mkdtempSync,realpathSync,rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {mount} from '../index.js';
const root=resolve(process.argv[2]);
const req=createRequire(join(root,'packages/mcp/mcp-client/package.json'));
const load=name=>import(pathToFileURL(req.resolve(name)).href);
const [{Context},scope,McpClient,{default:Tools},{default:SystemPrompt}]=await Promise.all([
 load('@deepseek-ai/cordis'),load('@deepseek-ai/dsh-scope'),import(pathToFileURL(join(root,'packages/mcp/mcp-client/lib/index.js')).href),load('@deepseek-ai/dsh-tools'),load('@deepseek-ai/dsh-system-prompt')]);
const ctx=new Context();let owned;
const temp=realpathSync(mkdtempSync(join(tmpdir(),'fleet-official-')));
try {
 await ctx.plugin(SystemPrompt);await ctx.plugin(Tools);
 const agent={},other={};owned=scope.createScope(ctx,agent);
 const input={relayUrl:'https://fixture.invalid/mcp',nodePath:process.execPath,proxyPath:fileURLToPath(new URL('./stdio-fixture.mjs',import.meta.url)),oauthDirectory:temp,callbackPort:13345};
 await mount(owned.ctx,input,{McpClient,scopeOf:scope.scopeOf,verify:()=>{}});
 assert.deepEqual(ctx.tools.schemas(agent).map(t=>t.name).filter(n=>n.startsWith('mcp__agenvo__')).sort(),['mcp__agenvo__execute','mcp__agenvo__search']);
 assert.equal(ctx.tools.schemas(other).some(t=>t.name.startsWith('mcp__agenvo__')),false);
 const execute=ctx.tools.get('mcp__agenvo__execute',agent);
 const call=async code=>JSON.parse((await execute.execute({code},{signal:new AbortController().signal,agent})).content[0].text);
 assert.equal((await call('submit-fixture')).execution,'accepted');
 assert.equal((await call('unknown-fixture')).execution,'unknown');
 const read=await call('observe-fixture');assert.equal(read.submissions,2);assert.equal(read.result.status,'completed');
 await owned.dispose();owned=undefined;
 assert.equal(ctx.tools.schemas(agent).some(t=>t.name.startsWith('mcp__agenvo__')),false);
 console.log('PASS: official DSH MCP stdio, scoped search/execute, unchanged ACK/unknown/readback, disposal. Fixture only; no OAuth/native dispatch/UI.');
} finally {if(owned)await owned.dispose();await ctx.fiber.dispose();rmSync(temp,{recursive:true,force:true});}
