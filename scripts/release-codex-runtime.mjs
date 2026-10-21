import { request } from "node:http";
import { existsSync, readFileSync, lstatSync } from "node:fs";
import { dirname, isAbsolute, join } from "node:path";

const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
export function runtimeConfig(plists) {
  if (plists.length !== 1) throw new Error("codex-runtime requires exactly one dedicated service");
  const p = plists[0], index = p.args.indexOf("--config");
  if (p.label !== "io.agent-fleet.codex-runtime" || index < 0 || !isAbsolute(p.args[index + 1] ?? ""))
    throw new Error("dedicated codex-runtime label and absolute --config required");
  if (JSON.stringify(p.keepAlive) !== JSON.stringify({ SuccessfulExit: false }))
    throw new Error("codex-runtime requires KeepAlive SuccessfulExit=false for graceful release");
  const config = JSON.parse(readFileSync(p.args[index + 1], "utf8"));
  if (!isAbsolute(config.socketPath ?? "") || !isAbsolute(config.stateDir ?? ""))
    throw new Error("codex-runtime socketPath and stateDir must be absolute");
  return { ...config, lockPath: join(config.stateDir, "service.lock") };
}

export function runtimeRequest(config, method = "GET", path = "/health") {
  // Inspect only the dedicated private endpoint, never fall back to another server.
  const parent = lstatSync(dirname(config.socketPath));
  const socket = lstatSync(config.socketPath);
  if (!parent.isDirectory() || parent.isSymbolicLink() || parent.uid !== process.getuid() || (parent.mode & 0o077) || !socket.isSocket() || socket.uid !== parent.uid)
    throw new Error("codex-runtime requires its own private Unix socket");
  return new Promise((resolve, reject) => {
    const req = request({ socketPath: config.socketPath, method, path, timeout: 3000, agent: false }, res => {
      let raw = "";
      res.on("data", chunk => { raw += chunk; if (raw.length > 1024 * 1024) req.destroy(new Error("runtime health response too large")); });
      res.on("end", () => {
        try {
          if (res.statusCode !== 200) throw new Error(`codex-runtime ${path} refused (${res.statusCode})`);
          const data = JSON.parse(raw);
          resolve(data);
        } catch (error) { reject(error); }
      });
      res.on("error", reject);
    });
    req.on("timeout", () => req.destroy(new Error("codex-runtime control request timed out")));
    req.on("error", reject);
    req.end();
  });
}

function healthy(value) {
  return value?.ok === true && value.service === "codex-owned-runtime" && Number.isSafeInteger(value.pid) && value.pid > 0 && Array.isArray(value.units);
}
const alive = pid => {
  try { process.kill(pid, 0); return true; }
  catch (error) { if (error.code === "ESRCH") return false; throw error; }
};

export async function stopRuntime(config, registered, { timeoutMs = 10000, isAlive = alive } = {}) {
  if (!registered && !existsSync(config.socketPath) && !existsSync(config.lockPath)) return null;
  const before = await runtimeRequest(config);
  if (!healthy(before)) throw new Error("unexpected codex-runtime health identity");
  if (before.units.some(unit => unit.state !== "released")) throw new Error("codex-runtime has unreleased or unknown work; release refused");
  // The server must atomically fence inputs and recheck resources; GET is not the fence.
  const accepted = await runtimeRequest(config, "POST", "/shutdown");
  if (accepted?.ok !== true || accepted.pid !== before.pid) throw new Error("codex-runtime shutdown acknowledgement mismatch; inspect without killing");
  const deadline = Date.now() + timeoutMs;
  do {
    if (!isAlive(before.pid) && !existsSync(config.socketPath) && !existsSync(config.lockPath)) return before.pid;
    await delay(50);
  } while (Date.now() < deadline);
  throw new Error("codex-runtime did not finish graceful shutdown; no bootout or lock removal performed");
}

export async function waitRuntime(config, oldPid, timeoutMs = 10000) {
  const deadline = Date.now() + timeoutMs;
  let last;
  do {
    try {
      const health = await runtimeRequest(config);
      if (healthy(health) && health.pid !== oldPid) return health;
      last = "unexpected health identity or unchanged PID";
    } catch (error) { last = error.message; }
    await delay(100);
  } while (Date.now() < deadline);
  throw new Error("codex-runtime did not become healthy: " + last);
}
