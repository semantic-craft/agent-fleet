/** Pure acceptance and recovery gates over caller-supplied evidence, never a dispatcher. */
import { createHash } from 'node:crypto';
const object = x => x !== null && typeof x === 'object' && !Array.isArray(x);
const keys = (x, fields) => object(x) && Object.keys(x).sort().join(',') === [...fields].sort().join(',');
const hash = x => typeof x === 'string' && /^[a-f0-9]{64}$/.test(x);
const token = x => typeof x === 'string' && /^[A-Za-z0-9_.:/@+-]{1,240}$/.test(x);
const concrete = x => token(x) && !/(^|[^a-z])(unknown|candidate|latest|default)([^a-z]|$)/i.test(x);
const identityFields = ['runtime', 'adapter', 'provider', 'model', 'modelAlias', 'effort', 'preset', 'policy', 'harness', 'schema'];
const dependencyFields = ['task', 'context', 'journal', 'policy', 'remote', 'route'];
const result = (status, code, extra = {}) => Object.freeze({ status, code, ...extra });
const blocked = code => result('blocked', code);
function validIdentity(identity) { return keys(identity, identityFields) && identityFields.every(k => concrete(identity[k])); }
/** A deterministic binding, not proof that the supplied identity was observed. */
export function identityHash(identity) {
  if (!validIdentity(identity)) throw new TypeError('invalid_identity');
  return createHash('sha256').update(JSON.stringify(identityFields.map(k => [k, identity[k]]))).digest('hex');
}
export function assessCompatibility(input, { simulation = false, now = Date.now() } = {}) {
  if (!keys(input, ['identity', 'capabilities', 'requiredCapabilities', 'scope', 'canary']) || !validIdentity(input.identity)) return blocked('identity_unproven');
  const { identity, capabilities, requiredCapabilities: required, scope, canary } = input;
  if (!object(capabilities) || Object.entries(capabilities).some(([k, v]) => !token(k) || !['verified', 'candidate', 'unknown', 'unsupported'].includes(v)) || !Array.isArray(required) || !required.length || required.some(k => !token(k)) || new Set(required).size !== required.length) return blocked('invalid_capabilities');
  if (required.some(k => !Object.hasOwn(capabilities, k) || capabilities[k] !== 'verified')) return blocked('capability_unverified');
  if (!['marker', 'read-fixture'].includes(scope)) return blocked('scope_unsupported');
  if (!keys(canary, ['identityHash', 'source', 'status', 'terminal', 'output', 'tools', 'expiresAt', ...(scope === 'read-fixture' ? ['permissionBefore', 'permissionAfter'] : [])])) return blocked('canary_missing');
  if (canary.identityHash !== identityHash(identity)) return blocked('identity_changed_revalidate');
  if (!Number.isSafeInteger(now) || now < 0 || !Number.isSafeInteger(canary.expiresAt) || canary.expiresAt <= now) return blocked('canary_expired');
  if (scope === 'read-fixture' && (canary.permissionBefore !== 'read-only' || canary.permissionAfter !== 'read-only')) return blocked('read_only_permission_required');
  if (canary.status !== 'verified' || canary.terminal !== 'completed') return blocked('canary_not_completed');
  if (!['observed', 'simulated'].includes(canary.source) || (!simulation && canary.source !== 'observed')) return blocked('observed_evidence_required');
  if (!keys(canary.output, ['expectedHash', 'actualHash']) || !hash(canary.output.expectedHash) || canary.output.expectedHash !== canary.output.actualHash) return blocked('output_not_verified');
  const tools = canary.tools;
  if (scope === 'marker') {
    if (!keys(tools, ['callCount', 'resultCount']) || tools.callCount !== 0 || tools.resultCount !== 0) return blocked('tool_evidence_mismatch');
  } else {
    if (!keys(tools, ['callCount', 'resultCount', 'name', 'expectedArgsHash', 'actualArgsHash', 'callId', 'resultCallId', 'success']) || tools.callCount !== 1 || tools.resultCount !== 1 || tools.name !== 'read' || !hash(tools.expectedArgsHash) || tools.actualArgsHash !== tools.expectedArgsHash || !token(tools.callId) || tools.resultCallId !== tools.callId || tools.success !== true) return blocked('tool_evidence_mismatch');
  }
  return result(simulation ? 'simulation-passed' : 'verified', 'bounded_canary_verified');
}
function validDependencies(x) { return keys(x, dependencyFields) && dependencyFields.every(k => hash(x[k])); }
function validNative(x) { return keys(x, ['sessionId', 'requestId']) && token(x.sessionId) && token(x.requestId); }
/** Recovery never admits a new prompt; the existing native request must be observed first. */
export function assessRecovery(input) {
  if (!keys(input, ['saved', 'current'])) return blocked('recovery_evidence_missing');
  const { saved, current } = input;
  if (!keys(saved, ['format', 'native', 'dependencies', 'admission']) || !keys(current, ['format', 'native', 'dependencies', 'journalPresent'])) return blocked('recovery_evidence_missing');
  if (saved.format !== 1 || current.format !== 1) return blocked('recovery_version_unsupported');
  if (!['unknown', 'accepted', 'running'].includes(saved.admission)) return blocked('recovery_state_unsupported');
  if (!validNative(saved.native) || !validNative(current.native) || !validDependencies(saved.dependencies) || !validDependencies(current.dependencies)) return blocked('recovery_evidence_invalid');
  if (current.journalPresent !== true) return blocked('journal_missing');
  if (saved.native.sessionId !== current.native.sessionId || saved.native.requestId !== current.native.requestId) return blocked('native_identity_changed');
  if (dependencyFields.some(k => saved.dependencies[k] !== current.dependencies[k])) return blocked('recovery_dependency_changed');
  return result('observe-only', 'original_native_request_required', { maySubmit: false });
}
