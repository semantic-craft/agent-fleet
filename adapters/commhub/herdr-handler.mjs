import { mkdir, readFile, writeFile } from "node:fs/promises";
import { existsSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { truncateReply } from "./client.mjs";

const execFileAsync = promisify(execFile);

const AGENT_COMMANDS = {
  agy: (dir) =>
    `agy -p "$(cat "${dir}/brief.md")" --output-format json --dangerously-skip-permissions --model gemini-4-argon-medium > "${dir}/result.json" 2> "${dir}/stderr.log"`,
  dsh: (dir) =>
    `dsh --profile headless --json - < "${dir}/brief.md" > "${dir}/events.jsonl" 2> "${dir}/stderr.log"`,
};

export function taskText(task) {
  return String(task?.content ?? task?.task?.text ?? task?.text ?? task ?? "");
}

export function selectedAgent(task, text = taskText(task)) {
  const candidates = [
    task?.meta?.agent,
    task?.meta?.header?.agent,
    text.match(/^\s*agent\s*:\s*(agy|dsh)\b/im)?.[1],
    text.match(/\bagent\s*=\s*(agy|dsh)\b/i)?.[1],
  ];
  const found = candidates.find((value) => ["agy", "dsh"].includes(String(value).toLowerCase()));
  return found ? String(found).toLowerCase() : "agy";
}

export function smokeMarker(task, text = taskText(task)) {
  const marker = text.match(/HERDR_BRIDGE_OK_[A-Za-z0-9._-]+/)?.[0];
  if (!marker) return null;
  const noWrites = /禁(?:止)?\s*改文件|不改文件|read-only|no\s+writes/i.test(text);
  const noModels = /禁(?:止)?\s*调模型|不调(?:用)?模型|no\s+model/i.test(text);
  return noWrites && noModels ? marker : null;
}

function safeTaskId(value) {
  const id = String(value || `bridge-${Date.now()}`).replace(/[^A-Za-z0-9._-]+/g, "-");
  return id.slice(0, 100).replace(/^[-.]+|[-.]+$/g, "") || `bridge-${Date.now()}`;
}

function jsonOutput(stdout) {
  try {
    return JSON.parse(stdout);
  } catch {
    return null;
  }
}

function unwrap(value) {
  return value?.result?.result ?? value?.result ?? value;
}

function paneId(value) {
  const tab = unwrap(value);
  return tab?.root_pane?.pane_id ?? tab?.pane_id ?? null;
}

async function readMaybe(path, limit = 5000) {
  try {
    return (await readFile(path, "utf8")).slice(0, limit);
  } catch {
    return "";
  }
}

function describeOutput(agent, dir) {
  if (agent === "dsh") return `${dir}/events.jsonl`;
  return `${dir}/result.json`;
}

export function createHerdrHandler({
  home = homedir(),
  statusRoot = join(homedir(), "Projects", "_status"),
  workspaceId = process.env.HERDR_WORKSPACE_ID || null,
  herdrBin = "herdr",
  execFileImpl = execFileAsync,
  pollMs = 500,
  startedTimeoutMs = 3 * 60_000,
  timeoutMs = 29 * 60_000,
  onStarted = null,
} = {}) {
  return async function handleHerdrTask(task) {
    const text = taskText(task);
    const marker = smokeMarker(task, text);
    if (marker) {
      return { reply: marker, status: "replied" };
    }

    const agent = selectedAgent(task, text);
    const id = safeTaskId(task?.task_id || task?.id);
    const dir = join(statusRoot, id);
    const brief = join(dir, "brief.md");
    const startedFile = join(dir, "started");
    const exitFile = join(dir, "exit");
    await mkdir(dir, { recursive: true });
    await writeFile(brief, `${text}\n`, { mode: 0o600 });

    const label = `commhub-${id}`.slice(0, 80);
    const tabArgs = ["tab", "create", "--cwd", dir, "--label", label, "--no-focus"];
    if (workspaceId) tabArgs.push("--workspace", workspaceId);
    const created = jsonOutput((await execFileImpl(herdrBin, tabArgs)).stdout);
    const pane = paneId(created);
    if (!pane) {
      throw new Error(`herdr tab create did not return a pane: ${JSON.stringify(created)?.slice(0, 500)}`);
    }

    const command = [
      `D=${JSON.stringify(dir)}`,
      "mkdir -p \"$D\"",
      "touch \"$D/started\"",
      AGENT_COMMANDS[agent]("$D"),
      "echo $? > \"$D/exit\"",
      "exit",
    ].join("; ");
    await execFileImpl(herdrBin, ["pane", "run", pane, command]);

    const started = Date.now();
    while (Date.now() - started < startedTimeoutMs) {
      if (existsSync(startedFile)) {
        if (onStarted) await onStarted(id);
        break;
      }
      await new Promise((resolve) => setTimeout(resolve, pollMs));
    }

    if (!existsSync(startedFile)) {
      return {
        reply: truncateReply(`task ${id}: 命令未起跑（not started within ${startedTimeoutMs} ms）`),
        status: "failed",
      };
    }

    const startedAt = new Date();
    if (onStarted) await onStarted(id, task);

    let exitCode = null;
    while (Date.now() - started < timeoutMs) {
      if (existsSync(exitFile)) {
        exitCode = Number.parseInt((await readFile(exitFile, "utf8")).trim(), 10);
        break;
      }
      await new Promise((resolve) => setTimeout(resolve, pollMs));
    }

    if (exitCode === null || Number.isNaN(exitCode)) {
      const output = await readMaybe(describeOutput(agent, dir), 1200);
      return {
        reply: truncateReply(`task ${id} timed out after ${timeoutMs} ms\n${output}`),
        status: "failed",
      };
    }

    await writeFile(
      join(dir, "metadata.json"),
      JSON.stringify({ task_id: id, agent, pane, started_at: startedAt.toISOString(), exit: exitCode, completed_at: new Date().toISOString() }, null, 2)
    );
    const output = await readMaybe(describeOutput(agent, dir), 6000);
    const stderr = await readMaybe(join(dir, "stderr.log"), 1200);
    const reply = [
      `task ${id}: ${exitCode === 0 ? "ok" : `exit ${exitCode}`}`,
      `agent=${agent}`,
      `artifacts=${dir}`,
      output,
      stderr ? `stderr:\n${stderr}` : "",
    ].filter(Boolean).join("\n");

    return {
      reply: truncateReply(reply),
      status: exitCode === 0 ? "replied" : "failed",
    };
  };
}
