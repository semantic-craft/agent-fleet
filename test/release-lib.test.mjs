import test from "node:test";
import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { COMPONENTS, manifestDiff, ownedPlists, releaseOf, treeHashes } from "../scripts/release-lib.mjs";

const SHA = "a".repeat(40);

test("agenvo owns only connector plists whose entry is a known connector app", () => {
  const plists = [
    { label: "io.agenvo.connector.1", programArg: "/u/Projects/agenvo/apps/dsh/dist/cli.js" },
    { label: "io.agenvo.connector.2", programArg: `/u/.local/share/agent-fleet/releases/agenvo/${SHA}/apps/herdr/cli.js` },
    { label: "io.agenvo.connector.3", programArg: "/u/Projects/agenvo/apps/amp/dist/cli.js" },
    { label: "dev.xw.herdr-server", programArg: "server" },
    { label: undefined, programArg: undefined },
  ];
  assert.deepEqual(ownedPlists("agenvo", plists).map((p) => p.label), ["io.agenvo.connector.1", "io.agenvo.connector.2"]);
  assert.equal(COMPONENTS.agenvo.entry(plists[0].programArg), "apps/dsh/cli.js");
  assert.equal(COMPONENTS.agenvo.entry(plists[1].programArg), "apps/herdr/cli.js");
});

test("unknown components are rejected", () => {
  assert.throws(() => ownedPlists("telnet", [{ label: "dev.other", programArg: "/x/receiver/server.mjs" }]), /unknown component/);
});

test("releaseOf recognises only paths inside a full-SHA release under the root", () => {
  const root = "/u/.local/share/agent-fleet/releases/agenvo";
  assert.equal(releaseOf(`${root}/${SHA}/apps/dsh/cli.js`, root), `${root}/${SHA}`);
  assert.equal(releaseOf(`${root}/main/apps/dsh/cli.js`, root), null);
  assert.equal(releaseOf("/u/Projects/agenvo/apps/dsh/dist/cli.js", root), null);
});

test("treeHashes detects hand edits and refuses symlinks; skip excludes top-level names", () => {
  const dir = mkdtempSync(join(tmpdir(), "rel-"));
  mkdirSync(join(dir, "apps/dsh"), { recursive: true });
  writeFileSync(join(dir, "apps/dsh/cli.js"), "a");
  writeFileSync(join(dir, "release.json"), "{}");
  const before = treeHashes(dir, ["release.json"]);
  assert.deepEqual(Object.keys(before), ["apps/dsh/cli.js"]);
  writeFileSync(join(dir, "apps/dsh/cli.js"), "b");
  assert.deepEqual(manifestDiff(before, treeHashes(dir, ["release.json"])), ["apps/dsh/cli.js"]);
  symlinkSync("cli.js", join(dir, "apps/dsh/link"));
  assert.throws(() => treeHashes(dir), /symlink/);
});

test("profileWithPlugin swaps the old plugin for agent-fleet-dsh in place and is idempotent", async () => {
  const { profileWithPlugin } = await import("../scripts/release-lib.mjs");
  const pkg = { dsh: { profile: { bundles: ["a", "dsh-grokbot-dispatch", "b"] } }, dependencies: { a: "1", "dsh-grokbot-dispatch": "link:/old" } };
  const opts = { plugin: "agent-fleet-dsh", linkPath: "/r/live/plugin", replaces: ["dsh-grokbot-dispatch", "hermes-dsh-dispatch"] };
  const next = profileWithPlugin(pkg, opts);
  assert.deepEqual(next.dsh.profile.bundles, ["a", "agent-fleet-dsh", "b"]);
  assert.deepEqual(next.dependencies, { a: "1", "agent-fleet-dsh": "link:/r/live/plugin" });
  assert.equal(pkg.dependencies["dsh-grokbot-dispatch"], "link:/old");
  assert.equal(profileWithPlugin(next, opts), null);
});

test("dsh owns no plist", () => {
  assert.deepEqual(ownedPlists("dsh", [{ label: "x", programArg: "/a/cli.js" }]), []);
});
