// Existing official source + built peers only. No sockets, credentials or model calls.
// Run node --import /approved/dsh/node_modules/tsx/dist/loader.mjs THIS /approved/dsh
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {pathToFileURL} from 'node:url';
import {join,resolve} from 'node:path';
import {registerStatus} from '../status.js';
import {fleetCallerCreate} from '../../../scripts/dsh-native-caller.mjs';
const root=resolve(process.argv[2]);
const require=createRequire(join(root,'packages/mcp/mcp-client/package.json'));
const {Context}=await import(pathToFileURL(require.resolve('@deepseek-ai/cordis')));
const load=path=>import(pathToFileURL(join(root,path)));
const {default:Commands}=await load('packages/interaction/commands/lib/index.js');
const {default:Sessions,SessionId}=await load('packages/core/session/lib/index.js');
const {createScope}=await load('packages/core/scope/lib/index.js');
const {ApiSessionAgentController}=await load('packages/api/session-controller/src/agent.ts');
const ctx=new Context();await ctx.plugin(Sessions);await ctx.plugin(Commands);
const session=ctx.sessions.create(SessionId('synthetic-caller'),{meta:{cwd:'/synthetic'}});
const agent={id:session.id,session,status:'idle'};
let scope,disposeStatus,current='fleet-agenvo';
async function mount(preset){
 disposeStatus?.();await scope?.ctx.fiber.dispose();
 current=preset;
 if(preset==='fleet-agenvo'){
  await ctx.plugin(Object.assign(inner=>{scope=createScope(inner,agent);},{inject:['commands']}));
  scope.ctx.provide('credentials',{describeRecord:async()=>({configured:false,writable:true}),readRecord(){throw Error('forbidden');}});
  disposeStatus=registerStatus(scope.ctx,{key:'synthetic/grant',connectionState:()=> 'disconnected'});
 }
}
await mount(current);
const presets={resolve:async id=>({id:id??'standard'}),select:async(_agent,id)=>{await mount(id);session.append('agent-preset/selected',{agentPreset:id});}};
// Inject public service boundaries around actual upstream createSession/reuseLive.
// Roster activation is a small fixture mounting real scoped Commands; no runtime mutation.
const host={typert:{lookups:{configure(){}},contexts:{configureHost(){}}},sessions:ctx.sessions,agents:{get:id=>id===agent.id?agent:undefined},
 sessionProjections:{stateOf:(_session,name)=>name==='agentPreset'?current:{openTurnStartSeq:null,lastTurn:0}},get:name=>name==='agentPresets'?presets:undefined};
const api=new ApiSessionAgentController(host);
try{
 assert.equal(ctx.commands.list(agent)[0].name,'agenvo-status');
 const status=await ctx.commands.execute(agent,'/agenvo-status',[],new AbortController().signal);assert.equal(status.result.kind,'success');
 assert.equal(session.snapshotEvents().some(e=>e.type==='turn/start'),false); // status does not make a blank non-reusable
 const explicit=fleetCallerCreate({sessionId:agent.id,workspaceId:'synthetic-workspace'});
 await api.createSession({...explicit.params.request,cwd:'/synthetic',reusable:()=>true});
 assert.equal(current,'fleet-agenvo');assert.equal(ctx.commands.list(agent).length,1);
 await api.createSession({sessionId:agent.id,cwd:'/synthetic',reusable:()=>true});
 assert.equal(current,'standard');assert.equal(ctx.commands.list(agent).length,0);
 await api.createSession({...explicit.params.request,cwd:'/synthetic',reusable:()=>true});
 assert.equal(current,'fleet-agenvo');assert.equal(ctx.commands.list(agent)[0].name,'agenvo-status');
 assert.equal(session.snapshotEvents().some(e=>e.type==='turn/start'),false);
 console.log('official ApiSessionAgentController + Commands: explicit retains, omitted resets, status remains blank; zero credential values/model/network');
}finally{disposeStatus?.();await ctx.fiber.dispose();}
