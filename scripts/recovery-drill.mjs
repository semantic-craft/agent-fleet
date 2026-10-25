#!/usr/bin/env node
/** Executable offline simulation. No transport, config loader, credentials, or deployment. */
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { assessCompatibility, assessRecovery, identityHash } from './compatibility.mjs';
export function runSimulation() {
  const identity = { runtime: 'synthetic-runtime@1', adapter: 'synthetic-adapter@1', provider: 'synthetic-provider@1', model: 'synthetic-model@1', modelAlias: 'synthetic-alias@1', effort: 'low', preset: 'synthetic-preset@1', policy: 'synthetic-policy@1', harness: 'synthetic-harness@1', schema: 'synthetic-schema@1' };
  const route = { identity, capabilities: { observe: 'verified' }, requiredCapabilities: ['observe'], scope: 'marker', canary: { identityHash: identityHash(identity), source: 'simulated', expiresAt: 3000, status: 'verified', terminal: 'completed', output: { expectedHash: 'a'.repeat(64), actualHash: 'a'.repeat(64) }, tools: { callCount: 0, resultCount: 0 } } };
  const saved = { format: 1, native: { sessionId: 'synthetic-session', requestId: 'synthetic-request' }, dependencies: Object.fromEntries(['task', 'context', 'journal', 'policy', 'remote', 'route'].map(k => [k, 'b'.repeat(64)])), admission: 'unknown' };
  const current = { format: 1, native: saved.native, dependencies: saved.dependencies, journalPresent: true };
  const originalSnapshot = structuredClone(route);
  const candidate = { ...route, identity: { ...identity, runtime: 'synthetic-runtime@2' } };
  const cases = {
    rollbackOriginalSnapshot: assessCompatibility(originalSnapshot, { simulation: true, now: 2000 }),
    rollbackCandidateBlocked: assessCompatibility(candidate, { simulation: true, now: 2000 }),
    rollbackRestoredRechecked: assessCompatibility(structuredClone(originalSnapshot), { simulation: true, now: 2000 }),
    simulatedCanary: assessCompatibility(route, { simulation: true, now: 2000 }),
    simulationCannotVerifyProduction: assessCompatibility(route, { now: 2000 }),
    aliasNeedsNewCanary: assessCompatibility({ ...route, identity: { ...identity, modelAlias: 'synthetic-alias@2' } }, { simulation: true, now: 2000 }),
    candidateNotVerified: assessCompatibility({ ...route, capabilities: { observe: 'candidate' } }, { simulation: true, now: 2000 }),
    unknownAdmission: assessRecovery({ saved, current }),
    missingJournal: assessRecovery({ saved, current: { ...current, journalPresent: false } }),
    changedNativeIdentity: assessRecovery({ saved, current: { ...current, native: { ...current.native, requestId: 'synthetic-other' } } }),
  };
  return { mode: 'offline-simulation', physicalMigrationVerified: false, cases };
}
if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  if (process.argv.length === 3 && process.argv[2] === '--child') {
    process.stdout.write(JSON.stringify(runSimulation()) + '\n');
  } else if (process.argv.length === 2) {
    const home = mkdtempSync(join(tmpdir(), 'fleet-recovery-drill-'));
    try {
      const run = spawnSync(process.execPath, [fileURLToPath(import.meta.url), '--child'], { encoding: 'utf8', timeout: 10000, env: { HOME: home, USERPROFILE: home, XDG_CONFIG_HOME: join(home, '.config'), PATH: '/usr/bin:/bin', PYTHONDONTWRITEBYTECODE: '1' } });
      if (run.status !== 0 || run.error) { process.stderr.write('offline_drill_failed\n'); process.exitCode = 1; }
      else process.stdout.write(run.stdout);
    } finally { rmSync(home, { recursive: true, force: true }); }
  } else { process.stderr.write('usage: node scripts/recovery-drill.mjs\n'); process.exitCode = 1; }
}
