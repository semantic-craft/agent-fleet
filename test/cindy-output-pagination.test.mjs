import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import vm from 'node:vm';
const source = fs.readFileSync(new URL('../plugins/agent-network-cindy/main.js', import.meta.url), 'utf8');
function host(readMessages, run = { taskId: 'owned', runId: 'run', status: 'completed', outputMessageId: 'exact' }) {
  const context = vm.createContext({ cindy: { tasks: { getRun: async () => run, readMessages }, node: { request: async () => ({ ok: true }) }, onHostMessage() {} } });
  vm.runInContext(source, context);
  return vm.runInContext('statusJob', context);
}
test('follows byte-budget pages even with fewer than the requested message count', async () => {
  const calls = [];
  const read = host(async args => {
    calls.push(args);
    return args.after ? { items: [{ id: 'exact', role: 'assistant', content: 'actual final result' }] } : { items: [{ id: 'stale', role: 'assistant', content: 'not final' }], nextCursor: 'page-two' };
  });
  const result = await read({ taskId: 'owned', runId: 'run' });
  assert.equal(calls.length, 2); assert.equal(calls[1].after, 'page-two');
  assert.equal(result.messages.length, 1); assert.equal(result.messages[0].id, 'exact');
  assert.equal(result.messages[0].text, 'actual final result');
});
test('repeated cursor fails closed without stale output', async () => {
  const read = host(async () => ({ items: [{ id: 'old', role: 'assistant', content: 'stale' }], nextCursor: 'same' }));
  const result = await read({ taskId: 'owned', runId: 'run' });
  assert.equal(result.messages.length, 0); assert.ok(result.messagesError);
});
test('running host does not read or guess a final result', async () => {
  let reads = 0;
  const read = host(async () => { reads++; return {}; }, { status: 'running' });
  await read({ taskId: 'owned', runId: 'run' }); assert.equal(reads, 0);
});
test('missing exact output and pagination ceiling remain unverified', async () => {
  let reads = 0;
  const read = host(async () => ({ items: [], nextCursor: 'page-' + ++reads }));
  const result = await read({ taskId: 'owned', runId: 'run' });
  assert.equal(reads, 32); assert.equal(result.messages.length, 0); assert.ok(result.messagesError);
});
