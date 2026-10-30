// Durable ledger of task ids handled by this node (#132).
// Avoids duplicate execution or duplicate reply on reconnect or process restart.
// States: started -> answered -> done.
import { mkdirSync, readFileSync, renameSync, writeFileSync } from "node:fs";
import { dirname } from "node:path";

const MAX_ENTRIES = 500;

export function createLedger(path = null) {
  let entries = {};
  if (path) {
    try {
      const parsed = JSON.parse(readFileSync(path, "utf8"));
      if (parsed && typeof parsed.tasks === "object" && parsed.tasks !== null) {
        entries = parsed.tasks;
      }
    } catch {
      entries = {};
    }
  }

  const persist = () => {
    if (!path) return;
    const ids = Object.keys(entries);
    if (ids.length > MAX_ENTRIES) {
      ids.sort((a, b) => (entries[a].at ?? 0) - (entries[b].at ?? 0));
      for (const id of ids.slice(0, ids.length - MAX_ENTRIES)) {
        delete entries[id];
      }
    }
    mkdirSync(dirname(path), { recursive: true });
    const tmp = `${path}.${process.pid}.tmp`;
    writeFileSync(tmp, JSON.stringify({ tasks: entries }), { mode: 0o600 });
    renameSync(tmp, path);
  };

  return {
    get: (id) => entries[String(id)],
    set(id, value) {
      entries[String(id)] = { ...value, at: Date.now() };
      persist();
    },
    snapshot: () => JSON.parse(JSON.stringify(entries)),
  };
}
