/** Isolated process fixture. Every transport response is synthetic; never network. */
import fs from 'node:fs';
import childProcess from 'node:child_process';
import { syncBuiltinESMExports } from 'node:module';
import { once } from 'node:events';
import { createHash } from 'node:crypto';
import { join } from 'node:path';
const options = JSON.parse(process.argv[2]);
const { directory, backend, trace, mode = 'run', requestId = 'synthetic-request' } = options;
const log = value => fs.appendFileSync(trace, JSON.stringify(value) + '\n');
const emit = value => process.stdout.write(JSON.stringify(value) + '\n');
let helper;
const originalSpawn = childProcess.spawn;
childProcess.spawn = (...args) => { helper = originalSpawn(...args); return helper; };
const failingFds = new Set();
const originalWrite = fs.writeFileSync;
fs.writeFileSync = (target, data, ...rest) => {
  if (typeof target === 'number' && typeof data === 'string' && mode.startsWith('fail-save-')) {
    let doc; try { doc = JSON.parse(data); } catch {}
    if (Object.values(doc?.runs ?? {}).some(e => mode === 'fail-save-final' ? e.final : e.accepted)) failingFds.add(target);
  }
  return originalWrite(target, data, ...rest);
};
const originalSync = fs.fsyncSync;
fs.fsyncSync = fd => { if (failingFds.has(fd)) throw Error('SYNTHETIC_FSYNC_FAILURE'); originalSync(fd); log({ fsync: fs.fstatSync(fd).isDirectory() ? 'directory' : 'file' }); };
const originalRename = fs.renameSync;
fs.renameSync = (from, to) => {
  if (mode === 'crash-before-rename') {
    const doc = JSON.parse(fs.readFileSync(from));
    if (Object.values(doc.runs).some(e => e.attempted)) { emit({ phase: 'before-rename' }); process.kill(process.pid, 'SIGKILL'); }
  }
  originalRename(from, to);
};
syncBuiltinESMExports();
const { FileJournal } = await import('../../scripts/file-journal.mjs');
const { TaskSpec, RunContext, DshProviderAdapter, HarnessRunner } = await import('../../scripts/harness.mjs');
const config = { provider: 'synthetic', model: 'synthetic', reasoningEffort: options.effort ?? 'low' };
const task = new TaskSpec({ objective: 'PRIVATE_TASK_TEXT', acceptance: { kind: 'exact-text', text: 'PRIVATE_ANSWER_TEXT' }, sideEffects: 'write', budget: { maxPolls: 2, pollMs: 0, timeoutMs: 3000 } });
const context = new RunContext({ requestId, sessionId: options.sessionId ?? 'synthetic-session', lessonsVersion: 'reviewed-v1', expected: { ...config, preset: 'standard' } });
class Journal extends FileJournal {
  async confirmHeld(options) {
    if (mode === 'replace-root') {
      fs.renameSync(directory, `${directory}-moved`); fs.mkdirSync(directory, { mode: 0o700 });
    }
    if (mode === 'replace-lock') {
      const lock = join(directory, `.lock-${createHash('sha256').update(context.sessionId).digest('hex')}`);
      fs.unlinkSync(lock); fs.writeFileSync(lock, '', { mode: 0o600 });
    }
    return super.confirmHeld(options);
  }
  set(key, entry) {
    super.set(key, entry);
    if (mode === 'crash-after-attempt' && entry.attempted && entry.observedThrough === undefined) {
      emit({ phase: 'durable-attempt' }); process.kill(process.pid, 'SIGKILL');
    }
    return this;
  }
}
const journal = new Journal(directory, options.python ? { python: options.python } : {});
const read = () => fs.existsSync(backend) ? JSON.parse(fs.readFileSync(backend)) : [];
const save = events => { const fd = fs.openSync(backend, 'w', 0o600); fs.writeFileSync(fd, JSON.stringify(events)); originalSync(fd); fs.closeSync(fd); };
const event = (seq, type, data) => ({ seq, type, data });
const nativeCall = async (method, { request }) => {
  if (method === 'session/projections') {
    if (mode === 'hold') { emit({ phase: 'held' }); await new Promise(() => {}); }
    if (mode === 'lose-helper') { helper.kill('SIGKILL'); await once(helper, 'exit'); }
    return { result: { values: { agentPreset: 'standard', modelSelection: { next: config } } } };
  }
  if (method === 'session/follow') return { result: { cursor: read().at(-1)?.seq ?? 0 } };
  if (method === 'session/page') return { result: { records: read().filter(e => e.seq < request.beforeSeq).map(event => ({ event })) } };
  if (method === 'session/prompt') {
    const stored = journal.get(JSON.stringify([context.sessionId, context.requestId]));
    if (!stored.attempted) throw Error('attempt not durable');
    const events = read(); let seq = events.at(-1)?.seq ?? 0;
    const turn = events.filter(e => e.type === 'turn/start').length + 1;
    events.push(event(++seq, 'turn/start', { turn }), event(++seq, 'user/message', { source: { rpcId: request.requestId } }));
    if (mode !== 'queued' && !mode.startsWith('no-header')) events.push(event(++seq, 'request/header', { header: { config } }));
    if (!['running', 'queued', 'no-header-running'].includes(mode)) events.push(event(++seq, 'assistant/message', { turn, message: { content: [{ type: 'text', text: 'PRIVATE_ANSWER_TEXT' }] } }), event(++seq, 'turn/end', { turn, reason: { kind: 'completed' } }));
    save(events);
    if (mode === 'hang-after-send') { emit({ phase: 'sent' }); await new Promise(() => {}); }
    if (mode === 'unknown' || mode === 'no-header-unknown') throw Error('PRIVATE_TRANSPORT_ERROR');
    return { result: {}, execution: 'accepted' };
  }
  if (method === 'session/cancel') {
    const events = read(); const turn = events.filter(e => e.type === 'turn/start').at(-1).data.turn;
    if (mode === 'cancel-delayed-unknown') throw Error('PRIVATE_CANCEL_ERROR');
    events.push(event(events.at(-1).seq + 1, 'turn/end', { turn, reason: { kind: 'cancelled' } })); save(events);
    if (mode === 'cancel-unknown') throw Error('PRIVATE_CANCEL_ERROR');
    return { result: {} };
  }
  throw Error('unexpected fake endpoint');
};
let envelopeCounter = 0;
const call = async (method, params) => {
  const envelopeRequestId = `${options.connection ?? 'synthetic-connection'}-dispatch-${++envelopeCounter}`;
  log({ method, requestId, envelopeRequestId, nestedNativeRequestId: params.request?.requestId });
  return { ...await nativeCall(method, params), requestId: envelopeRequestId };
};
const runner = new HarnessRunner(new DshProviderAdapter(call), { journal, sleep: async () => {} });
let result;
if (mode === 'prepare') result = (await runner.prepareExternalRun(task, context)).result;
else result = await runner.run(task, context, { requestCancellation: mode.startsWith('cancel'), exclusiveSession: true });
emit({ result });
