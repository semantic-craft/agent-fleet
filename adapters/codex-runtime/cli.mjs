#!/usr/bin/env node
import { readFileSync, mkdirSync, lstatSync, openSync, closeSync, writeFileSync, unlinkSync } from 'node:fs';
import { isAbsolute, join } from 'node:path';
import { execFileSync } from 'node:child_process';
import { parseArgs } from 'node:util';
import { randomUUID } from 'node:crypto';
import { OwnedRuntimeManager } from './manager.mjs';
import { StdioWorker } from './worker.mjs';
import { serve } from './server.mjs';

const {values}=parseArgs({options:{config:{type:'string'},status:{type:'boolean'}}});
if(!values.config)throw new Error('--config PATH required');
const cfg=JSON.parse(readFileSync(values.config,'utf8'));
for(const key of ['command','stateDir','socketPath','cwd','codexHome'])if(!isAbsolute(cfg[key]??''))throw new Error(`${key} must be absolute`);
if(values.status){
  const saved=JSON.parse(readFileSync(join(cfg.stateDir,'runtime.json'),'utf8'));
  console.log(JSON.stringify({offline:true,units:saved.units.map(u=>({...u,...(u.state==='released'?{}:{state:'unknown',reason:'offline inspection cannot establish live ownership'})}))},null,2));
}else{
  mkdirSync(cfg.stateDir,{recursive:true,mode:0o700});
  const st=lstatSync(cfg.stateDir);if(st.isSymbolicLink()||!st.isDirectory()||(st.mode&0o077)||st.uid!==process.getuid())throw new Error('private state directory required');
  const version=execFileSync(cfg.command,['--version'],{encoding:'utf8',stdio:['ignore','pipe','pipe']}).trim();
  console.log(`native runtime: ${version}`);
  const lock=join(cfg.stateDir,'service.lock'),token=randomUUID();
  let fd;try{fd=openSync(lock,'wx',0o600);}catch{throw new Error('owned runtime lock exists: inspect offline metadata and reconcile owned processes; no automatic stale-lock takeover');}
  writeFileSync(fd,JSON.stringify({token,pid:process.pid,createdAt:new Date().toISOString()}));closeSync(fd);
  const manager=new OwnedRuntimeManager({stateDir:cfg.stateDir,createWorker:()=>new StdioWorker({command:cfg.command,cwd:cfg.cwd,env:{...process.env,CODEX_HOME:cfg.codexHome}})});
  const service=await serve({socketPath:cfg.socketPath,manager,onClose:()=>{
    if(JSON.parse(readFileSync(lock,'utf8')).token!==token)throw new Error('service lock ownership changed');
    unlinkSync(lock);process.exitCode=0;
  }});
  // Explicit signals refuse busy shutdown. SIGKILL/system loss cannot be made safe:
  // the retained lock and metadata require reconciliation instead of input replay.
  for(const signal of ['SIGTERM','SIGINT'])process.on(signal,()=>{
    if(manager.status().some(u=>u.state!=='released')){console.error('shutdown refused: owned workers remain');return;}
    service.close().catch(e=>console.error(e.message));
  });
  console.log('owned runtime listening on private Unix endpoint');
}
