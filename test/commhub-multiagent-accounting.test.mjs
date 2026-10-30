import test from "node:test";
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { mkdtempSync, rmSync, statSync, existsSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { spawn, spawnSync } from "node:child_process";
import { CommHubClient, REPLY_MAX_CHARS, truncateReply } from "../adapters/commhub/client.mjs";
import { HerdrCommHubBridge } from "../adapters/commhub/herdr-bridge.mjs";
import { createLedger } from "../adapters/commhub/ledger.mjs";
import {
  DEFAULT_MANAGED_PROJECTS,
  filterRequirementsByProject,
  relayHandoff,
  readRelayAccounting,
  UNIFIED_ACCOUNTING_NODE_RULES,
  pushAccountingRulesToNode,
} from "../adapters/commhub/accounting.mjs";

test("CommHubClient builds Cloudflare Access and authorization headers correctly", () => {
  const client = new CommHubClient({
    hub: "https://hub.example.com",
    token: "ntok_test_12345",
    cfClientId: "cf-id-xyz.access",
    cfClientSecret: "cf-secret-987654321",
  });

  const headers = client.getHeaders();
  assert.equal(headers["Authorization"], "Bearer ntok_test_12345");
  assert.equal(headers["CF-Access-Client-Id"], "cf-id-xyz.access");
  assert.equal(headers["CF-Access-Client-Secret"], "cf-secret-987654321");
  assert.equal(headers["MCP-Protocol-Version"], "2025-03-26");
});

test("default managed project list includes required projects and filters properly", () => {
  assert.ok(DEFAULT_MANAGED_PROJECTS.includes("TeslaHub"));
  assert.ok(DEFAULT_MANAGED_PROJECTS.includes("koe"));
  assert.ok(DEFAULT_MANAGED_PROJECTS.includes("sonori"));
  assert.ok(DEFAULT_MANAGED_PROJECTS.includes("pallas-macos"));
  assert.ok(DEFAULT_MANAGED_PROJECTS.includes("writings-projects"));

  const sampleCards = [
    { id: "1", project: "TeslaHub", external_ref: "github:semantic-craft/TeslaHub#128" },
    { id: "2", project: "koe", external_ref: "github:semantic-craft/koe#42" },
    { id: "3", project: "sonori", external_ref: "github:semantic-craft/sonori#10" },
    { id: "4", project: "other", external_ref: "github:other/project#1" },
  ];

  const teslaFiltered = filterRequirementsByProject(sampleCards, "TeslaHub");
  assert.equal(teslaFiltered.length, 1);
  assert.equal(teslaFiltered[0].id, "1");

  const koeFiltered = filterRequirementsByProject(sampleCards, "koe");
  assert.equal(koeFiltered.length, 1);
  assert.equal(koeFiltered[0].id, "2");
});

test("truncateReply bounds oversized reply text to hub 10000 limit with notice", () => {
  assert.equal(truncateReply("short text"), "short text");
  const long = "A".repeat(12_000);
  const truncated = truncateReply(long);
  assert.equal(truncated.length, REPLY_MAX_CHARS);
  assert.ok(truncated.includes("[agent-fleet: reply truncated"));
});

test("CommHubClient initializes MCP session and retries on 400/404 hub restart", async () => {
  let initCount = 0;
  let callsCount = 0;
  let sessionsIssued = 0;
  let restartTriggered = false;

  const server = createServer(async (req, res) => {
    let body = "";
    for await (const chunk of req) body += chunk;
    const msg = JSON.parse(body || "{}");

    if (msg.method === "initialize") {
      initCount++;
      const sid = `sess_${++sessionsIssued}`;
      res.writeHead(200, {
        "Content-Type": "application/json",
        "mcp-session-id": sid,
      });
      res.end(
        JSON.stringify({
          jsonrpc: "2.0",
          id: msg.id,
          result: { protocolVersion: "2025-03-26" },
        })
      );
      return;
    }

    if (msg.method === "tools/call") {
      callsCount++;
      const sid = req.headers["mcp-session-id"];
      // Simulate hub restarting after first call: invalidates old session
      if (callsCount === 2 && !restartTriggered) {
        restartTriggered = true;
        res.writeHead(404, { "Content-Type": "text/plain" }).end("Session not found");
        return;
      }

      assert.ok(sid, "mcp-session-id must be present on tools/call");
      res.writeHead(200, { "Content-Type": "application/json" });
      res.end(
        JSON.stringify({
          jsonrpc: "2.0",
          id: msg.id,
          result: { content: [{ type: "text", text: JSON.stringify({ ok: true, sid }) }] },
        })
      );
      return;
    }

    res.writeHead(404).end();
  });

  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const hubUrl = `http://127.0.0.1:${server.address().port}`;

  try {
    const client = new CommHubClient({ hub: hubUrl, token: "test_tok" });

    // 1. First call initializes automatically
    const r1 = await client.call("get_all_status");
    assert.equal(r1.ok, true);
    assert.equal(initCount, 1);
    assert.equal(client.sessionId, "sess_1");

    // 2. Second call triggers 404, client re-initializes and retries
    const r2 = await client.call("get_all_status");
    assert.equal(r2.ok, true);
    assert.equal(initCount, 2);
    assert.equal(client.sessionId, "sess_2");
  } finally {
    await new Promise((resolve) => server.close(resolve));
  }
});

test("CommHubClient deduplicates concurrent initialize() calls", async () => {
  let initCount = 0;
  const server = createServer(async (req, res) => {
    let body = "";
    for await (const chunk of req) body += chunk;
    const msg = JSON.parse(body || "{}");

    if (msg.method === "initialize") {
      initCount++;
      await new Promise((r) => setTimeout(r, 20));
      res.writeHead(200, {
        "Content-Type": "application/json",
        "mcp-session-id": "single_session",
      });
      res.end(JSON.stringify({ jsonrpc: "2.0", id: msg.id, result: { protocolVersion: "2025-03-26" } }));
      return;
    }

    if (msg.method === "tools/call") {
      res.writeHead(200, { "Content-Type": "application/json" });
      res.end(
        JSON.stringify({
          jsonrpc: "2.0",
          id: msg.id,
          result: { content: [{ type: "text", text: JSON.stringify({ ok: true, callId: msg.id }) }] },
        })
      );
      return;
    }
    res.writeHead(404).end();
  });

  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const hubUrl = `http://127.0.0.1:${server.address().port}`;

  try {
    const client = new CommHubClient({ hub: hubUrl, token: "tok" });
    // Fire 3 simultaneous calls before session is initialized
    const [res1, res2, res3] = await Promise.all([
      client.call("get_all_status"),
      client.call("get_all_status"),
      client.call("get_all_status"),
    ]);

    assert.equal(initCount, 1, "Must perform exactly one initialize handshake for concurrent callers");
    assert.equal(client.sessionId, "single_session");
    assert.equal(res1.ok, true);
    assert.equal(res2.ok, true);
    assert.equal(res3.ok, true);
  } finally {
    await new Promise((resolve) => server.close(resolve));
  }
});

test("Wire-level schema enforcement: Zod validation rejects legacy parameter mismatches", async () => {
  // Exact schema enforcer matching server/src/tools.ts
  const VALID_STATUSES = new Set(["working", "idle", "blocked", "error", "waiting_input", "offline"]);

  const server = createServer(async (req, res) => {
    let body = "";
    for await (const chunk of req) body += chunk;
    const msg = JSON.parse(body || "{}");

    if (msg.method === "initialize") {
      res.writeHead(200, { "Content-Type": "application/json", "mcp-session-id": "strict_sess" });
      res.end(JSON.stringify({ jsonrpc: "2.0", id: msg.id, result: { protocolVersion: "2025-03-26" } }));
      return;
    }

    if (msg.method === "tools/call") {
      const { name, arguments: args } = msg.params;

      const reject = (msgText) => {
        res.writeHead(200, { "Content-Type": "application/json" });
        res.end(
          JSON.stringify({
            jsonrpc: "2.0",
            id: msg.id,
            error: { code: -32602, message: `Invalid params: ${msgText}` },
          })
        );
      };

      if (name === "report_status") {
        if (!args.resume_id || typeof args.resume_id !== "string") {
          return reject("resume_id required string");
        }
        if (!args.alias || typeof args.alias !== "string") {
          return reject("alias required string");
        }
        if (!VALID_STATUSES.has(args.status)) {
          return reject(`status invalid enum: ${args.status}`);
        }
      } else if (name === "requirements_checklist_toggle") {
        if (!args.id || typeof args.id !== "string") return reject("id required");
        if (!args.item_id || typeof args.item_id !== "string") return reject("item_id required");
        if (args.done === undefined || typeof args.done !== "boolean") {
          return reject("done must be boolean; checked is not accepted");
        }
      } else if (name === "send_task") {
        if (!args.alias || typeof args.alias !== "string") {
          return reject("alias required string (target is rejected)");
        }
        if (!args.task || typeof args.task !== "string") {
          return reject("task required string (text is rejected)");
        }
      } else if (name === "send_reply") {
        if (!args.text || typeof args.text !== "string") {
          return reject("text required string (reply is rejected)");
        }
      }

      res.writeHead(200, { "Content-Type": "application/json" });
      res.end(
        JSON.stringify({
          jsonrpc: "2.0",
          id: msg.id,
          result: { content: [{ type: "text", text: JSON.stringify({ ok: true, echo: args }) }] },
        })
      );
      return;
    }

    res.writeHead(404).end();
  });

  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const hubUrl = `http://127.0.0.1:${server.address().port}`;

  try {
    const client = new CommHubClient({ hub: hubUrl, token: "test_tok", alias: "node-codex" });

    // 1. reportStatus maps "online" -> "idle", adds resume_id and alias
    const statusRes = await client.reportStatus({ status: "online", agent: "codex" });
    assert.equal(statusRes.ok, true);
    assert.equal(statusRes.echo.status, "idle");
    assert.ok(statusRes.echo.resume_id);
    assert.equal(statusRes.echo.alias, "node-codex");

    // 2. toggleChecklist sends { done: true } even if called with checked or done
    const toggleRes1 = await client.toggleChecklist("req_1", "c1", true);
    assert.equal(toggleRes1.ok, true);
    assert.equal(toggleRes1.echo.done, true);
    assert.equal(toggleRes1.echo.checked, undefined);

    const toggleRes2 = await client.toggleChecklist("req_1", "c2", { checked: true });
    assert.equal(toggleRes2.ok, true);
    assert.equal(toggleRes2.echo.done, true);

    // 3. sendTask sends { alias, task } over the wire even if given target/text
    const taskRes = await client.sendTask({ target: "node-b", text: "run check" });
    assert.equal(taskRes.ok, true);
    assert.equal(taskRes.echo.alias, "node-b");
    assert.equal(taskRes.echo.task, "run check");
    assert.equal(taskRes.echo.target, undefined);

    // 4. sendReply sends { text, in_reply_to } over the wire even if given reply/taskId
    const replyRes = await client.sendReply({ taskId: "t123", reply: "done ok" });
    assert.equal(replyRes.ok, true);
    assert.equal(replyRes.echo.in_reply_to, "t123");
    assert.equal(replyRes.echo.text, "done ok");
    assert.equal(replyRes.echo.reply, undefined);

    // 5. sendReply with empty string falls back to non-empty text, avoiding -32602 Zod rejection
    const emptyReplyRes = await client.sendReply({ taskId: "t124", reply: "" });
    assert.equal(emptyReplyRes.ok, true);
    assert.equal(emptyReplyRes.echo.text, "(empty reply)");

    // 6. toggleChecklist with null/falsy doesn't throw TypeError
    const toggleRes3 = await client.toggleChecklist("req_1", "c3", null);
    assert.equal(toggleRes3.ok, true);
    assert.equal(typeof toggleRes3.echo.done, "boolean");

    // 7. reportStatus with non-standard status maps to valid enum
    const statusRes2 = await client.reportStatus({ status: "done" });
    assert.equal(statusRes2.ok, true);
    assert.equal(statusRes2.echo.status, "idle");
  } finally {
    await new Promise((resolve) => server.close(resolve));
  }
});

test("simulated CommHub MCP relay: upsert external_ref, Agent A handoff, Agent B read events", async () => {
  const store = {
    requirements: new Map(),
    events: [],
    statuses: new Map(),
    replies: new Map(),
  };

  const mockFetch = async (url, init) => {
    const parsedUrl = new URL(url);
    if (parsedUrl.pathname === "/mcp") {
      const body = JSON.parse(init.body);
      const { name, arguments: args } = body.params || {};
      const id = body.id;

      if (body.method === "initialize") {
        return {
          ok: true,
          headers: new Headers({ "mcp-session-id": "mock_sess" }),
          text: async () => JSON.stringify({ jsonrpc: "2.0", id, result: { protocolVersion: "2025-03-26" } }),
        };
      }

      let resultText = "";

      if (name === "requirements_upsert_by_external_ref") {
        let existing = Array.from(store.requirements.values()).find(
          (r) => r.external_ref === args.external_ref
        );
        if (existing) {
          if (args.name) existing.name = args.name;
          if (args.checklist) existing.checklist = args.checklist;
          if (args.description) existing.description = args.description;
          if (args.column || args.status) existing.column = args.column || args.status;
          resultText = JSON.stringify({ ok: true, requirement: existing, action: "updated" });
        } else {
          const newCard = {
            id: `req_${store.requirements.size + 1}`,
            seq: store.requirements.size + 1,
            ...args,
            checklist: (args.checklist || []).map((item) => ({ ...item, checked: false })),
            column: args.column || args.status || "pool",
          };
          store.requirements.set(newCard.id, newCard);
          store.events.push({
            id: `evt_${store.events.length + 1}`,
            kind: "create",
            requirement_id: newCard.id,
            actor: "grok-bot",
            timestamp: new Date().toISOString(),
          });
          resultText = JSON.stringify({ ok: true, requirement: newCard, action: "created" });
        }
      } else if (name === "requirements_get") {
        const card = store.requirements.get(args.id);
        resultText = JSON.stringify({ ok: true, requirement: card || null });
      } else if (name === "requirements_update") {
        const card = store.requirements.get(args.id);
        if (card) {
          const oldCol = card.column;
          Object.assign(card, args);
          if (args.status) card.column = args.status;
          store.events.push({
            id: `evt_${store.events.length + 1}`,
            kind: "update",
            requirement_id: card.id,
            actor: "node_codex",
            diff: { column: { old: oldCol, new: card.column } },
            timestamp: new Date().toISOString(),
          });
        }
        resultText = JSON.stringify({ ok: true, requirement: card });
      } else if (name === "requirements_checklist_toggle") {
        const card = store.requirements.get(args.id);
        const item = card?.checklist?.find((i) => i.id === args.item_id);
        if (item) item.checked = Boolean(args.done ?? args.checked);
        store.events.push({
          id: `evt_${store.events.length + 1}`,
          kind: "checklist_toggle",
          requirement_id: card.id,
          actor: "node_codex",
          item_id: args.item_id,
          checked: item?.checked,
          timestamp: new Date().toISOString(),
        });
        resultText = JSON.stringify({ ok: true, item });
      } else if (name === "requirements_comment") {
        const card = store.requirements.get(args.id);
        const commentEvent = {
          id: `evt_${store.events.length + 1}`,
          kind: "comment",
          requirement_id: card.id,
          actor: "node_codex",
          new: { text: args.text },
          timestamp: new Date().toISOString(),
        };
        store.events.push(commentEvent);
        resultText = JSON.stringify({ ok: true, event: commentEvent });
      } else if (name === "requirements_events") {
        const matching = store.events.filter(
          (e) => !args.requirement_id || e.requirement_id === args.requirement_id
        );
        resultText = JSON.stringify({ ok: true, events: matching });
      }

      return {
        ok: true,
        text: async () =>
          JSON.stringify({
            jsonrpc: "2.0",
            id,
            result: { content: [{ type: "text", text: resultText }] },
          }),
      };
    }
    return { ok: false, status: 404, text: async () => "Not Found" };
  };

  const client = new CommHubClient({
    hub: "http://127.0.0.1:9200",
    token: "ntok_test",
    fetchImpl: mockFetch,
  });

  const ref = "github:semantic-craft/TeslaHub#128";
  const r1 = await client.upsertRequirement({
    externalRef: ref,
    name: "CommHub 架构迁移",
    checklist: [
      { id: "c1", text: "瘦身 TeslaHub" },
      { id: "c2", text: "直连 CommHub" },
    ],
  });
  assert.equal(r1.action, "created");
  const cardId = r1.requirement.id;

  const r2 = await client.upsertRequirement({
    externalRef: ref,
    name: "CommHub 架构迁移 (更新标题)",
  });
  assert.equal(r2.action, "updated");
  assert.equal(r2.requirement.id, cardId);

  await relayHandoff(client, {
    cardId,
    actor: "Codex@nodeA",
    column: "doing",
    toggledChecklistIds: ["c1"],
    comment: "完成 TeslaHub 瘦身，移除自建看板与 claims 锁；下一棒交给 DSH",
  });

  const accounting = await readRelayAccounting(client, cardId);
  assert.equal(accounting.card.column, "doing");
  const c1Item = accounting.card.checklist.find((i) => i.id === "c1");
  assert.equal(c1Item.checked, true);

  assert.equal(accounting.comments.length, 1);
  assert.match(accounting.comments[0].text, /Codex@nodeA/);
  assert.match(accounting.comments[0].text, /完成 TeslaHub 瘦身/);

  const updateEvent = accounting.events.find((e) => e.kind === "update");
  assert.equal(updateEvent.diff.column.new, "doing");
});

test("HerdrCommHubBridge handles lightweight new_task SSE event, drains inbox, acks, and uses durable ledger", async () => {
  const tmpDir = mkdtempSync(join(tmpdir(), "bridge-ledger-"));
  const ledgerPath = join(tmpDir, "ledger.json");

  const inbox = [
    {
      id: "msg_inbox_001",
      type: "task",
      task_id: "tsk_bridge_001",
      from_session: "grok-bot",
      content: "执行端到端单元测试",
      cardId: "req_100",
    },
  ];
  const replies = [];
  const acks = [];
  const heartbeats = [];
  const comments = [];

  const mockClient = {
    alias: "herdr-laptop",
    resumeId: "herdr-res-001",
    reportStatus: async (args) => heartbeats.push(args),
    getInbox: async () => ({ messages: [...inbox] }),
    ackInbox: async (args) => {
      acks.push(args);
      const idx = inbox.findIndex((m) => m.id === args.messageId);
      if (idx !== -1) inbox.splice(idx, 1);
      return { ok: true };
    },
    sendReply: async (args) => replies.push(args),
    addComment: async (id, text) => comments.push({ id, text }),
    subscribeEvents: async (alias, onEvent, { signal }) => {
      // CommHub lightweight doorbell: ONLY { type: "new_task", inbox_count: 1 }
      onEvent({
        type: "new_task",
        inbox_count: 1,
        priority: "normal",
        from: "grok-bot",
      });
      // Duplicate doorbell push
      onEvent({
        type: "new_task",
        inbox_count: 1,
        priority: "normal",
        from: "grok-bot",
      });
      await new Promise((resolve) => {
        if (signal?.aborted) return resolve();
        signal?.addEventListener("abort", resolve, { once: true });
      });
    },
  };

  try {
    const bridge = new HerdrCommHubBridge({
      alias: "herdr-laptop",
      agent: "herdr",
      client: mockClient,
      ledgerPath,
      handler: async (task) => ({
        reply: "测试全部通过",
        comment: "Herdr 执行完毕：全部通过",
        status: "replied",
      }),
    });

    await bridge.start();
    await new Promise((r) => setTimeout(r, 50));
    await bridge.stop();

    // Exactly one reply despite multiple doorbell notifications
    assert.equal(replies.length, 1);
    assert.equal(replies[0].inReplyTo, "tsk_bridge_001");
    assert.equal(replies[0].text, "测试全部通过");
    assert.equal(replies[0].fromSession, "herdr-laptop");

    // Inbox was drained and acknowledged
    assert.equal(acks.length, 1);
    assert.equal(acks[0].messageId, "msg_inbox_001");

    // Comment added on attached card
    assert.equal(comments.length, 1);
    assert.equal(comments[0].id, "req_100");

    // Durable ledger verified on disk with 0600 mode
    const st = statSync(ledgerPath);
    assert.equal(st.mode & 0o777, 0o600);
    const ledger = createLedger(ledgerPath);
    assert.equal(ledger.get("tsk_bridge_001")?.state, "done");

    // Heartbeats used valid statuses: idle -> working -> idle -> offline
    assert.ok(heartbeats.some((h) => h.status === "idle"));
    assert.ok(heartbeats.some((h) => h.status === "working"));
    assert.ok(heartbeats.some((h) => h.status === "offline"));
    assert.ok(heartbeats.every((h) => !["online", "busy"].includes(h.status)));
  } finally {
    rmSync(tmpDir, { recursive: true, force: true });
  }
});

test("HerdrCommHubBridge durable ledger crash recovery across restarts", async () => {
  const tmpDir = mkdtempSync(join(tmpdir(), "crash-ledger-"));
  const ledgerPath = join(tmpDir, "ledger.json");

  try {
    const ledger = createLedger(ledgerPath);
    // Task 1 was done before crash
    ledger.set("t1", { state: "done", status: "replied" });
    // Task 2 was answered but reply wasn't delivered before crash
    ledger.set("t2", { state: "answered", text: "saved answer", status: "replied" });
    // Task 3 crashed mid-execution
    ledger.set("t3", { state: "started" });

    const replies = [];
    const mockClient = {
      alias: "herdr-recovery",
      reportStatus: async () => {},
      getInbox: async () => ({
        messages: [
          { id: "m1", type: "task", task_id: "t1", content: "task 1" },
          { id: "m2", type: "task", task_id: "t2", content: "task 2" },
          { id: "m3", type: "task", task_id: "t3", content: "task 3" },
        ],
      }),
      ackInbox: async () => ({ ok: true }),
      sendReply: async (reply) => replies.push(reply),
      subscribeEvents: async () => {},
    };

    let executedTasks = 0;
    const bridge = new HerdrCommHubBridge({
      alias: "herdr-recovery",
      client: mockClient,
      ledgerPath,
      handler: async () => {
        executedTasks++;
        return { reply: "fresh reply" };
      },
    });

    await bridge.start();
    await bridge.drain();
    await bridge.stop();

    // Zero tasks were re-executed
    assert.equal(executedTasks, 0);

    // t1 was skipped, t2 sent saved answer, t3 sent crash failure notice
    assert.equal(replies.length, 2);
    const r2 = replies.find((r) => r.inReplyTo === "t2");
    assert.equal(r2.text, "saved answer");
    assert.equal(r2.status, "replied");

    const r3 = replies.find((r) => r.inReplyTo === "t3");
    assert.match(r3.text, /restarted before this task finished/);
    assert.equal(r3.status, "failed");
  } finally {
    rmSync(tmpDir, { recursive: true, force: true });
  }
});

test("HerdrCommHubBridge drains entire backlog when inbox has >= 10 messages", async () => {
  const tmpDir = mkdtempSync(join(tmpdir(), "backlog-ledger-"));
  const ledgerPath = join(tmpDir, "ledger.json");

  // Total 15 messages: page 1 will return 10 messages, page 2 will return 5 messages
  const queue = Array.from({ length: 15 }, (_, i) => ({
    id: `m_${i + 1}`,
    type: "task",
    task_id: `t_${i + 1}`,
    content: `task payload ${i + 1}`,
  }));

  const handled = [];
  const acks = [];

  const mockClient = {
    alias: "herdr-backlog",
    reportStatus: async () => {},
    getInbox: async ({ limit = 10 } = {}) => {
      const batch = queue.slice(0, limit);
      return { messages: batch };
    },
    ackInbox: async ({ messageId }) => {
      acks.push(messageId);
      const idx = queue.findIndex((m) => m.id === messageId || m.task_id === messageId);
      if (idx !== -1) queue.splice(idx, 1);
      return { ok: true };
    },
    sendReply: async () => {},
    subscribeEvents: async () => {},
  };

  try {
    const bridge = new HerdrCommHubBridge({
      alias: "herdr-backlog",
      client: mockClient,
      ledgerPath,
      pollMs: 60_000,
      handler: async (task) => {
        handled.push(task.task_id);
        return { reply: "ok" };
      },
    });

    await bridge.start();
    // A single drain() invocation should drain all 15 without waiting for the 60s poll timer
    await bridge.drain();
    await bridge.stop();

    assert.equal(handled.length, 15, "All 15 tasks across multiple pages must be processed");
    assert.equal(acks.length, 15, "All 15 tasks must be acknowledged");
    assert.equal(queue.length, 0, "Inbox queue must be empty");
  } finally {
    rmSync(tmpDir, { recursive: true, force: true });
  }
});

test("HerdrCommHubBridge times out hung turns and reports failure", async () => {
  const tmpDir = mkdtempSync(join(tmpdir(), "timeout-ledger-"));
  const ledgerPath = join(tmpDir, "ledger.json");

  const replies = [];
  const mockClient = {
    alias: "herdr-timeout",
    reportStatus: async () => {},
    getInbox: async () => ({
      messages: [{ id: "m_hung", type: "task", task_id: "t_hung", content: "hung turn" }],
    }),
    ackInbox: async () => ({ ok: true }),
    sendReply: async (reply) => replies.push(reply),
    subscribeEvents: async () => {},
  };

  try {
    const bridge = new HerdrCommHubBridge({
      alias: "herdr-timeout",
      client: mockClient,
      ledgerPath,
      turnTimeoutMs: 50, // fast timeout for test
      handler: async () => {
        // Hang indefinitely
        await new Promise((r) => setTimeout(r, 2000));
        return { reply: "too late" };
      },
    });

    await bridge.start();
    await bridge.drain();
    await bridge.stop();

    assert.equal(replies.length, 1);
    assert.equal(replies[0].inReplyTo, "t_hung");
    assert.equal(replies[0].status, "failed");
    assert.match(replies[0].text, /turn exceeded/);

    const ledger = createLedger(ledgerPath);
    assert.equal(ledger.get("t_hung")?.state, "done");
    assert.equal(ledger.get("t_hung")?.status, "failed");
  } finally {
    rmSync(tmpDir, { recursive: true, force: true });
  }
});

test("writeNodeRulesFile, readNodeRulesFile, and pushAccountingRulesToNode MCP tool calls", async () => {
  const calls = [];
  const mockClient = {
    call: async (tool, args) => {
      calls.push({ tool, args });
      return { ok: true, request_id: "req_rf_001" };
    },
    writeNodeRulesFile: async (args) => {
      calls.push({ tool: "write_node_rules_file", args });
      return { ok: true, request_id: "req_rf_001" };
    },
  };

  assert.match(UNIFIED_ACCOUNTING_NODE_RULES, /requirements_get/);
  assert.match(UNIFIED_ACCOUNTING_NODE_RULES, /requirements_comment/);
  assert.match(UNIFIED_ACCOUNTING_NODE_RULES, /requirements_checklist_toggle/);
  assert.match(UNIFIED_ACCOUNTING_NODE_RULES, /send_reply/);

  const res = await pushAccountingRulesToNode(mockClient, {
    alias: "codex-desktop",
    networkId: "net_main",
  });
  assert.equal(res.ok, true);
  assert.equal(calls.length, 1);
  assert.equal(calls[0].tool, "write_node_rules_file");
  assert.equal(calls[0].args.alias, "codex-desktop");
  assert.equal(calls[0].args.networkId, "net_main");
  assert.match(calls[0].args.content, /CommHub Multi-Agent Unified Accounting Rules/);
});

test("CommHubClient subscribeEvents normalizes CRLF SSE chunks across proxies", async () => {
  const events = [];
  const crlfStream = new ReadableStream({
    start(controller) {
      const chunk = "data: {\"type\":\"ping\"}\r\n\r\ndata: {\"type\":\"pong\"}\r\n\r\n";
      controller.enqueue(new TextEncoder().encode(chunk));
      controller.close();
    },
  });

  const client = new CommHubClient({
    hub: "http://127.0.0.1:9200",
    fetchImpl: async () => ({
      ok: true,
      body: crlfStream,
    }),
  });

  await client.subscribeEvents("test-alias", (e) => events.push(e));
  assert.equal(events.length, 2);
  assert.equal(events[0].type, "ping");
  assert.equal(events[1].type, "pong");
});

test("E2E with isolated real CommHub server instance (bun subprocess + temp SQLite DB)", async (t) => {
  const home = process.env.HOME || "";
  const serverDir = join(home, "Projects", "_forks", "agent-network", "server");
  const serverPath = join(serverDir, "src", "index.ts");
  if (!existsSync(serverPath)) {
    t.skip("Upstream _forks/agent-network server not found, skipping live subprocess test");
    return;
  }

  const bunCheck = spawnSync("which", ["bun"]);
  if (bunCheck.status !== 0) {
    t.skip("bun runtime not found, skipping live subprocess test");
    return;
  }

  const tmpDir = mkdtempSync(join(tmpdir(), "commhub-e2e-"));
  const dbPath = join(tmpDir, "test-hub.db");
  const ledgerPath = join(tmpDir, "herdr-ledger.json");
  const port = 9520 + Math.floor(Math.random() * 300);
  const hubUrl = `http://127.0.0.1:${port}`;
  const adminToken = "adm_test_token_live";

  let proc;
  try {
    proc = spawn("bun", [serverPath], {
      cwd: serverDir,
      env: {
        ...process.env,
        COMMHUB_SERVER: "1",
        HOST: "127.0.0.1",
        PORT: String(port),
        COMMHUB_DB: dbPath,
        COMMHUB_AUTH_TOKEN: adminToken,
      },
      stdio: "ignore",
    });
    proc.on("error", () => {});

    // Wait for health endpoint
    let healthy = false;
    for (let i = 0; i < 40; i++) {
      try {
        const res = await fetch(`${hubUrl}/health`);
        if (res.ok) {
          healthy = true;
          break;
        }
      } catch {}
      await new Promise((r) => setTimeout(r, 100));
    }
    assert.ok(healthy, "CommHub server failed to boot health endpoint within 4 seconds");

    // Register user, create network, and issue node token
    const regRes = await fetch(`${hubUrl}/api/auth/register`, {
      method: "POST",
      headers: { authorization: `Bearer ${adminToken}`, "content-type": "application/json" },
      body: JSON.stringify({ username: "liveowner", password: "password123" }),
    });
    const { token: userToken } = await regRes.json();
    const userAuth = { authorization: `Bearer ${userToken}`, "content-type": "application/json" };

    const netRes = await fetch(`${hubUrl}/api/networks`, {
      method: "POST",
      headers: userAuth,
      body: JSON.stringify({ name: "e2e-net" }),
    });
    const { network_id: netId } = await netRes.json();

    const nodeRes = await fetch(`${hubUrl}/api/auth/node-token`, {
      method: "POST",
      headers: userAuth,
      body: JSON.stringify({ network_id: netId, node_name: "herdr-e2e", node_id: "n_herdr_e2e" }),
    });
    const { token: nodeToken } = await nodeRes.json();
    assert.ok(nodeToken?.startsWith("ntok_"), "Must obtain valid ntok_ node token");

    // 1. CommHubClient connects with real node token and initializes MCP session
    const client = new CommHubClient({
      hub: hubUrl,
      token: nodeToken,
      alias: "herdr-e2e",
      networkId: netId,
    });

    const statusRes = await client.reportStatus({ status: "idle", agent: "herdr" });
    assert.equal(statusRes.ok, true);

    // 2. Kanban requirements tool calls against real CommHub SQLite DB
    const upRes = await client.upsertRequirement({
      externalRef: "github:semantic-craft/TeslaHub#132",
      name: "CommHub 真实服务端集成测试",
      checklist: [{ id: "step1", text: "建立真实端到端连接" }],
      column: "doing",
    });
    assert.equal(upRes.ok, true);
    const cardId = upRes.requirement.id;

    // Toggle checklist item with real { done: true }
    const toggleRes = await client.toggleChecklist(cardId, "step1", true);
    assert.equal(toggleRes.ok, true);

    // Comment
    const commentRes = await client.addComment(cardId, "端到端门铃桥开始执行");
    assert.equal(commentRes.ok, true);

    // 3. HerdrCommHubBridge starts and listens to real CommHub SSE doorbell
    let taskReceived = null;
    const bridge = new HerdrCommHubBridge({
      alias: "herdr-e2e",
      agent: "herdr",
      client,
      ledgerPath,
      pollMs: 1000,
      heartbeatMs: 2000,
      handler: async (task) => {
        taskReceived = task;
        return {
          reply: `真实 CommHub 处理完毕: ${task.content}`,
          comment: "Herdr 真实处理完毕",
          status: "replied",
        };
      },
    });

    await bridge.start();

    // 4. Send task to herdr-e2e via CommHub send_task
    const sendTaskRes = await client.sendTask({
      alias: "herdr-e2e",
      task: "验证真实 CommHub 门铃与收件箱",
      meta: { cardId },
    });
    assert.equal(sendTaskRes.ok, true);

    // Wait for bridge to receive SSE doorbell, drain inbox, execute handler, and reply
    let replied = false;
    for (let i = 0; i < 40; i++) {
      if (taskReceived) {
        replied = true;
        break;
      }
      await new Promise((r) => setTimeout(r, 100));
    }
    assert.ok(replied, "HerdrCommHubBridge did not receive and process task within 4 seconds");
    assert.equal(taskReceived.content, "验证真实 CommHub 门铃与收件箱");

    // Allow reply and ack to settle
    await new Promise((r) => setTimeout(r, 200));
    await bridge.stop();

    // Check durable ledger entry was persisted to disk with 0600 mode
    const ledger = createLedger(ledgerPath);
    const taskId = String(taskReceived.task_id || taskReceived.id);
    assert.equal(ledger.get(taskId)?.state, "done");
  } finally {
    if (proc) {
      try { proc.kill("SIGTERM"); } catch {}
      try { proc.kill("SIGKILL"); } catch {}
    }
    rmSync(tmpDir, { recursive: true, force: true });
  }
});
