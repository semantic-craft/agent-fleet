import test from 'node:test';
import assert from 'node:assert/strict';
import {browserOpener,registerHostAuthorization,COMMAND} from '../action.js';
function fixture(begin=async()=>({status:'authorized'}),connect=async()=>{}) {
 let command, flow, removed=0, opens=0;
 const ctx={commands:{register(value){command=value;return ()=>removed++;}},authorization:{registerFlow(value){flow=value;return ()=>removed++;},begin}};
 const dispose=registerHostAuthorization(ctx,{key:'owner/grant',oauthController:{beginAuthorization(){}},auth(){},openBrowser:async()=>{opens++;},connect});
 return {ctx,dispose,command,flow,get removed(){return removed;},get opens(){return opens;},run:(rawInput='')=>command.handler({rawInput,signal:new AbortController().signal})};
}
test('passive registration discovers human command, creates no grant or connection',()=>{
 let calls=0;const f=fixture(()=>calls++,()=>calls++);assert.equal(calls,0);assert.equal(f.command.name,COMMAND);assert.equal(f.command.recordInput,false);assert.equal(f.flow,undefined);f.dispose();assert.equal(f.removed,1);
});
test('explicit success opens browser once then connects; only static result',async()=>{
 let connected=0;const f=fixture(async r=>{r.interaction.notify({url:'https://issuer.example/authorize?state=private'});return {status:'authorized'};},async()=>connected++);
 const result=await f.run();assert.equal(result.kind,'success');assert.equal(f.opens,1);assert.equal(connected,1);assert.doesNotMatch(JSON.stringify(result),/private|https:/);f.dispose();
});
test('invalid args and disposed owner cannot begin',async()=>{let n=0;const f=fixture(()=>n++);assert.equal((await f.run(' secret')).kind,'error');f.dispose();assert.equal((await f.run()).kind,'error');assert.equal(n,0);});
test('concurrent explicit invocation rejected; abort propagated',async()=>{
 let request;const f=fixture(r=>{request=r;return new Promise(resolve=>r.signal.addEventListener('abort',()=>resolve({status:'cancelled'})));});const first=f.run();await new Promise(resolve=>setImmediate(resolve));assert.equal((await f.run()).kind,'error');f.dispose();assert.equal(request.signal.aborted,true);assert.equal((await first).kind,'error');
});
test('remote errors and failed connection never expose URL or tokens',async()=>{
 for(const f of [fixture(async()=>{throw Error('secret-token');}),fixture(undefined,async()=>{throw Error('secret-token');})]) {const result=await f.run();assert.equal(result.kind,'error');assert.doesNotMatch(result.text,/secret/);f.dispose();}
});
test('native browser uses bounded explicit argv; invalid destinations never execute',async()=>{
 const calls=[];const open=browserOpener({issuer:'https://issuer.example/',platform:'darwin',runNativeCommand:async(...args)=>calls.push(args)});const signal=new AbortController().signal;
 await open('https://issuer.example/auth?state=private',signal);assert.deepEqual(calls[0],['/usr/bin/open',['https://issuer.example/auth?state=private'],signal,'visible']);
 for(const url of ['http://issuer.example/','https://other.example/','https://user@issuer.example/','https://issuer.example/#fragment','bad'])await assert.rejects(open(url,signal));assert.equal(calls.length,1);
 await assert.rejects(browserOpener({issuer:'https://issuer.example/',platform:'linux',runNativeCommand:()=>calls.push(1)})('https://issuer.example/',signal),/unsupported/);assert.equal(calls.length,1);
});
test('browser native errors sanitized',async()=>{await assert.rejects(browserOpener({issuer:'https://issuer.example/',platform:'darwin',runNativeCommand:async()=>{throw Error('private-url');}})('https://issuer.example/',new AbortController().signal),error=>error.message==='authorization_browser_open_failed'&&!error.cause);});

test('unsupported platform never registers a flow or begins authorization',async()=>{
 let handler,flows=0;const dispose=registerHostAuthorization({commands:{register(d){handler=d.handler;return ()=>{};}},authorization:{registerFlow(){flows++;return ()=>{};}}},{key:'owner/grant',oauthController:{beginAuthorization(){}},auth(){},openBrowser(){},connect(){},platformSupported:false});
 assert.equal((await handler({rawInput:'',signal:new AbortController().signal})).kind,'error');assert.equal(flows,0);dispose();
});
test('existing grant retries connection without flow or DCR',async()=>{
 let handler,connects=0;const dispose=registerHostAuthorization({commands:{register(d){handler=d.handler;return ()=>{};}},authorization:{registerFlow(){throw Error('unexpected flow');}}},{key:'owner/grant',oauthController:{beginAuthorization(){}},auth(){},openBrowser(){},connect:async()=>connects++,hasGrant:async()=>true});
 assert.equal((await handler({rawInput:'',signal:new AbortController().signal})).kind,'success');assert.equal(connects,1);dispose();
});

test('late connection resolution after cancellation never reports success',async()=>{
 for(const existing of [true,false]){
  let handler;const started=Promise.withResolvers(),finish=Promise.withResolvers();
  const dispose=registerHostAuthorization({commands:{register(d){handler=d.handler;return ()=>{};}},authorization:{registerFlow(){return ()=>{};},begin:async()=>({status:'authorized'})}},{key:'owner/grant',oauthController:{beginAuthorization(){}},auth(){},openBrowser(){},connect:async()=>{started.resolve();await finish.promise;},hasGrant:async()=>existing});
  const controller=new AbortController();const run=handler({rawInput:'',signal:controller.signal});await started.promise;controller.abort();finish.resolve();assert.equal((await run).kind,'error');dispose();
 }
});
