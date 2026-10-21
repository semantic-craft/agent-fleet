import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, rmSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawn, spawnSync } from 'node:child_process';
import { ownedPlists, treeHashes } from '../scripts/release-lib.mjs';
import { runtimeRequest, runtimeConfig, stopRuntime } from '../scripts/release-codex-runtime.mjs';

const A='a'.repeat(40), B='b'.repeat(40), label='io.agent-fleet.codex-runtime';
const moduleURL=new URL('../scripts/release.mjs',import.meta.url).href;
const fixtureServer=`import fs from 'node:fs';import http from 'node:http';
const c=JSON.parse(fs.readFileSync(process.argv[2]));
fs.writeFileSync(c.stateDir+'/service.lock','fixture');
const s=http.createServer((req,res)=>{
 res.setHeader('content-type','application/json');
 if(req.url==='/health')return res.end(JSON.stringify({ok:true,service:'codex-owned-runtime',pid:process.pid,units:fs.existsSync(c.busy)?[{state:'active'}]:[]}));
 if(req.url==='/shutdown'&&req.method==='POST'){
  if(fs.existsSync(c.busy)||fs.existsSync(c.race)){res.statusCode=409;return res.end(JSON.stringify({error:'busy'}));}
  res.end(JSON.stringify({ok:true,pid:process.pid}));s.close(()=>{fs.unlinkSync(c.stateDir+'/service.lock');process.exit(0);});return;
 }res.statusCode=404;res.end('{}');
});s.listen(c.socketPath);`;
function lab(t){
 const home=mkdtempSync(join(tmpdir(),'runtime-release-')),root=join(home,'.local/share/agent-fleet/releases/codex-runtime'),agents=join(home,'Library/LaunchAgents'),bin=join(home,'bin'),state=join(home,'state');
 for(const p of [agents,bin,state])mkdirSync(p,{recursive:true,mode:0o700});
 const config={socketPath:join(state,'native.sock'),stateDir:state,busy:join(home,'busy'),race:join(home,'race')}, configFile=join(home,'config.json');
 writeFileSync(configFile,JSON.stringify(config));
 for(const sha of [A,B]){const dir=join(root,sha,'tree/adapters/codex-runtime');mkdirSync(dir,{recursive:true});writeFileSync(join(dir,'cli.mjs'),fixtureServer);writeFileSync(join(root,sha,'release.json'),JSON.stringify({files:treeHashes(join(root,sha))}));}
 const plist=join(agents,label+'.plist'), marker=join(home,'registration'), calls=join(home,'calls');
 const definition={Label:label,ProgramArguments:[process.execPath,join(root,A,'tree/adapters/codex-runtime/cli.mjs'),'--config',configFile],KeepAlive:{SuccessfulExit:false}};
 writeFileSync(plist,JSON.stringify(definition));
 const other=join(agents,'dev.shared.plist');writeFileSync(other,JSON.stringify({Label:'dev.shared',ProgramArguments:['codex','app-server','--listen','unix://']}));
 const common=`import fs from 'node:fs';import {spawn} from 'node:child_process';const a=process.argv.slice(2);`;
 const exe=(name,body)=>writeFileSync(join(bin,name),`#!${process.execPath}\n${common}${body}`,{mode:0o755});
 exe('git',`if(a.includes('show'))process.exit(90);`);
 exe('plutil',`const f=a.at(-1),d=JSON.parse(fs.readFileSync(f));if(a[0]==='-convert')console.log(JSON.stringify(d));else if(a[0]==='-replace'){d.ProgramArguments=JSON.parse(a[3]);fs.writeFileSync(f,JSON.stringify(d));}else process.exit(90);`);
 exe('launchctl',`const marker=${JSON.stringify(marker)};fs.appendFileSync(${JSON.stringify(calls)},JSON.stringify(a)+'\\n');
 if(a[0]==='print'){if(!fs.existsSync(marker)){console.error('Could not find service');process.exit(1);}}
 else if(a[0]==='bootout'){if(fs.existsSync(marker)){const pid=Number(fs.readFileSync(marker));try{process.kill(pid,0);console.error('BOOTOUT_WHILE_RUNNING');process.exit(91);}catch(e){if(e.code!=='ESRCH')throw e;}fs.unlinkSync(marker);}}
 else if(a[0]==='bootstrap'){const p=JSON.parse(fs.readFileSync(a[2]));const child=spawn(p.ProgramArguments[0],[p.ProgramArguments[1],p.ProgramArguments[3]],{detached:true,stdio:'ignore',env:{PATH:${JSON.stringify(process.env.PATH)},HOME:${JSON.stringify(home)}}});fs.writeFileSync(marker,String(child.pid));child.unref();}else process.exit(90);`);
 const run=opts=>new Promise(resolve=>{const p=spawn(process.execPath,['--input-type=module','-e',`import {release} from ${JSON.stringify(moduleURL)};await release('codex-runtime',${JSON.stringify({cfg:{},home,runtimeTimeoutMs:3000,...opts})});`],{env:{PATH:bin,HOME:home},stdio:['ignore','pipe','pipe']});let out='',err='';p.stdout.on('data',d=>out+=d);p.stderr.on('data',d=>err+=d);p.on('exit',code=>resolve({code,out,err}));});
 t.after(async()=>{rmSync(config.busy,{force:true});rmSync(config.race,{force:true});try{await stopRuntime({...config,lockPath:join(state,'service.lock')},existsSync(marker));}catch{}rmSync(home,{recursive:true,force:true});});
 return {home,config,configFile,root,plist,marker,calls,other,definition,run};
}
test('owned runtime component cannot select shared native or connector services',()=>{
 const rows=[{label,programArg:'/release/tree/adapters/codex-runtime/cli.mjs'},{label:'dev.xw.codex-app-server',programArg:'app-server'},{label:'io.agenvo.connector.x',programArg:'/apps/codex-app-server/cli.js'},{label:'wrong',programArg:'/adapters/codex-runtime/cli.mjs'}];
 assert.deepEqual(ownedPlists('codex-runtime',rows),[rows[0]]);
 assert.throws(()=>ownedPlists('codex-runtime',rows,'codex'),/supports only/);
});
test('runtime release installs, gracefully upgrades and rolls back only dedicated service',async t=>{
 const f=lab(t),other=readFileSync(f.other,'utf8');
 let r=await f.run({sha:A});assert.equal(r.code,0,r.err);const first=await runtimeRequest(f.config);
 r=await f.run({sha:B});assert.equal(r.code,0,r.err);assert.notEqual((await runtimeRequest(f.config)).pid,first.pid);
 r=await f.run({rollback:true});assert.equal(r.code,0,r.err);assert.match(readFileSync(f.plist,'utf8'),new RegExp(A));
 assert.equal(readFileSync(f.other,'utf8'),other);assert.ok(!readFileSync(f.calls,'utf8').includes('dev.shared'));t.diagnostic('first install, upgrade and rollback: graceful shutdown before each own bootout; shared plist unchanged');
});
for(const guard of ['busy','race'])test(`runtime ${guard} blocks release before any plist change or bootout`,async t=>{
 const f=lab(t);let r=await f.run({sha:A});assert.equal(r.code,0,r.err);
 writeFileSync(f.config[guard],'');const before=readFileSync(f.plist,'utf8'),calls=readFileSync(f.calls,'utf8');r=await f.run({sha:B});assert.notEqual(r.code,0);assert.match(r.err,/unreleased|refused/);
 assert.equal(readFileSync(f.plist,'utf8'),before);const added=readFileSync(f.calls,'utf8').slice(calls.length);assert.ok(!added.includes('bootout'));assert.ok(!added.includes('bootstrap'));
});
test('runtime configuration rejects unsafe restart policy and stale endpoint is never removed',async t=>{
 const f=lab(t);assert.throws(()=>runtimeConfig([{label,args:f.definition.ProgramArguments,keepAlive:true}]),/KeepAlive/);
 writeFileSync(join(f.config.stateDir,'service.lock'),'stale');const r=await f.run({sha:A});assert.notEqual(r.code,0);assert.ok(existsSync(join(f.config.stateDir,'service.lock')));assert.ok(!readFileSync(f.calls,'utf8').includes('bootout'));
});

test('runtime build carries its pinned ws dependency and runs without checkout node_modules',t=>{
 const home=mkdtempSync(join(tmpdir(),'runtime-package-'));t.after(()=>rmSync(home,{recursive:true,force:true}));
 const source=join(home,'source'),bin=join(home,'bin'),dest=join(home,'release');
 mkdirSync(join(source,'adapters/codex-runtime/test'),{recursive:true});mkdirSync(bin);mkdirSync(dest);
 writeFileSync(join(source,'adapters/codex-runtime/cli.mjs'),"import {WebSocket} from 'ws'; if(typeof WebSocket !== 'function')throw new Error('missing ws');");
 writeFileSync(join(source,'adapters/codex-runtime/test/unused.mjs'),'throw 0;');
 writeFileSync(join(source,'package.json'),JSON.stringify({dependencies:{ws:'8.22.0'}}));writeFileSync(join(source,'package-lock.json'),'{}');
 const wsDir=dirname(fileURLToPath(import.meta.resolve('ws')));
 writeFileSync(join(bin,'git'),`#!${process.execPath}\nimport {spawnSync} from 'node:child_process';const r=spawnSync('/usr/bin/tar',['-cf','-','-C',${JSON.stringify(source)},'adapters','package.json','package-lock.json'],{stdio:'inherit'});process.exit(r.status);`,{mode:0o755});
 writeFileSync(join(bin,'npm'),`#!${process.execPath}\nimport fs from 'node:fs';const a=process.argv.slice(2);if(!a.includes('ci')||!a.includes('--ignore-scripts')||!a.includes('--omit=optional'))process.exit(90);fs.mkdirSync('node_modules');fs.cpSync(${JSON.stringify(wsDir)},'node_modules/ws',{recursive:true});`,{mode:0o755});
 const r=spawnSync(process.execPath,['--input-type=module','-e',`import {build} from ${JSON.stringify(moduleURL)};build('codex-runtime',{dir:${JSON.stringify(source)}},${JSON.stringify(A)},${JSON.stringify(dest)});`],{env:{PATH:bin+':/usr/bin:/bin',HOME:home},stdio:['ignore','pipe','pipe'],encoding:'utf8'});
 assert.equal(r.status,0,r.stderr);assert.equal(existsSync(join(dest,'.src')),false);assert.equal(existsSync(join(dest,'tree/adapters/codex-runtime/test')),false);
 assert.equal(JSON.parse(readFileSync(join(dest,'tree/node_modules/ws/package.json'),'utf8')).version,'8.22.0');
 const smoke=spawnSync(process.execPath,[join(dest,'tree/adapters/codex-runtime/cli.mjs')],{cwd:home,env:{HOME:home},encoding:'utf8'});assert.equal(smoke.status,0,smoke.stderr);
});
