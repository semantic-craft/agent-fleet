import test from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { HerdrCommHubBridge } from "../herdr-bridge.mjs";

test("bridge marks the CommHub task running only when Herdr reports command start", async () => {
  const tmpDir = mkdtempSync(join(tmpdir(), "bridge-started-"));
  const acks = [];
  const statuses = [];
  const msg = {
    id: "msg-start-1",
    task_id: "task-start-1",
    type: "task",
    content: "agent: agy\nread-only smoke",
  };
  const client = {
    alias: "herdr-local",
    resumeId: "herdr-resume",
    reportStatus: async (args) => statuses.push(args),
    ackTask: async (args) => acks.push(args),
    getInbox: async () => ({ messages: [{ ...msg }] }),
    ackInbox: async () => ({ ok: true }),
    sendReply: async () => ({ ok: true }),
    subscribeEvents: async () => {},
  };

  try {
    const bridge = new HerdrCommHubBridge({
      alias: "herdr-local",
      client,
      ledgerPath: join(tmpDir, "ledger.json"),
      handler: async (_task, started) => {
        assert.ok(!statuses.some((s) => s.status === "working"));
        assert.deepEqual(acks, []);
        await started();
        return { reply: "ok", status: "replied" };
      },
    });

    await bridge.start();
    await bridge.drain();
    await bridge.stop();

    assert.deepEqual(acks, [{ task_id: "task-start-1" }]);
    const running = statuses.find((s) => s.status === "working");
    assert.equal(running.task, msg.content);
    assert.equal(statuses[0].status, "idle");
    assert.ok(statuses.indexOf(running) === 1, "working follows the actual start ack");
    assert.ok(statuses.some((s) => s.status === "idle"));
    assert.ok(statuses.at(-1).status === "offline");
  } finally {
    rmSync(tmpDir, { recursive: true, force: true });
  }
});
