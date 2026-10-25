// Pure preflight for trusted, independently collected evidence. No RPC or persistence.
// A true result is not a capability probe or authorization to stop any service.
const nonempty = value => typeof value === 'string' && value.trim().length > 0;
const identityKeys = ['deviceId', 'instanceId', 'runtimeVersion', 'adapterRevision', 'desktopVersion'];
const same = (a, b, keys) => keys.every(k => nonempty(a?.[k]) && a[k] === b?.[k]);
export function checkCodexHandoff({phase, target, capability, brief, threadId, ownership, desktop, admission} = {}) {
  const blocked = reason => ({allowed: false, reason});
  if (!['create', 'submit'].includes(phase)) return blocked('invalid_phase');
  if (!same(target, capability, identityKeys) || capability?.supported !== true ||
      capability?.status !== 'verified' || !nonempty(capability?.nativeMethod) ||
      !nonempty(capability?.evidenceRef) || capability?.canary?.create !== true ||
      capability?.canary?.nativeHandoff !== true || capability?.canary?.desktopVisible !== true ||
      capability?.canary?.agentSubmit !== true || capability?.canary?.resultReadback !== true ||
      capability?.canary?.desktopCanTakeOverAfterSubmit !== true) {
    return blocked('blocked_native_writer_release');
  }
  if (brief?.deviceId !== target.deviceId || brief?.readback !== true || !nonempty(brief?.path) ||
      !brief.path.startsWith('/') || !nonempty(brief?.evidenceRef)) return blocked('blocked_target_brief');
  // The caller must supply a known non-submission state; unknown writes are observe-only.
  if (admission !== 'not_submitted') return blocked('observe_only');
  if (phase === 'create') return {allowed: true, action: 'thread/start'};
  const matchThread = evidence => nonempty(threadId) && same(target, evidence, identityKeys) &&
    evidence?.threadId === threadId && nonempty(evidence?.evidenceRef);
  if (!matchThread(ownership) || ownership?.verified !== true || ownership?.nativeMethod !== capability.nativeMethod) {
    return blocked('blocked_thread_ownership');
  }
  if (!matchThread(desktop) || desktop?.visible !== true) return blocked('blocked_desktop_visibility');
  return {allowed: true, action: 'turn/start'};
}
