// Explicit caller operation; never imported as an automatic model fallback.
const method = 'session/selectModelForSession';
const fields = ['provider', 'model', 'reasoningEffort'];
const blocked = code => ({ status: 'blocked', code });
export async function selectSessionModel(call, { sessionId, selection, exclusiveSession } = {}) {
  if (typeof call !== 'function' || typeof sessionId !== 'string' || !sessionId.trim() ||
      !selection || fields.some(k => typeof selection[k] !== 'string' || !selection[k].trim()) ||
      Object.keys(selection).some(k => !fields.includes(k))) return blocked('invalid_selection');
  // Ownership is the caller's responsibility, not a server-side lease assertion.
  if (exclusiveSession !== true) return blocked('exclusive_session_required');
  const request = { sessionId, ...Object.fromEntries(fields.map(k => [k, selection[k]])) };
  let catalog;
  try { catalog = await call('session/modelCatalog', {}); }
  catch { return blocked('capability_read_failed'); }
  if (catalog?.execution !== 'accepted' || catalog.error ||
      catalog.result?.capabilities?.selectModelForSession !== true) return blocked('unsupported');
  let receipt;
  try { receipt = await call(method, { request }); }
  catch { return { status: 'unknown', code: 'selection_readback_required', retryAllowed: false }; }
  if (receipt?.execution === 'not_started' || receipt?.execution === 'rejected') return blocked('selection_not_applied');
  const value = receipt?.result;
  if (receipt?.execution !== 'accepted' || receipt.error ||
      (receipt.nativeIds?.sessionId !== undefined && receipt.nativeIds.sessionId !== sessionId) || value?.scope !== 'session' ||
      value?.status !== 'applied' || !Number.isSafeInteger(value.seq) || value.seq < 0 ||
      fields.some(k => value.selected?.[k] !== request[k]))
    return { status: 'unknown', code: 'selection_readback_required', retryAllowed: false };
  return { status: 'applied', scope: 'session', sessionId, selection: Object.fromEntries(fields.map(k => [k, request[k]])),
    seq: value.seq, durable: false, nextRequestHeaderRequired: true };
}
