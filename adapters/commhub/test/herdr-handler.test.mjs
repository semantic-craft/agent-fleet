import test from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync } from "node:fs";
import { readFile, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createHerdrHandler, selectedAgent, smokeMarker } from "../herdr-handler.mjs";

function fixtureExecImpl(exitCode = 0) {
  const calls = [];
  return {
    calls,
    async execFileImpl(bin, args) {
      calls.push({ bin, args });
      if (args[0] === "tab") {
        return { stdout: JSON.stringify({ root_pane: { pane_id: "pane-1" } }) };
      }
      if (args[0] === "pane") {
        const dir = args[3].match(/D="([^"]+)"/)[1];
        await writeFile(join(dir, "started"), "");
        await writeFile(join(dir, "exit"), `${exitCode}\n`);
        return { stdout: "" };
      }
      throw new Error(`unexpected command: ${args.join(" ")}`);
    },
  };
}

test("smoke tasks short-circuit without starting a Herdr pane", async () => {
  const fixture = fixtureExecImpl();
  const handler = createHerdrHandler({ statusRoot: "/unused", execFileImpl: fixture.execFileImpl });
  const marker = "HERDR_BRIDGE_OK_smoke";
  const result = await handler({
    id: "task-1",
    content: `只回复 ${marker}，禁止改文件，禁调模型`,
  });
  assert.equal(result.reply, marker);
  assert.equal(result.status, "replied");
  assert.equal(fixture.calls.length, 0);
});

test("handler writes the brief, starts the selected agy pane, and reports artifacts", async () => {
  const root = mkdtempSync(join(tmpdir(), "herdr-handler-"));
  const fixture = fixtureExecImpl(0);
  let acked = null;
  let startedTask = null;
  let startedAt = null;
  const handler = createHerdrHandler({
    statusRoot: root,
    execFileImpl: fixture.execFileImpl,
    onStarted: async (id, task) => {
      acked = id;
      startedTask = task;
      startedAt = new Date();
    },
  });
  const result = await handler({ task_id: "task agy/1", content: "agent: agy\nDo the thing" });

  assert.equal(result.status, "replied");
  assert.match(result.reply, /task task-agy-1: ok/);
  assert.match(result.reply, /agent=agy/);
  const tabArgs = fixture.calls[0].args;
  assert.equal(tabArgs[0], "tab");
  assert.equal(tabArgs[1], "create");
  assert.equal(tabArgs[3], join(root, "task-agy-1"));
  const paneCommand = fixture.calls[1].args[3];
  assert.match(paneCommand, /agy -p/);
  assert.match(paneCommand, /--model gemini-4-argon-medium/);
  assert.match(paneCommand, /touch "\$D\/started"/);
  assert.equal(acked, "task-agy-1");
  assert.equal(startedTask.task_id, "task agy/1");
  assert.ok(startedAt <= new Date());
  assert.equal(await readFile(join(root, "task-agy-1", "brief.md"), "utf8"), "agent: agy\nDo the thing\n");
});

test("handler executes a brief larger than 8KB without embedding it in the command", async () => {
  const root = mkdtempSync(join(tmpdir(), "herdr-handler-"));
  const fixture = fixtureExecImpl(0);
  const marker = `${"x".repeat(9 * 1024)}HERDR_BRIDGE_DONE`;
  const handler = createHerdrHandler({ statusRoot: root, execFileImpl: fixture.execFileImpl });
  const result = await handler({ task_id: "large-1", content: `agent: agy\n${marker}` });

  assert.equal(result.status, "replied");
  const paneCommand = fixture.calls[1].args[3];
  assert.ok(paneCommand.length < 1024);
  assert.ok(!paneCommand.includes(marker));
  const brief = await readFile(join(root, "large-1", "brief.md"), "utf8");
  assert.ok(brief.length > 8 * 1024);
  assert.ok(brief.endsWith("HERDR_BRIDGE_DONE\n"));
});

test("handler fails quickly when Herdr never starts the command", async () => {
  const root = mkdtempSync(join(tmpdir(), "herdr-handler-"));
  const handler = createHerdrHandler({
    statusRoot: root,
    execFileImpl: async (bin, args) => {
      if (args[0] === "tab") {
        return { stdout: JSON.stringify({ root_pane: { pane_id: "pane-1" } }) };
      }
      return { stdout: "" };
    },
    pollMs: 1,
    startedTimeoutMs: 10,
  });
  const result = await handler({ id: "slow-start", content: "agent: agy\nDo the thing" });

  assert.equal(result.status, "failed");
  assert.match(result.reply, /not started within 10 ms/);
  assert.match(result.reply, /命令未起跑/);
});

test("handler routes DSH tasks to the headless profile", async () => {
  const root = mkdtempSync(join(tmpdir(), "herdr-handler-"));
  const fixture = fixtureExecImpl(2);
  const handler = createHerdrHandler({ statusRoot: root, execFileImpl: fixture.execFileImpl });
  const result = await handler({ id: "dsh-1", meta: { agent: "dsh" }, content: "Do the other thing" });

  assert.equal(result.status, "failed");
  assert.match(result.reply, /exit 2/);
  assert.match(fixture.calls[1].args[3], /dsh --profile headless --json/);
});

test("routing and smoke parsing cover text and metadata", () => {
  assert.equal(selectedAgent({ content: "agent = dsh" }), "dsh");
  assert.equal(selectedAgent({ meta: { agent: "AGY" }, content: "x" }), "agy");
  assert.equal(selectedAgent({ content: "no route" }), "agy");
  assert.equal(smokeMarker({ content: "HERDR_BRIDGE_OK_x only, 禁止改文件 but models allowed" }), null);
  assert.equal(smokeMarker({ content: "HERDR_BRIDGE_OK_x only, 禁止改文件, 禁调模型" }), "HERDR_BRIDGE_OK_x");
});
