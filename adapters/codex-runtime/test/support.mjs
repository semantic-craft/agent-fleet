import { once } from 'node:events';
import { createServer } from 'node:http';
import { connect } from 'node:net';
import { join } from 'node:path';
import WebSocket from 'ws';

export const isolatedEnvironment = (root) => ({
  PATH: process.env.PATH, HOME: root, CODEX_HOME: join(root, 'codex'),
  TMPDIR: root, XDG_CONFIG_HOME: join(root, 'config'),
  XDG_DATA_HOME: join(root, 'data'), XDG_CACHE_HOME: join(root, 'cache'),
  TERM: 'xterm-256color', LANG: 'en_US.UTF-8', SHELL: '/bin/sh',
});

export async function until(read, predicate = Boolean, timeout = 10000) {
  const end = Date.now() + timeout;
  let value;
  do {
    value = await read();
    if (predicate(value)) return value;
    await new Promise(r => setTimeout(r, 20));
  } while (Date.now() < end);
  throw new Error(`Timed out: ${JSON.stringify(value)}`);
}

export async function rpcClient(socketPath) {
  const ws = new WebSocket('ws://localhost/rpc', { createConnection: () => connect(socketPath) });
  const pending = new Map(), notifications = [];
  let sequence = 0;
  ws.on('message', raw => {
    const packet = JSON.parse(String(raw));
    if (packet.method) { notifications.push(packet); return; }
    const waiter = pending.get(packet.id);
    if (!waiter) return;
    pending.delete(packet.id); clearTimeout(waiter.timer);
    packet.error ? waiter.reject(Object.assign(new Error(packet.error.message), packet.error)) : waiter.resolve(packet.result);
  });
  ws.on('close', () => {
    for (const p of pending.values()) { clearTimeout(p.timer); p.reject(new Error('Client closed')); }
    pending.clear();
  });
  await once(ws, 'open');
  const rpc = (method, params = {}) => new Promise((resolve, reject) => {
    const id = ++sequence;
    const timer = setTimeout(() => { pending.delete(id); reject(new Error(`RPC timed out: ${method}`)); }, 8000);
    pending.set(id, { resolve, reject, timer });
    ws.send(JSON.stringify({ jsonrpc: '2.0', id, method, params }));
  });
  const initialize = () => rpc('initialize', { clientInfo: { name: 'owned-runtime-fixture', version: '1' }, capabilities: { experimentalApi: true } });
  return { ws, rpc, initialize, notifications, close: () => ws.close(), notify: (method, params = {}) => ws.send(JSON.stringify({ method, params })) };
}

// All responses originate locally; no account, API key or real model is used.
export async function modelServer() {
  const requests = [], releases = new Set();
  let hold = false;
  const server = createServer(async (req, res) => {
    let body = ''; for await (const chunk of req) body += chunk;
    requests.push(JSON.parse(body));
    const text = `OWNED_RUNTIME_RESULT_${requests.length}`;
    res.writeHead(200, { 'Content-Type': 'text/event-stream' });
    const send = value => res.write(`event: ${value.type}\ndata: ${JSON.stringify(value)}\n\n`);
    send({ type: 'response.created', response: { id: 'fixture', status: 'in_progress', output: [] } });
    if (hold) await new Promise(resolve => { releases.add(resolve); res.once('close', () => { releases.delete(resolve); resolve(); }); });
    if (res.destroyed) return;
    const item = { id: `fixture-${requests.length}`, type: 'message', role: 'assistant', status: 'completed', content: [{ type: 'output_text', text, annotations: [] }] };
    send({ type: 'response.output_item.added', output_index: 0, item: { ...item, status: 'in_progress', content: [] } });
    send({ type: 'response.output_text.delta', output_index: 0, content_index: 0, item_id: item.id, delta: text });
    send({ type: 'response.output_text.done', output_index: 0, content_index: 0, item_id: item.id, text });
    send({ type: 'response.output_item.done', output_index: 0, item });
    send({ type: 'response.completed', response: { id: 'fixture', status: 'completed', output: [item], usage: { input_tokens: 1, output_tokens: 1, total_tokens: 2, input_tokens_details: { cached_tokens: 0 }, output_tokens_details: { reasoning_tokens: 0 } } } });
    res.end();
  }).listen(0, '127.0.0.1');
  await once(server, 'listening');
  return {
    requests, hold() { hold = true; }, release() { hold = false; for (const done of releases) done(); releases.clear(); },
    config: {
      'model_providers.fixture.name': 'Isolated model',
      'model_providers.fixture.base_url': `http://127.0.0.1:${server.address().port}/v1`,
      'model_providers.fixture.wire_api': 'responses',
      'model_providers.fixture.requires_openai_auth': false,
      'model_providers.fixture.supports_websockets': false,
    },
    async close() { for (const done of releases) done(); server.closeAllConnections(); await new Promise(resolve => server.close(resolve)); },
  };
}
