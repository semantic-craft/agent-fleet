import { createHash, randomBytes, timingSafeEqual } from 'node:crypto';

const OWNER = 'agent-fleet-agenvo-native';
const fail = code => { throw Object.assign(new Error(code), { code }); };
// SDK schemas may expose absent optional object fields as undefined. DSH's
// credential seam admits only JSON values: omit those fields, but refuse lossy
// conversions (undefined array slots, nonfinite numbers, exotic objects).
function clone(value, seen = new Set()) {
  if (value === undefined || value === null || typeof value === 'string' || typeof value === 'boolean') return value;
  if (typeof value === 'number') {
    if (!Number.isFinite(value) || Object.is(value, -0)) fail('oauth_non_json_metadata');
    return value;
  }
  if (typeof value !== 'object' || seen.has(value)) fail('oauth_non_json_metadata');
  seen.add(value);
  try {
    if (Array.isArray(value)) {
      return Array.from(value, item => {
        if (item === undefined) fail('oauth_non_json_metadata');
        return clone(item, seen);
      });
    }
    if (![Object.prototype, null].includes(Object.getPrototypeOf(value))) fail('oauth_non_json_metadata');
    return Object.fromEntries(Object.entries(value).filter(([, item]) => item !== undefined)
      .map(([name, item]) => [name, clone(item, seen)]));
  } finally { seen.delete(value); }
}
const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
function url(value, redirect = false) {
  let parsed;
  try { parsed = new URL(value); } catch { fail('oauth_invalid_url'); }
  if (parsed.username || parsed.password || parsed.search || parsed.hash ||
      (parsed.protocol !== 'https:' && !(redirect && parsed.protocol === 'http:' &&
        ['localhost', '127.0.0.1', '[::1]'].includes(parsed.hostname)))) fail('oauth_invalid_url');
  return parsed.href;
}
function normalizeBinding(input) {
  if (!input || typeof input.scope !== 'string' || !input.scope.trim()) fail('oauth_invalid_binding');
  const binding = {
    serverUrl: url(input.serverUrl), issuer: url(input.issuer), resource: url(input.resource),
    scope: [...new Set(input.scope.trim().split(/\s+/))].sort().join(' '),
  };
  if (new URL(binding.resource).origin !== new URL(binding.serverUrl).origin) fail('oauth_resource_mismatch');
  return binding;
}
/** Identity only; no credential value is part of the record address. */
export function credentialBindingId(binding) {
  return `grant-${createHash('sha256').update(JSON.stringify(normalizeBinding(binding))).digest('hex')}`;
}
function tokenSet(value, binding, previous) {
  if (!value || typeof value.access_token !== 'string' || !value.access_token ||
      typeof value.token_type !== 'string' || value.token_type.toLowerCase() !== 'bearer') fail('oauth_invalid_tokens');
  if (value.issuer !== undefined && url(value.issuer) !== binding.issuer) fail('oauth_issuer_mismatch');
  if (value.scope !== undefined && [...new Set(value.scope.trim().split(/\s+/))].sort().join(' ') !== binding.scope)
    fail('oauth_scope_mismatch');
  if (value.expires_in !== undefined && (!Number.isFinite(value.expires_in) || value.expires_in < 0))
    fail('oauth_invalid_tokens');
  const refresh = value.refresh_token ?? previous?.refresh_token;
  if (refresh !== undefined && (typeof refresh !== 'string' || !refresh)) fail('oauth_invalid_tokens');
  return {
    access_token: value.access_token, token_type: 'Bearer', issuer: binding.issuer,
    ...(refresh === undefined ? {} : { refresh_token: refresh }),
    ...(value.expires_in === undefined ? {} : { expires_in: value.expires_in }),
    scope: binding.scope,
  };
}
function discovery(value, binding) {
  if (!value || url(value.authorizationServerUrl) !== binding.issuer ||
      url(value.authorizationServerMetadata?.issuer) !== binding.issuer) fail('oauth_issuer_mismatch');
  const metadata = value.authorizationServerMetadata;
  url(metadata.token_endpoint);
  if (value.resourceMetadata && url(value.resourceMetadata.resource) !== binding.resource) fail('oauth_resource_mismatch');
  return clone(value);
}

/**
 * Uses DSH credentials.readRecord/modifyRecord and SDK 2.2 refreshAuthorization.
 * Inject SDK functions; constructing this object does not authorize, fetch or write.
 * The credential backend must supply exclusive async modifyRecord semantics.
 */
export function createCredentialAuth({ credentials, key, binding: supplied, refreshAuthorization,
  fetchFn, now = Date.now, refreshSkewMs = 30000 }) {
  const binding = normalizeBinding(supplied);
  if (key !== `${OWNER}/${credentialBindingId(binding)}` || typeof credentials?.modifyRecord !== 'function' ||
      typeof credentials?.readRecord !== 'function' || typeof refreshAuthorization !== 'function' ||
      typeof fetchFn !== 'function' || !Number.isFinite(refreshSkewMs) || refreshSkewMs < 0) fail('oauth_invalid_configuration');
  function payload(record) {
    if (!record) fail('oauth_authorization_required');
    const p = record.payload;
    if (record.kind !== 'grant' || p?.format !== 1 || !same(p.binding, binding) ||
        !Number.isSafeInteger(p.generation) || p.generation < 1 ||
        (p.expiresAt !== null && !Number.isFinite(p.expiresAt)) || !p.clientInformation?.client_id || p.clientInformation.client_secret !== undefined ||
        (p.clientInformation.token_endpoint_auth_method !== undefined && p.clientInformation.token_endpoint_auth_method !== 'none'))
      fail('oauth_record_mismatch');
    tokenSet(p.tokens, binding);
    discovery(p.discovery, binding);
    return p;
  }
  const expired = p => p.expiresAt !== null && p.expiresAt <= now() + refreshSkewMs;
  async function refresh(force) {
    try {
      const record = await credentials.modifyRecord(key, async current => {
        const p = payload(current);
        if (!force && !expired(p)) return undefined;
        if (!p.tokens.refresh_token) fail('oauth_authorization_required');
        // Hold the native record lock across rotation. A stale 401 has no rejected
        // token identifier in SDK 2.2: refresh the CURRENT token even before expiry.
        // Concurrent 401s can rotate sequentially; this is not request singleflight.
        const received = await refreshAuthorization(binding.issuer, {
          metadata: p.discovery.authorizationServerMetadata,
          clientInformation: p.clientInformation, refreshToken: p.tokens.refresh_token,
          resource: binding.resource, fetchFn,
        });
        const tokens = tokenSet(received, binding, p.tokens);
        return { kind: 'grant', payload: { ...clone(p), tokens, generation: p.generation + 1,
          expiresAt: tokens.expires_in === undefined ? null : now() + tokens.expires_in * 1000 } };
      });
      return payload(record).tokens.access_token;
    } catch { fail('oauth_refresh_failed'); }
  }
  const authProvider = Object.freeze({
    async token() {
      try {
        const p = payload(await credentials.readRecord(key));
        return expired(p) ? await refresh(false) : p.tokens.access_token;
      } catch { fail('oauth_authorization_unavailable'); }
    },
    async onUnauthorized(context) {
      if (url(context?.serverUrl) !== binding.serverUrl || context?.response?.status !== 401)
        fail('oauth_unauthorized_context_mismatch');
      // Use the caller-injected auth fetch, not resource-request headers carried
      // by the transport's context.fetchFn (which may contain the old bearer).
      await refresh(true);
    },
  });

  /** Explicit host action only. No startup or 401 path calls this method. */
  async function beginAuthorization({ auth, clientMetadata, redirectUrl, timeoutMs = 300000 }) {
    if (typeof auth !== 'function' || !Number.isFinite(timeoutMs) || timeoutMs <= 0 || timeoutMs > 600000)
      fail('oauth_invalid_authorization_request');
    const redirect = url(redirectUrl, true);
    if (!clientMetadata || clientMetadata.token_endpoint_auth_method !== 'none' || clientMetadata.client_secret !== undefined ||
        !Array.isArray(clientMetadata.redirect_uris) ||
        clientMetadata.redirect_uris.length !== 1 || url(clientMetadata.redirect_uris[0], true) !== redirect)
      fail('oauth_redirect_mismatch');
    try { if (await credentials.readRecord(key)) fail('oauth_record_exists'); }
    catch { fail('oauth_authorization_unavailable'); }
    const deadline = now() + timeoutMs;
    const state = randomBytes(32).toString('hex');
    let consumed = false, cancelled = false, ended = false, client, tokens, verifier, discovered, authorizationUrl;
    const assertActive = () => { if (cancelled || ended || now() >= deadline) fail('oauth_authorization_expired'); };
    const issuer = ctx => { if (ctx && url(ctx.issuer) !== binding.issuer) fail('oauth_issuer_mismatch'); };
    const provider = {
      redirectUrl: redirect, clientMetadata: clone(clientMetadata),
      state: () => state,
      clientInformation: ctx => { issuer(ctx); return clone(client); },
      saveClientInformation: (value, ctx) => {
        issuer(ctx); assertActive();
        if (!value?.client_id || value.client_secret !== undefined ||
            (value.token_endpoint_auth_method !== undefined && value.token_endpoint_auth_method !== 'none'))
          fail('oauth_public_client_required');
        client = clone(value);
      },
      tokens: ctx => { issuer(ctx); return clone(tokens); },
      saveTokens: (value, ctx) => { assertActive(); issuer(ctx); tokens = tokenSet(value, binding); },
      saveCodeVerifier: value => { assertActive(); verifier = value; },
      codeVerifier: () => { if (!verifier) fail('oauth_verifier_missing'); return verifier; },
      saveDiscoveryState: value => { assertActive(); discovered = discovery(value, binding); },
      discoveryState: () => clone(discovered),
      validateResourceURL: async (server, resource) => {
        if (url(server) !== binding.serverUrl || (resource !== undefined && url(resource) !== binding.resource))
          fail('oauth_resource_mismatch');
        return new URL(binding.resource);
      },
      redirectToAuthorization: value => {
        const target = new URL(value);
        if (!discovered || target.origin !== new URL(discovered.authorizationServerMetadata.authorization_endpoint).origin ||
            target.searchParams.get('state') !== state) fail('oauth_redirect_mismatch');
        authorizationUrl = target.href;
      },
    };
    try {
      const result = await auth(provider, { serverUrl: binding.serverUrl, scope: binding.scope, fetchFn });
      assertActive();
      if (result !== 'REDIRECT' || !authorizationUrl || !verifier || !client?.client_id || !discovered)
        fail('oauth_authorization_incomplete');
    } catch { ended = true; fail('oauth_authorization_failed'); }
    return Object.freeze({
      authorizationUrl,
      cancel() { cancelled = true; verifier = undefined; tokens = undefined; },
      async finish(callbackUrl) {
        assertActive();
        if (consumed) fail('oauth_authorization_expired');
        // Every callback attempt is one-use, including rejected state or issuer.
        consumed = true;
        try {
          const callback = new URL(callbackUrl);
          if (`${callback.origin}${callback.pathname}` !== `${new URL(redirect).origin}${new URL(redirect).pathname}` ||
              callback.hash || callback.username || callback.password || callback.searchParams.has('error')) fail('oauth_callback_mismatch');
          const states = callback.searchParams.getAll('state'), codes = callback.searchParams.getAll('code');
          const receivedState = Buffer.from(states[0] ?? '');
          if (states.length !== 1 || codes.length !== 1 || !codes[0] || receivedState.length !== state.length ||
              !timingSafeEqual(receivedState, Buffer.from(state))) fail('oauth_state_mismatch');
          const issuers = callback.searchParams.getAll('iss');
          if (issuers.length > 1 || (issuers.length === 1 && url(issuers[0]) !== binding.issuer)) fail('oauth_issuer_mismatch');
          const result = await auth(provider, { serverUrl: binding.serverUrl, scope: binding.scope, fetchFn,
            authorizationCode: codes[0], ...(issuers.length ? { iss: issuers[0] } : {}) });
          assertActive();
          if (result !== 'AUTHORIZED' || !tokens) fail('oauth_authorization_incomplete');
          await credentials.modifyRecord(key, async current => {
            assertActive();
            if (current !== undefined) fail('oauth_record_changed');
            return { kind: 'grant', payload: { format: 1, binding, generation: 1,
              clientInformation: clone(client), discovery: clone(discovered), tokens: clone(tokens),
              expiresAt: tokens.expires_in === undefined ? null : now() + tokens.expires_in * 1000 } };
          });
          return { status: 'authorized' };
        } catch { fail('oauth_authorization_failed'); }
        finally { ended = true; verifier = undefined; tokens = undefined; }
      },
    });
  }
  return Object.freeze({ authProvider, beginAuthorization });
}
