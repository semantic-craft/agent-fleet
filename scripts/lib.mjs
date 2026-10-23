import { readFileSync, writeFileSync, renameSync, openSync, closeSync } from "node:fs";
import { homedir, hostname } from "node:os";
import { join } from "node:path";

export const CONNECTOR_DIRS = { codex: "codex-app-server", herdr: "herdr", dsh: "dsh", dispatch: "dispatch" };

export const expandHome = (p, home = homedir()) =>
  typeof p === "string" && (p === "~" || p.startsWith("~/")) ? join(home, p.slice(2)) : p;

export function configPath(env = process.env, home = homedir()) {
  return env.AGENT_FLEET_CONFIG || join(home, ".config/agent-fleet/fleet.json");
}

export function loadConfig(path, home = homedir()) {
  const raw = JSON.parse(readFileSync(path, "utf8"));
  if (!raw.relay?.baseUrl) throw new Error("fleet config: relay.baseUrl is required");
  if (!Array.isArray(raw.machines) || !raw.machines.length)
    throw new Error("fleet config: machines[] is required");
  const ids = new Set();
  for (const m of raw.machines) {
    if (!m.name) throw new Error("fleet config: every machine needs a name");
    for (const i of m.instances ?? []) {
      if (!i.id || !CONNECTOR_DIRS[i.connector])
        throw new Error(`fleet config: instance ${i.id ?? "?"} needs id and connector (${Object.keys(CONNECTOR_DIRS).join("|")})`);
      if (ids.has(i.id)) throw new Error(`fleet config: duplicate instance id ${i.id}`);
      ids.add(i.id);
    }
  }
  const fix = (v) => (typeof v === "string" ? expandHome(v, home) : v);
  raw.relay.adminSecretFile = fix(raw.relay.adminSecretFile ?? "~/.config/agenvo/admin-secret");
  raw.relay.baseUrl = raw.relay.baseUrl.replace(/\/$/, "");
  for (const m of raw.machines) {
    m.projectsRoot = fix(m.projectsRoot);
    for (const i of m.instances ?? []) if (i.keyFile) i.keyFile = fix(i.keyFile);
  }
  return raw;
}

export function localMachine(config, name, host = hostname()) {
  if (name) {
    const m = config.machines.find((x) => x.name === name);
    if (!m) throw new Error(`no machine named ${name} in fleet config`);
    return m;
  }
  const short = host.split(".")[0].toLowerCase();
  const m = config.machines.find(
    (x) =>
      x.name.toLowerCase() === short ||
      (x.hostnames ?? []).some((h) => h.toLowerCase() === host.toLowerCase() || h.split(".")[0].toLowerCase() === short),
  );
  if (!m) throw new Error(`this host (${host}) is not in fleet config; pass --machine NAME`);
  return m;
}

export const connectorConfigPath = (connector, home = homedir()) =>
  join(home, ".config/agenvo", CONNECTOR_DIRS[connector], "config.json");

/** Desired vs current context for each instance; `current` comes from connector config files. */
export function contextPlan(machine, readConnector) {
  const plan = [];
  for (const inst of machine.instances ?? []) {
    const cfg = readConnector(inst.connector);
    const live = cfg?.instances?.find((x) => x.id === inst.id);
    plan.push({
      id: inst.id,
      connector: inst.connector,
      missing: !live,
      changed: Boolean(live) && (live.context ?? "") !== (inst.context ?? ""),
    });
  }
  return plan;
}

export function writeJson0600(path, value) {
  const tmp = path + ".tmp";
  const fd = openSync(tmp, "w", 0o600);
  try {
    writeFileSync(fd, JSON.stringify(value, null, 2) + "\n");
  } finally {
    closeSync(fd);
  }
  renameSync(tmp, path);
}

/** Replace {{KEY}} placeholders; unknown keys are an error so a template never ships half-filled. */
export function render(template, vars) {
  return template.replace(/\{\{([A-Z_]+)\}\}/g, (_, k) => {
    if (!(k in vars)) throw new Error(`template variable ${k} is not set`);
    return String(vars[k]);
  });
}

/** OAuth approval may answer with a 303 Location or with an HTML page that links back to the client. */
export function approvalRedirect(location, html, callback) {
  if (location) return location;
  const esc = callback.replace(/[.*+?^${}()|[\]\\/]/g, "\\$&");
  const m = new RegExp(`(${esc}\\?[^"'<\\s]+)`).exec(html ?? "");
  if (!m) throw new Error("OAuth approval returned neither a redirect nor a client link");
  return m[1].replace(/&amp;/g, "&");
}

/** Relay items whose instance is not in the fleet config: approved and reachable, but no context here and no check probes them. */
export function unlistedInstances(items, config) {
  const known = new Set(config.machines.flatMap((m) => (m.instances ?? []).map((i) => i.id)));
  return items.filter((i) => !known.has(i.instanceId));
}

/** Native value of a Herdr call: the CLI's {id, result} envelope unwrapped; a rejection throws. */
export function herdrValue(outcome) {
  if (outcome.error) throw new Error(JSON.stringify(outcome.error).slice(0, 300));
  const r = outcome.result;
  return r && typeof r === "object" && "id" in r && "result" in r ? r.result : r;
}

/** A turn pane runs `...; exit`, so Herdr removes it when the turn ends: pane.get then rejects with pane_not_found. */
export const paneGone = (outcome) => outcome.error?.native?.code === "pane_not_found";

/** `<remote>/<branch>` for the fork branch, whichever local remote points at agenvo.remote (origin or not). */
export function forkRef(remotes, url, branch) {
  const remote = remotes.split("\n").map((l) => l.split(/\s+/)).find(([, u]) => u === url)?.[0];
  if (!remote) throw new Error(`no git remote in the agenvo checkout points at agenvo.remote (${url})`);
  return `${remote}/${branch}`;
}

/** Project directory for check --dispatch: --project wins, else overlay check.project. It must exist under projectsRoot on every dispatch machine. */
export function checkProject(config, flag) {
  const project = flag ?? config.check?.project;
  if (!project) throw new Error("check --dispatch needs a project that exists on every dispatch machine: set check.project in the fleet config or pass --project");
  return project;
}
