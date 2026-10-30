// Herdr / Antigravity Desktop / ZCode CommHub SSE Doorbell & Dispatch Bridge (#132)
import { CommHubClient } from "./client.mjs";

export class HerdrCommHubBridge {
  constructor({
    alias,
    agent = "herdr",
    client = new CommHubClient(),
    handler = async (task) => ({ reply: "ok", status: "replied" }),
  } = {}) {
    this.alias = alias;
    this.agent = agent;
    this.client = client;
    this.handler = handler;
    this.processedTaskIds = new Set();
    this.abortController = null;
    this.running = false;
  }

  async start() {
    this.running = true;
    this.abortController = new AbortController();

    // Initial heartbeat
    await this.client.reportStatus({
      agent: this.agent,
      status: "online",
      queueDepth: 0,
    });

    // Start SSE listener loop
    this.listenLoop();
  }

  async listenLoop() {
    while (this.running && !this.abortController?.signal?.aborted) {
      try {
        await this.client.subscribeEvents(
          this.alias,
          (event) => this.handleEvent(event),
          { signal: this.abortController.signal }
        );
      } catch (err) {
        if (!this.running || this.abortController?.signal?.aborted) break;
      }
      if (!this.running || this.abortController?.signal?.aborted) break;
      await new Promise((r) => setTimeout(r, 200));
    }
  }

  async handleEvent(event) {
    if (event.kind !== "task" && event.type !== "task") return;
    const task = event.task || event;
    const taskId = task.id || task.taskId || task.task_id;
    if (!taskId) return;

    // Idempotency guard: never execute or reply twice
    if (this.processedTaskIds.has(taskId)) return;
    this.processedTaskIds.add(taskId);

    try {
      await this.client.reportStatus({
        agent: this.agent,
        status: "busy",
        currentTask: taskId,
        queueDepth: 1,
      });

      const outcome = await this.handler(task);

      // Post progress comment if requirementId or card context is attached
      const reqId = task.requirement_id || task.requirementId || task.cardId;
      if (reqId && outcome.comment) {
        await this.client.addComment(reqId, outcome.comment);
      }

      await this.client.sendReply({
        taskId,
        reply: outcome.reply || "任务已完成",
        status: outcome.status || "replied",
      });
    } catch (err) {
      await this.client.sendReply({
        taskId,
        reply: `任务执行失败: ${err.message}`,
        status: "failed",
      });
    } finally {
      await this.client.reportStatus({
        agent: this.agent,
        status: "online",
        currentTask: null,
        queueDepth: 0,
      });
    }
  }

  async stop() {
    this.running = false;
    this.abortController?.abort();
    try {
      await this.client.reportStatus({
        agent: this.agent,
        status: "offline",
      });
    } catch {
      /* ignore shutdown network errors */
    }
  }
}
