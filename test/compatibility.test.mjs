import test from 'node:test';
import assert from 'node:assert/strict';
import { assessCompatibility, assessRecovery, identityHash } from '../scripts/compatibility.mjs';
const identity = () => ({ runtime: 'runtime@1', adapter: 'adapter@1', provider: 'provider@1', model: 'model@1', modelAlias: 'alias@1', effort: 'low', preset: 'ptc', policy: 'read-only@1', harness: 'harness@1', schema: 'schema@1' });
const receipt = () => ({ identity: identity(), capabilities: { observe: 'verified', submit: 'verified' }, requiredCapabilities: ['observe', 'submit'], scope: 'marker', canary: { identityHash: identityHash(identity()), source: 'observed', expiresAt: Date.now() + 60000, status: 'verified', terminal: 'completed', output: { expectedHash: 'a'.repeat(64), actualHash: 'a'.repeat(64) }, tools: { callCount: 0, resultCount: 0 } } });
const recovery = () => {
  const saved = { format: 1, native: { sessionId: 'synthetic-session', requestId: 'synthetic-native-request' }, admission: 'unknown', dependencies: Object.fromEntries(['task', 'context', 'journal', 'policy', 'remote', 'route'].map(k => [k, 'a'.repeat(64)])) };
  return { saved, current: { format: 1, native: { ...saved.native }, dependencies: { ...saved.dependencies }, journalPresent: true } };
};
test('bounded observed marker canary validates without changing inputs', () => {
  const input = receipt(); const snapshot = structuredClone(input);
  assert.equal(assessCompatibility(input).status, 'verified'); assert.deepEqual(input, snapshot);
  assert.equal(identityHash(Object.fromEntries(Object.entries(input.identity).reverse())), identityHash(input.identity));
});
for (const key of Object.keys(identity())) test(`${key} identity change invalidates old canary`, () => {
  const input = receipt(); input.identity[key] += '-changed';
  assert.equal(assessCompatibility(input).code, 'identity_changed_revalidate');
});
for (const value of ['unknown', 'candidate', 'latest', 'default', 'runtime@unknown']) test(`unproven identity ${value} cannot pass`, () => {
  const input = receipt(); input.identity.runtime = value; assert.equal(assessCompatibility(input).status, 'blocked');
});
for (const state of ['candidate', 'unknown', 'unsupported']) test(`required capability ${state} is not verified`, () => {
  const input = receipt(); input.capabilities.submit = state; assert.equal(assessCompatibility(input).code, 'capability_unverified');
});
test('missing required capability and empty requirement set fail closed', () => {
  const input = receipt(); delete input.capabilities.submit; assert.equal(assessCompatibility(input).status, 'blocked');
  input.requiredCapabilities = []; assert.equal(assessCompatibility(input).status, 'blocked');
});
test('simulation never grants production verification', () => {
  const input = receipt(); input.canary.source = 'simulated';
  assert.equal(assessCompatibility(input).code, 'observed_evidence_required');
  assert.equal(assessCompatibility(input, { simulation: true }).status, 'simulation-passed');
  input.canary.source = 'observed'; assert.equal(assessCompatibility(input, { simulation: true }).status, 'simulation-passed');
});
for (const mutate of [x => { x.canary.status = 'accepted'; }, x => { x.canary.terminal = 'running'; }, x => { x.canary.output.actualHash = 'b'.repeat(64); }, x => { x.canary.tools.callCount = 1; }]) test('behavior canary requires terminal output and tool evidence', () => {
  const input = receipt(); mutate(input); assert.equal(assessCompatibility(input).status, 'blocked');
});
function readReceipt() {
  const input = receipt(); input.scope = 'read-fixture'; input.canary.permissionBefore = 'read-only'; input.canary.permissionAfter = 'read-only';
  input.canary.tools = { callCount: 1, resultCount: 1, name: 'read', expectedArgsHash: 'c'.repeat(64), actualArgsHash: 'c'.repeat(64), callId: 'synthetic-call', resultCallId: 'synthetic-call', success: true };
  return input;
}
test('read fixture requires matching argument and result correlation receipts', () => assert.equal(assessCompatibility(readReceipt()).status, 'verified'));
for (const change of [{ name: 'write' }, { callCount: 2 }, { resultCount: 0 }, { success: false }, { actualArgsHash: 'd'.repeat(64) }, { resultCallId: 'foreign' }]) test(`tool receipt mismatch ${Object.keys(change)[0]} blocks`, () => {
  const input = readReceipt(); Object.assign(input.canary.tools, change); assert.equal(assessCompatibility(input).code, 'tool_evidence_mismatch');
});
test('raw private fields are rejected, not echoed or stored', () => {
  const input = receipt(); input.canary.prompt = 'PRIVATE_SYNTHETIC_TEXT';
  const result = assessCompatibility(input); assert.equal(result.status, 'blocked'); assert.ok(!JSON.stringify(result).includes('PRIVATE'));
});
for (const admission of ['unknown', 'accepted', 'running']) test(`${admission} recovery only allows observing the original native request`, () => {
  const input = recovery(); input.saved.admission = admission;
  assert.deepEqual(assessRecovery(input), { status: 'observe-only', code: 'original_native_request_required', maySubmit: false });
});
for (const key of ['task', 'context', 'journal', 'policy', 'remote', 'route']) test(`changed ${key} dependency blocks recovery`, () => {
  const input = recovery(); input.current.dependencies[key] = 'b'.repeat(64); assert.equal(assessRecovery(input).code, 'recovery_dependency_changed');
});
for (const key of ['sessionId', 'requestId']) test(`changed native ${key} never creates a replacement request`, () => {
  const input = recovery(); input.current.native[key] = 'new-envelope-or-native-id'; assert.equal(assessRecovery(input).code, 'native_identity_changed');
});
test('missing journal and incompatible format do not fall back to fresh submission', () => {
  const input = recovery(); input.current.journalPresent = false; assert.equal(assessRecovery(input).code, 'journal_missing');
  input.current.format = 2; assert.equal(assessRecovery(input).code, 'recovery_version_unsupported');
});
test('partial or malformed recovery evidence fails closed', () => {
  assert.equal(assessRecovery().status, 'blocked');
  const input = recovery(); delete input.saved.dependencies.remote; assert.equal(assessRecovery(input).status, 'blocked');
});

test('expired and missing expiry canaries require a new observed check', () => {
  const input = receipt(); input.canary.expiresAt = 2000;
  assert.equal(assessCompatibility(input, { now: 1999 }).status, 'verified');
  assert.equal(assessCompatibility(input, { now: 2000 }).code, 'canary_expired');
  delete input.canary.expiresAt; assert.equal(assessCompatibility(input).status, 'blocked');
});
for (const field of ['permissionBefore', 'permissionAfter']) test(`read fixture requires ${field} evidence`, () => {
  const input = readReceipt(); input.canary[field] = 'workspace-write'; assert.equal(assessCompatibility(input).code, 'read_only_permission_required');
  delete input.canary[field]; assert.equal(assessCompatibility(input).status, 'blocked');
});
test('null recovery input fails closed', () => assert.equal(assessRecovery(null).status, 'blocked'));
