#!/usr/bin/env node
// Mechanical boundaries, so rules do not depend on memory:
//   private  - no personal value from the overlay (or tailnet IPs / emails) appears in tracked files
//   landscape - every agent-related directory in ~/Projects has a row in docs/landscape.md, and vice versa
import { execFileSync } from "node:child_process";
import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { configPath } from "./lib.mjs";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
let failed = 0;
const fail = (msg) => (failed++, console.log("FAIL " + msg));

function personalValues(overlay) {
  const out = new Set();
  const walk = (v, key = "") => {
    if (typeof v === "string") {
      if (/context|\$comment|connector|kind|source/i.test(key)) return;
      for (const m of v.matchAll(/[A-Za-z0-9.-]+\.(?:dev|com|net|io|app|ai|local)\b|\b\d{1,3}(?:\.\d{1,3}){3}\b|[\w.+-]+@[\w-]+\.[\w.]+/g)) out.add(m[0]);
      if (/^(name|hostnames)$/.test(key) && v.length > 3) out.add(v);
    } else if (Array.isArray(v)) v.forEach((x) => walk(x, key));
    else if (v && typeof v === "object") for (const [k, x] of Object.entries(v)) walk(x, k);
  };
  walk(overlay);
  for (const generic of ["github.com", "git@github.com", "example.dev", "laptop.local", "agenvo.example.dev"]) out.delete(generic);
  return [...out].filter((x) => !/^127\.0\.0\.1$/.test(x));
}

function privateGuard() {
  const files = execFileSync("git", ["-C", root, "ls-files", "--cached", "--others", "--exclude-standard"], { encoding: "utf8" })
    .split("\n").filter((f) => f && f !== "package-lock.json");
  const generic = [/\b100\.(?:6[4-9]|[7-9]\d|1[01]\d|12[0-7])\.\d{1,3}\.\d{1,3}\b/, /\b192\.168\.\d{1,3}\.\d{1,3}\b/, /\/Users\/[a-z]/, /[\w.+-]+@(?:gmail|icloud|outlook|qq|163)\.com/i];
  const overlay = existsSync(configPath()) ? JSON.parse(readFileSync(configPath(), "utf8")) : null;
  const values = overlay ? personalValues(overlay) : [];
  for (const f of files) {
    const text = readFileSync(join(root, f), "utf8");
    for (const re of generic) if (re.test(text)) fail(`${f}: matches ${re}`);
    for (const v of values) if (text.toLowerCase().includes(v.toLowerCase())) fail(`${f}: contains an overlay value (${v.slice(0, 3)}…)`);
  }
  console.log(`private: ${files.length} files, ${values.length} overlay values${overlay ? "" : " (no overlay found; generic patterns only)"}`);
}

function landscapeGuard() {
  const projects = join(homedir(), "Projects");
  if (!existsSync(projects)) return console.log("landscape: no ~/Projects, skipped");
  const doc = readFileSync(join(root, "docs/landscape.md"), "utf8");
  const listed = new Map();
  for (const line of doc.split("\n")) {
    const m = /^\| ([^|]+) \|.*\| ([^|]+) \|$/.exec(line);
    if (!m) continue;
    const status = m[2].trim();
    for (const name of m[1].matchAll(/`([^`]+)`/g)) {
      const base = name[1].replace(/^agent-interop-private$/, "agent-interop-private");
      if (base.includes("/")) continue;
      listed.set(base, status);
    }
  }
  // `agent-interop-private` / `-hub` ... shorthand
  if (listed.has("agent-interop-private")) for (const s of ["-hub", "-hub-lease", "-prod"]) listed.set("agent-interop" + s, listed.get("agent-interop-private"));
  for (const k of [...listed.keys()]) if (k.startsWith("-")) listed.delete(k);
  const pattern = /agent|bridge|dispatch|relay|tailmsg|grok|hermes|^dot-|zcode|^dsh-|agenvo|fleet/i;
  const present = readdirSync(projects).filter((d) => !d.startsWith(".") && !d.startsWith("_") && pattern.test(d) && statSync(join(projects, d)).isDirectory());
  for (const d of present) if (!listed.has(d)) fail(`landscape: ~/Projects/${d} has no row in docs/landscape.md`);
  for (const [d, status] of listed) {
    const here = existsSync(join(projects, d));
    if (status.startsWith("已归档") && here) fail(`landscape: ${d} is marked archived but still in ~/Projects`);
    if (!status.startsWith("已归档") && !here) fail(`landscape: ${d} is listed as "${status}" but is not in ~/Projects`);
  }
  console.log(`landscape: ${present.length} agent-related directories checked`);
}

const which = process.argv[2];
if (!which || which === "private") privateGuard();
if (!which || which === "landscape") landscapeGuard();
console.log(failed ? `FAILED (${failed})` : "OK");
process.exit(failed ? 1 : 0);
