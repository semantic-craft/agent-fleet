// `fleet.mjs release dsh [--sha SHA] [--rollback]`: build a pinned, pushed commit into
// ~/.local/share/agent-fleet/releases/dsh/<sha>/, point the desktop profile at it, and verify the
// native bridge. Live services never run from a git checkout. See docs/runbook.md "发布与回滚".
import { execFileSync } from "node:child_process";
import { cpSync, existsSync, lstatSync, mkdirSync, readdirSync, readFileSync, readlinkSync, renameSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { homedir } from "node:os";
import { join } from "node:path";
import { writeJson0600 } from "./lib.mjs";
import { MANIFEST, component, fullSha, manifestDiff, profileWithPlugin, releaseStateName, treeHashes } from "./release-lib.mjs";

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
function source(name) {
  if (component(name).self) return { dir: repoRoot, remote: "origin", ref: "refs/remotes/origin/main" };
  throw new Error(`no source for ${name}`);
}

export function build(name, src, sha, dest) {
  const c = component(name);
  if (name !== "dsh") throw new Error(`no build for ${name}`);
  const tree = join(dest, ".src");
  mkdirSync(tree, { recursive: true, mode: 0o700 });
  execFileSync("/bin/sh", ["-c", 'r=$1 s=$2 d=$3; shift 3; git -C "$r" archive "$s" -- "$@" | tar -x -C "$d"', "sh", src.dir, sha, tree, ...(c.paths ?? ["."])]);
  renameSync(join(tree, "adapters/dsh"), join(dest, "plugin"));
  rmSync(join(dest, "plugin/test"), { recursive: true, force: true });
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
  throw new Error(`release installed; the native bridge on ${dshSocket(home)} is not serving it yet: quit and reopen DeepSeek Harness\nroll back with: node scripts/fleet.mjs release dsh --rollback`);
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

export async function release(name, { sha, rollback = false, connector, home = homedir(), log = console.log } = {}) {
  const stateName = releaseStateName(name, connector);
  if (rollback && sha !== undefined) throw new Error("--rollback cannot be combined with --sha");
  if (name !== "dsh") throw new Error(`release ${name} is not supported`);
  const c = component(name);
  const root = join(home, ".local/share/agent-fleet/releases", name);
  const currentFile = join(root, stateName);
  mkdirSync(root, { recursive: true, mode: 0o700 });
  if (rollback) return rollbackDsh({ root, currentFile, home, log });

  const src = source(name);
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
  return releaseDsh({ c, src, sha, root, currentFile, home, log });
}
