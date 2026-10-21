import { EventEmitter } from 'node:events';
import { randomUUID } from 'node:crypto';
import { mkdirSync, existsSync, readFileSync, writeFileSync, renameSync, openSync, fsyncSync, closeSync, lstatSync } from 'node:fs';
import { join } from 'node:path';

const terminal = new Set(['completed','failed','interrupted']);
const edits = new Set(['thread/archive','thread/unarchive']);
const reads = new Set(['thread/read','thread/list','thread/turns/list','thread/items/list','model/list']);
const writes = new Set(['turn/start','turn/steer','turn/interrupt','thread/queue/list','thread/backgroundTerminals/list']);
const benign = /^(deprecationNotice$|thread\/goal\/cleared$|remoteControl\/status\/changed$|thread\/(archived|unarchived|closed)$|item\/(agentMessage|reasoning|plan|commandExecution|fileChange)\/|turn\/(diff|plan)\/updated$|thread\/(name\/updated|readState\/changed|tokenUsage\/updated|status\/changed)$|account\/|model\/|mcpServer\/|warning$)/;
const blocked = message => Object.assign(new Error(message),{code:-32001});

/** Metadata contains identifiers/state only, never prompts, results or credentials. */
export class OwnedRuntimeManager extends EventEmitter {
  constructor({stateDir,createWorker,persist}) {
    super(); mkdirSync(stateDir,{recursive:true,mode:0o700});
    const st=lstatSync(stateDir); if(!st.isDirectory()||st.isSymbolicLink()||(st.mode&0o077)||st.uid!==process.getuid()) throw blocked('private state directory required');
    this.file=join(stateDir,'runtime.json');this.persist=persist; this.createWorker=createWorker; this.units=new Map(); this.routes=new Map(); this.pending=new Map();this.resuming=new Set();
    if(existsSync(this.file)) {
      const saved=JSON.parse(readFileSync(this.file,'utf8')); if(saved.version!==1) throw blocked('unknown metadata version');
      for(const r of saved.units) {
        if(r.state!=='released') {r.state='unknown';r.reason='manager restarted; ownership requires manual reconciliation';}
        const u={record:r,threads:new Set(r.threads),references:new Set(r.references??[]),turns:new Map(r.turns),active:0,epoch:0,requests:new Map()}; this.units.set(r.id,u);
        for(const id of r.threads) this.routes.set(id,u);
      }
      this.save();
    }
    this.timer=setInterval(()=>{for(const u of this.units.values())if(u.record.state==='open'&&u.turns.size&&[...u.turns.values()].every(s=>terminal.has(s)))this.releaseUnit(u).catch(()=>{});},5000);this.timer.unref();
  }
  save() {
    try {
    if(this.persistenceFault)throw blocked('metadata unavailable; runtime retained');
    const units=[...this.units.values()].map(u=>({...u.record,threads:[...u.threads],references:[...u.references],turns:[...u.turns]}));
    if(this.persist){this.persist({version:1,units});return;}
    const tmp=this.file+'.tmp'; writeFileSync(tmp,JSON.stringify({version:1,units})+'\n',{mode:0o600});
    const fd=openSync(tmp,'r');try{fsyncSync(fd);}finally{closeSync(fd);} renameSync(tmp,this.file);
    const dir=openSync(join(this.file,'..'),'r');try{fsyncSync(dir);}finally{closeSync(dir);}
    }catch(e){
      this.persistenceFault=true;
      for(const u of this.units.values()){u.record.state='unknown';u.record.reason='metadata persistence failed; worker retained';}
      throw blocked('metadata persistence failed; input not replayed and workers retained');
    }
  }
  unknown(u,reason) {if(u.record.state!=='unknown')u.record.reason=reason;u.record.state='unknown';this.save();}
  status() {return [...this.units.values()].map(u=>({...u.record,threads:[...u.threads],references:[...u.references],turns:[...u.turns]}));}
  pendingPackets() {return [...this.pending].map(([id,{u}])=>({...u.requests.get(id),id}));}
  async initialize() { if(this.persistenceFault)throw blocked('metadata unavailable; runtime retained');if(this.draining)throw blocked('service draining'); const u=await this.newUnit(true); const result=u.initialized; await this.releaseUnit(u); return result; }
  async newUnit(readOnly=false) {
    const id=randomUUID(),u={record:{id,state:'starting',readOnly,createdAt:new Date().toISOString(),managerPid:process.pid,threads:[],turns:[]},threads:new Set(),references:new Set(),turns:new Map(),active:0,epoch:0,requests:new Map()};
    this.units.set(id,u);this.save();
    try {
      u.worker=await this.createWorker({id,readOnly});
      u.record.childPid=u.worker.child?.pid??null;u.record.childObservedAt=new Date().toISOString();this.save();
      const event=fn=>{try{fn();}catch{u.record.state='unknown';u.record.reason=this.persistenceFault?'metadata persistence failed; worker retained':'native event processing failed';}};
      u.worker.on('packet',p=>event(()=>this.packet(u,p)));u.worker.on('fault',()=>event(()=>this.unknown(u,'native transport/protocol failure')));
      u.worker.on('exit',()=>event(()=>{if(u.record.state!=='closing')this.unknown(u,'unexpected worker exit');}));
      u.initialized=await u.worker.initialize();if(u.record.state!=='starting')throw blocked('worker initialization became unknown');u.record.state='open';this.save();return u;
    }catch(e){this.unknown(u,'worker initialization failed');throw e;}
  }
  claim(u,id) {
    const previous=this.routes.get(id);
    if(previous&&previous!==u&&previous.record.state!=='released'){this.unknown(u,'thread ownership conflicts with another managed worker');return false;}
    u.threads.add(id);this.routes.set(id,u);return true;
  }
  packet(u,p) {
    if(!benign.test(p.method??'')||p.method==='thread/status/changed')u.epoch++;
    const v=p.params??{},threadId=v.threadId??v.thread?.id;
    if(!u.record.readOnly&&threadId&&(['thread/started','turn/started','turn/completed','item/started','item/completed'].includes(p.method)||u.threads.has(threadId))){this.claim(u,threadId);}
    if(p.id!==undefined&&p.method) {
      const id=`${u.record.id}:${String(p.id)}`;this.pending.set(id,{u,nativeId:p.id});u.requests.set(id,p);this.save();this.emit('packet',{...p,id});return;
    }
    if(p.method==='serverRequest/resolved') {
      for(const [id,entry] of this.pending)if(entry.u===u&&entry.nativeId===v.requestId){this.pending.delete(id);u.requests.delete(id);}
      if(v.requestId===undefined)this.unknown(u,'resolved request missing identity');
      else p={...p,params:{...v,requestId:`${u.record.id}:${String(v.requestId)}`}};
    } else if(p.method==='turn/started'||p.method==='turn/completed') {
      if(!v.turn?.id||!threadId) this.unknown(u,'turn event missing identity');
      else u.turns.set(`${threadId}:${v.turn.id}`,p.method==='turn/completed'&&terminal.has(v.turn.status)?v.turn.status:'active');
    } else if(p.method==='thread/started') {
      if(!threadId)this.unknown(u,'thread event missing identity');
    } else if(p.method==='item/started'||p.method==='item/completed') {
      const item=v.item;
      if(p.method==='item/completed'&&item?.id&&threadId) {
        for(const [id,entry] of this.pending) {
          const request=entry.u.requests.get(id);
          if(entry.u===u&&request?.params?.threadId===threadId&&request.params.itemId===item.id){this.pending.delete(id);u.requests.delete(id);}
        }
      }
      if(!item?.type)this.unknown(u,'item missing type');
      if(item?.type==='collabAgentToolCall') {
        if(!Array.isArray(item.receiverThreadIds)||item.receiverThreadIds.some(id=>typeof id!=='string'))this.unknown(u,'missing subagent identities');
        else if(!u.record.readOnly)for(const id of item.receiverThreadIds)u.references.add(id);
        if(!['completed','failed','interrupted'].includes(item.status)&&p.method==='item/completed')this.unknown(u,'unconfirmed subagent operation');
      }
    } else if(!benign.test(p.method??'')) this.unknown(u,`unclassified notification: ${p.method}`);
    this.save();this.emit('packet',p);
    if(['turn/completed','item/completed','serverRequest/resolved'].includes(p.method))setImmediate(()=>{if(u.turns.size)this.releaseUnit(u).catch(()=>{});});
  }
  async invoke(u,method,params) {
    if(u.record.state!=='open')throw blocked(`runtime ${u.record.state}; input not sent`);
    u.active++;u.epoch++;u.record.inflight=method;this.save();
    try {
      const result=await u.worker.rpc(method,params);
      if(method==='thread/start'||method==='thread/resume') {
        if(!result?.thread?.id)throw blocked('native thread identity missing');
        if(!this.claim(u,result.thread.id))throw blocked('native thread ownership conflict');
      }
      if(method==='turn/start') {
        if(!result?.turn?.id)throw blocked('accepted turn identity missing');
        const key=`${params.threadId}:${result.turn.id}`;
        if(!terminal.has(u.turns.get(key)))u.turns.set(key,'active');
      }
      return result;
    } catch(e) {if(!e.native)this.unknown(u,'execution_unknown; request not replayed');throw e;}
    finally {u.active--;delete u.record.inflight;this.save();if(method==='turn/start'&&[...u.turns.values()].some(s=>terminal.has(s)))setImmediate(()=>this.releaseUnit(u).catch(()=>{}));}
  }
  async call(method,params={}) {
    if(method==='runtime/status')return this.status();
    if(this.persistenceFault)throw blocked('metadata unavailable; runtime retained');
    if(this.draining)throw blocked('service draining; input not sent');
    if(method==='runtime/release')return this.release(params.threadId);
    if(method==='thread/loaded/list')return {data:[...this.routes].filter(([,u])=>u.record.state!=='released').map(([id])=>id),nextCursor:null};
    if(method==='thread/unsubscribe') {await this.release(params.threadId);return {status:'unsubscribed'};}
    if(method==='thread/start')return this.invoke(await this.newUnit(),'thread/start',params);
    if(method==='thread/resume') {
      if(!params.threadId)throw blocked('threadId required; history/path resume unsupported');
      let u=this.routes.get(params.threadId);
      if(!u||u.record.state==='released') {
        if(this.resuming.has(params.threadId))throw blocked('concurrent resume; input not sent');
        this.resuming.add(params.threadId);const previous=u;
        try {
          u=await this.newUnit();
          u.threads.add(params.threadId);this.routes.set(params.threadId,u);this.save();
          try{return await this.invoke(u,method,params);}catch(e){
            if(e.native&&u.turns.size===0){
              u.threads.delete(params.threadId);if(previous)this.routes.set(params.threadId,previous);else this.routes.delete(params.threadId);this.save();
              await this.releaseUnit(u).catch(()=>{});
            }
            throw e;
          }
        }finally{this.resuming.delete(params.threadId);}
      }
      return this.invoke(u,method,params);
    }
    if(reads.has(method)||edits.has(method)) {
      const own=this.routes.get(params.threadId);
      if(own?.worker&&own.record.state==='open') {
        const result=await this.invoke(own,method,params);
        return method==='thread/read'?{...result,managedRuntime:{id:own.record.id,state:own.record.state,reason:own.record.reason}}:result;
      }
      const u=await this.newUnit(true);
      try {const result=await this.invoke(u,method,params);return method==='thread/read'?{...result,managedRuntime:own?{id:own.record.id,state:own.record.state,reason:own.record.reason}:null}:result;} finally {await this.releaseUnit(u);}
    }
    if(!writes.has(method))throw blocked(`unsupported managed method: ${method}`);
    const u=this.routes.get(params.threadId);if(!u)throw blocked('explicit thread/resume required');
    return this.invoke(u,method,params);
  }
  reply(id,response) {
    const p=this.pending.get(id);if(!p)throw blocked('unknown pending request');
    if(p.u.record.state!=='open')throw blocked('runtime not open');
    try{p.u.worker.send({id:p.nativeId,...response});}catch(e){this.unknown(p.u,'pending reply transport unknown');throw e;}this.pending.delete(id);p.u.requests.delete(id);p.u.epoch++;this.save();
  }
  async pages(u,method,params={}) {
    const data=[],seen=new Set();let cursor;
    do {
      const page=await u.worker.rpc(method,{...params,...(cursor?{cursor}:{} )});
      if(!Array.isArray(page?.data))throw blocked('malformed safety response');data.push(...page.data);
      cursor=page.nextCursor;if(cursor){if(typeof cursor!=='string'||seen.has(cursor))throw blocked('invalid pagination');seen.add(cursor);}
    }while(cursor);return data;
  }
  async release(threadId) {const u=this.routes.get(threadId);if(!u)throw blocked('unmanaged thread');return this.releaseUnit(u);}
  async releaseUnit(u) {
    if(u.record.state==='released')return {released:true};
    if(u.record.state!=='open'||u.active||u.requests.size||[...u.turns.values()].some(s=>!terminal.has(s)))throw blocked('runtime busy or unknown; retained');
    u.record.state='checking';this.save();const epoch=u.epoch;
    try {
      const loaded=await this.pages(u,'thread/loaded/list');
      if(u.record.readOnly&&loaded.length)throw blocked('read-only worker unexpectedly loaded threads');
      for(const id of new Set([...loaded,...u.threads,...u.references])) {
        if(typeof id!=='string')throw blocked('invalid loaded thread identity');
        const r=await u.worker.rpc('thread/read',{threadId:id,includeTurns:false});
        const turns=await this.pages(u,'thread/turns/list',{threadId:id});
        if(!['idle','notLoaded'].includes(r?.thread?.status?.type)||turns.some(t=>!terminal.has(t.status)))throw blocked('thread not conclusively idle');
        for(const [key,status] of u.turns)if(key.startsWith(id+':')&&(!terminal.has(status)||!turns.some(t=>key===`${id}:${t.id}`&&terminal.has(t.status))))throw blocked('accepted turn missing from terminal history');
        const background=await this.pages(u,'thread/backgroundTerminals/list',{threadId:id});
        const queue=await this.pages(u,'thread/queue/list',{threadId:id});
        u.record.background=(u.record.background??[]).filter(p=>p.threadId!==id).concat(background.map(p=>({threadId:id,osPid:p.osPid??null,processId:p.processId})));this.save();
        if(background.length||queue.length)throw blocked('background terminals or queued input remain');
      }
      if(u.epoch!==epoch||u.active||u.requests.size||u.record.state!=='checking')throw blocked('state changed during safety check');
      u.record.state='closing';this.save();const exit=await u.worker.eof();
      if(u.record.state!=='closing'||u.epoch!==epoch||exit.code!==0||exit.signal)throw blocked('worker did not exit normally');
      u.record.state='released';delete u.record.reason;this.save();for(const threadId of u.threads)this.emit('packet',{method:'thread/closed',params:{threadId,managedRuntime:{id:u.record.id,state:'released'}}});return {released:true};
    }catch(e){if(u.record.state==='checking')u.record.state='open';else if(u.record.state==='closing')u.record.state='unknown';if(u.record.state!=='unknown'||!u.record.reason)u.record.reason=e.message;this.save();throw e;}
  }
}
