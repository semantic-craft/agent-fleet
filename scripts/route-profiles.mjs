/** Explicit, versioned acceptance records. These never select a runtime model or preset. */
const selection = (preset, reasoningEffort = 'low') => Object.freeze({ provider: 'magpie-responses', model: 'codex/gpt-6.1-sol', reasoningEffort, preset });
const evidence = Object.freeze({ validatedOn: '2026-10-24', dsh: '0.2.1-rc.3', piAi: '1.1.0', magpieSource: 'exact-deployed-source-unproven', freshness: 'Snapshot, not a live health probe; revalidate after upgrades.' });
export const ROUTE_PROFILES = Object.freeze({
  'sol-low-ptc-marker-v1': Object.freeze({
    status: 'verified-marker-only', evidence, expected: selection('ptc'),
    scope: 'Exact marker, read-only, zero observed tool events; no tool execution or general workload claim.',
  }),
  'sol-high-standard-v1': Object.freeze({
    status: 'known-failure', evidence, expected: selection('standard', 'high'),
    scope: 'Empty max-tokens result in a fresh standard/high marker session; not a universal model limitation. Revalidate after upgrades.',
  }),
  'sol-low-standard-v1': Object.freeze({
    status: 'known-failure', evidence, expected: selection('standard'),
    scope: 'Empty max-tokens result on the tested deployment; not a universal model limitation. Revalidate after upgrades.',
  }),
});
export function routeProfileError(id, task, expected, requiresTool) {
  if (id === undefined) return null;
  const profile = Object.hasOwn(ROUTE_PROFILES, id) ? ROUTE_PROFILES[id] : null;
  if (!profile) return 'route_profile_unknown';
  if (profile.status === 'known-failure') return 'route_profile_known_failure';
  if (Object.entries(profile.expected).some(([key, value]) => expected[key] !== value)) return 'route_profile_selection_required';
  if (task.sideEffects !== 'read-only' || task.acceptance.kind !== 'exact-text' || !task.acceptance.text.trim() || requiresTool || task.requiredCapabilities.some(c => !['observe', 'submit'].includes(c))) return 'route_profile_scope_unsupported';
  return null;
}
