// `fleet.mjs release agenvo|dsh [--sha SHA] [--rollback]`: build a pinned, pushed commit into
// ~/.local/share/agent-fleet/releases/<component>/<sha>/, point the component's launchd plists at it,
// restart them and verify. Live services never run from a git checkout, so switching branches or
// rebuilding there cannot break them. See docs/runbook.md "发布与回滚".
import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { cpSync, existsSync, lstatSync, mkdirSync, readdirSync, readFileSync, readlinkSync, renameSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { homedir, userInfo } from "node:os";
import { join } from "node:path";
import { CONNECTOR_DIRS, expandHome, writeJson0600 } from "./lib.mjs";
import { MANIFEST, component, fullSha, manifestDiff, ownedPlists, profileWithPlugin, releaseOf, releaseStateName, rollbackPlan, treeHashes } from "./release-lib.mjs";

const fileHash = (file) => createHash("sha256").update(readFileSync(file)).digest("hex");

const repoRoot = join(fileURLToPath(new URL(".", import.meta.url)), "..");

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
  if (component(name).self) return { dir: repoRoot, remote: "origin", ref: "refs/remotes/origin/main" };
  if (name === "agenvo") {
    const dir = expandHome(cfg.agenvo?.checkout ?? "~/Projects/agenvo", home);
    const remotes = run("git", ["-C", dir, "remote", "-v"]).split("\n").map((l) => l.split(/\s+/));
    const remote = remotes.find(([, url]) => url === cfg.agenvo?.remote)?.[0];
    if (!remote) throw new Error(`no remote in ${dir} with url ${cfg.agenvo?.remote} (fleet.json agenvo.remote)`);
    return { dir, remote, ref: `refs/remotes/${remote}/${cfg.agenvo.branch}` };
  }
  throw new Error(`no source for ${name}`);
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

/** Every connector this release restarted reports `online` from a new process. */
async function connectorsOnline(home, plists, before, timeoutMs = 90_000) {
  const dirs = plists.map((p) => /\/apps\/([^/]+)\/(?:dist\/)?cli\.js$/.exec(p.target)[1]);
  const deadline = Date.now() + timeoutMs;
  const state = () => dirs.map((d) => [d, readJson(join(home, ".config/agenvo", d, "status.json"))]);
  while (Date.now() < deadline) {
    const s = state();
    if (s.every(([d, st]) => st?.state === "online" && st.pid !== before[d])) return s.map(([d, st]) => `${d}: online pid ${st.pid}`);
    await sleep(2000);
  }
  throw new Error("connectors not online after restart: " + state().map(([d, st]) => `${d}=${st?.state ?? "?"}${st?.lastError ? " " + st.lastError : ""}`).join(", "));
}

export function build(name, src, sha, dest) {
  const c = component(name);
  const tree = join(dest, ".src");
  mkdirSync(tree, { recursive: true, mode: 0o700 });
  execFileSync("/bin/sh", ["-c", 'r=$1 s=$2 d=$3; shift 3; git -C "$r" archive "$s" -- "$@" | tar -x -C "$d"', "sh", src.dir, sha, tree, ...(c.paths ?? ["."])]);
  if (name === "dsh") {
    renameSync(join(tree, "adapters/dsh"), join(dest, "plugin"));
    rmSync(join(dest, "plugin/test"), { recursive: true, force: true });
    rmSync(tree, { recursive: true, force: true });
    return;
  }
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

/** Build into root/<sha> unless an identical release is already there. */
function ensureRelease(name, src, sha, root, log) {
  const dest = join(root, sha);
  if (existsSync(dest)) {
    const m = readJson(join(dest, MANIFEST));
    if (!m || manifestDiff(m.files, treeHashes(dest, [MANIFEST])).length) throw new Error(`${dest} is incomplete or edited; remove it and run again`);
    log(`reusing ${dest}`);
    return dest;
  }
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
  return dest;
}

const DSH_PROFILE = (home) => join(home, ".dsh/profiles/desktop");
const DSH_REPLACES = ["dsh-grokbot-dispatch", "hermes-dsh-dispatch"];
export const dshSocket = (home = homedir()) => join(home, ".local/state/agent-fleet-dsh/native.sock");

/**
 * `pnpm install` in the desktop profile with the Host's own pnpm. No TTY: pnpm aborts when it wants to
 * recreate node_modules (CI=true accepts), and CI implies a frozen lockfile, which a changed link breaks.
 */
function pnpmInstall() {
  run(dshBin(), ["plugin", "--profile", "desktop", "install", "--prefer-offline", "--no-frozen-lockfile"], { cwd: "/tmp", stdio: ["ignore", "pipe", "inherit"], env: { ...process.env, CI: "true" } });
}

function dshBin() {
  for (const b of ["dsh", "/usr/local/bin/dsh", "/Applications/DeepSeek Harness.app/Contents/Resources/runtime/cli/bin/dsh"]) if (ok("/bin/sh", ["-c", 'command -v "$1"', "sh", b])) return b;
  throw new Error("dsh CLI not found (needed to install the profile's plugin links)");
}

/** GET /health on the bridge socket; resolves the parsed body or null. */
function socketHealth(file) {
  return new Promise((resolve) => {
    import("node:http").then(({ request }) => {
      const req = request({ socketPath: file, path: "/health", timeout: 3000 }, async (res) => {
        let raw = "";
        for await (const c of res) raw += c;
        try { resolve(JSON.parse(raw)); } catch { resolve(null); }
      });
      req.on("error", () => resolve(null));
      req.on("timeout", () => (req.destroy(), resolve(null)));
      req.end();
    });
  });
}

async function waitBridge(home, beforePid, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const h = await socketHealth(dshSocket(home));
    if (h?.ok && h.pid !== beforePid) return h;
    await sleep(2000);
  }
  return null;
}

/** Point releases/dsh/live at `dest` (atomic rename of a fresh symlink). */
function pointLive(root, dest) {
  const tmp = join(root, `.live-${process.pid}`);
  rmSync(tmp, { force: true });
  symlinkSync(dest, tmp);
  renameSync(tmp, join(root, "live"));
}

async function releaseDsh({ c, src, sha, root, currentFile, home, log }) {
  const dest = ensureRelease("dsh", src, sha, root, log);
  const profile = DSH_PROFILE(home);
  const pkgFile = join(profile, "package.json");
  const pkg = JSON.parse(readFileSync(pkgFile, "utf8"));
  const livePath = join(root, "live", "plugin");
  const next = profileWithPlugin(pkg, { plugin: c.plugin, linkPath: livePath, replaces: DSH_REPLACES });
  const previousLive = (() => { try { return lstatSync(join(root, "live")).isSymbolicLink() ? readlinkSync(join(root, "live")) : null; } catch { return null; } })();
  const before = (await socketHealth(dshSocket(home)))?.pid ?? null;
  const backup = join(root, "backups", `${sha}-${Date.now()}`);
  mkdirSync(backup, { recursive: true, mode: 0o700 });
  for (const f of ["package.json", "pnpm-lock.yaml"]) if (existsSync(join(profile, f))) cpSync(join(profile, f), join(backup, f));
  pointLive(root, dest);
  writeJson0600(currentFile, { sha, backup, previousLive, switchedAt: new Date().toISOString() });
  if (next) {
    writeFileSync(pkgFile, JSON.stringify(next, null, 2) + "\n");
    pnpmInstall();
    log(`desktop profile now links ${c.plugin} -> ${livePath}`);
  }
  log(`live -> ${dest}`);
  // A running desktop app does not load a newly linked plugin; it needs to be reopened.
  const h = await waitBridge(home, before, 15_000);
  if (h) return log(`native bridge up on ${dshSocket(home)}: pid ${h.pid} dsh ${h.dsh} peer ${JSON.stringify(h.peer)}`);
  throw new Error(`release installed; the native bridge on ${dshSocket(home)} is not serving it yet: quit and reopen DeepSeek Harness, then run fleet.mjs check\nroll back with: node scripts/fleet.mjs release dsh --rollback`);
}

function rollbackDsh({ root, currentFile, home, log }) {
  const cur = readJson(currentFile);
  if (!cur?.backup || !existsSync(cur.backup)) throw new Error(`no recorded switch to roll back in ${currentFile}`);
  const profile = DSH_PROFILE(home);
  for (const f of readdirSync(cur.backup)) cpSync(join(cur.backup, f), join(profile, f));
  pnpmInstall();
  if (cur.previousLive) pointLive(root, cur.previousLive);
  else rmSync(join(root, "live"), { force: true });
  writeJson0600(currentFile, { sha: null, rolledBackFrom: cur.sha, at: new Date().toISOString() });
  log("restored the desktop profile; quit and reopen DeepSeek Harness if the bridge does not reload");
}

export async function release(name, { cfg, sha, rollback = false, connector, home = homedir(), log = console.log, connectorTimeoutMs = 90_000 }) {
  const stateName = releaseStateName(name, connector);
  if (rollback && sha !== undefined) throw new Error("--rollback cannot be combined with --sha");
  const c = component(name);
  const uid = userInfo().uid;
  const agents = join(home, "Library/LaunchAgents");
  const root = join(home, ".local/share/agent-fleet/releases", name);
  const currentFile = join(root, stateName);
  mkdirSync(root, { recursive: true, mode: 0o700 });

  if (rollback && name === "dsh") return rollbackDsh({ root, currentFile, home, log });
  if (rollback) {
    const cur = readJson(currentFile);
    if (!cur?.backup || !existsSync(cur.backup)) throw new Error(`no recorded switch to roll back in ${currentFile}`);
    const plan = rollbackPlan(name, connector, cur, livePlists(cur.backup), livePlists(agents), root);
    for (const { previous, current } of plan) {
      if (!existsSync(previous.programArg)) throw new Error(`rollback entry missing: ${previous.programArg}`);
      const rel = releaseOf(previous.programArg, root);
      if (rel) {
        const manifest = readJson(join(rel, MANIFEST));
        if (!manifest || manifestDiff(manifest.files, treeHashes(rel, [MANIFEST])).length)
          throw new Error(`rollback release incomplete or edited: ${rel}`);
      }
      if (cur.plistHashes?.[current.label] && cur.plistHashes[current.label] !== fileHash(current.file))
        throw new Error(`stale rollback: ${current.label} plist changed after release`);
    }
    const before = Object.fromEntries(Object.values(CONNECTOR_DIRS).map((d) => [d, readJson(join(home, ".config/agenvo", d, "status.json"))?.pid]));
    for (const { backup, current } of plan) {
      cpSync(backup.file, current.file);
    }
    // Keep a scoped, freshness-checked retry record until restarted connectors are healthy.
    writeJson0600(currentFile, { ...cur, rollbackPending: true, plistHashes: Object.fromEntries(plan.map(({ current }) => [current.label, fileHash(current.file)])) });
    for (const { current } of plan) restart(uid, current);
    if (name === "agenvo") {
      for (const line of await connectorsOnline(home, plan.map(({ previous, current }) => ({ ...current, target: previous.programArg })), before, connectorTimeoutMs)) log(line);
    }
    for (const { current } of plan) log(`restored ${current.label}.plist`);
    writeJson0600(currentFile, { sha: null, connector: connector ?? null, rolledBackFrom: cur.sha, at: new Date().toISOString() });
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

  if (name === "dsh") return releaseDsh({ c, src, sha, root, currentFile, home, log });

  const plists = ownedPlists(name, livePlists(agents), connector).map((p) => ({ ...p, target: join(root, sha, c.entry(p.programArg)) }));
  if (!plists.length) throw new Error(`no launchd plist in ${agents} belongs to ${name}`);
  for (const p of plists) {
    const rel = releaseOf(p.programArg, root);
    if (!rel) continue;
    const m = readJson(join(rel, MANIFEST));
    const diff = m ? manifestDiff(m.files, treeHashes(rel, [MANIFEST])) : ["(no manifest)"];
    if (diff.length) throw new Error(`live release ${rel} was edited by hand (${diff.slice(0, 3).join(", ")}); reconcile it first`);
  }

  const dest = ensureRelease(name, src, sha, root, log);

  // Outside the release itself, so its manifest stays exact.
  const backup = join(root, "backups", ...(connector ? [connector] : []), `${sha}-${Date.now()}`);
  mkdirSync(backup, { recursive: true, mode: 0o700 });
  const before = Object.fromEntries(Object.values(CONNECTOR_DIRS).map((d) => [d, readJson(join(home, ".config/agenvo", d, "status.json"))?.pid]));
  for (const p of plists) {
    cpSync(p.file, join(backup, `${p.label}.plist`));
    // Replace the whole array: `-replace ProgramArguments.1` inserts instead of overwriting.
    run("plutil", ["-replace", "ProgramArguments", "-json", JSON.stringify(p.args.map((a, i) => (i === 1 ? p.target : a))), p.file]);
    for (const k of c.dropEnv ?? []) if (k in p.env) run("plutil", ["-remove", `EnvironmentVariables.${k}`, p.file]);
  }
  writeJson0600(currentFile, { sha, connector: connector ?? null, backup, previous: plists.map(({ label, programArg }) => ({ label, programArg })), plistHashes: Object.fromEntries(plists.map((p) => [p.label, fileHash(p.file)])), switchedAt: new Date().toISOString() });
  for (const p of plists) restart(uid, p);
  log(`switched ${plists.map((p) => p.label).join(", ")} to ${dest}`);

  try {
    for (const line of await connectorsOnline(home, plists, before)) log(line);
  } catch (e) {
    throw new Error(`${e.message}\nroll back with: node scripts/fleet.mjs release ${name}${connector ? ` --connector ${connector}` : ""} --rollback`);
  }
  log("next: node scripts/fleet.mjs check (end-to-end through the relay)");
}
