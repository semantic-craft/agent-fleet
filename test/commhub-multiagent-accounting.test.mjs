import test from "node:test";
import assert from "node:assert/strict";
import { CommHubClient } from "../adapters/commhub/client.mjs";
import { HerdrCommHubBridge } from "../adapters/commhub/herdr-bridge.mjs";
import {
  DEFAULT_MANAGED_PROJECTS,
  filterRequirementsByProject,
  relayHandoff,
  readRelayAccounting,
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

test("simulated CommHub MCP relay: upsert external_ref, Agent A handoff, Agent B read events", async () => {
  // Mock CommHub state store simulating MCP tools behavior
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
      const { name, arguments: args } = body.params;
      const id = body.id;

      let resultText = "";

      if (name === "requirements_upsert_by_external_ref") {
        let existing = Array.from(store.requirements.values()).find(
          (r) => r.external_ref === args.external_ref
        );
        if (existing) {
          if (args.name) existing.name = args.name;
          if (args.checklist) existing.checklist = args.checklist;
          if (args.description) existing.description = args.description;
          if (args.column) existing.column = args.column;
          resultText = JSON.stringify({ ok: true, requirement: existing, action: "updated" });
        } else {
          const newCard = {
            id: `req_${store.requirements.size + 1}`,
            seq: store.requirements.size + 1,
            ...args,
            checklist: (args.checklist || []).map((item) => ({ ...item, checked: false })),
            column: args.column || "pool",
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
        if (item) item.checked = Boolean(args.checked);
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
      } else if (name === "report_status") {
        store.statuses.set(args.agent, args);
        resultText = JSON.stringify({ ok: true, status: args });
      } else if (name === "send_reply") {
        store.replies.set(args.task_id, args);
        resultText = JSON.stringify({ ok: true, replied: true });
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

  // 1. Grok Bot upserts requirement bound to GitHub Issue idempotently
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

  // Second upsert is idempotent
  const r2 = await client.upsertRequirement({
    externalRef: ref,
    name: "CommHub 架构迁移 (更新标题)",
  });
  assert.equal(r2.action, "updated");
  assert.equal(r2.requirement.id, cardId);

  // 2. Agent A (Codex) claims card, toggles checklist c1, and leaves handoff note
  await relayHandoff(client, {
    cardId,
    actor: "Codex@nodeA",
    column: "doing",
    toggledChecklistIds: ["c1"],
    comment: "完成 TeslaHub 瘦身，移除自建看板与 claims 锁；下一棒交给 DSH",
  });

  // 3. Agent B (DSH Desktop) queries card and events to inherit context
  const accounting = await readRelayAccounting(client, cardId);
  assert.equal(accounting.card.column, "doing");
  const c1Item = accounting.card.checklist.find((i) => i.id === "c1");
  assert.equal(c1Item.checked, true);

  assert.equal(accounting.comments.length, 1);
  assert.match(accounting.comments[0].text, /Codex@nodeA/);
  assert.match(accounting.comments[0].text, /完成 TeslaHub 瘦身/);

  // Check event diff sequence
  const updateEvent = accounting.events.find((e) => e.kind === "update");
  assert.equal(updateEvent.diff.column.new, "doing");
});

test("HerdrCommHubBridge handles task, prevents duplicates, writes progress comment and sends reply", async () => {
  const replies = [];
  const comments = [];
  const heartbeats = [];

  const mockClient = {
    reportStatus: async (status) => heartbeats.push(status),
    addComment: async (id, text) => comments.push({ id, text }),
    sendReply: async (reply) => replies.push(reply),
    subscribeEvents: async (alias, onEvent, { signal }) => {
      // simulate delivering a task event
      onEvent({
        kind: "task",
        task: {
          id: "tsk_bridge_001",
          cardId: "req_100",
          text: "执行单元测试并汇报",
        },
      });
      // simulate duplicate delivery of the same task
      onEvent({
        kind: "task",
        task: {
          id: "tsk_bridge_001",
          cardId: "req_100",
          text: "执行单元测试并汇报",
        },
      });
      await new Promise((resolve) => {
        if (signal?.aborted) return resolve();
        signal?.addEventListener("abort", resolve, { once: true });
      });
    },
  };

  const bridge = new HerdrCommHubBridge({
    alias: "herdr-laptop",
    agent: "herdr",
    client: mockClient,
    handler: async (task) => ({
      reply: "单元测试全部通过",
      comment: "Herdr 执行完毕：全部通过",
      status: "replied",
    }),
  });

  await bridge.start();
  // Allow async events loop to process
  await new Promise((r) => setTimeout(r, 20));
  await bridge.stop();

  // Exactly one execution despite duplicate event
  assert.equal(replies.length, 1);
  assert.equal(replies[0].taskId, "tsk_bridge_001");
  assert.equal(replies[0].reply, "单元测试全部通过");

  assert.equal(comments.length, 1);
  assert.equal(comments[0].id, "req_100");
  assert.equal(comments[0].text, "Herdr 执行完毕：全部通过");

  // Heartbeats: online -> busy -> online -> offline
  assert.ok(heartbeats.some((h) => h.status === "online"));
  assert.ok(heartbeats.some((h) => h.status === "busy"));
  assert.ok(heartbeats.some((h) => h.status === "offline"));
});
