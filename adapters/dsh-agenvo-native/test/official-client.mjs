// All fetches are synthetic; no sockets, authorization or installed-app access.
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {pathToFileURL} from 'node:url';
import {resolve,join} from 'node:path';
import {connectScoped} from '../transport.js';
const root=resolve(process.argv[2]);
const req=createRequire(join(root,'packages/mcp/mcp-client/package.json'));
const load=name=>import(pathToFileURL(req.resolve(name)).href);
const [sdk,{Context},scope,McpClient,{default:Tools},{default:SystemPrompt}]=await Promise.all([
 load('@modelcontextprotocol/client'),load('@deepseek-ai/cordis'),load('@deepseek-ai/dsh-scope'),import(pathToFileURL(join(root,'packages/mcp/mcp-client/lib/index.js')).href),load('@deepseek-ai/dsh-tools'),load('@deepseek-ai/dsh-system-prompt')]);
let submissions=0, rotations=0, rejected=false;
const fetch=async(url,init)=>{
 assert.equal(String(url),'https://fixture.invalid/mcp');
 if(init.method!=='POST') return new Response(null,{status:405});
 const request=JSON.parse(init.body);
 if(!Object.hasOwn(request,'id'))return new Response(null,{status:202});
 let result;
 switch(request.method){
 case 'initialize':result={protocolVersion:request.params.protocolVersion,capabilities:{tools:{}},serverInfo:{name:'fixture',version:'1'}};break;
 case 'tools/list':result={tools:['search','execute'].map(name=>({name,inputSchema:{type:'object'}}))};break;
 case 'tools/call':
   if(!rejected){rejected=true;return new Response(null,{status:401});}
   if(request.params.arguments.code!=='observe')submissions++;
   result={content:[{type:'text',text:JSON.stringify({execution:request.params.arguments.code==='unknown'?'unknown':'accepted',submissions})}]};break;
 default:throw Error('unexpected_fixture_method');
 }
 return new Response(JSON.stringify({jsonrpc:'2.0',id:request.id,result}),{headers:{'content-type':'application/json'}});
};
const ctx=new Context();let owned;
try{
 await ctx.plugin(SystemPrompt);await ctx.plugin(Tools);
 const agent={},other={};owned=scope.createScope(ctx,agent);
 await owned.ctx.plugin({name:'fixture-native',inject:['tools'],async apply(pluginCtx){await connectScoped(pluginCtx,{relayUrl:'https://fixture.invalid/mcp',authProvider:{token:async()=>`synthetic-${rotations}`,onUnauthorized:async()=>{rotations++;}}},{...sdk,scopeOf:scope.scopeOf,createMcpToolDefinition:McpClient.createMcpToolDefinition,fetch});}});
 assert.deepEqual(ctx.tools.schemas(agent).map(t=>t.name).filter(n=>n.startsWith('mcp__agenvo__')).sort(),['mcp__agenvo__execute','mcp__agenvo__search']);
 assert.equal(ctx.tools.schemas(other).some(t=>t.name.startsWith('mcp__agenvo__')),false);
 const tool=ctx.tools.get('mcp__agenvo__execute',agent);
 const call=async code=>JSON.parse((await tool.execute({code},{agent,signal:new AbortController().signal})).content[0].text);
 assert.equal((await call('submit')).execution,'accepted');assert.equal(rotations,1);assert.equal((await call('unknown')).execution,'unknown');assert.equal((await call('observe')).submissions,2);
 await owned.dispose();owned=undefined;assert.equal(ctx.tools.schemas(agent).length,0);
 console.log('PASS real SDK2.2 HTTP + DSH rc3 projection/scope/disposal; synthetic fetch only, no OAuth/live dispatch/Agent approval execution');
}finally{if(owned)await owned.dispose();await ctx.fiber.dispose();}
