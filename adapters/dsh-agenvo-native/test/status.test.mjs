import test from 'node:test';
import assert from 'node:assert/strict';
import {registerStatus} from '../status.js';
function fixture(info={configured:true,kind:'grant',writable:true}){
 let definition,reads=0;
 const ctx={commands:{register(value){definition=value;return ()=>{};}},credentials:{describeRecord:async()=>{reads++;return info;},readRecord(){throw Error('must_not_read_values');},modifyRecord(){throw Error('must_not_write');}}};
 const dispose=registerStatus(ctx,{key:'synthetic/grant',connectionState:()=> 'connected'});
 return {dispose,definition,get reads(){return reads;},run:()=>definition.handler({rawInput:'',signal:new AbortController().signal})};
}
test('status reads only official presence metadata and whitelists output',async()=>{
 const f=fixture({configured:true,kind:'grant',writable:true,token:'do-not-return',url:'do-not-return'});
 assert.equal(f.reads,0);const r=await f.run();assert.deepEqual(JSON.parse(r.text),{configured:true,kind:'grant',writable:true,connection:'connected',expiry:'unknown',grantedScopes:'unknown'});assert.equal(f.reads,1);assert.equal(f.definition.recordInput,false);assert.doesNotMatch(r.text,/do-not-return/);f.dispose();
});
test('absent record and invalid metadata are not authentication claims',async()=>{
 const absent=fixture({configured:false,writable:true});assert.equal(JSON.parse((await absent.run()).text).configured,false);
 const bad=fixture({configured:'yes',writable:true});assert.equal((await bad.run()).kind,'error');
});
test('disposed and invalid input perform zero reads',async()=>{const f=fixture();assert.equal((await f.definition.handler({rawInput:'private',signal:new AbortController().signal})).kind,'error');f.dispose();assert.equal((await f.run()).kind,'error');assert.equal(f.reads,0);});
