import { EventEmitter } from 'node:events';

// A controllable native protocol peer. It never executes a command or model.
export class FakeWorker extends EventEmitter {
  constructor(history = new Map(), id = 'root') {
    super(); this.history = history; this.id = id; this.calls = [];
    this.loaded = new Set(); this.eofCount = 0; this.turnSequence = 0;
    this.queue = []; this.background = []; this.replies = [];
  }
  async initialize() { return { codexHome: '/fixture', userAgent: 'codex/0.163.0' }; }
  send(packet) { this.replies.push(packet); }
  emitPacket(method, params, id) { this.emit('packet', { method, params, ...(id === undefined ? {} : { id }) }); }
  thread(id = this.id) {
    if (!this.history.has(id)) this.history.set(id, { id, status: { type: 'idle' }, turns: [] });
    return this.history.get(id);
  }
  complete(turnId, threadId = this.id) {
    const thread = this.thread(threadId);
    const turn = thread.turns.find(v => v.id === turnId);
    if (!turn) throw new Error('Unknown fixture turn');
    turn.status = 'completed'; thread.status.type = 'idle';
    this.emitPacket('turn/completed', { threadId, turn: { ...turn } });
  }
  async rpc(method, params = {}) {
    this.calls.push({ method, params });
    if (this.intercept) { const answer = await this.intercept(method, params); if (answer !== undefined) return answer; }
    const id = params.threadId ?? this.id;
    switch (method) {
      case 'thread/start': this.loaded.add(this.id); return { thread: structuredClone(this.thread()) };
      case 'thread/resume': this.id = id; this.loaded.add(id); return { thread: structuredClone(this.thread(id)) };
      case 'thread/read': return { thread: structuredClone(this.thread(id)) };
      case 'thread/turns/list': return { data: structuredClone(this.thread(id).turns), nextCursor: null };
      case 'thread/loaded/list': return { data: [...this.loaded], nextCursor: null };
      case 'thread/queue/list': return { data: this.queue, nextCursor: null };
      case 'thread/backgroundTerminals/list': return { data: this.background, nextCursor: null };
      case 'thread/list': return { data: [...this.history.values()].map(t => ({ id: t.id })), nextCursor: null };
      case 'model/list': return { data: [] };
      case 'turn/start': {
        const turn = { id: `turn-${++this.turnSequence}`, status: 'inProgress', items: [] };
        this.thread(id).turns.push(turn);
        // Deliberately leave thread.status idle: native acceptance can precede status updates.
        return { turn: structuredClone(turn) };
      }
      case 'thread/unsubscribe': return { status: 'unsubscribed' };
      default: throw new Error(`Fixture does not implement ${method}`);
    }
  }
  async eof() { this.eofCount++; this.emit('exit', { code: 0, signal: null }); return { code: 0, signal: null }; }
}

export function fakeFactory() {
  const workers = [], history = new Map();
  return {
    workers, history,
    createWorker({ id, readOnly } = {}) {
      const worker = new FakeWorker(history, `thread-${workers.length + 1}`);
      worker.runtimeId = id; worker.readOnly = readOnly; workers.push(worker); return worker;
    },
  };
}
