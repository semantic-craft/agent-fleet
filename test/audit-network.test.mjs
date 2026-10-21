import test from "node:test";
import assert from "node:assert/strict";
import { auditProfile } from "../scripts/audit-network.mjs";

test("reports structural evidence without leaking URLs, credentials or rule payloads", () => {
  const output = auditProfile(`[General]
encrypted-dns-server = https://resolver.invalid/secret-xyz
http-listen = 0.0.0.0:6152
[Proxy]
private = custom, secret-host.invalid, password=secret-abc
[Rule]
DOMAIN,controlplane.tailscale.com,DIRECT
RULE-SET,https://subscription.invalid/token-xyz,PrivatePolicy
FINAL,PrivatePolicy
[Host]
private-host.invalid = server:example.invalid
`);
  const serialized = JSON.stringify(output);
  for (const value of ["secret", "invalid", "token-xyz", "PrivatePolicy", "6152"])
    assert.equal(serialized.includes(value), false);
  assert.equal(output.rules.tailscale, 1);
  assert.equal(output.rules.remoteRuleSets, 1);
  assert.equal(output.activeProfileAndRuntimeRouteVerified, false);
});

test("comments do not establish dependencies and unknown sections do not become rules", () => {
  const output = auditProfile("# tailscaled\n[Proxy]\nDOMAIN,controlplane.tailscale.com,DIRECT\n[Rule]\n; iroH\nFINAL,DIRECT\n");
  assert.equal(output.rules.total, 1);
  assert.equal(output.rules.tailscale, 0);
  assert.equal(output.rules.reasonOrIroh, 0);
});
