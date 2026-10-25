import { randomUUID } from 'node:crypto';
// Pure operator preflight. This module never writes settings or calls a runtime.
const keys = ['provider', 'model', 'reasoningEffort'];
const valid = x => x && keys.every(k => typeof x[k] === 'string' && x[k].length > 0) && Object.keys(x).length === keys.length;
const copy = x => Object.freeze(Object.fromEntries(keys.map(k => [k, x[k]])));
const same = (a, b) => valid(a) && valid(b) && keys.every(k => a[k] === b[k]);
const blocked = code => ({ status: 'blocked', code, mutationAllowed: false });
const coordinated = x => x?.authorized === true && x?.allSelectorsPaused === true && x?.newSessionCreationPaused === true;
export function planModelSelection({ before, desired, sessionId, coordination } = {}) {
  if (!valid(before) || !valid(desired) || typeof sessionId !== 'string' || !sessionId.trim()) return blocked('selection_snapshot_required');
  if (!coordinated(coordination)) return blocked('global_selection_coordination_required');
  return Object.freeze({ status: 'planned', mutationAllowed: false, sessionId, operationId: randomUUID(),
    before: copy(before), desired: copy(desired),
    action: same(before, desired) ? 'verify_existing_session' : 'operator_select_then_restore',
  });
}
export function reviewModelRestoration(plan, { currentDefault, selectedSession, selectionOutcome, coordination, persistenceBarrier } = {}) {
  if (plan?.status !== 'planned' || !valid(plan.before) || !valid(plan.desired)) return blocked('selection_plan_required');
  if (!coordinated(coordination)) return blocked('global_selection_coordination_lost');
  if (selectionOutcome !== 'confirmed' || selectedSession?.sessionId !== plan.sessionId || !same(selectedSession?.selection, plan.desired)) return blocked('selection_unknown_read_back');
  if (persistenceBarrier?.kind !== 'runtime-persistence-barrier' || persistenceBarrier?.sessionId !== plan.sessionId || persistenceBarrier?.operationId !== plan.operationId || persistenceBarrier?.settled !== true) return blocked('default_persistence_unproven');
  if (same(currentDefault, plan.before)) return { status: 'verified', code: 'default_restored', mutationAllowed: false };
  if (!same(currentDefault, plan.desired)) return blocked('external_default_drift');
  return { status: 'operator_action_required', code: 'restore_original_default_and_verify', mutationAllowed: false };
}
