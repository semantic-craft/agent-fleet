/** Registered capabilities are supplied by the host integration, never by task JSON.
 * Availability is not live acceptance. This is not a sandbox for hostile JS hosts. */
export const CALLER_ROUTES = Object.freeze([
  Object.freeze({ caller: 'dot', target: 'codex', route: 'native-platform', acceptance: 'partial-maintenance-evidence' }),
  Object.freeze({ caller: 'dsh', target: 'codex', route: 'agenvo-desktop', acceptance: 'unverified' }),
  Object.freeze({ caller: 'grokbot', target: 'codex', route: 'agenvo-desktop', acceptance: 'unverified' }),
]);
export function createCallerRouter({ nativePlatform, agenvo, codexExecution } = {}) {
  // Snapshot executable capabilities: a task's {native: true, verified: true} is not one.
  const native = nativePlatform && ['discoverEnvironment', 'create', 'read', 'continue'].every(k => typeof nativePlatform[k] === 'function');
  const rpc = typeof agenvo === 'function';
  const mode = codexExecution?.mode;
  const sameHost = mode === 'same-host' && ['observeTarget', 'create', 'read', 'startTurn'].every(k => typeof codexExecution[k] === 'function');
  return Object.freeze({
    resolve({ caller, target }) {
      if (!['dot', 'dsh', 'grokbot'].includes(caller)) return Object.freeze({ status: 'unsupported', code: 'caller_unknown' });
      if (target === 'codex') {
        const spec = CALLER_ROUTES.find(r => r.caller === caller);
        if (spec.route === 'native-platform') return Object.freeze({ ...spec, status: native ? 'available' : 'unsupported', code: native ? 'discover_authorized_environment_before_create' : 'native_platform_unavailable' });
        if (!rpc) return Object.freeze({ ...spec, status: 'unsupported', code: 'agenvo_unavailable' });
        // Host registration is capability availability, never proof of Desktop visibility
        // or caller acceptance. Task JSON cannot select or certify this topology.
        if (sameHost) return Object.freeze({ ...spec, execution: 'same-host', status: 'available',
          code: 'observe_target_before_dispatch', desktop: 'unverified', acceptance: 'unverified' });
        return Object.freeze({ ...spec, status: 'blocked', code: mode === 'cross-backend'
          ? 'native_writer_unverified' : 'codex_execution_capability_unavailable' });
      }
      if (['dsh', 'herdr'].includes(target)) return Object.freeze({ caller, target, route: 'agenvo', status: rpc ? 'available' : 'unsupported', code: rpc ? 'runtime_contract_required' : 'agenvo_unavailable' });
      return Object.freeze({ status: 'unsupported', code: 'target_unknown' });
    },
  });
}
export function routeTable() {
  return ['| Caller | Target | Route | Acceptance |', '| --- | --- | --- | --- |', ...CALLER_ROUTES.map(r => `| ${r.caller} | ${r.target} | ${r.route} | ${r.acceptance} |`)].join('\n');
}
