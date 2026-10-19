#!/usr/bin/env node
// agent-fleet CLI: doctor | contexts [--apply] | launchd NAME | check [--dispatch] [--project P] [--instance ID --session SID] | release agenvo|zcode [--sha SHA] [--rollback]
import { createHash, randomBytes } from "node:crypto";
import { existsSync, readFileSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { homedir, userInfo } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";
import {
  approvalRedirect,
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
    dispatch: { type: "boolean" },
    "try-merge": { type: "boolean" },
    project: { type: "string", default: "agenvo" },
    instance: { type: "string" },
    session: { type: "string" },
    config: { type: "string" },
    sha: { type: "string" },
    rollback: { type: "boolean" },
  },
});
const [command, arg] = positionals;
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
  if (!name || !existsSync(file)) throw new Error("usage: fleet.mjs launchd codex-app-server|herdr-server|desktop-agents-autostart|fleet-weekly-check|zcode-receiver");
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

async function check() {
  const cfg = config();
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
    const probes = { codex: ["thread/list", { limit: 1 }], herdr: ["session.list", {}], dsh: ["session/list", { _request: {}, limit: 1 }], zcode: ["health", {}] };
    for (const i of items.filter((x) => x.online && probes[x.kind])) {
      const [m, p] = probes[i.kind];
      const r = await call(i, m, p);
      const ok = r.execution === "accepted" && !r.error;
      if (!ok) failures++;
      console.log(`  ${i.instanceId} ${m}: ${ok ? "ok" : "FAILED " + JSON.stringify(r.error ?? r).slice(0, 200)}`);
    }
    if (opt.dispatch) {
      const jobs = [];
      for (const i of items.filter((x) => x.online && x.kind === "zcode")) {
        const r = await call(i, "jobs.create", { project: opt.project, text: "只读验收：用一句话说明这个目录是什么项目，不要修改任何文件。", timeout_seconds: 600 });
        if (r.result?.id) jobs.push([i, r.result.id]);
        else (failures++, console.log(`  ${i.instanceId} jobs.create FAILED ${JSON.stringify(r.error ?? r).slice(0, 200)}`));
      }
      const deadline = Date.now() + 10 * 60_000;
      while (jobs.length && Date.now() < deadline) {
        await new Promise((r) => setTimeout(r, 15_000));
        for (let k = jobs.length - 1; k >= 0; k--) {
          const [i, id] = jobs[k];
          const j = (await call(i, "jobs.get", { id })).result ?? {};
          if (["running", "queued"].includes(j.state)) continue;
          jobs.splice(k, 1);
          if (j.state !== "completed") failures++;
          console.log(`  ${i.instanceId} job ${id}: ${j.state} runner=${j.runner} source=${j.source} :: ${String(j.result ?? j.error ?? "").replace(/\s+/g, " ").slice(0, 120)}`);
        }
      }
      for (const [i, id] of jobs) (failures++, console.log(`  ${i.instanceId} job ${id}: still running after 10 min`));
    }
    // DSH: native session/prompt into a new session (or --session SID on --instance ID), then wait for its turn/end.
    if (opt.dispatch || opt.session) {
      const targets = items.filter((x) => x.online && x.kind === "dsh" && (!opt.instance || x.instanceId === opt.instance));
      await Promise.all(targets.map(async (i) => {
        const root = cfg.machines.find((m) => (m.instances ?? []).some((x) => x.id === i.instanceId))?.projectsRoot;
        const r = await dshTurn((m, p) => call(i, m, p), { root, project: opt.project, sessionId: opt.session });
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

const READ_ONLY = "只读验收：用一句话说明这个目录是什么项目，不要修改任何文件。";

/** One read-only DSH turn through native endpoints; `ok` only when the turn after our input ends completed. */
async function dshTurn(call, { root, project, sessionId }) {
  const fail = (text) => ({ ok: false, sessionId, text: String(text).slice(0, 200) });
  if (!sessionId) {
    if (!root) return fail("no projectsRoot for this machine in fleet.json");
    const ws = await call("workspace/create", { request: { path: `${root}/${project}` } });
    if (ws.error) return fail(JSON.stringify(ws.error));
    const created = await call("session/create", { request: { workspaceId: ws.result.workspace.workspaceId } });
    sessionId = created.nativeIds?.sessionId;
    if (!sessionId) return fail(JSON.stringify(created.error ?? created));
  }
  const address = { kind: "session", sessionId };
  // A single DSH message (a tool result, a long reply) can exceed the 64 KiB result cap, so never read
  // ranges: watch the last message for turn/end, then look up our input as the first message after `base`.
  const tooLarge = (r) => r.error?.code === "result_too_large";
  const tail = async () => {
    const r = await call("session/follow", { request: { address, maxMessages: 1 } });
    if (tooLarge(r)) return null;
    if (r.error) throw new Error(JSON.stringify(r.error));
    return r.result;
  };
  try {
    const first = await tail();
    if (!first) return fail("session/follow: last message exceeds 64 KiB before prompting");
    const base = first.cursor;
    const requestId = `fleet-check-${Date.now()}`;
    const prompt = await call("session/prompt", { request: { requestId, sessionId, mode: "queue", content: [{ type: "text", text: READ_ONLY }] } });
    if (prompt.error) return fail(JSON.stringify(prompt.error));
    let input;
    const deadline = Date.now() + 10 * 60_000;
    while (Date.now() < deadline) {
      await new Promise((r) => setTimeout(r, 10_000));
      const snap = await tail();
      if (!snap || snap.cursor <= base) continue;
      for (let before = base + 2; !input && before <= snap.cursor + 1; before++) {
        const r = await call("session/page", { request: { address, beforeSeq: before, throughSeq: snap.cursor, maxMessages: 1 } });
        if (tooLarge(r)) continue;
        if (r.error) throw new Error(JSON.stringify(r.error));
        input = (r.result.records ?? []).map((x) => x.event).find((e) => e.seq > base && e.type === "user/message" && e.data?.source?.rpcId === requestId);
      }
      const events = (snap.records ?? []).map((r) => r.event);
      const end = input && events.find((e) => e.type === "turn/end" && e.seq > input.seq);
      if (!end) continue;
      const text = events
        .filter((e) => e.type === "assistant/message")
        .flatMap((e) => e.data?.message?.content ?? [])
        .filter((c) => c.type === "text")
        .map((c) => c.text)
        .join(" ")
        .replace(/\s+/g, " ");
      return { ok: end.data?.reason?.kind === "completed", sessionId, text: `${end.data?.reason?.kind}: ${text.slice(0, 160)}` };
    }
  } catch (e) {
    return fail(e.message);
  }
  return fail("no turn/end within 10 min");
}

// How far the fork is from upstream, and whether upstream changed the parts our patch touches.
function upstream() {
  const cfg = config();
  const dir = cfg.agenvo?.checkout ? cfg.agenvo.checkout.replace(/^~(?=\/)/, homedir()) : join(homedir(), "Projects/agenvo");
  const git = (...a) => execFileSync("git", ["-C", dir, ...a], { encoding: "utf8" }).trim();
  git("fetch", "-q", "origin", "--tags");
  const head = git("rev-parse", "--abbrev-ref", "HEAD");
  const latestTag = git("tag", "--list", "v*", "--sort=-v:refname").split("\n").find((t) => !t.includes("-rc")) ?? "";
  const behind = git("rev-list", "--count", "HEAD..origin/main");
  const ours = git("rev-list", "--count", "origin/main..HEAD");
  const base = git("merge-base", "HEAD", "origin/main");
  const touched = git("log", "--oneline", `${base}..origin/main`, "--", "packages/protocol/src/index.ts", "packages/relay/src/releases.ts");
  const upstreamKinds = /"dsh"|"zcode"|external|plugin/i.test(git("show", "origin/main:packages/protocol/src/index.ts").match(/kind: z\.enum\(\[[^\]]*\]/s)?.[0] ?? "");
  console.log(`fork ${head}: ${ours} own commits, ${behind} upstream commits not merged; latest upstream release ${latestTag || "-"}`);
  console.log(touched ? `upstream changed files our patch touches (expect conflicts):\n${touched}` : "upstream has not touched the patched files since the merge base");
  console.log(upstreamKinds ? "upstream kind enum now looks extensible or includes dsh/zcode: review ADR-0002 exit condition" : "upstream still has a closed kind enum: keep the fork");
  if (!opt["try-merge"] || !latestTag) return;
  let merged = true;
  try {
    git("merge-base", "--is-ancestor", latestTag, "HEAD");
  } catch {
    merged = false;
  }
  if (merged) return console.log(`release ${latestTag} is already merged`);
  // Rehearse the merge in a throwaway worktree; the live checkout and running connectors stay untouched.
  const wt = join(homedir(), "Projects/.worktrees/agenvo", `upstream-${latestTag}`);
  const branch = `xw/upstream-${latestTag}`;
  if (existsSync(wt)) return console.log(`rehearsal already exists: ${wt} (branch ${branch}); review or remove it with git worktree remove`);
  git("worktree", "add", "-q", "-b", branch, wt, "HEAD");
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

const commands = { doctor, contexts, launchd: () => launchd(arg), check, upstream, release: async () => (await import("./release.mjs")).release(arg, { cfg: arg === "agenvo" ? config() : {}, sha: opt.sha, rollback: opt.rollback }) };
if (!commands[command]) {
  console.error("usage: fleet.mjs doctor | contexts [--apply] | launchd NAME | check [--dispatch] [--project P] [--instance ID --session SID] | upstream [--try-merge] | release agenvo|zcode [--sha SHA] [--rollback]  [--machine NAME] [--config PATH]");
  process.exit(2);
}
await commands[command]();
