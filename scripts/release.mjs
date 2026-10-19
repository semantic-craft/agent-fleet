// `fleet.mjs release agenvo|zcode [--sha SHA] [--rollback]`: build a pinned, pushed commit into
// ~/.local/share/agent-fleet/releases/<component>/<sha>/, point the component's launchd plists at it,
// restart them and verify. Live services never run from a git checkout, so switching branches or
// rebuilding there cannot break them. See docs/runbook.md "发布与回滚".
import { execFileSync } from "node:child_process";
import { cpSync, existsSync, mkdirSync, readdirSync, readFileSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { homedir, userInfo } from "node:os";
import { join } from "node:path";
import { CONNECTOR_DIRS, expandHome, writeJson0600 } from "./lib.mjs";
import { MANIFEST, component, fullSha, manifestDiff, ownedPlists, releaseOf, treeHashes } from "./release-lib.mjs";

const run = (cmd, args, opts = {}) => execFileSync(cmd, args, { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"], ...opts }).trim();
const ok = (cmd, args) => {
  try {
    run(cmd, args);
    return true;
  } catch {
    return false;
  }
};
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const readJson = (p) => {
  try {
    return JSON.parse(readFileSync(p, "utf8"));
  } catch {
    return undefined;
  }
};

/** Where the source lives and which remote-tracking ref a deployable commit must be on. */
function source(name, cfg, home) {
  if (name === "agenvo") {
    const dir = expandHome(cfg.agenvo?.checkout ?? "~/Projects/agenvo", home);
    const remotes = run("git", ["-C", dir, "remote", "-v"]).split("\n").map((l) => l.split(/\s+/));
    const remote = remotes.find(([, url]) => url === cfg.agenvo?.remote)?.[0];
    if (!remote) throw new Error(`no remote in ${dir} with url ${cfg.agenvo?.remote} (fleet.json agenvo.remote)`);
    return { dir, remote, ref: `refs/remotes/${remote}/${cfg.agenvo.branch}` };
  }
  const dir = expandHome(cfg.zcode?.checkout ?? "~/Projects/zcode-dispatch", home);
  return { dir, remote: "origin", ref: "refs/remotes/origin/main" };
}

function livePlists(agents) {
  return readdirSync(agents)
    .filter((f) => f.endsWith(".plist"))
    .map((f) => {
      const file = join(agents, f);
      const d = JSON.parse(run("plutil", ["-convert", "json", "-o", "-", file]));
      return { file, label: d.Label, args: d.ProgramArguments ?? [], programArg: d.ProgramArguments?.[1], env: d.EnvironmentVariables ?? {} };
    });
}

function restart(uid, { label, file }) {
  ok("launchctl", ["bootout", `gui/${uid}/${label}`]);
  // bootout returns before the job is fully gone; bootstrap fails with EIO until it is.
  for (let i = 0; i < 20; i++) {
    if (ok("launchctl", ["bootstrap", `gui/${uid}`, file])) return;
    execFileSync("sleep", ["0.5"]);
  }
  throw new Error(`launchctl bootstrap ${file} failed; check it by hand`);
}

function zcodeEndpoint(home) {
  const c = readJson(join(home, ".config/agenvo/dispatch/config.json"));
  const e = c?.instances?.find((i) => i.source === "agenvo")?.endpoint;
  if (!e) throw new Error("no agenvo-dispatch instance with source agenvo; cannot reach the ZCode receiver /health");
  return e.replace(/\/$/, "");
}

async function zcodeHealth(home, timeoutMs = 60_000) {
  const url = zcodeEndpoint(home) + "/health";
  const deadline = Date.now() + timeoutMs;
  let last;
  while (Date.now() < deadline) {
    try {
      const r = await fetch(url, { signal: AbortSignal.timeout(5000) });
      if (r.ok) return await r.json();
      last = new Error("HTTP " + r.status);
    } catch (e) {
      last = e;
    }
    await sleep(1000);
  }
  throw new Error(`ZCode receiver ${url} unhealthy: ${last?.message}`);
}

/** Every connector this release restarted reports `online` from a new process. */
async function connectorsOnline(home, plists, before, timeoutMs = 90_000) {
  const dirs = plists.map((p) => /\/apps\/([^/]+)\/cli\.js$/.exec(p.target)[1]);
  const deadline = Date.now() + timeoutMs;
  const state = () => dirs.map((d) => [d, readJson(join(home, ".config/agenvo", d, "status.json"))]);
  while (Date.now() < deadline) {
    const s = state();
    if (s.every(([d, st]) => st?.state === "online" && st.pid !== before[d])) return s.map(([d, st]) => `${d}: online pid ${st.pid}`);
    await sleep(2000);
  }
  throw new Error("connectors not online after restart: " + state().map(([d, st]) => `${d}=${st?.state ?? "?"}${st?.lastError ? " " + st.lastError : ""}`).join(", "));
}

function build(name, src, sha, dest) {
  const tree = name === "zcode" ? join(dest, "tree") : join(dest, ".src");
  mkdirSync(tree, { recursive: true, mode: 0o700 });
  execFileSync("/bin/sh", ["-c", 'git -C "$1" archive "$2" | tar -x -C "$3"', "sh", src.dir, sha, tree]);
  if (name === "zcode") return;
  const inTree = (args) => run("npm", args, { cwd: tree, stdio: ["ignore", "pipe", "inherit"] });
  inTree(["ci", "--no-audit", "--no-fund"]);
  for (const app of Object.values(CONNECTOR_DIRS)) inTree(["run", "build", "--workspace", `@agenvo/${app}`]);
  inTree(["prune", "--omit=dev", "--no-audit", "--no-fund"]);
  for (const app of Object.values(CONNECTOR_DIRS)) {
    mkdirSync(join(dest, "apps", app), { recursive: true });
    cpSync(join(tree, "apps", app, "dist/cli.js"), join(dest, "apps", app, "cli.js"));
  }
  // Only third-party runtime deps; workspace links and .bin shims are symlinks the bundles never use.
  cpSync(join(tree, "node_modules"), join(dest, "node_modules"), { recursive: true, verbatimSymlinks: true });
  run("find", [join(dest, "node_modules"), "-type", "l", "-delete"]);
  writeFileSync(join(dest, "package.json"), JSON.stringify({ private: true, type: "module" }) + "\n");
  rmSync(tree, { recursive: true, force: true });
}

export async function release(name, { cfg, sha, rollback = false, home = homedir(), log = console.log }) {
  const c = component(name);
  const uid = userInfo().uid;
  const agents = join(home, "Library/LaunchAgents");
  const root = join(home, ".local/share/agent-fleet/releases", name);
  const currentFile = join(root, "current.json");
  mkdirSync(root, { recursive: true, mode: 0o700 });

  if (rollback) {
    const cur = readJson(currentFile);
    if (!cur?.backup || !existsSync(cur.backup)) throw new Error(`no recorded switch to roll back in ${currentFile}`);
    if (name === "zcode" && (await zcodeHealth(home)).running) throw new Error("ZCode receiver has running jobs; wait for them to finish");
    for (const f of readdirSync(cur.backup)) {
      const file = join(agents, f);
      cpSync(join(cur.backup, f), file);
      restart(uid, { label: f.replace(/\.plist$/, ""), file });
      log(`restored ${f}`);
    }
    writeJson0600(currentFile, { sha: null, rolledBackFrom: cur.sha, at: new Date().toISOString() });
    return;
  }

  const src = source(name, cfg, home);
  run("git", ["-C", src.dir, "fetch", "-q", src.remote]);
  sha ??= run("git", ["-C", src.dir, "rev-parse", src.ref]);
  if (!fullSha(sha)) throw new Error("--sha must be a full 40-hex commit");
  if (!ok("git", ["-C", src.dir, "merge-base", "--is-ancestor", sha, src.ref])) throw new Error(`${sha.slice(0, 12)} is not on ${src.ref}; push or merge it first`);
  for (const [path, needle] of c.probes ?? []) {
    let text = "";
    try {
      text = run("git", ["-C", src.dir, "show", `${sha}:${path}`]);
    } catch {}
    if (!text.includes(needle)) throw new Error(`${sha.slice(0, 12)} lacks ${needle} in ${path}; refusing a release that drops it`);
  }

  const plists = ownedPlists(name, livePlists(agents)).map((p) => ({ ...p, target: join(root, sha, c.entry(p.programArg)) }));
  if (!plists.length) throw new Error(`no launchd plist in ${agents} belongs to ${name}`);
  for (const p of plists) {
    const rel = releaseOf(p.programArg, root);
    if (!rel) continue;
    const m = readJson(join(rel, MANIFEST));
    const diff = m ? manifestDiff(m.files, treeHashes(rel, [MANIFEST])) : ["(no manifest)"];
    if (diff.length) throw new Error(`live release ${rel} was edited by hand (${diff.slice(0, 3).join(", ")}); reconcile it first`);
  }
  if (name === "zcode" && (await zcodeHealth(home)).running) throw new Error("ZCode receiver has running jobs; wait for them to finish");

  const dest = join(root, sha);
  if (existsSync(dest)) {
    const m = readJson(join(dest, MANIFEST));
    if (!m || manifestDiff(m.files, treeHashes(dest, [MANIFEST])).length) throw new Error(`${dest} is incomplete or edited; remove it and run again`);
    log(`reusing ${dest}`);
  } else {
    const staging = join(root, `.staging-${sha}-${process.pid}`);
    rmSync(staging, { recursive: true, force: true });
    try {
      build(name, src, sha, staging);
      writeJson0600(join(staging, MANIFEST), { component: name, sha, ref: src.ref, builtAt: new Date().toISOString(), files: treeHashes(staging, [MANIFEST]) });
      renameSync(staging, dest);
    } catch (e) {
      rmSync(staging, { recursive: true, force: true });
      throw e;
    }
    log(`built ${dest}`);
  }

  // Outside the release itself, so its manifest stays exact.
  const backup = join(root, "backups", `${sha}-${Date.now()}`);
  mkdirSync(backup, { recursive: true, mode: 0o700 });
  const before = Object.fromEntries(Object.values(CONNECTOR_DIRS).map((d) => [d, readJson(join(home, ".config/agenvo", d, "status.json"))?.pid]));
  for (const p of plists) {
    cpSync(p.file, join(backup, `${p.label}.plist`));
    // Replace the whole array: `-replace ProgramArguments.1` inserts instead of overwriting.
    run("plutil", ["-replace", "ProgramArguments", "-json", JSON.stringify(p.args.map((a, i) => (i === 1 ? p.target : a))), p.file]);
    for (const k of c.dropEnv ?? []) if (k in p.env) run("plutil", ["-remove", `EnvironmentVariables.${k}`, p.file]);
  }
  writeJson0600(currentFile, { sha, backup, previous: plists.map(({ label, programArg }) => ({ label, programArg })), switchedAt: new Date().toISOString() });
  for (const p of plists) restart(uid, p);
  log(`switched ${plists.map((p) => p.label).join(", ")} to ${dest}`);

  try {
    if (name === "zcode") {
      const h = await zcodeHealth(home);
      if (!h.ok || h.strict_runner !== "app") throw new Error("ZCode receiver unhealthy after switch: " + JSON.stringify(h.checks ?? h));
      log(`zcode receiver ok: runner=${h.runner} strict_runner=${h.strict_runner} dot_route=${h.dot_route}`);
    } else {
      for (const line of await connectorsOnline(home, plists, before)) log(line);
    }
  } catch (e) {
    throw new Error(`${e.message}\nroll back with: node scripts/fleet.mjs release ${name} --rollback`);
  }
  log("next: node scripts/fleet.mjs check (end-to-end through the relay)");
}
