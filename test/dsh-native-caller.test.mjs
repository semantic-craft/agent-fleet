import test from 'node:test';
import assert from 'node:assert/strict';
import {fleetCallerCreate,observeFleetCaller,checkFleetCaller} from '../scripts/dsh-native-caller.mjs';
test('named reuse always transmits the explicit preset, never a default change',()=>{
 const descriptor=fleetCallerCreate({sessionId:'synthetic-session',workspaceId:'synthetic-workspace'});
 assert.deepEqual(descriptor,{method:'session/create',params:{request:{sessionId:'synthetic-session',workspaceId:'synthetic-workspace',agentPreset:'fleet-agenvo'}}});assert.throws(()=>fleetCallerCreate({workspaceId:'synthetic-workspace'}));
});
test('unknown admission recovery only observes original identity, never recreates',async()=>{
 const methods=[];for(const response of [{error:{code:'not-found'}},{result:{kind:'sequenced',asOfSeq:5,values:{agentPreset:'standard'}}},{result:{kind:'sequenced',asOfSeq:5,values:{agentPreset:'fleet-agenvo'}}}]){
  const result=await observeFleetCaller(async(method,args)=>{methods.push(method);assert.equal(args.request.sessionId,'synthetic-session');return response;},{sessionId:'synthetic-session'});
  assert.equal(result.status,response.result?.values.agentPreset==='fleet-agenvo'?'observed':'blocked');
 }assert.deepEqual(methods,['session/projections','session/projections','session/projections']);
});
test('foreign identity and malformed snapshots fail closed',async()=>{
 for(const response of [{nativeIds:{sessionId:'other'},result:{kind:'sequenced',asOfSeq:2,values:{agentPreset:'fleet-agenvo'}}},{result:{values:{agentPreset:'fleet-agenvo'}}}])assert.equal((await observeFleetCaller(async()=>response,{sessionId:'synthetic-session'})).status,'blocked');
});

test('old transport without command discovery fails closed before calls',async()=>{assert.equal((await checkFleetCaller(()=>{throw Error('must not call');},{sessionId:'synthetic-session'})).code,'command_discovery_unsupported');});
test('public fleet caller-plan uses the helper without reading an overlay or OAuth',async()=>{
 const {execFileSync}=await import('node:child_process');
 const output=execFileSync(process.execPath,['scripts/fleet.mjs','caller-plan','--session','synthetic-session','--workspace','synthetic-workspace'],{encoding:'utf8',env:{PATH:process.env.PATH,HOME:'/nonexistent-fixture-home'}});
 assert.deepEqual(JSON.parse(output),fleetCallerCreate({sessionId:'synthetic-session',workspaceId:'synthetic-workspace'}));
});

test('both scoped commands must be discovered; no mutation from missing commands',async()=>{
 let reads=0;const call=async method=>{assert.equal(method,'session/projections');reads++;return {result:{kind:'sequenced',asOfSeq:2,values:{agentPreset:'fleet-agenvo'}}};};
 for(const names of [[],['agenvo-status'],['agenvo-authorize'],['agenvo-status','agenvo-authorize']]){
  const result=await checkFleetCaller(call,{sessionId:'synthetic-session',readCommands:async id=>{assert.equal(id,'synthetic-session');return names.map(name=>({name}));}});
  assert.equal(result.status,names.length===2?'ready':'blocked');
 }assert.equal(reads,4);
});
