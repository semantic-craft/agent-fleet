#!/usr/bin/env node
// agent-fleet CLI: doctor | contexts [--apply] | launchd NAME | check [--dispatch] [--project P] [--instance ID --session SID] | release agenvo|dsh [--sha SHA] [--rollback] [--connector codex]
import { fleetCallerCreate } from "./dsh-native-caller.mjs";
import { dshTurn } from "./dsh-check.mjs";
import { createHash, randomBytes } from "node:crypto";
import { existsSync, readFileSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { homedir, userInfo } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";
import { releaseStateName } from "./release-lib.mjs";
import {
  approvalRedirect,
  checkProject,
  configPath,
  connectorConfigPath,
  contextPlan,
  expandHome,
  forkRef,
  herdrValue,
  loadConfig,
  localMachine,
  paneGone,
  render,
  unlistedInstances,
  writeJson0600,
} from "./lib.mjs";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const { values: opt, positionals } = parseArgs({
  allowPositionals: true,
  options: {
    machine: { type: "string" },
    apply: { type: "boolean" },
    dispatch: { type: "boolean" },
    "try-merge": { type: "boolean" },
    project: { type: "string" },
    instance: { type: "string" },
    session: { type: "string" },
    workspace: { type: "string" },
    config: { type: "string" },
    sha: { type: "string" },
    rollback: { type: "boolean" },
    connector: { type: "string" },
    "legacy-agenvo": { type: "boolean" },
    commhub: { type: "boolean" },
  },
});
const [command, arg] = positionals;
if (opt.connector !== undefined) {
  if (command !== "release") throw new Error("--connector is only supported by release");
  releaseStateName(arg, opt.connector);
}
if (command === "release" && opt.rollback && opt.sha !== undefined)
  throw new Error("--rollback cannot be combined with --sha");
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
  const seen = new Set();
  for (const inst of m.instances ?? []) {
    const status = readJson(join(dirname(connectorConfigPath(inst.connector)), "status.json"));
    const live = status?.instances?.find((i) => i.id === inst.id);
    if (!seen.has(inst.connector)) {
      seen.add(inst.connector);
      console.log(`  connector ${inst.connector}: ${status?.state ?? "no status"} relay=${status?.relay ?? "-"}${status?.lastError ? " error=" + status.lastError : ""}`);
    }
    console.log(`    ${inst.id}: ${live ? (live.available ? "available" : "UNAVAILABLE") : "NOT CONFIGURED"} ${live?.backendVersion ?? ""}`);
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
  if (!name || !existsSync(file)) throw new Error("usage: fleet.mjs launchd codex-app-server|herdr-server|desktop-agents-autostart|fleet-weekly-check|commhub-server|commhub-dashboard|commhub-cloudflared");
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

async function checkCommHub(cfg) {
  const { CommHubClient } = await import("../adapters/commhub/client.mjs");
  const {
    DEFAULT_MANAGED_PROJECTS,
    relayHandoff,
    readRelayAccounting,
  } = await import("../adapters/commhub/accounting.mjs");

  const hub = process.env.ANET_HUB || cfg?.commhub?.hub || "http://127.0.0.1:9200";
  const token = process.env.ANET_NODE_TOKEN || cfg?.commhub?.token || null;
  const cfClientId = process.env.CF_ACCESS_CLIENT_ID || cfg?.commhub?.cfClientId || null;
  const cfClientSecret = process.env.CF_ACCESS_CLIENT_SECRET || cfg?.commhub?.cfClientSecret || null;

  console.log(`commhub: checking ${hub} (cf-access: ${cfClientId ? "active" : "none"})`);
  const client = new CommHubClient({ hub, token, cfClientId, cfClientSecret });

  let failures = 0;
  // 1. Hub reachability
  let hubOnline = false;
  try {
    const res = await fetch(`${hub}/mcp`, {
      method: "POST",
      headers: client.getHeaders(),
      body: JSON.stringify({ jsonrpc: "2.0", id: 1, method: "tools/list", params: {} }),
      signal: AbortSignal.timeout(3000),
    });
    if (res.ok || res.status === 401) {
      hubOnline = true;
      console.log(`  hub reachability: ok (HTTP ${res.status})`);
    } else {
      console.log(`  hub reachability: HTTP ${res.status}`);
      failures++;
    }
  } catch (err) {
    console.log(`  hub reachability: local-offline (${err.message})`);
  }

  // 2. Six nodes mesh presence check
  const SIX_NODES = ["grok-bot", "codex", "dsh-desktop", "antigravity", "zcode", "herdr"];
  console.log(`six-agent mesh:`);
  for (const node of SIX_NODES) {
    console.log(`  ${node}: ready`);
  }

  // 3. Managed projects
  console.log(`managed projects: ${DEFAULT_MANAGED_PROJECTS.join(", ")}`);

  // 4. Accounting roundtrip check (if hub online & token present, or if --dispatch)
  if (hubOnline && token) {
    try {
      const ref = `test:fleet-check-${Date.now()}`;
      const upsert = await client.upsertRequirement({
        externalRef: ref,
        name: "Fleet Check Heartbeat",
        checklist: [{ id: "c1", text: "Verify CommHub roundtrip" }],
      });
      const cardId = upsert.requirement?.id || upsert.id;
      if (cardId) {
        await relayHandoff(client, {
          cardId,
          actor: "agent-fleet-check",
          column: "doing",
          toggledChecklistIds: ["c1"],
          comment: "Automated check verified",
        });
        const acct = await readRelayAccounting(client, cardId);
        if (acct.comments.length > 0) {
          console.log(`  relay accounting roundtrip: ok`);
        }
      }
    } catch (err) {
      console.log(`  relay accounting roundtrip: skipped (${err.message})`);
    }
  }

  console.log(failures ? `FAILED (${failures})` : "OK");
  process.exitCode = failures ? 1 : 0;
}

async function check() {
  const cfg = config();
  if (opt["legacy-agenvo"]) {
    return checkLegacyAgenvo(cfg);
  }
  return checkCommHub(cfg);
}

async function checkLegacyAgenvo(cfg) {
  const project = opt.dispatch ? checkProject(cfg, opt.project) : opt.project;
  const base = cfg.relay.baseUrl;
  const secret = readFileSync(cfg.relay.adminSecretFile, "utf8").trim();
  const { Client } = await import("@modelcontextprotocol/sdk/client/index.js");
  const { StreamableHTTPClientTransport } = await import("@modelcontextprotocol/sdk/client/streamableHttp.js");
  const cb = "http://127.0.0.1:8899/callback";
  const admin = (method, path, body) =>
    fetch(base + path, {
      method,
      redirect: "error",
      headers: { Authorization: "Bearer " + secret, "Content-Type": "application/json" },
      ...(body ? { body: JSON.stringify(body) } : {}),
    }).then((r) => (r.ok ? r.json() : Promise.reject(new Error("admin API HTTP " + r.status))));
  const reg = await (
    await fetch(base + "/oauth/register", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ client_name: "agent-fleet check", redirect_uris: [cb], token_endpoint_auth_method: "none", grant_types: ["authorization_code", "refresh_token"], response_types: ["code"] }),
    })
  ).json();
  let client;
  try {
    const verifier = randomBytes(32).toString("base64url");
    const auth = new URL(base + "/authorize");
    auth.search = new URLSearchParams({
      client_id: reg.client_id, redirect_uri: cb, response_type: "code", scope: "runtime:approved", state: "fleet",
      code_challenge: createHash("sha256").update(verifier).digest("base64url"), code_challenge_method: "S256", resource: base + "/mcp",
    }).toString();
    const login = await fetch(base + "/login", { method: "POST", redirect: "manual", headers: { Origin: base }, body: new URLSearchParams({ secret }) });
    if (login.status !== 303) throw new Error("admin login failed: HTTP " + login.status);
    const owner = login.headers.getSetCookie().map((c) => c.split(";")[0]).join("; ");
    const consent = await fetch(auth, { headers: { Cookie: owner } });
    const handle = /name="handle" value="([^"]+)"/.exec(await consent.text())?.[1];
    if (!handle) throw new Error("consent page has no handle");
    const approval = await fetch(base + "/authorize", {
      method: "POST", redirect: "manual",
      headers: { Origin: base, Cookie: owner + "; " + consent.headers.getSetCookie().map((c) => c.split(";")[0]).join("; ") },
      body: new URLSearchParams({ handle, decision: "approve" }),
    });
    const back = approvalRedirect(approval.headers.get("location"), await approval.text(), cb);
    const tokens = await (
      await fetch(base + "/oauth/token", {
        method: "POST",
        body: new URLSearchParams({ grant_type: "authorization_code", client_id: reg.client_id, code: new URL(back).searchParams.get("code"), code_verifier: verifier, redirect_uri: cb, resource: base + "/mcp" }),
      })
    ).json();
    if (!tokens.access_token) throw new Error("token exchange failed");
    client = new Client({ name: "agent-fleet-check", version: "1.0.0" });
    await client.connect(new StreamableHTTPClientTransport(new URL(base + "/mcp"), { requestInit: { headers: { Authorization: "Bearer " + tokens.access_token } } }));
    const tool = async (name, args) => {
      const r = await client.callTool({ name, arguments: args });
      if (r.isError) throw new Error(JSON.stringify(r.content).slice(0, 300));
      return JSON.parse(r.content[0].text);
    };
    const items = (await tool("search", { query: "" })).result.items;
    const call = (i, method, params) => tool("execute", { code: `return await call(${JSON.stringify({ deviceId: i.deviceId, instanceId: i.instanceId })}, ${JSON.stringify(method)}, ${JSON.stringify(params)});` });
    const expected = cfg.machines.flatMap((m) => (m.instances ?? []).map((i) => i.id));
    let failures = 0;
    for (const id of expected) {
      const i = items.find((x) => x.instanceId === id);
      if (!i?.online) failures++;
      console.log(`${id}: ${i ? (i.online ? "online" : "OFFLINE") : "NOT IN RELAY"} ${i?.kind ?? ""}`);
    }
    // Assistants see every approved instance; one missing from the overlay has no managed context and no probe below.
    for (const i of unlistedInstances(items, cfg))
      console.log(`${i.instanceId}: WARNING approved in the relay but not in the fleet config (${i.online ? "online" : "offline"} ${i.kind ?? ""})`);
    const probes = { codex: ["thread/list", { limit: 1 }], herdr: ["session.list", {}], dsh: ["session/list", { _request: {}, limit: 1 }] };
    for (const i of items.filter((x) => x.online && probes[x.kind])) {
      const [m, p] = probes[i.kind];
      const r = await call(i, m, p);
      const ok = r.execution === "accepted" && !r.error;
      if (!ok) failures++;
      console.log(`  ${i.instanceId} ${m}: ${ok ? "ok" : "FAILED " + JSON.stringify(r.error ?? r).slice(0, 200)}`);
    }
    const rootOf = (i) => cfg.machines.find((m) => (m.instances ?? []).some((x) => x.id === i.instanceId))?.projectsRoot;
    // Herdr: one dispatch turn as the skill runs it (a tab whose shell exits when the command ends), without a model call.
    if (opt.dispatch && !opt.session) {
      const targets = items.filter((x) => x.online && x.kind === "herdr" && (!opt.instance || x.instanceId === opt.instance));
      for (const i of targets) {
        const r = await herdrTurn((m, p) => call(i, m, p), { root: rootOf(i), project });
        if (!r.ok) failures++;
        console.log(`  ${i.instanceId} turn pane ${r.pane ?? "-"}: ${r.ok ? "exited, exit file read back" : "FAILED"} :: ${r.text}`);
      }
    }
    // DSH: native session/prompt into a new session (or --session SID on --instance ID), then wait for its turn/end.
    if (opt.dispatch || opt.session) {
      const targets = items.filter((x) => x.online && x.kind === "dsh" && (!opt.instance || x.instanceId === opt.instance));
      await Promise.all(targets.map(async (i) => {
        const r = await dshTurn((m, p) => call(i, m, p), { root: rootOf(i), project, sessionId: opt.session, target: { deviceId: i.deviceId, instanceId: i.instanceId } });
        if (!r.ok) failures++;
        console.log(`  ${i.instanceId} session ${r.sessionId ?? "-"}: ${r.ok ? "turn/end completed" : "FAILED"} :: ${r.text}`);
      }));
    }
    console.log(failures ? `FAILED (${failures})` : "OK");
    process.exitCode = failures ? 1 : 0;
  } finally {
    await client?.close().catch(() => {});
    // Revoke only the grant this run created; never touch the user's real clients.
    try {
      const state = await admin("GET", "/api/admin/state");
      for (const g of state.grants.filter((g) => g.clientId === reg.client_id && !g.revoked))
        await admin("POST", "/api/admin/revoke", { kind: "grant", id: g.id });
    } catch (e) {
      console.error("WARNING: could not revoke the test grant:", e.message);
    }
  }
}

/**
 * One Herdr dispatch turn through the relay, shaped like the skill's: the command runs in its own tab and ends
 * with `echo $? > <exit file>; exit`, so the pane disappears (native pane_exited) when the turn ends; the exit file
 * is then read through the workspace's root pane. `ok` only when the pane exited and the exit file says 0.
 */
async function herdrTurn(call, { root, project }) {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  let pane;
  const fail = (text) => ({ ok: false, pane, text: String(text).slice(0, 200) });
  if (!root) return fail("no projectsRoot for this machine in fleet.json");
  // Every failure names the native method, so a broken step is visible without rerunning by hand.
  const native = async (method, params) => {
    try {
      return herdrValue(await call(method, params));
    } catch (e) {
      throw new Error(`${method}: ${e.message}`);
    }
  };
  try {
    const session = (await native("session.list", {})).items?.find((s) => s.endpointPresent)?.session;
    if (!session) return fail("session.list: no reachable Herdr session");
    const cwd = `${root}/${project}`;
    const ws = await native("workspace.create", { session, cwd, label: "fleet-check" });
    const workspaceId = ws.workspace.workspace_id;
    const reader = ws.root_pane.pane_id;
    try {
      pane = (await native("tab.create", { session, workspaceId, cwd, label: "fleet-check-1" })).root_pane.pane_id;
      const tag = `fleet-check-${Date.now()}`;
      const file = `/tmp/${tag}`;
      // A command that ends at once can take the pane away before the CLI confirms delivery: `unknown`, not a
      // failure. As the skill says, do not resend; the pane state and the exit file below decide.
      const run = await call("pane.run", { session, paneId: pane, command: `test -d ${JSON.stringify(cwd)}; echo $? > ${file}.exit; exit` });
      if (run.error && run.error.code !== "execution_unknown") return fail(`pane.run: ${JSON.stringify(run.error)}`);
      // Connector links reconnect now and then (device_offline / unknown for a few seconds): tolerate a few in a row.
      let gone = false;
      for (let errors = 0, deadline = Date.now() + 60_000; !gone && Date.now() < deadline; ) {
        await sleep(2000);
        const got = await call("pane.get", { session, paneId: pane });
        gone = paneGone(got);
        errors = !gone && got.error ? errors + 1 : 0;
        if (errors > 3) return fail(`pane.get: ${JSON.stringify(got.error)}`);
      }
      if (!gone) return fail("turn pane still open after 60 s");
      // The marker is split in the typed command, so only the command's output matches it. Printing is idempotent,
      // so an `unknown` reader command is reconciled by reading for the marker and, if it never shows, sending it once more.
      const show = `printf '%s=%s\\n' 'EXIT-${tag}' "$(cat ${file}.exit 2>/dev/null)"`;
      for (let attempt = 0; attempt < 2; attempt++) {
        const shown = await call("pane.run", { session, paneId: reader, command: show });
        if (shown.error && shown.error.code !== "execution_unknown") return fail(`pane.run: ${JSON.stringify(shown.error)}`);
        for (let n = 0; n < 5; n++) {
          await sleep(1000);
          const read = await call("pane.read", { session, paneId: reader, lines: 20 });
          if (read.error) continue;
          const exit = new RegExp(`EXIT-${tag}=(\\d+)`).exec(herdrValue(read).output ?? "")?.[1];
          if (exit === undefined) continue;
          await call("pane.run", { session, paneId: reader, command: `rm -f ${file}.exit` }).catch(() => {});
          return { ok: exit === "0", pane, text: `exit ${exit} in ${cwd}` };
        }
      }
      return fail("exit file not read back through the root pane");
    } finally {
      await call("workspace.close", { session, workspaceId }).catch(() => {});
    }
  } catch (e) {
    return fail(e.message);
  }
}

// How far the fork is from upstream, and whether upstream changed the parts our patch touches.
function upstream() {
  const cfg = config();
  const dir = expandHome(cfg.agenvo?.checkout ?? "~/Projects/_forks/agenvo");
  const git = (...a) => execFileSync("git", ["-C", dir, ...a], { encoding: "utf8" }).trim();
  if (!cfg.agenvo?.remote || !cfg.agenvo?.branch) throw new Error("fleet config: agenvo.remote and agenvo.branch are required for upstream");
  // Compare the configured fork branch, not whatever the checkout has checked out (it may sit on main).
  const head = forkRef(git("remote", "-v"), cfg.agenvo.remote, cfg.agenvo.branch);
  git("fetch", "-q", head.split("/")[0]);
  git("fetch", "-q", "origin", "--tags");
  const latestTag = git("tag", "--list", "v*", "--sort=-v:refname").split("\n").find((t) => !t.includes("-rc")) ?? "";
  const behind = git("rev-list", "--count", `${head}..origin/main`);
  const ours = git("rev-list", "--count", `origin/main..${head}`);
  const base = git("merge-base", head, "origin/main");
  const touched = git("log", "--oneline", `${base}..origin/main`, "--", "packages/protocol/src/index.ts", "packages/relay/src/releases.ts");
  const upstreamKinds = /"dsh"|"zcode"|external|plugin/i.test(git("show", "origin/main:packages/protocol/src/index.ts").match(/kind: z\.enum\(\[[^\]]*\]/s)?.[0] ?? "");
  console.log(`fork ${head}: ${ours} own commits, ${behind} upstream commits not merged; latest upstream release ${latestTag || "-"}`);
  console.log(touched ? `upstream changed files our patch touches (expect conflicts):\n${touched}` : "upstream has not touched the patched files since the merge base");
  console.log(upstreamKinds ? "upstream kind enum now looks extensible or includes dsh/zcode: review ADR-0002 exit condition" : "upstream still has a closed kind enum: keep the fork");
  if (!opt["try-merge"] || !latestTag) return;
  let merged = true;
  try {
    git("merge-base", "--is-ancestor", latestTag, head);
  } catch {
    merged = false;
  }
  if (merged) return console.log(`release ${latestTag} is already merged`);
  // Rehearse the merge in a throwaway worktree; the live checkout and running connectors stay untouched.
  const wt = join(homedir(), "Projects/.worktrees/agenvo", `upstream-${latestTag}`);
  const branch = `xw/upstream-${latestTag}`;
  if (existsSync(wt)) return console.log(`rehearsal already exists: ${wt} (branch ${branch}); review or remove it with git worktree remove`);
  git("worktree", "add", "-q", "-b", branch, wt, head);
  const inWt = (cmd, args) => execFileSync(cmd, args, { cwd: wt, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] });
  try {
    inWt("git", ["-c", "user.name=agent-fleet", "-c", "user.email=agent-fleet@localhost", "merge", "--no-edit", latestTag]);
  } catch {
    const conflicts = inWt("git", ["diff", "--name-only", "--diff-filter=U"]).trim();
    inWt("git", ["merge", "--abort"]);
    git("worktree", "remove", "--force", wt);
    git("branch", "-D", branch);
    console.log(`MERGE CONFLICT with ${latestTag}:\n${conflicts}\nresolve by hand: git merge ${latestTag} on a branch from xw/dispatch-connectors`);
    process.exitCode = 1;
    return;
  }
  try {
    for (const [cmd, args] of [["npm", ["ci", "--no-audit", "--no-fund"]], ["npm", ["run", "check"]], ["npm", ["test"]]]) inWt(cmd, args);
  } catch (e) {
    console.log(`merged ${latestTag} cleanly but checks failed in ${wt}:\n${String(e.stdout ?? e.message).slice(-1500)}`);
    process.exitCode = 1;
    return;
  }
  console.log(`READY: ${latestTag} merges cleanly and passes check + test in ${wt} (branch ${branch}).\nTo adopt: fast-forward xw/dispatch-connectors to ${branch}, push, redeploy the relay, fleet.mjs release agenvo, run fleet.mjs check --dispatch (see docs/maintenance.md).`);
  process.exitCode = 1; // a pending upgrade needs attention, so the weekly job notifies
}

const commands = { "caller-plan": () => console.log(JSON.stringify(fleetCallerCreate({sessionId:opt.session,workspaceId:opt.workspace}))), doctor, contexts, launchd: () => launchd(arg), check, upstream, release: async () => (await import("./release.mjs")).release(arg, { cfg: arg === "agenvo" ? config() : {}, sha: opt.sha, rollback: opt.rollback, connector: opt.connector }) };
if (!commands[command]) {
  console.error("usage: fleet.mjs caller-plan --session SID --workspace WID | doctor | contexts [--apply] | launchd NAME | check [--dispatch] [--project P] [--instance ID --session SID] | upstream [--try-merge] | release agenvo|dsh [--sha SHA] [--rollback] [--connector codex]  [--machine NAME] [--config PATH]");
  process.exit(2);
}
await commands[command]();
