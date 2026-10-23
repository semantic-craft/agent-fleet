import test from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, writeFileSync, readFileSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { approvalRedirect, checkProject, contextPlan, forkRef, herdrValue, loadConfig, localMachine, paneGone, render, unlistedInstances, writeJson0600 } from "../scripts/lib.mjs";

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
  assert.equal(c.machines[0].instances.find((i) => i.id === "dsh-laptop").connector, "dsh");
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

test("check --dispatch project: --project wins, else overlay check.project, else a clear error", () => {
  const c = loadConfig(new URL("../fleet.example.json", import.meta.url), "/home/u");
  assert.equal(checkProject(c), c.check.project);
  assert.equal(checkProject(c, "other"), "other");
  assert.throws(() => checkProject({ machines: [] }), /check\.project/);
});

test("relay instances missing from the fleet config are reported, configured ones are not", () => {
  const c = { machines: [{ name: "m", instances: [{ id: "codex-m", connector: "codex" }] }, { name: "n" }] };
  const items = [{ instanceId: "codex-m", online: true }, { instanceId: "zcode-m", online: true, kind: "zcode" }];
  assert.deepEqual(unlistedInstances(items, c).map((i) => i.instanceId), ["zcode-m"]);
  assert.deepEqual(unlistedInstances([], c), []);
});

test("herdrValue unwraps the CLI envelope, passes connector-built results through, and throws native rejections", () => {
  assert.deepEqual(herdrValue({ execution: "accepted", result: { id: "cli:tab:create", result: { root_pane: { pane_id: "w1:p2" } }, session: "default" } }), { root_pane: { pane_id: "w1:p2" } });
  assert.deepEqual(herdrValue({ execution: "accepted", result: { items: [{ session: "default" }] } }), { items: [{ session: "default" }] });
  assert.deepEqual(herdrValue({ execution: "accepted", result: { output: "text\n", session: "default" } }), { output: "text\n", session: "default" });
  assert.throws(() => herdrValue({ execution: "rejected", error: { code: "native_error" } }), /native_error/);
});

test("paneGone is true only for a native pane_not_found rejection", () => {
  assert.equal(paneGone({ execution: "rejected", error: { code: "native_error", message: "Herdr rejected the request", native: { code: "pane_not_found", message: "pane w1:p2 not found" } } }), true);
  assert.equal(paneGone({ execution: "rejected", error: { code: "native_error", native: { code: "workspace_not_found" } } }), false);
  assert.equal(paneGone({ execution: "accepted", result: { id: "cli:pane:get", result: { pane: { pane_id: "w1:p2" } } } }), false);
  assert.equal(paneGone({ execution: "unknown", error: { code: "execution_unknown" } }), false);
});

test("forkRef finds the configured fork remote whatever it is called", () => {
  const remotes = [
    "origin\thttps://github.com/Xuanwo/agenvo.git (fetch)",
    "origin\thttps://github.com/Xuanwo/agenvo.git (push)",
    "xw\tgit@github.com:OWNER/agenvo.git (fetch)",
    "xw\tgit@github.com:OWNER/agenvo.git (push)",
  ].join("\n");
  assert.equal(forkRef(remotes, "git@github.com:OWNER/agenvo.git", "xw/dispatch-connectors"), "xw/xw/dispatch-connectors");
  assert.equal(forkRef("origin\tgit@github.com:OWNER/agenvo.git (fetch)", "git@github.com:OWNER/agenvo.git", "b"), "origin/b");
  assert.throws(() => forkRef(remotes, "git@github.com:OTHER/agenvo.git", "b"), /agenvo\.remote/);
});
