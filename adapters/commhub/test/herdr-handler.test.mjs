import test from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, writeFileSync } from "node:fs";
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
        writeFileSync(join(dir, "exit"), `${exitCode}\n`);
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
  const handler = createHerdrHandler({ statusRoot: root, execFileImpl: fixture.execFileImpl });
  const result = await handler({ task_id: "task agy/1", content: "agent: agy\nDo the thing" });

  assert.equal(result.status, "replied");
  assert.match(result.reply, /task task-agy-1: ok/);
  assert.match(result.reply, /agent=agy/);
  const tabArgs = fixture.calls[0].args;
  assert.equal(tabArgs[0], "tab");
  assert.equal(tabArgs[1], "create");
  const paneCommand = fixture.calls[1].args[3];
  assert.match(paneCommand, /agy -p/);
  assert.match(paneCommand, /--model gemini-4-argon-medium/);
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
