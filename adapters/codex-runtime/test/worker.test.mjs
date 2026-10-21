import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { StdioWorker } from '../worker.mjs';
import { isolatedEnvironment } from './support.mjs';

test('EOF waits for stdout close and drains notifications after parent exit', { timeout: 5000 }, async t => {
  const root = await mkdtemp(join(tmpdir(), 'owned-worker-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  const last = { method: 'item/completed', params: { threadId: 'fixture', item: { id: 'last', type: 'agentMessage' } } };
  const descendant = `setTimeout(() => process.stdout.write(${JSON.stringify(JSON.stringify(last) + '\n')}), 100);`;
  const source = `const {spawn}=require('node:child_process'); process.stdin.resume(); process.stdin.on('end',()=>{spawn(process.execPath,['-e',${JSON.stringify(descendant)}],{stdio:['ignore','inherit','ignore']});process.exit(0);});`;
  const worker = new StdioWorker({ command: process.execPath, args: ['-e', source], env: isolatedEnvironment(root), cwd: root });
  const events = []; worker.on('packet', p => events.push(p));
  worker.on('fault', e => { throw e; });
  const exit = await worker.eof();
  assert.deepEqual(exit, { code: 0, signal: null });
  assert.deepEqual(events, [last], 'EOF may resolve only after final inherited stdout packet was delivered');
});

test('spawn ENOENT rejects initialization and settles EOF without hanging', { timeout: 3000 }, async t => {
  const root = await mkdtemp(join(tmpdir(), 'owned-worker-missing-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  const worker = new StdioWorker({ command: join(root, 'missing-codex'), env: isolatedEnvironment(root), cwd: root, timeoutMs: 500 });
  const faults = []; worker.on('fault', error => faults.push(error));
  await assert.rejects(worker.initialize(), /ENOENT|EPIPE|unavailable|exited/);
  const exit = await worker.eof();
  assert.notEqual(exit.code, 0);
  assert.ok(faults.some(error => error.code === 'ENOENT'));
});
