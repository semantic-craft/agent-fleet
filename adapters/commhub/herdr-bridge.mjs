// Herdr / Antigravity Desktop / ZCode CommHub SSE Doorbell & Dispatch Bridge (#132)
import { randomUUID } from "node:crypto";
import { CommHubClient } from "./client.mjs";
import { createLedger } from "./ledger.mjs";

const isTerminalAlready = (err) =>
  /reply_task_terminal|already terminal/i.test(String(err?.message ?? err));

export class HerdrCommHubBridge {
  constructor({
    alias,
    agent = "herdr",
    resumeId = null,
    client = null,
    handler = async (task) => ({ reply: "ok", status: "replied" }),
    ledger = null,
    ledgerPath = null,
    pollMs = 60_000,
    heartbeatMs = 30_000,
    turnTimeoutMs = 30 * 60_000,
    version = "agent-fleet",
  } = {}) {
    if (!alias) throw new Error("HerdrCommHubBridge requires alias");
    this.alias = alias;
    this.agent = agent;
    this.resumeId = resumeId || `herdr-${randomUUID()}`;
    this.client = client || new CommHubClient({ alias: this.alias, resumeId: this.resumeId });
    this.handler = handler;
    this.ledger = ledger || createLedger(ledgerPath);
    this.pollMs = pollMs;
    this.heartbeatMs = heartbeatMs;
    this.turnTimeoutMs = turnTimeoutMs;
    this.version = version;

    this.running = false;
    this.draining = null;
    this.drainAgain = false;
    this.status = "idle";
    this.currentTask = null;
    this.timers = [];
    this.abortController = null;
    this.listenPromise = null;
  }

  async heartbeat(nextStatus = null, currentTask = undefined) {
    if (nextStatus) this.status = nextStatus;
    if (currentTask !== undefined) this.currentTask = currentTask;
    try {
      return await this.client.reportStatus({
        resumeId: this.resumeId,
        alias: this.alias,
        status: this.status,
        agent: this.agent,
        task: this.currentTask,
        version: this.version,
      });
    } catch {
      /* ignore heartbeat delivery error */
    }
  }

  drain() {
    if (!this.running) return Promise.resolve();
    if (this.draining) {
      this.drainAgain = true;
      return this.draining;
    }
    this.draining = (async () => {
      do {
        this.drainAgain = false;
        try {
          await this.drainOnce();
        } catch {
          /* drain error ignored for retry on next poll */
        }
      } while (this.drainAgain && this.running);
    })().finally(() => {
      this.draining = null;
    });
    return this.draining;
  }

  async drainOnce() {
    const res = await this.client.getInbox({ alias: this.alias, limit: 10 });
    const messages = res?.messages || [];
    for (const msg of messages) {
      if (!this.running) return;
      try {
        if (msg.type === "task") {
          await this.handleTask(msg);
        }
        await this.client.ackInbox({ alias: this.alias, messageId: msg.id || msg.task_id });
      } catch {
        // Leave unacked so next drain retries; ledger prevents duplicate reply
      }
    }
    if (messages.length > 0) {
      await this.heartbeat("idle", null);
    }
    // If we drained a full batch of 10, drain again immediately to empty the backlog
    if (messages.length >= 10) {
      this.drainAgain = true;
    }
  }

  async sendReplySafe(id, text, replyStatus) {
    try {
      await this.client.sendReply({
        inReplyTo: id,
        text,
        status: replyStatus,
        fromSession: this.alias,
      });
    } catch (err) {
      if (!isTerminalAlready(err)) throw err;
    }
    this.ledger.set(id, { state: "done", status: replyStatus });
  }

  async handleTask(msg) {
    const id = String(msg.task_id || msg.id);
    const seen = this.ledger.get(id);
    if (seen?.state === "done") {
      return;
    }
    if (seen?.state === "answered") {
      await this.sendReplySafe(id, seen.text, seen.status);
      return;
    }
    if (seen?.state === "started") {
      await this.sendReplySafe(
        id,
        "herdr-bridge: the process restarted before this task finished; please resend",
        "failed"
      );
      return;
    }

    this.ledger.set(id, { state: "started" });
    const content = String(msg.content ?? msg.task?.text ?? msg.text ?? "");
    await this.heartbeat("working", content);

    let outcome;
    try {
      let t;
      const timeoutPromise = new Promise((_, reject) => {
        t = setTimeout(
          () => reject(new Error(`turn exceeded ${Math.round(this.turnTimeoutMs / 1000)}s`)),
          this.turnTimeoutMs
        );
      });
      try {
        outcome = await Promise.race([this.handler(msg), timeoutPromise]);
      } finally {
        clearTimeout(t);
      }
    } catch (err) {
      outcome = { reply: `任务执行失败: ${err.message}`, status: "failed" };
    }

    const replyText = outcome?.reply || outcome?.text || "任务已完成";
    const replyStatus = outcome?.status || "replied";

    const reqId =
      msg.requirement_id ||
      msg.requirementId ||
      msg.cardId ||
      msg.meta?.requirement_id ||
      msg.meta?.cardId ||
      msg.task?.cardId ||
      msg.task?.requirement_id;
    if (reqId && outcome?.comment) {
      try {
        await this.client.addComment(reqId, outcome.comment);
      } catch {
        /* comment error does not block reply */
      }
    }

    this.ledger.set(id, { state: "answered", text: replyText, status: replyStatus });
    await this.sendReplySafe(id, replyText, replyStatus);
  }

  async listenLoop() {
    while (this.running && !this.abortController?.signal?.aborted) {
      try {
        if (typeof this.client.events === "function") {
          for await (const event of this.client.events(this.alias, this.abortController.signal)) {
            if (!this.running) break;
            if (event.type === "connected") {
              await this.heartbeat("idle");
              void this.drain();
            } else if (
              event.type === "new_task" ||
              event.type === "new_message" ||
              event.type === "new_reply" ||
              event.type === "broadcast" ||
              (typeof event.inbox_count === "number" && event.inbox_count > 0)
            ) {
              void this.drain();
            } else if (
              event.kind === "task" ||
              (event.type === "task" && (event.id || event.task_id || event.task))
            ) {
              await this.handleTask(event.task || event);
              if (event.id) {
                await this.client.ackInbox({ alias: this.alias, messageId: event.id }).catch(() => {});
              }
            }
          }
        } else {
          await this.client.subscribeEvents(
            this.alias,
            async (event) => {
              if (event.type === "connected") {
                await this.heartbeat("idle");
                void this.drain();
              } else if (
                event.type === "new_task" ||
                event.type === "new_message" ||
                event.type === "new_reply" ||
                event.type === "broadcast" ||
                (typeof event.inbox_count === "number" && event.inbox_count > 0)
              ) {
                void this.drain();
              } else if (
                event.kind === "task" ||
                (event.type === "task" && (event.id || event.task_id || event.task))
              ) {
                await this.handleTask(event.task || event);
                if (event.id) {
                  await this.client.ackInbox({ alias: this.alias, messageId: event.id }).catch(() => {});
                }
              }
            },
            { signal: this.abortController.signal }
          );
        }
      } catch {
        if (!this.running || this.abortController?.signal?.aborted) break;
      }
      if (!this.running || this.abortController?.signal?.aborted) break;
      await new Promise((r) => setTimeout(r, 200));
    }
  }

  async start() {
    if (this.running) return;
    this.running = true;
    this.abortController = new AbortController();

    await this.heartbeat("idle");
    this.timers.push(
      setInterval(() => void this.heartbeat(null, this.currentTask), this.heartbeatMs)
    );
    this.timers.push(
      setInterval(() => void this.drain(), this.pollMs)
    );

    this.listenPromise = this.listenLoop();
    void this.drain();
  }

  async stop() {
    if (!this.running) return;
    this.running = false;
    this.abortController?.abort();
    for (const t of this.timers) {
      clearInterval(t);
      clearTimeout(t);
    }
    this.timers = [];

    if (this.listenPromise) {
      await this.listenPromise.catch(() => {});
      this.listenPromise = null;
    }

    if (this.draining) {
      await this.draining.catch(() => {});
    }

    await this.heartbeat("offline");
  }
}
