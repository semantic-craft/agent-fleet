import test from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, writeFileSync, readFileSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { approvalRedirect, contextPlan, loadConfig, localMachine, render, writeJson0600 } from "../scripts/lib.mjs";

const dir = mkdtempSync(join(tmpdir(), "fleet-"));
const write = (obj) => {
  const p = join(dir, `c${Math.random()}.json`);
  writeFileSync(p, JSON.stringify(obj));
  return p;
};

test("example config loads and expands ~ against the given home", () => {
  const c = loadConfig(new URL("../fleet.example.json", import.meta.url), "/home/u");
  assert.equal(c.relay.adminSecretFile, "/home/u/.config/agenvo/admin-secret");
  assert.equal(c.machines[0].projectsRoot, "/home/u/Projects");
  assert.equal(c.machines[0].instances.find((i) => i.id === "dsh-laptop").keyFile, "/home/u/.config/dsh-grokbot-dispatch/key");
});

test("config rejects unknown connectors and duplicate ids", () => {
  const base = { relay: { baseUrl: "https://r" }, machines: [{ name: "m", instances: [] }] };
  base.machines[0].instances = [{ id: "a", connector: "telnet" }];
  assert.throws(() => loadConfig(write(base)), /connector/);
  base.machines[0].instances = [{ id: "a", connector: "codex" }, { id: "a", connector: "herdr" }];
  assert.throws(() => loadConfig(write(base)), /duplicate/);
});

test("local machine matches name or hostnames, case-insensitively", () => {
  const c = { machines: [{ name: "laptop", hostnames: ["Laptop-ABC.local"] }, { name: "mini" }] };
  assert.equal(localMachine(c, undefined, "laptop-abc.local").name, "laptop");
  assert.equal(localMachine(c, undefined, "Mini.local").name, "mini");
  assert.throws(() => localMachine(c, undefined, "other"), /--machine/);
});

test("context plan reports missing, changed and in-sync instances", () => {
  const m = { instances: [{ id: "a", connector: "codex", context: "x" }, { id: "b", connector: "codex", context: "y" }, { id: "c", connector: "herdr" }] };
  const live = { codex: { instances: [{ id: "a", context: "x" }, { id: "b", context: "old" }] }, herdr: { instances: [] } };
  const plan = contextPlan(m, (c) => live[c]);
  assert.deepEqual(plan.map((p) => [p.id, p.missing, p.changed]), [["a", false, false], ["b", false, true], ["c", true, false]]);
});

test("render refuses unset variables", () => {
  assert.equal(render("{{HOME}}/x", { HOME: "/h" }), "/h/x");
  assert.throws(() => render("{{NOPE}}", {}), /NOPE/);
});

test("writeJson0600 writes owner-only files", () => {
  const p = join(dir, "secret.json");
  writeJson0600(p, { a: 1 });
  assert.equal(statSync(p).mode & 0o777, 0o600);
  assert.deepEqual(JSON.parse(readFileSync(p, "utf8")), { a: 1 });
});

test("approval redirect handles both 303 and the production HTML page", () => {
  const cb = "http://127.0.0.1:8899/callback";
  assert.equal(approvalRedirect("http://x/cb?code=1", "", cb), "http://x/cb?code=1");
  const html = `<a href="${cb}?code=abc&amp;state=fleet">Return</a>`;
  assert.equal(approvalRedirect(null, html, cb), `${cb}?code=abc&state=fleet`);
  assert.throws(() => approvalRedirect(null, "<p>no</p>", cb), /neither/);
});
