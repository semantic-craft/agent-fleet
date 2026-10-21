import { spawn } from 'node:child_process';
import { EventEmitter } from 'node:events';
import { createInterface } from 'node:readline';

/** One owned stdio connection. No retries, no signal/kill fallback. */
export class StdioWorker extends EventEmitter {
  constructor({ command, args = ['app-server'], env = process.env, cwd, timeoutMs = 30_000 }) {
    super(); this.next = 0; this.pending = new Map(); this.timeoutMs = timeoutMs;
    this.child = spawn(command, args, { env, cwd, stdio: ['pipe', 'pipe', 'ignore'] });
    this.exited = new Promise(resolve => this.child.once('close', (code, signal) => {
      this.dead = true; this.fail(new Error('worker exited; execution_unknown')); resolve({ code, signal }); this.emit('exit', { code, signal });
    }));
    this.child.once('exit',()=>{this.dead=true;});
    this.child.on('error', e => { this.fail(e); this.emit('fault', e); });
    this.child.stdin.on('error', e => { this.fail(e); this.emit('fault', e); });
    const lines = createInterface({ input: this.child.stdout });
    lines.on('line', line => {
      try {
        const p = JSON.parse(line);
        if (!p || typeof p !== 'object') throw new Error('invalid native packet');
        if (!p.method && p.id !== undefined) {
          const request = this.pending.get(p.id);
          if (!request) throw new Error('unmatched native response');
          this.pending.delete(p.id); clearTimeout(request.timer);
          if (p.error) request.reject(Object.assign(new Error(p.error.message ?? 'native rejection'), { native: p.error }));
          else request.resolve(p.result);
        } else this.emit('packet', p);
      } catch(e) { this.emit('fault', e); }
    });
  }
  fail(error) { for (const p of this.pending.values()) { clearTimeout(p.timer); p.reject(error); } this.pending.clear(); }
  send(packet) { if (this.dead || this.child.stdin.writableEnded) throw new Error('worker unavailable'); this.child.stdin.write(JSON.stringify(packet)+'\n'); }
  rpc(method, params = {}) {
    const id = ++this.next;
    return new Promise((resolve,reject) => {
      const timer = setTimeout(() => { this.pending.delete(id); const e = new Error('execution_unknown: native response timeout'); reject(e); this.emit('fault',e); }, this.timeoutMs);
      this.pending.set(id,{resolve,reject,timer});
      try { this.send({id,method,params}); } catch(e) { clearTimeout(timer); this.pending.delete(id); reject(e); }
    });
  }
  async initialize() {
    const result = await this.rpc('initialize',{clientInfo:{name:'agent-fleet-owned-runtime',version:'0.1.0'},capabilities:{experimentalApi:true}});
    this.send({method:'initialized',params:{}}); return result;
  }
  async eof() { this.child.stdin.end(); return this.exited; }
}
