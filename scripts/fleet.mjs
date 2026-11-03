#!/usr/bin/env node
// agent-fleet CLI: doctor | contexts [--apply] | launchd NAME | check
import { fleetCallerCreate } from "./dsh-native-caller.mjs";
import { existsSync, readFileSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { homedir, userInfo } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";
import {
  configPath,
  connectorConfigPath,
  contextPlan,
  loadConfig,
  localMachine,
  render,
  writeJson0600,
} from "./lib.mjs";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const { values: opt, positionals } = parseArgs({
  allowPositionals: true,
  options: {
    machine: { type: "string" },
    apply: { type: "boolean" },
    session: { type: "string" },
    workspace: { type: "string" },
    config: { type: "string" },
    sha: { type: "string" },
    rollback: { type: "boolean" },
    connector: { type: "string" },
  },
});
const [command, arg] = positionals;
if (command === "release") {
  console.error("fleet.mjs release 已退役。adapters/dsh 只服务已退出的 Agenvo dsh connector，不是现行派活。见 docs/legacy/dsh-adapter.md。");
  process.exit(2);
}
const readJson = (p) => {
  try {
    return JSON.parse(readFileSync(p, "utf8"));
  } catch {
    return undefined;
  }
};
const cfgFile = opt.config ?? configPath();
const config = () => loadConfig(cfgFile);

async function doctor() {
  const m = localMachine(config(), opt.machine);
  const uid = userInfo().uid;
  console.log(`machine ${m.name}`);
  for (const label of m.services ?? []) {
    let ok = true;
    try {
      execFileSync("launchctl", ["print", `gui/${uid}/${label}`], { stdio: "ignore" });
    } catch {
      ok = false;
    }
    console.log(`  service ${label}: ${ok ? "loaded" : "MISSING"}`);
  }
}

function contexts() {
  const m = localMachine(config(), opt.machine);
  const plan = contextPlan(m, (c) => readJson(connectorConfigPath(c)));
  const restart = new Set();
  for (const p of plan) {
    const state = p.missing ? "missing in connector (run instance add first)" : p.changed ? "differs" : "in sync";
    console.log(`${p.id} [${p.connector}]: ${state}`);
    if (opt.apply && p.changed) {
      const file = connectorConfigPath(p.connector);
      const live = readJson(file);
      live.instances.find((x) => x.id === p.id).context = m.instances.find((x) => x.id === p.id).context;
      writeJson0600(file, live);
      restart.add(p.connector);
    }
  }
  for (const c of restart) console.log(`updated ${c}; restart that connector to publish the context (no reapproval needed)`);
}

function launchd(name) {
  const file = join(root, "templates/launchd", `${name}.plist`);
  if (!name || !existsSync(file)) throw new Error("usage: fleet.mjs launchd codex-app-server|desktop-agents-autostart|fleet-weekly-check");
  const home = homedir();
  process.stdout.write(
    render(readFileSync(file, "utf8"), {
      HOME: home,
      LABEL_PREFIX: process.env.LABEL_PREFIX ?? "dev.fleet",
      BIN: process.env.FLEET_BIN ?? join(home, ".local/bin"),
      PROJECTS: localMachine(config(), opt.machine).projectsRoot ?? join(home, "Projects"),
    }),
  );
}

function check() {
  console.error("fleet.mjs check no longer probes a hub. Acceptance is anet delegate --watch and plugins/agent-network-cindy smoke (docs/anet.md).");
  process.exitCode = 2;
}

const commands = {
  "caller-plan": () => console.log(JSON.stringify(fleetCallerCreate({ sessionId: opt.session, workspaceId: opt.workspace }))),
  doctor,
  contexts,
  launchd: () => launchd(arg),
  check,
};
if (!commands[command]) {
  console.error("usage: fleet.mjs caller-plan --session SID --workspace WID | doctor | contexts [--apply] | launchd NAME | check [--machine NAME] [--config PATH]");
  process.exit(2);
}
await commands[command]();
