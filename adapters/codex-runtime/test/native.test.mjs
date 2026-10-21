import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, rm, realpath } from 'node:fs/promises';
import { join } from 'node:path';
import { once } from 'node:events';
import { OwnedRuntimeManager } from '../manager.mjs';
import { StdioWorker } from '../worker.mjs';
import { serve } from '../server.mjs';
import { isolatedEnvironment, modelServer, rpcClient, until } from './support.mjs';

test('native 0.163: Unix client disconnect does not interrupt; completion releases writer and explicit continuation preserves history', {
  skip: !process.env.CODEX_NATIVE_BINARY && 'Set CODEX_NATIVE_BINARY to explicitly run isolated native compatibility test',
  timeout: 45000,
}, async t => {
  const root = await realpath(await mkdtemp('/tmp/owned-native-'));
  const env = isolatedEnvironment(root); await mkdir(env.CODEX_HOME);
  const workers = [], clients = [];
  const createWorker = () => { const w = new StdioWorker({ command: process.env.CODEX_NATIVE_BINARY, env, cwd: root }); workers.push(w); return w; };
  const manager = new OwnedRuntimeManager({ stateDir: join(root, 'state'), createWorker });
  const eventMethods = []; manager.on('packet', p => eventMethods.push(p.method));
  const socketPath = join(root, 'runtime.sock');
  const service = await serve({ socketPath, manager });
  const model = await modelServer();
  t.after(async () => {
    if (!t.passed) t.diagnostic(`Managed states at failure: ${JSON.stringify(manager.status())}; events: ${eventMethods.join(',')}`);
    for (const c of clients) c.ws.terminate();
    model.release();
    // Cleanup owns only children created by this test; never a shared/native production service.
    await Promise.all(workers.map(w => w.eof().catch(() => {})));
    service.wss.close(); await new Promise(resolve => service.server.close(resolve));
    await model.close(); await rm(root, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
  });
  async function client() {
    const c = await rpcClient(socketPath); clients.push(c);
    const info = await c.initialize();
    assert.equal(info.codexHome, env.CODEX_HOME);
    c.notify('initialized'); return c;
  }
  const a = await client();
  const { thread } = await a.rpc('thread/start', { model: 'fixture', modelProvider: 'fixture', historyMode: 'paginated', config: model.config });
  model.hold();
  const first = await a.rpc('turn/start', { threadId: thread.id, input: [{ type: 'text', text: 'Return isolated result one.' }] });
  await until(() => model.requests.length, n => n === 1);
  await assert.rejects(a.rpc('runtime/release', { threadId: thread.id }), /busy|unknown|active/);
  a.close(); await once(a.ws, 'close');
  assert.ok(workers.some(w => !w.dead), 'disconnect leaves worker running');
  const b = await client();
  assert.equal((await b.rpc('thread/read', { threadId: thread.id })).thread.status.type, 'active');
  model.release();
  await until(() => manager.status().find(u => u.threads.includes(thread.id)), u => u?.state === 'released');
  await assert.rejects(b.rpc('turn/start', { threadId: thread.id, input: [{ type: 'text', text: 'Must explicitly resume.' }] }), /released|resume|input not sent/);
  assert.equal(model.requests.length, 1);

  // A genuinely separate native app-server can now take the writer without deleting any lock.
  const external = createWorker();
  const info = await external.initialize(); assert.match(info.userAgent, /0\.163\.0/);
  await external.rpc('thread/resume', { threadId: thread.id, config: model.config });
  await external.eof();

  await b.rpc('thread/resume', { threadId: thread.id, config: model.config });
  const second = await b.rpc('turn/start', { threadId: thread.id, input: [{ type: 'text', text: 'Return isolated result two.' }] });
  await until(() => model.requests.length, n => n === 2);
  await until(() => manager.status().filter(u => u.threads.includes(thread.id)), units => units.length >= 2 && units.every(u => u.state === 'released'));
  const history = await b.rpc('thread/turns/list', { threadId: thread.id, itemsView: 'summary', limit: 10 });
  assert.ok(history.data.some(turn => turn.id === first.turn.id && turn.status === 'completed'));
  assert.ok(history.data.some(turn => turn.id === second.turn.id && turn.status === 'completed'));
  assert.match(JSON.stringify(history), /OWNED_RUNTIME_RESULT_1/);
  assert.match(JSON.stringify(history), /OWNED_RUNTIME_RESULT_2/);
  assert.equal(model.requests.length, 2, 'no model input replay');
  t.diagnostic('Two mock turns preserved; independent native writer acquired after managed EOF; no GUI claim or production session used');
});
