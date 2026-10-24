/** Optional, explicit project-local recovery store. No prompts, credentials or service. */
import { constants as C, openSync, closeSync, fstatSync, lstatSync, readFileSync, writeFileSync, fsyncSync, renameSync, unlinkSync } from 'node:fs';
import { createHash, randomUUID } from 'node:crypto';
import { spawn } from 'node:child_process';
import { createInterface } from 'node:readline';
import { resolve, join } from 'node:path';
import { fileURLToPath } from 'node:url';
const fail = code => Object.assign(new Error(code), { code });
const hash = text => createHash('sha256').update(text).digest('hex');
const identity = path => { const info = lstatSync(path, { bigint: true }); return `${info.dev}:${info.ino}`; };
const copy = value => JSON.parse(JSON.stringify(value));
const object = x => x && typeof x === 'object' && !Array.isArray(x);
const opaque = x => typeof x === 'string' && /^[A-Za-z0-9_.:-]{1,200}$/.test(x);
const hex = x => typeof x === 'string' && /^[a-f0-9]{64}$/.test(x);
const integer = x => Number.isSafeInteger(x) && x >= 0;
function keys(value, allowed, required = allowed) {
  if (!object(value) || Object.keys(value).some(k => !allowed.includes(k)) || required.some(k => !Object.hasOwn(value, k))) throw fail('journal_invalid_entry');
}
function validateFinal(value, entry) {
  keys(value, ['status', 'code', 'requestId', 'sessionId', 'harnessVersion', 'lessonsVersion', 'phase', 'accepted', 'completed', 'evidence']);
  if (!['success', 'failed', 'cancelled'].includes(value.status) || !['verified', 'terminal_not_verified'].includes(value.code) || typeof value.accepted !== 'boolean' || typeof value.completed !== 'boolean' || !['production', 'heldout', 'discovery'].includes(value.phase)) throw fail('journal_invalid_entry');
  for (const k of ['requestId', 'sessionId', 'harnessVersion', 'lessonsVersion']) if (value[k] !== entry[k]) throw fail('journal_invalid_entry');
  const e = value.evidence;
  keys(e, ['turn', 'inputSeq', 'endSeq', 'reason', 'toolEvents', 'headers', 'models', 'headerInherited', 'headerOriginSeq', 'outputHash', 'outputLength']);
  for (const k of ['turn', 'inputSeq', 'endSeq', 'toolEvents', 'headers', 'outputLength', 'headerOriginSeq']) if (!integer(e[k])) throw fail('journal_invalid_entry');
  if (typeof e.headerInherited !== 'boolean') throw fail('journal_invalid_entry');
  if (e.inputSeq <= entry.base || e.endSeq <= e.inputSeq || !opaque(e.reason) || !hex(e.outputHash) || !Array.isArray(e.models) || e.models.length > 256) throw fail('journal_invalid_entry');
  for (const m of e.models) {
    keys(m, ['provider', 'model', 'reasoningEffort']);
    for (const k of ['provider', 'model']) if (m[k] !== null && (typeof m[k] !== 'string' || !/^[A-Za-z0-9._/@:+-]{1,160}$/.test(m[k]))) throw fail('journal_invalid_entry');
    if (m.reasoningEffort !== null && !['off', 'none', 'minimal', 'low', 'medium', 'high', 'xhigh', 'max', 'ultra'].includes(m.reasoningEffort)) throw fail('journal_invalid_entry');
  }
}
function validateEntry(e) {
  const required = ['requestId', 'sessionId', 'harnessVersion', 'lessonsVersion', 'fingerprint', 'configuration', 'base', 'attempted', 'accepted'];
  keys(e, [...required, 'external', 'cancelAttempted', 'observedThrough', 'headerSnapshot', 'final'], required);
  for (const k of ['requestId', 'sessionId', 'harnessVersion', 'lessonsVersion']) if (!opaque(e[k])) throw fail('journal_invalid_entry');
  if (!hex(e.fingerprint) || !hex(e.configuration) || !integer(e.base) || typeof e.attempted !== 'boolean' || typeof e.accepted !== 'boolean') throw fail('journal_invalid_entry');
  for (const k of ['external', 'cancelAttempted']) if (e[k] !== undefined && typeof e[k] !== 'boolean') throw fail('journal_invalid_entry');
  if ((e.accepted || e.external || e.cancelAttempted || e.final) && !e.attempted) throw fail('journal_invalid_entry');
  if (e.observedThrough !== undefined && (!integer(e.observedThrough) || e.observedThrough < e.base)) throw fail('journal_invalid_entry');
  if (e.headerSnapshot) {
    const h = e.headerSnapshot;
    keys(h, ['originSeq', 'throughSeq', 'config']);
    if (!integer(h.originSeq) || h.originSeq > e.base || h.throughSeq !== e.base) throw fail('journal_invalid_entry');
    keys(h.config, ['provider', 'model', 'reasoningEffort']);
    for (const k of ['provider', 'model']) if (typeof h.config[k] !== 'string' || !/^[A-Za-z0-9._/@:+-]{1,160}$/.test(h.config[k])) throw fail('journal_invalid_entry');
    if (h.config.reasoningEffort !== null && !['off', 'none', 'minimal', 'low', 'medium', 'high', 'xhigh', 'max', 'ultra'].includes(h.config.reasoningEffort)) throw fail('journal_invalid_entry');
  }
  if (e.final) validateFinal(e.final, e);
}
function secure(info, directory = false) {
  if (!(directory ? info.isDirectory() : info.isFile()) || info.uid !== process.getuid() || (!directory && info.nlink !== 1) || (info.mode & 0o077)) throw fail('journal_unsafe_path');
}
/** Explicit existing 0700 directory, same-host POSIX filesystem, Python 3 stdlib.
 * acquire/get/set/release form one session transaction; no directory scanning. */
export class FileJournal {
  #directory; #python; #held; #rootIdentity;
  constructor(directory, { python = 'python3' } = {}) {
    if (typeof process.getuid !== 'function' || !directory) throw fail('journal_platform_unsupported');
    this.#directory = resolve(directory); this.#python = python;
    secure(lstatSync(this.#directory), true); this.#rootIdentity = identity(this.#directory);
  }
  async acquire(context, { timeoutMs = 5000, signal } = {}) {
    if (this.#held) throw fail('journal_locked');
    for (const k of ['sessionId', 'requestId', 'harnessVersion', 'lessonsVersion']) if (!opaque(context[k])) throw fail('journal_invalid_entry');
    secure(lstatSync(this.#directory), true);
    const sessionHash = hash(context.sessionId);
    const child = spawn(this.#python, ['-I', '-S', '-B', fileURLToPath(new URL('./journal-lock.py', import.meta.url)), this.#directory, `.lock-${sessionHash}`], { stdio: ['pipe', 'pipe', 'ignore'] });
    child.stdin.on('error', () => {});
    const lines = createInterface({ input: child.stdout });
    const held = { child, context: copy(context), sessionHash, ready: false, lost: false, pending: new Map(), lines };
    this.#held = held;
    const lose = () => { held.lost = true; for (const reject of held.pending.values()) reject(fail('journal_lock_lost')); held.pending.clear(); };
    child.on('error', lose); child.on('exit', lose); child.stdout.on('end', lose);
    lines.on('line', line => {
      let message; try { message = JSON.parse(line); } catch { lose(); return; }
      const key = message.state === 'held' ? message.nonce : 'ready';
      const pending = held.pending.get(key);
      if (!pending) return;
      held.pending.delete(key);
      if (message.state === 'locked') { held.rootIdentity = message.rootIdentity; held.lockIdentity = message.lockIdentity; held.ready = true; pending(null); }
      else if (message.state === 'held') pending(null);
      else pending(fail(message.state === 'busy' ? 'journal_locked' : 'journal_lock_lost'));
    });
    try {
      await this.#wait('ready', timeoutMs, signal);
      this.#read(); // Reject pending ownership/version before any transport call.
      return async () => { await this.#release(held); };
    } catch (error) { await this.#release(held); throw error.code?.startsWith('journal_') ? error : fail('journal_unavailable'); }
  }
  #wait(key, timeoutMs, signal) {
    const held = this.#held;
    return new Promise((resolvePromise, reject) => {
      let timer;
      const finish = error => { clearTimeout(timer); signal?.removeEventListener('abort', abort); held.pending.delete(key); error ? reject(error) : resolvePromise(); };
      const abort = () => finish(fail('journal_lock_lost'));
      if (signal?.aborted || held.lost) { abort(); return; }
      timer = setTimeout(abort, Math.max(1, Math.min(timeoutMs, 5000)));
      signal?.addEventListener('abort', abort, { once: true });
      held.pending.set(key, finish);
    });
  }
  assertHeld() {
    const held = this.#held;
    if (!held?.ready || held.lost || held.child.exitCode !== null || held.child.killed || held.child.stdin.destroyed) throw fail('journal_lock_lost');
    try {
      process.kill(held.child.pid, 0);
      const lock = join(this.#directory, `.lock-${held.sessionHash}`);
      secure(lstatSync(this.#directory), true); secure(lstatSync(lock));
      if (identity(this.#directory) !== this.#rootIdentity || held.rootIdentity !== this.#rootIdentity || identity(lock) !== held.lockIdentity) throw fail('journal_lock_lost');
    } catch { throw fail('journal_lock_lost'); }
  }
  async confirmHeld({ signal, timeoutMs = 1000 } = {}) {
    this.assertHeld();
    const nonce = randomUUID();
    const ready = this.#wait(nonce, timeoutMs, signal);
    this.#held.child.stdin.write(JSON.stringify({ op: 'check', nonce }) + '\n');
    await ready; this.assertHeld();
  }
  async #release(held) {
    if (this.#held !== held) return;
    this.#held = undefined;
    held.child.stdin.end();
    if (held.child.exitCode === null && held.child.signalCode === null) await new Promise(resolvePromise => {
      const timer = setTimeout(() => { held.child.kill('SIGTERM'); resolvePromise(); }, 1000);
      held.child.once('exit', () => { clearTimeout(timer); resolvePromise(); });
    });
    held.lines.close();
  }
  #path() { return join(this.#directory, `${this.#held.sessionHash}.json`); }
  #read() {
    this.assertHeld(); secure(lstatSync(this.#directory), true);
    let fd, doc;
    try {
      fd = openSync(this.#path(), C.O_RDONLY | C.O_NOFOLLOW);
      const info = fstatSync(fd); secure(info);
      if (info.size > 2 * 1024 * 1024) throw fail('journal_size_limit');
      try { doc = JSON.parse(readFileSync(fd, 'utf8')); } catch { throw fail('journal_corrupt'); }
    } catch (error) {
      if (error.code !== 'ENOENT') throw error;
      doc = { format: 1, sessionId: this.#held.context.sessionId, runs: {} };
    } finally { if (fd !== undefined) closeSync(fd); }
    if (doc.format !== 1) throw fail('journal_version_mismatch');
    keys(doc, ['format', 'sessionId', 'runs']);
    if (doc.sessionId !== this.#held.context.sessionId || !object(doc.runs)) throw fail('journal_corrupt');
    const pending = [];
    for (const [key, entry] of Object.entries(doc.runs)) {
      validateEntry(entry);
      if (key !== hash(entry.requestId) || entry.sessionId !== doc.sessionId) throw fail('journal_corrupt');
      // Native cancellation has no turn fence: a delayed cancel can hit a later run.
      // Keep this session quarantined even after the cancelled owner reaches a terminal.
      if (entry.cancelAttempted && entry.requestId !== this.#held.context.requestId) throw fail('journal_session_cancelled_owner');
      if (entry.attempted && !entry.final) pending.push(entry.requestId);
    }
    if (pending.length > 1) throw fail('journal_corrupt');
    if (pending.length && pending[0] !== this.#held.context.requestId) throw fail('journal_session_pending');
    const current = doc.runs[hash(this.#held.context.requestId)];
    if (current && current.harnessVersion !== this.#held.context.harnessVersion) throw fail('journal_version_mismatch');
    return doc;
  }
  #checkKey(key) {
    this.assertHeld();
    const { sessionId, requestId } = this.#held.context;
    if (key !== JSON.stringify([sessionId, requestId])) throw fail('journal_invalid_entry');
  }
  get(key) {
    try { this.#checkKey(key); const entry = this.#read().runs[hash(this.#held.context.requestId)]; return entry ? copy(entry) : undefined; }
    catch (error) { throw error.code?.startsWith('journal_') ? error : fail('journal_read_failed'); }
  }
  set(key, entry) {
    try { return this.#store(key, entry); }
    catch (error) { throw error.code?.startsWith('journal_') ? error : fail('journal_write_failed'); }
  }
  #store(key, entry) {
    this.#checkKey(key); validateEntry(entry);
    const context = this.#held.context;
    for (const k of ['sessionId', 'requestId', 'harnessVersion', 'lessonsVersion']) if (entry[k] !== context[k]) throw fail('journal_invalid_entry');
    const doc = this.#read(); const index = hash(context.requestId); const prior = doc.runs[index];
    if (prior) {
      for (const k of ['fingerprint', 'configuration', 'base', 'harnessVersion', 'lessonsVersion']) if (entry[k] !== prior[k]) throw fail('journal_conflict');
      for (const k of ['attempted', 'accepted', 'external', 'cancelAttempted']) if (prior[k] === true && entry[k] !== true) throw fail('journal_conflict');
      if (JSON.stringify(entry.headerSnapshot) !== JSON.stringify(prior.headerSnapshot)) throw fail('journal_conflict');
      if ((entry.observedThrough ?? entry.base) < (prior.observedThrough ?? prior.base) || (prior.final && JSON.stringify(prior.final) !== JSON.stringify(entry.final))) throw fail('journal_conflict');
    }
    doc.runs[index] = copy(entry);
    const data = JSON.stringify(doc) + '\n';
    if (Buffer.byteLength(data) > 2 * 1024 * 1024) throw fail('journal_size_limit');
    const temp = join(this.#directory, `.pending-${randomUUID()}`); let fd;
    try {
      fd = openSync(temp, C.O_WRONLY | C.O_CREAT | C.O_EXCL | C.O_NOFOLLOW, 0o600);
      writeFileSync(fd, data); fsyncSync(fd); closeSync(fd); fd = undefined;
      this.assertHeld(); renameSync(temp, this.#path());
      const directory = openSync(this.#directory, C.O_RDONLY);
      try { fsyncSync(directory); } finally { closeSync(directory); }
      this.assertHeld();
    } finally {
      if (fd !== undefined) closeSync(fd);
      try { unlinkSync(temp); } catch (error) { if (error.code !== 'ENOENT') throw error; }
    }
    return this;
  }
}
