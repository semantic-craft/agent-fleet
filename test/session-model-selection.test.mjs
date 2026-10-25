import test from 'node:test';
import assert from 'node:assert/strict';
import { selectSessionModel } from '../scripts/session-model-selection.mjs';
const selection = {provider:'provider',model:'model',reasoningEffort:'high'};
const input = {sessionId:'owned', selection, exclusiveSession:true};
function mock({capability=true,receipt,fail}={}) {
 const calls=[];
 const call=async(method,params)=>{calls.push({method,params}); if(method==='session/modelCatalog') return {execution:'accepted',result:{capabilities:{selectModelForSession:capability}}};
 if(fail) throw new Error('private error');
 return receipt ?? {execution:'accepted',result:{scope:'session',status:'applied',selected:selection,seq:0}};};
 return {call,calls};
}
test('explicit applied receipt is not durable; no legacy selection',async()=>{const m=mock();const r=await selectSessionModel(m.call,input);assert.equal(r.status,'applied');assert.equal(r.durable,false);assert.equal(r.nextRequestHeaderRequired,true);assert.deepEqual(m.calls.map(x=>x.method),['session/modelCatalog','session/selectModelForSession']);assert.deepEqual(m.calls[1].params,{request:{sessionId:'owned',...selection}});});
test('old or malformed capabilities never dispatch',async()=>{for(const capability of [false,undefined,'true',1]) {const m=mock({capability});if(capability===undefined) m.call=async()=>({execution:'accepted',result:{}});assert.equal((await selectSessionModel(m.call,input)).code,'unsupported');assert.ok(m.calls.length<=1);}});
test('ownership and malformed input block before reading',async()=>{for(const value of [{...input,exclusiveSession:false},{...input,selection:{...selection,secret:'x'}}]) {const m=mock();assert.equal((await selectSessionModel(m.call,value)).status,'blocked');assert.equal(m.calls.length,0);}});
test('lost or inconsistent write receipt stays unknown without retry',async()=>{for(const options of [{fail:true},{receipt:{execution:'unknown'}},{receipt:{execution:'accepted',result:{scope:'session',status:'applied',selected:{...selection,model:'other'},seq:1}}}]) {const m=mock(options);const r=await selectSessionModel(m.call,input);assert.equal(r.status,'unknown');assert.equal(r.retryAllowed,false);assert.equal(m.calls.length,2);assert.equal(JSON.stringify(r).includes('private'),false);}});
test('explicit rejected operation remains blocked',async()=>{const m=mock({receipt:{execution:'rejected'}});assert.equal((await selectSessionModel(m.call,input)).code,'selection_not_applied');});

test('foreign envelope session cannot confirm this selection',async()=>{const m=mock({receipt:{execution:'accepted',nativeIds:{sessionId:'foreign'},result:{scope:'session',status:'applied',selected:selection,seq:1}}});const r=await selectSessionModel(m.call,input);assert.equal(r.status,'unknown');assert.equal(r.retryAllowed,false);assert.equal(m.calls.length,2);});
