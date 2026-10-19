// Pure helpers for `fleet.mjs release`: which plists a component owns, where they point inside a
// release, and whether a release on disk still matches its manifest. No I/O beyond what callers pass in.
import { createHash } from "node:crypto";
import { lstatSync, readdirSync, readFileSync } from "node:fs";
import { join, relative } from "node:path";
import { CONNECTOR_DIRS } from "./lib.mjs";

export const MANIFEST = "release.json";
export const ZCODE_LABEL = "dev.semantic-craft.zcode-dispatch";

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
  // The ZCode receiver that agenvo-dispatch calls (source "agenvo", runner "app").
  zcode: {
    label: ZCODE_LABEL,
    entry: (programArg) => (/\/receiver\/server\.mjs$/.test(programArg ?? "") ? "tree/receiver/server.mjs" : null),
    // The archived tree must carry these, or the release would drop ADR-0004 behaviour.
    probes: [
      ["receiver/server.mjs", "strict_runner: 'app'"],
      ["receiver/core.mjs", "'agenvo'"],
    ],
    // Retired with the Dot gateway (ADR-0006); a release never carries it forward.
    dropEnv: ["ZCODE_DOT_ACCESS_CONFIG"],
  },
};

export function component(name) {
  const c = COMPONENTS[name];
  if (!c) throw new Error(`unknown component ${name} (${Object.keys(COMPONENTS).join("|")})`);
  return c;
}

/** Plists (as {label, programArg}) this component owns; anything else is left alone. */
export function ownedPlists(name, plists) {
  const c = component(name);
  return plists.filter(({ label, programArg }) =>
    (c.label ? label === c.label : String(label ?? "").startsWith(c.labelPrefix)) && c.entry(programArg));
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
