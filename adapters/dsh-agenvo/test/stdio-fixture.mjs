// Owned synthetic MCP server: stdio only, no network, credentials, files or eval.
import {createInterface} from 'node:readline';
let submissions=0;
for await (const line of createInterface({input:process.stdin})) {
 let q;try{q=JSON.parse(line);}catch{continue;}
 if(q.id===undefined) continue;
 let result;
 if(q.method==='initialize') result={protocolVersion:q.params.protocolVersion,capabilities:{tools:{}},serverInfo:{name:'fleet-fixture',version:'1'}};
 else if(q.method==='tools/list') result={tools:['search','execute'].map(name=>({name,description:'Synthetic Agenvo fixture',inputSchema:{type:'object',properties:name==='search'?{query:{type:'string'}}:{code:{type:'string'}}}}))};
 else if(q.method==='tools/call') {
  const {name,arguments:a}=q.params;
  let value;
  if(name==='search') value={instances:[{deviceId:'fixture-device',instanceId:'fixture-codex'}]};
  else if(a.code==='submit-fixture') value={execution:'accepted',requestId:'fixture-envelope',nativeIds:{threadId:'fixture-thread',turnId:'fixture-turn'},result:{status:'inProgress'},submissions:++submissions};
  else if(a.code==='unknown-fixture') value={execution:'unknown',requestId:'fixture-unknown',submissions:++submissions};
  else if(a.code==='observe-fixture') value={result:{status:'completed',text:'FIXTURE_ONLY'},submissions};
  else value={error:{code:'unsupported_fixture_operation'}};
  result={content:[{type:'text',text:JSON.stringify(value)}]};
 } else result={};
 process.stdout.write(JSON.stringify({jsonrpc:'2.0',id:q.id,result})+'\n');
}
