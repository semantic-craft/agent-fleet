/**
 * Native bridge: exposes the desktop Host's own Typert Remote endpoints
 * (`session/list`, `session/page`, `session/follow`, `session/prompt`, ...)
 * on a same-user Unix socket, so a local client such as the Agenvo `dsh`
 * connector can manage every desktop session — including ones the user opened
 * by hand — through the same endpoints the desktop UI uses.
 *
 * Nothing here interprets sessions. Each request names one native endpoint and
 * its native named arguments; the call goes through `ctx.typertGateway` with
 * one dedicated owner Peer (`interactions: 'none'`, so it never receives or
 * answers forwarded waterfalls), and the native result or RemoteError comes
 * back unchanged.
 *
 *   GET  /health                     -> {ok, pid, peer}
 *   POST /api/<namespace>/<method>   {args} -> {ok:true, value} | {ok:false, error:{code,message,details}}
 *   POST /stream/<namespace>/<method> {args} -> NDJSON lines {item} ..., then {end:true} or {error}
 *
 * Access control is the socket itself: its directory is 0700 and the socket
 * 0600, so only the macOS user who runs the desktop app can connect.
 */
import fs from 'node:fs';
import http from 'node:http';
import net from 'node:net';
import path from 'node:path';

const ENDPOINT = /^\/(api|stream)\/([A-Za-z$][A-Za-z0-9_$.-]*)\/([A-Za-z$][A-Za-z0-9_$.-]*)$/;
export const BODY_LIMIT = 32 * 1024 * 1024;

// The Host runs inside the DSH app binary; its bundle manifest names the DSH version.
function dshVersion() {
  try { return JSON.parse(fs.readFileSync(path.join(path.dirname(process.execPath), '../Resources/app.asar/package.json'), 'utf8')).version ?? null; } catch { return null; }
}
const DSH_VERSION = dshVersion();

export function socketPath(cfg) {
  return process.env.DSH_NATIVE_SOCKET || cfg.nativeSocket || path.join(cfg.stateDir, 'native.sock');
}

/** RemoteError (or any thrown value) -> the native RPC failure shape. */
export function remoteFailure(error) {
  if (error && typeof error === 'object' && typeof error.code === 'string')
    return { code: error.code, message: String(error.message ?? error.code), details: error.details ?? {} };
  return { code: 'gateway/internal', message: error instanceof Error ? error.message : String(error), details: {} };
}

// Native results are JSON values; keep byte values and bigints representable instead of failing the reply.
function replacer(_key, value) {
  if (typeof value === 'bigint') return value.toString();
  if (value instanceof Uint8Array) return { $bytes: Buffer.from(value).toString('base64') };
  return value;
}
const json = (value) => JSON.stringify(value, replacer);

function readJson(req) {
  return new Promise((resolve, reject) => {
    let size = 0;
    const chunks = [];
    req.on('data', (c) => {
      size += c.length;
      if (size > BODY_LIMIT) { reject(Object.assign(new Error('body too large'), { status: 413 })); req.destroy(); } else chunks.push(c);
    });
    req.on('end', () => {
      try { resolve(JSON.parse(Buffer.concat(chunks).toString('utf8'))); } catch { reject(Object.assign(new Error('body is not JSON'), { status: 400 })); }
    });
    req.on('error', reject);
  });
}

/**
 * Build the request handler. `getGateway()` and `getPeer()` are read per call so
 * a reloaded Gateway or a fresh Peer is used without restarting the listener.
 * `getPeer()` returns undefined on DSH versions without `connection.createPeer`.
 */
export function createHandler({ getGateway, getPeer, logger = console }) {
  return async (req, res) => {
    const send = (status, body) => { res.writeHead(status, { 'content-type': 'application/json' }); res.end(json(body)); };
    try {
      if (req.method === 'GET' && req.url === '/health') {
        const peer = getPeer();
        return send(200, { ok: Boolean(getGateway()), pid: process.pid, dsh: DSH_VERSION, peer: peer ? { principal: 'owner', interactions: 'none' } : 'gateway-operator' });
      }
      const match = ENDPOINT.exec(req.url ?? '');
      if (req.method !== 'POST' || !match) return send(404, { error: 'not found' });
      const [, carrier, namespace, method] = match;
      const body = await readJson(req);
      if (!body || typeof body !== 'object' || Array.isArray(body) || !body.args || typeof body.args !== 'object' || Array.isArray(body.args))
        return send(400, { error: 'body must be {"args": {...}} with the native named arguments' });
      const gateway = getGateway();
      if (!gateway) return send(503, { error: 'DSH Host gateway is not available' });
      // No Peer (DSH before per-Peer principals, e.g. 0.2.0-rc.2): the Gateway uses its own operator Peer.
      const peer = getPeer();
      const abort = new AbortController();
      res.on('close', () => abort.abort());
      const request = { namespace, method, args: body.args, ...(peer ? { peer } : {}), signal: abort.signal };
      if (carrier === 'api') {
        let reply;
        try { reply = { ok: true, value: await gateway.invoke(request) }; } catch (error) { reply = { ok: false, error: remoteFailure(error) }; }
        return send(200, reply);
      }
      let stream;
      try { stream = await gateway.stream(request); } catch (error) { return send(200, { error: remoteFailure(error) }); }
      res.writeHead(200, { 'content-type': 'application/x-ndjson' });
      try {
        for await (const item of stream) {
          if (abort.signal.aborted) break;
          if (!res.write(json({ item }) + '\n')) await new Promise((r) => res.once('drain', r));
        }
        if (!abort.signal.aborted) res.end(json({ end: true }) + '\n');
      } catch (error) {
        if (!abort.signal.aborted) res.end(json({ error: remoteFailure(error) }) + '\n');
      }
    } catch (error) {
      if (!res.headersSent) send(error.status ?? 500, { error: error.message });
      else res.destroy();
      if (!error.status) logger.warn?.(`native bridge: ${error.message}`);
    }
  };
}

/** Remove a stale socket file left by a previous process, but never one that still accepts connections. */
async function clearStaleSocket(file) {
  let stat;
  try { stat = fs.lstatSync(file); } catch { return; }
  if (!stat.isSocket()) throw new Error(`${file} exists and is not a socket`);
  const live = await new Promise((resolve) => {
    const probe = net.connect(file);
    probe.once('connect', () => { probe.destroy(); resolve(true); });
    probe.once('error', () => resolve(false));
  });
  if (live) throw new Error(`${file} is already served by another process`);
  fs.unlinkSync(file);
}

/** Listen on the Unix socket; resolves to a close function. */
export async function listenNativeBridge(file, handler, logger = console) {
  fs.mkdirSync(path.dirname(file), { recursive: true, mode: 0o700 });
  fs.chmodSync(path.dirname(file), 0o700);
  await clearStaleSocket(file);
  const server = http.createServer(handler);
  server.requestTimeout = 0;
  // Never touch process.umask here: it is process-wide, and the Host creates its own files concurrently.
  // The 0700 directory already keeps other users out before the chmod below.
  await new Promise((resolve, reject) => { server.once('error', reject); server.listen(file, resolve); });
  fs.chmodSync(file, 0o600);
  logger.info?.(`native bridge listening on ${file}`);
  return () => new Promise((resolve) => {
    server.close(() => resolve());
    server.closeAllConnections?.();
    try { fs.unlinkSync(file); } catch {}
  });
}

/** Mount the bridge inside the DSH Host (desktop profile). */
export function installNativeBridge(ctx, { cfg, logger = console }) {
  if (cfg.nativeBridge === false || process.env.DSH_NATIVE_BRIDGE === '0') return;
  ctx.effect(() => {
    let peer;
    const getPeer = () => {
      if (peer && !peer.signal?.aborted) return peer;
      const connection = ctx.get?.('connection');
      if (typeof connection?.createPeer !== 'function') return undefined;
      peer = connection.createPeer({ principal: { kind: 'owner' }, interactions: 'none' });
      return peer;
    };
    const file = socketPath(cfg);
    let close;
    let stopped = false;
    listenNativeBridge(file, createHandler({ getGateway: () => ctx.get?.('typertGateway'), getPeer, logger }), logger)
      .then((fn) => { if (stopped) fn(); else close = fn; })
      .catch((e) => logger.error?.(`native bridge not started: ${e.message}`));
    return async () => {
      stopped = true;
      await close?.();
      await peer?.dispose?.();
    };
  });
}
