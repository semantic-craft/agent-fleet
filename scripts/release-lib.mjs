// Pure helpers for `fleet.mjs release`: which plists a component owns, where they point inside a
// release, and whether a release on disk still matches its manifest. No I/O beyond what callers pass in.
import { createHash } from "node:crypto";
import { lstatSync, readdirSync, readFileSync } from "node:fs";
import { join, relative } from "node:path";
import { CONNECTOR_DIRS } from "./lib.mjs";

export const MANIFEST = "release.json";

/**
 * Components a release can switch. `entry(programArg)` maps a live ProgramArguments[1] to the path
 * inside the release, or null when the plist is not this component's.
 */
export const COMPONENTS = {
  // The four agenvo connectors (upstream codex/herdr plus the fork's dsh/dispatch), one shared release.
  agenvo: {
    labelPrefix: "io.agenvo.connector.",
    entry(programArg) {
      const m = /\/apps\/([^/]+)\/(?:dist\/)?cli\.js$/.exec(programArg ?? "");
      return m && Object.values(CONNECTOR_DIRS).includes(m[1]) ? `apps/${m[1]}/cli.js` : null;
    },
  },
  // The ZCode receiver that agenvo-dispatch calls (adapters/zcode in this repository). The old
  // zcode-dispatch entry (receiver/server.mjs) is still recognised so the first switch can adopt it.
  zcode: {
    labelPrefix: "", // any label: the entry path identifies the receiver (templates/launchd/zcode-receiver.plist)
    self: true,
    paths: ["adapters/zcode"],
    entry: (p = "") => (/\/adapters\/zcode\/receiver\.mjs$/.test(p) || (/\/zcode[^/]*\//.test(p) && /\/receiver\/server\.mjs$/.test(p)) ? "tree/adapters/zcode/receiver.mjs" : null),
    // The archived tree must carry these, or the release would drop ADR-0004 behaviour.
    probes: [
      ["adapters/zcode/receiver.mjs", "strict_runner: 'app'"],
      ["adapters/zcode/core.mjs", 'source must be "agenvo"'],
    ],
    // Retired with the Dot gateway (ADR-0006); a release never carries it forward.
    dropEnv: ["ZCODE_DOT_ACCESS_CONFIG"],
  },
  // The DSH desktop plugin (adapters/dsh). No launchd job: the desktop profile links the plugin at
  // releases/dsh/live/plugin, and `live` points at the current release.
  dsh: {
    self: true,
    paths: ["adapters/dsh"],
    plugin: "agent-fleet-dsh",
    probes: [["adapters/dsh/native-bridge.js", "export function installNativeBridge"]],
  },
};

export function component(name) {
  const c = COMPONENTS[name];
  if (!c) throw new Error(`unknown component ${name} (${Object.keys(COMPONENTS).join("|")})`);
  return c;
}

/** Plists (as {label, programArg}) this component owns; anything else is left alone. */
export function releaseStateName(name, connector) {
  component(name);
  if (connector !== undefined && (name !== "agenvo" || connector !== "codex"))
    throw new Error("--connector supports only: release agenvo --connector codex");
  return connector ? "current-codex.json" : "current.json";
}

export function ownedPlists(name, plists, connector) {
  releaseStateName(name, connector);
  const c = component(name);
  if (!c.entry) return [];
  return plists.filter(({ label, programArg }) =>
    String(label ?? "").startsWith(c.labelPrefix) && c.entry(programArg) &&
    (!connector || c.entry(programArg) === `apps/${CONNECTOR_DIRS[connector]}/cli.js`));
}

/** Validate every rollback target before changing any plist. A scoped rollback
 * cannot consume a full-release record or overwrite a later release. */
export function rollbackPlan(name, connector, record, backups, live, root) {
  releaseStateName(name, connector);
  if ((record.connector ?? null) !== (connector ?? null) || !fullSha(record.sha))
    throw new Error("rollback record scope or SHA mismatch");
  if (!Array.isArray(record.previous) || !record.previous.length)
    throw new Error("rollback record has no previous targets");
  const labels = record.previous.map((p) => p.label).sort();
  const same = (a, b) => JSON.stringify(a.slice().sort()) === JSON.stringify(b.slice().sort());
  if (new Set(labels).size !== labels.length || labels.some((l) => !/^[\w.-]+$/.test(l)))
    throw new Error("invalid rollback target labels");
  if (!same(labels, ownedPlists(name, backups, connector).map((p) => p.label)) || backups.length !== labels.length)
    throw new Error("backup targets do not match rollback scope");
  const selected = ownedPlists(name, live, connector);
  if (!same(labels, selected.map((p) => p.label)))
    throw new Error("live targets do not match rollback scope");
  return record.previous.map((previous) => {
    const backup = backups.find((p) => p.label === previous.label);
    const current = selected.find((p) => p.label === previous.label);
    const entry = component(name).entry(previous.programArg);
    if (!entry || backup.programArg !== previous.programArg)
      throw new Error("backup entry does not match recorded previous target");
    if (current.programArg !== (record.rollbackPending ? previous.programArg : join(root, record.sha, entry)))
      throw new Error(`stale rollback: ${previous.label} no longer runs the recorded release`);
    return { previous, backup, current };
  });
}

export const fullSha = (s) => /^[0-9a-f]{40}$/.test(s ?? "");

/** sha256 of every regular file under dir (minus top-level `skip` names), keyed by relative path; symlinks are refused. */
export function treeHashes(dir, skip = []) {
  const out = {};
  const walk = (d) => {
    for (const name of readdirSync(d).sort()) {
      if (d === dir && skip.includes(name)) continue;
      const p = join(d, name);
      const st = lstatSync(p);
      if (st.isSymbolicLink()) throw new Error(`symlink in release: ${p}`);
      if (st.isDirectory()) walk(p);
      else if (st.isFile()) out[relative(dir, p)] = createHash("sha256").update(readFileSync(p)).digest("hex");
    }
  };
  walk(dir);
  return out;
}

/** Paths that differ between a manifest and the files now on disk (empty = untouched). */
export function manifestDiff(expected, actual) {
  const keys = new Set([...Object.keys(expected), ...Object.keys(actual)]);
  return [...keys].filter((k) => expected[k] !== actual[k]).sort();
}

/** The release directory a live ProgramArguments[1] belongs to, if it is inside `releasesRoot`. */
export function releaseOf(programArg, releasesRoot) {
  if (!programArg?.startsWith(releasesRoot + "/")) return null;
  const sha = programArg.slice(releasesRoot.length + 1).split("/")[0];
  return fullSha(sha) ? join(releasesRoot, sha) : null;
}

/**
 * The desktop profile's package.json with `plugin` linked at `linkPath` and listed in bundles,
 * replacing any of `replaces` (older plugins that served the same socket). Pure; returns null when
 * nothing changes.
 */
export function profileWithPlugin(pkg, { plugin, linkPath, replaces = [] }) {
  const deps0 = pkg.dependencies ?? {};
  const bundles0 = pkg.dsh?.profile?.bundles ?? [];
  const done = deps0[plugin] === `link:${linkPath}` && bundles0.filter((b) => b === plugin).length === 1 && !replaces.some((r) => r in deps0 || bundles0.includes(r));
  if (done) return null;
  const next = structuredClone(pkg);
  const deps = (next.dependencies ??= {});
  const bundles = (((next.dsh ??= {}).profile ??= {}).bundles ??= []);
  for (const old of replaces) delete deps[old];
  deps[plugin] = `link:${linkPath}`;
  const at = bundles.findIndex((b) => replaces.includes(b));
  const kept = bundles.filter((b) => !replaces.includes(b) && b !== plugin);
  kept.splice(at >= 0 ? at : kept.length, 0, plugin);
  next.dsh.profile.bundles = kept;
  return next;
}
