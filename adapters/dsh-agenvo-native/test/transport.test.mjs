import test from 'node:test';
import assert from 'node:assert/strict';
import {connectScoped} from '../transport.js';
function fixture({names=['search','execute'],failRegister=false}={}) {
 const defs=new Map(), effects=[];let calls=0, closes=0,client, options;
 const ctx={fiber:{},effect(fn){effects.push(fn());},on(){},tools:{register(def){if(failRegister&&defs.size)throw Error('conflict');defs.set(def.name,def);return()=>defs.delete(def.name);}}};
 class Client {constructor(){client=this;} async connect(){} async listTools(){return {tools:names.map(name=>({name,inputSchema:{type:'object'}}))};} async close(){closes++;} async callTool(args){calls++;if(args.arguments.fail)throw Error('unknown');return {content:[{type:'text',text:'accepted'}]};}}
 class Transport {constructor(url,o){options=o;}}
 const deps={Client,StreamableHTTPClientTransport:Transport,scopeOf:()=>({}),createMcpToolDefinition:(_,x)=>x};
 const input={relayUrl:'https://fixture.invalid/mcp',authProvider:{token:async()=>'fixture-only'}};
 return {ctx,deps,input,defs,effects,get calls(){return calls;},get closes(){return closes;},get options(){return options;},get client(){return client;}};
}
test('native tools preserve ACK and unknown without replay; disposal blocks retained handles',async()=>{
 const f=fixture();const h=await connectScoped(f.ctx,f.input,f.deps);const tool=f.defs.get('mcp__agenvo__execute');
 assert.equal((await tool.call({},{})).content[0].text,'accepted');
 await assert.rejects(tool.call({fail:true},{}),/unknown/);assert.equal(f.calls,2);
 assert.equal(f.options.onInsufficientScope,'throw');assert.equal(f.options.reconnectionOptions.maxRetries,0);
 await h.dispose();assert.equal(f.defs.size,0);assert.throws(()=>tool.call({},{}),/connection_closed/);assert.equal(f.closes,1);
});
test('root scope and absent authorization never connect',async()=>{
 const f=fixture();await assert.rejects(connectScoped(f.ctx,f.input,{...f.deps,scopeOf:()=>undefined}),/scoped_context_required/);
 await assert.rejects(connectScoped(f.ctx,{...f.input,authProvider:{token:async()=>undefined}},f.deps),/authorization_required/);assert.equal(f.client,undefined);
});
test('unexpected catalogue and partial registration fail closed',async()=>{
 for(const options of [{names:['search','execute','admin']},{failRegister:true}]){const f=fixture(options);await assert.rejects(connectScoped(f.ctx,f.input,f.deps),/native_mcp_connection_failed/);assert.equal(f.defs.size,0);assert.equal(f.closes,1);}
});
test('lost connection unregisters all tools; no reconnect or new submit',async()=>{
 const f=fixture();await connectScoped(f.ctx,f.input,f.deps);f.client.onclose();assert.equal(f.defs.size,0);assert.equal(f.calls,0);
});
