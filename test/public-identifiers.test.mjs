import test from 'node:test';
import assert from 'node:assert/strict';
import { withoutPublicIdentifiers, overlayContains } from '../scripts/public-identifiers.mjs';
test('published reviewer does not collide with a private hostname substring', () => {
  assert.equal(withoutPublicIdentifiers('reviewer: `helios-codex`'), 'reviewer: ``');
});
test('private hosts and extended identifiers remain scanned', () => {
  const reviewer = 'helios-codex';
  const host = reviewer.split('-')[0];
  for (const value of [host, `${host}.local`, `${reviewer}.local`, `private-${reviewer}`, `${reviewer}-token`]) {
    assert.equal(withoutPublicIdentifiers(value), value);
  }
});

test('email and URL overlay values are never masked by a public reviewer', () => {
  const reviewer = 'helios-codex';
  for (const value of [`${reviewer}@corp.example`, `http://${reviewer}/`, `https://${reviewer}:443`]) {
    assert.equal(withoutPublicIdentifiers(value), value);
    assert.equal(overlayContains(value, value), true);
    assert.equal(overlayContains(value, reviewer.split('-')[0]), true);
  }
});
