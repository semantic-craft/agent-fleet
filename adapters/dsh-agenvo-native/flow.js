import { createServer } from 'node:http';

export const CALLBACK_URL = 'http://127.0.0.1:13346/oauth/callback';
const HOST = '127.0.0.1';
const PORT = 13346;
const error = code => Object.assign(new Error(code), { code });

/** Register with DSH's authorization service; registration performs no authorization or listen. */
export function registerAuthorizationFlow({ authorization, key, oauthController, auth, clientMetadata,
  label = 'Agenvo', timeoutMs = 300000, createServerImpl = createServer }) {
  if (typeof authorization?.registerFlow !== 'function' || typeof oauthController?.beginAuthorization !== 'function' ||
      typeof auth !== 'function' || !key || !Number.isFinite(timeoutMs) || timeoutMs <= 0 || timeoutMs > 300000 ||
      clientMetadata?.redirect_uris?.length !== 1 || clientMetadata.redirect_uris[0] !== CALLBACK_URL)
    throw error('oauth_flow_configuration_invalid');
  const metadata = structuredClone(clientMetadata);
  const active = new Set();
  let disposed = false;
  const unregister = authorization.registerFlow({
    key, label, methods: [{ id: 'oauth', label: 'Authorize Agenvo in browser' }],
    async run(session) {
      if (disposed || session.signal.aborted) throw error('oauth_flow_cancelled');
      if (session.method !== 'oauth') throw error('oauth_flow_method_invalid');
      let pending, server, timer, closing, callbackUsed = false, stopped = false;
      const sockets = new Set();
      const ended = Promise.withResolvers();
      // This promise can reject before listen/begin resolve. Attach a handler now;
      // all waits below race it and the final result still propagates the failure.
      ended.promise.catch(() => {});
      function stop(code) {
        if (stopped) return;
        stopped = true;
        pending?.cancel();
        ended.reject(error(code));
        void close();
      }
      function close() {
        if (closing) return closing;
        closing = new Promise(resolve => {
          if (!server) { resolve(); return; }
          server.close(() => resolve());
          server.closeAllConnections?.();
          for (const socket of sockets) socket.destroy();
        });
        return closing;
      }
      const abort = () => stop('oauth_flow_cancelled');
      active.add(abort);
      session.signal.addEventListener('abort', abort, { once: true });
      timer = setTimeout(() => stop('oauth_flow_timeout'), timeoutMs);
      function reply(response, status, text, finish) {
        response.writeHead(status, {
          'Content-Type': 'text/plain; charset=utf-8', 'Cache-Control': 'no-store',
          'Referrer-Policy': 'no-referrer', 'X-Content-Type-Options': 'nosniff',
          'Content-Security-Policy': "default-src 'none'", Connection: 'close',
        });
        response.end(text, finish);
      }
      try {
        server = createServerImpl((request, response) => {
          // Never reflect a URL, code, state, token, or SDK error into HTTP.
          if (stopped) { reply(response, 410, 'Authorization closed.'); return; }
          if (request.method !== 'GET') { reply(response, 405, 'GET required.'); return; }
          if (request.headers.host !== `${HOST}:${PORT}` ||
              !['127.0.0.1', '::ffff:127.0.0.1'].includes(request.socket.remoteAddress) ||
              (request.headers.origin !== undefined && request.headers.origin !== `http://${HOST}:${PORT}`)) {
            reply(response, 403, 'Callback refused.'); return;
          }
          let callback;
          try {
            if (typeof request.url !== 'string' || request.url.length > 8192 || !request.url.startsWith('/'))
              throw error('invalid');
            callback = new URL(request.url, CALLBACK_URL);
            if (callback.origin !== `http://${HOST}:${PORT}` || callback.pathname !== '/oauth/callback' || callback.hash)
              throw error('invalid');
          } catch { reply(response, 404, 'Callback not found.'); return; }
          if (!pending) { reply(response, 503, 'Authorization not ready.'); return; }
          if (callbackUsed) { reply(response, 409, 'Callback already received.'); return; }
          callbackUsed = true;
          Promise.resolve().then(() => pending.finish(callback.href)).then(result => {
            if (stopped) return;
            if (result?.status !== 'authorized') throw error('oauth_flow_incomplete');
            reply(response, 200, 'Authorization completed. You may close this window.', () => ended.resolve());
          }).catch(() => {
            if (stopped) return;
            reply(response, 400, 'Authorization failed. Start a new attempt.', () => ended.reject(error('oauth_flow_callback_failed')));
          });
        });
        server.maxConnections = 8;
        server.maxHeadersCount = 20;
        server.requestTimeout = 5000;
        server.headersTimeout = 5000;
        server.keepAliveTimeout = 1000;
        server.on('connection', socket => { sockets.add(socket); socket.once('close', () => sockets.delete(socket)); });
        const listening = Promise.withResolvers();
        server.on('error', () => { listening.reject(error('oauth_flow_listener_failed')); stop('oauth_flow_listener_failed'); });
        server.listen({ host: HOST, port: PORT, exclusive: true }, () => listening.resolve());
        await Promise.race([listening.promise, ended.promise]);
        if (stopped || session.signal.aborted) throw error('oauth_flow_cancelled');
        const starting = oauthController.beginAuthorization({ auth, clientMetadata: metadata,
          redirectUrl: CALLBACK_URL, timeoutMs });
        // A late SDK discovery result after withdrawal must never become usable.
        Promise.resolve(starting).then(value => { if (stopped) value.cancel(); }, () => {});
        pending = await Promise.race([starting, ended.promise]);
        if (stopped || session.signal.aborted) { pending.cancel(); throw error('oauth_flow_cancelled'); }
        session.notify({ message: 'Continue in your browser to authorize Agenvo.', url: pending.authorizationUrl });
        await ended.promise;
      } catch { throw error(stopped ? 'oauth_flow_cancelled_or_expired' : 'oauth_flow_failed'); }
      finally {
        stopped = true;
        clearTimeout(timer);
        session.signal.removeEventListener('abort', abort);
        active.delete(abort);
        pending?.cancel();
        await close();
      }
    },
  });
  return () => {
    if (disposed) return;
    disposed = true;
    for (const abort of active) abort();
    unregister();
  };
}
