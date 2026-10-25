/** Registered capabilities are supplied by the host integration, never by task JSON.
 * Availability is not live acceptance. This is not a sandbox for hostile JS hosts. */
export const CALLER_ROUTES = Object.freeze([
  Object.freeze({ caller: 'dot', target: 'codex', route: 'native-platform', acceptance: 'partial-maintenance-evidence' }),
  Object.freeze({ caller: 'dsh', target: 'codex', route: 'agenvo-desktop', acceptance: 'unverified' }),
  Object.freeze({ caller: 'grokbot', target: 'codex', route: 'agenvo-desktop', acceptance: 'unverified' }),
]);
export function createCallerRouter({ nativePlatform, agenvo } = {}) {
  // Snapshot executable capabilities: a task's {native: true, verified: true} is not one.
  const native = nativePlatform && ['discoverEnvironment', 'create', 'read', 'continue'].every(k => typeof nativePlatform[k] === 'function');
  const rpc = typeof agenvo === 'function';
  return Object.freeze({
    resolve({ caller, target }) {
      if (!['dot', 'dsh', 'grokbot'].includes(caller)) return Object.freeze({ status: 'unsupported', code: 'caller_unknown' });
      if (target === 'codex') {
        const spec = CALLER_ROUTES.find(r => r.caller === caller);
        if (spec.route === 'native-platform') return Object.freeze({ ...spec, status: native ? 'available' : 'unsupported', code: native ? 'discover_authorized_environment_before_create' : 'native_platform_unavailable' });
        // No production observer currently proves native ownership transfer. No boolean input can enable this.
        return Object.freeze({ ...spec, status: 'blocked', code: rpc ? 'native_writer_unverified' : 'agenvo_unavailable' });
      }
      if (['dsh', 'herdr'].includes(target)) return Object.freeze({ caller, target, route: 'agenvo', status: rpc ? 'available' : 'unsupported', code: rpc ? 'runtime_contract_required' : 'agenvo_unavailable' });
      return Object.freeze({ status: 'unsupported', code: 'target_unknown' });
    },
  });
}
export function routeTable() {
  return ['| Caller | Target | Route | Acceptance |', '| --- | --- | --- | --- |', ...CALLER_ROUTES.map(r => `| ${r.caller} | ${r.target} | ${r.route} | ${r.acceptance} |`)].join('\n');
}
