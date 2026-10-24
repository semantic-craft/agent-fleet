import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, readdirSync, readFileSync, writeFileSync, chmodSync, symlinkSync, rmSync, statSync } from 'node:fs';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import { FileJournal } from '../scripts/file-journal.mjs';
import { HARNESS_VERSION } from '../scripts/harness.mjs';
const fixture = fileURLToPath(new URL('./fixtures/journal-process.mjs', import.meta.url));
const hash = x => createHash('sha256').update(x).digest('hex');
const sleep = ms => new Promise(r => setTimeout(r, ms));
function setup(t) {
  const directory = mkdtempSync(join(tmpdir(), 'durable-harness-'));
  t.after(() => { rmSync(directory, { recursive: true, force: true }); rmSync(`${directory}-moved`, { recursive: true, force: true }); });
  return { directory, backend: join(directory, 'fake-backend'), trace: join(directory, 'fake-trace') };
}
function start(opts) {
  const child = spawn(process.execPath, [fixture, JSON.stringify(opts)], { stdio: ['ignore', 'pipe', 'pipe'], env: { ...process.env, PYTHONDONTWRITEBYTECODE: '1' } });
  let out = '', err = '';
  child.stdout.on('data', b => { out += b; }); child.stderr.on('data', b => { err += b; });
  const exit = new Promise((resolve, reject) => { child.on('error', reject); child.on('exit', (code, signal) => resolve({ code, signal })); });
  return { child, exit, output: () => out, error: () => err };
}
async function run(opts) {
  const p = start(opts); const status = await p.exit;
  assert.equal(status.code, 0, p.error());
  return JSON.parse(p.output().trim().split('\n').at(-1)).result;
}
async function phase(p, name) {
  for (let i = 0; i < 200; i++) { if (p.output().includes(`"phase":"${name}"`)) return; await sleep(10); }
  p.child.kill('SIGKILL'); throw Error(`fixture did not reach ${name}: ${p.error()}`);
}
const methods = env => readFileSync(env.trace, 'utf8').trim().split('\n').filter(Boolean).map(x => JSON.parse(x)).filter(x => x.method).map(x => x.method);
const docPath = env => join(env.directory, `${hash('synthetic-session')}.json`);
const doc = env => JSON.parse(readFileSync(docPath(env), 'utf8'));
const saveDoc = (env, value) => writeFileSync(docPath(env), JSON.stringify(value), { mode: 0o600 });
async function afterCrash(opts) {
  for (let i = 0; i < 50; i++) { const result = await run(opts); if (result.code !== 'journal_locked') return result; await sleep(20); }
  throw Error('test lock helper did not release');
}
test('separate processes persist success and deduplicate; directory fsync precedes mutation', async t => {
  const env = setup(t); const first = await run(env); assert.equal(first.status, 'success');
  const before = methods(env).length; assert.equal((await run(env)).status, 'success'); assert.equal(methods(env).length, before);
  assert.equal(methods(env).filter(m => m === 'session/prompt').length, 1);
  const trace = readFileSync(env.trace, 'utf8').trim().split('\n').map(x => JSON.parse(x));
  const at = trace.findIndex(x => x.method === 'session/prompt'); assert.equal(trace[at - 1].fsync, 'directory');
  const disk = readFileSync(docPath(env), 'utf8');
  for (const secret of ['PRIVATE_TASK_TEXT', 'PRIVATE_ANSWER_TEXT', 'PRIVATE_TRANSPORT_ERROR']) assert.ok(!disk.includes(secret));
  assert.equal(statSync(docPath(env)).mode & 0o777, 0o600);
});
test('SIGKILL after unknown send: different request blocked, same request reads terminal without resend', async t => {
  const env = setup(t); const p = start({ ...env, mode: 'hang-after-send' }); await phase(p, 'sent'); p.child.kill('SIGKILL'); await p.exit;
  assert.equal((await afterCrash({ ...env, requestId: 'different' })).code, 'journal_session_pending');
  assert.equal((await afterCrash(env)).status, 'success'); assert.equal(methods(env).filter(m => m === 'session/prompt').length, 1);
  assert.equal((await run({ ...env, requestId: 'different' })).status, 'success');
});
test('crash after fsynced attempt but before send cannot automatically submit on recovery', async t => {
  const env = setup(t); const p = start({ ...env, mode: 'crash-after-attempt' }); await p.exit;
  assert.ok(p.output().includes('durable-attempt'));
  const r = await afterCrash(env); assert.equal(r.status, 'timeout'); assert.equal(r.accepted, false);
  assert.equal(methods(env).filter(m => m === 'session/prompt').length, 0);
  assert.equal((await run({ ...env, requestId: 'different' })).code, 'journal_session_pending');
});
test('interrupted pre-rename write preserves valid prior record; no prior mutation occurred', async t => {
  const env = setup(t); const p = start({ ...env, mode: 'crash-before-rename' }); await p.exit;
  assert.ok(p.output().includes('before-rename')); assert.equal(Object.values(doc(env).runs)[0].attempted, false);
  assert.equal((await afterCrash(env)).status, 'success'); assert.equal(methods(env).filter(m => m === 'session/prompt').length, 1);
});
test('session lock blocks a different request in another active process', async t => {
  const env = setup(t); const p = start({ ...env, mode: 'hold' }); await phase(p, 'held');
  try { assert.equal((await run({ ...env, requestId: 'different' })).code, 'journal_locked'); }
  finally { p.child.kill('SIGKILL'); await p.exit; }
  assert.equal((await afterCrash(env)).status, 'success');
});
test('helper death fails closed before mutation', async t => {
  const env = setup(t); const r = await run({ ...env, mode: 'lose-helper' });
  assert.equal(r.status, 'blocked'); assert.equal(r.code, 'journal_lock_lost'); assert.equal(methods(env).filter(m => m === 'session/prompt').length, 0);
});
test('external prepare survives restart without generating a local prompt or fresh baseline', async t => {
  const env = setup(t); assert.equal((await run({ ...env, mode: 'prepare' })).status, 'prepared');
  const base = Object.values(doc(env).runs)[0].base;
  const r = await run(env); assert.equal(r.status, 'timeout'); assert.equal(r.accepted, false);
  assert.equal((await run({ ...env, mode: 'prepare' })).status, 'prepared');
  assert.equal(Object.values(doc(env).runs)[0].base, base); assert.equal(methods(env).filter(m => m === 'session/prompt').length, 0);
});
test('unknown cancellation is persisted and only matching request may recover', async t => {
  const env = setup(t); assert.equal((await run({ ...env, mode: 'running' })).status, 'timeout');
  assert.equal((await run({ ...env, requestId: 'different', mode: 'cancel' })).code, 'journal_session_pending');
  assert.equal((await run({ ...env, mode: 'cancel-unknown' })).code, 'cancellation_outcome_unknown');
  assert.equal((await run({ ...env, mode: 'cancel' })).status, 'cancelled'); assert.equal(methods(env).filter(m => m === 'session/cancel').length, 1);
});
test('queued cancellation remains pending across processes without a cancel call', async t => {
  const env = setup(t); await run({ ...env, mode: 'queued' });
  assert.equal((await run({ ...env, mode: 'cancel' })).code, 'queued_cancellation_unsupported');
  assert.equal(methods(env).filter(m => m === 'session/cancel').length, 0);
});
for (const [name, mutate, code] of [
  ['format version', d => { d.format = 99; }, 'journal_version_mismatch'],
  ['harness version', d => { Object.values(d.runs)[0].harnessVersion = 'bounded-dsh-v2'; }, 'journal_version_mismatch'],
  ['unknown field', d => { Object.values(d.runs)[0].prompt = 'PRIVATE'; }, 'journal_invalid_entry'],
]) test(`${name} fails closed before transport in a new process`, async t => {
  const env = setup(t); await run({ ...env, mode: 'prepare' }); const value = doc(env); mutate(value); saveDoc(env, value);
  const before = methods(env).length; assert.equal((await run(env)).code, code); assert.equal(methods(env).length, before);
});
test('truncated state and symlink targets fail closed', async t => {
  const env = setup(t); await run({ ...env, mode: 'prepare' }); writeFileSync(docPath(env), '{');
  assert.equal((await run(env)).code, 'journal_corrupt');
  rmSync(docPath(env)); symlinkSync(env.backend, docPath(env)); assert.equal((await run(env)).status, 'blocked');
});
test('allowlist refuses arbitrary journal.set payloads; shared instance owns only one lock', async t => {
  const env = setup(t); const journal = new FileJournal(env.directory);
  const ctx = { sessionId: 'synthetic-session', requestId: 'synthetic-request', harnessVersion: HARNESS_VERSION, lessonsVersion: 'reviewed-v1' };
  const release = await journal.acquire(ctx);
  try {
    await assert.rejects(journal.acquire(ctx), e => e.code === 'journal_locked');
    assert.throws(() => journal.set(JSON.stringify([ctx.sessionId, ctx.requestId]), { prompt: 'PRIVATE' }), e => e.code === 'journal_invalid_entry');
  } finally { await release(); }
  assert.equal(readdirSync(env.directory).filter(x => x.endsWith('.json')).length, 0);
});

test('replaced lock inode is lost before mutation, preserving pending owner', async t => {
  const env = setup(t); const r = await run({ ...env, mode: 'replace-lock' });
  assert.equal(r.status, 'blocked'); assert.equal(r.code, 'journal_lock_lost');
  assert.equal(methods(env).filter(m => m === 'session/prompt').length, 0);
  assert.equal((await run({ ...env, requestId: 'different' })).code, 'journal_session_pending');
});
test('historical header provenance survives process restart for a second turn', async t => {
  const env = setup(t); await run(env);
  const next = { ...env, requestId: 'second-request' };
  assert.equal((await run({ ...next, mode: 'no-header-unknown' })).code, 'submission_outcome_unknown');
  const r = await run(next); assert.equal(r.status, 'success'); assert.equal(r.evidence.headerInherited, true); assert.equal(r.evidence.headerOriginSeq, 3);
  assert.equal(methods(env).filter(m => m === 'session/prompt').length, 2);
});

test('replaced journal directory identity blocks the old process before mutation', async t => {
  const env = setup(t); const r = await run({ ...env, mode: 'replace-root' });
  assert.equal(r.status, 'blocked'); assert.equal(r.code, 'journal_lock_lost');
  const oldTrace = join(`${env.directory}-moved`, 'fake-trace');
  assert.equal(readFileSync(oldTrace, 'utf8').includes('session/prompt'), false);
});
test('missing helper is a blocked capability, never an automatic lock fallback', async t => {
  const env = setup(t); const r = await run({ ...env, python: join(env.directory, 'missing-helper') });
  assert.equal(r.status, 'blocked'); assert.equal(r.code, 'journal_lock_lost');
});

test('catalog effort off persists both terminal evidence and inherited header', async t => {
  const env = { ...setup(t), effort: 'off' }; assert.equal((await run(env)).status, 'success');
  const next = { ...env, requestId: 'off-second', mode: 'no-header-unknown' }; await run(next);
  const r = await run({ ...next, mode: 'run' }); assert.equal(r.status, 'success'); assert.equal(r.evidence.models[0].reasoningEffort, 'off');
});

test('headerless second-turn cancellation reports unsupported proof, not queued or cancelled', async t => {
  const env = setup(t); await run(env);
  const second = { ...env, requestId: 'second-running' };
  assert.equal((await run({ ...second, mode: 'no-header-running' })).status, 'timeout');
  const r = await run({ ...second, mode: 'cancel' }); assert.equal(r.code, 'headerless_cancellation_unsupported'); assert.equal(r.status, 'blocked');
  assert.equal(methods(env).filter(m => m === 'session/cancel').length, 0);
});

test('new connection and envelope IDs recover unknown native admission read-only after restart', async t => {
  const env = setup(t);
  const first = await run({ ...env, mode: 'unknown', connection: 'relay-connection-before' });
  assert.equal(first.code, 'submission_outcome_unknown'); assert.equal(first.requestId, 'synthetic-request');
  const before = readFileSync(env.trace, 'utf8').trim().split('\n').map(x => JSON.parse(x)).filter(x => x.method);
  const sent = before.find(x => x.method === 'session/prompt');
  assert.equal(sent.nestedNativeRequestId, 'synthetic-request'); assert.match(sent.envelopeRequestId, /^relay-connection-before-/);
  const recovered = await run({ ...env, connection: 'relay-connection-after' });
  assert.equal(recovered.status, 'success'); assert.equal(recovered.requestId, 'synthetic-request');
  const all = readFileSync(env.trace, 'utf8').trim().split('\n').map(x => JSON.parse(x)).filter(x => x.method);
  const recovery = all.slice(before.length);
  assert.ok(recovery.length > 0);
  assert.ok(recovery.every(x => ['session/projections', 'session/follow', 'session/page'].includes(x.method)));
  assert.ok(recovery.every(x => x.envelopeRequestId.startsWith('relay-connection-after-')));
  assert.equal(all.filter(x => x.method === 'session/prompt').length, 1);
  const state = readFileSync(docPath(env), 'utf8');
  assert.equal(Object.values(JSON.parse(state).runs)[0].requestId, 'synthetic-request');
  assert.equal(state.includes('relay-connection-'), false);
});

for (const outcome of ['cancelled', 'naturally-completed']) test(`cancel attempt quarantines session after ${outcome}, across processes`, async t => {
  const env = setup(t); await run({ ...env, mode: 'running' });
  if (outcome === 'naturally-completed') {
    assert.equal((await run({ ...env, mode: 'cancel-delayed-unknown' })).code, 'cancellation_outcome_unknown');
    // The remote turn completes normally while the unfenced cancel may still be in flight.
    const events = JSON.parse(readFileSync(env.backend, 'utf8')); let seq = events.at(-1).seq;
    events.push({ seq: ++seq, type: 'assistant/message', data: { turn: 1, message: { content: [{ type: 'text', text: 'PRIVATE_ANSWER_TEXT' }] } } },
      { seq: ++seq, type: 'turn/end', data: { turn: 1, reason: { kind: 'completed' } } });
    writeFileSync(env.backend, JSON.stringify(events));
    assert.equal((await run(env)).status, 'success');
  } else assert.equal((await run({ ...env, mode: 'cancel' })).status, 'cancelled');
  const before = methods(env).length;
  const blocked = await run({ ...env, requestId: 'new-native-request' });
  assert.equal(blocked.status, 'blocked'); assert.equal(blocked.code, 'journal_session_cancelled_owner');
  assert.equal(methods(env).length, before);
  if (outcome === 'naturally-completed') {
    // Simulate arrival of the delayed session-scoped cancel, not a cancel bound to A.
    const events = JSON.parse(readFileSync(env.backend, 'utf8'));
    assert.equal(events.filter(e => e.type === 'turn/start').length, 1);
    assert.ok(!events.some(e => e.data?.source?.rpcId === 'new-native-request'));
    writeFileSync(env.trace, readFileSync(env.trace, 'utf8') + JSON.stringify({ lateSessionCancelArrived: true, activeTurn: null }) + '\n');
  }
  assert.equal((await run(env)).status, outcome === 'cancelled' ? 'cancelled' : 'success');
  assert.equal(methods(env).filter(m => m === 'session/cancel').length, 1);
  assert.equal(methods(env).filter(m => m === 'session/prompt').length, 1);
});

for (const stage of ['accepted', 'final']) test(`post-admission ${stage} fsync failure recovers in a new process without resubmitting`, async t => {
  const env = setup(t); const failed = await run({ ...env, mode: `fail-save-${stage}` });
  assert.equal(failed.status, 'blocked'); assert.equal(failed.code, 'journal_write_failed');
  const before = methods(env).length;
  assert.equal((await run({ ...env, requestId: 'different' })).code, 'journal_session_pending');
  assert.equal(methods(env).length, before);
  assert.equal((await run(env)).status, 'success');
  assert.equal(methods(env).filter(m => m === 'session/prompt').length, 1);
});
