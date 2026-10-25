import test from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { runSimulation } from '../scripts/recovery-drill.mjs';
test('simulation exercises upgrade and unknown-admission recovery without transport', () => {
  const result = runSimulation(); assert.equal(result.mode, 'offline-simulation'); assert.equal(result.physicalMigrationVerified, false);
  assert.equal(result.cases.rollbackOriginalSnapshot.status, 'simulation-passed');
  assert.equal(result.cases.rollbackCandidateBlocked.code, 'identity_changed_revalidate');
  assert.equal(result.cases.rollbackRestoredRechecked.status, 'simulation-passed');
  assert.equal(result.cases.simulatedCanary.status, 'simulation-passed');
  assert.equal(result.cases.simulationCannotVerifyProduction.status, 'blocked');
  assert.equal(result.cases.aliasNeedsNewCanary.status, 'blocked');
  assert.equal(result.cases.candidateNotVerified.status, 'blocked');
  assert.equal(result.cases.unknownAdmission.status, 'observe-only');
  assert.equal(result.cases.unknownAdmission.maySubmit, false);
});
test('executable drill runs isolated child and does not emit inherited personal context', () => {
  const script = fileURLToPath(new URL('../scripts/recovery-drill.mjs', import.meta.url));
  const child = spawnSync(process.execPath, [script], { encoding: 'utf8', env: { ...process.env, CODEX_HOME: '/synthetic/private-home', OPENAI_API_KEY: 'PRIVATE_SYNTHETIC_TOKEN', AGENT_FLEET_CONFIG: '/synthetic/nonexistent-config' } });
  assert.equal(child.status, 0, child.stderr);
  const result = JSON.parse(child.stdout); assert.equal(result.mode, 'offline-simulation');
  assert.ok(!child.stdout.includes('PRIVATE')); assert.ok(!child.stdout.includes('private-home'));
});
