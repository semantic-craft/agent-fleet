// CommHub Streamable HTTP MCP Client & Doorbell Listener (#128, #129, #131, #132)
import { readFileSync } from "node:fs";

export class CommHubClient {
  constructor({
    hub = process.env.ANET_HUB || "http://127.0.0.1:9200",
    token = process.env.ANET_NODE_TOKEN || null,
    tokenFile = process.env.ANET_NODE_TOKEN_FILE || null,
    cfClientId = process.env.CF_ACCESS_CLIENT_ID || null,
    cfClientSecret = process.env.CF_ACCESS_CLIENT_SECRET || null,
    fetchImpl = globalThis.fetch,
  } = {}) {
    this.hub = hub.replace(/\/$/, "");
    this.token = token || (tokenFile ? readFileSync(tokenFile, "utf8").trim() : null);
    this.cfClientId = cfClientId;
    this.cfClientSecret = cfClientSecret;
    this.fetch = fetchImpl;
    this.rpcId = 1;
  }

  getHeaders(extra = {}) {
    const headers = {
      "Content-Type": "application/json",
      "Accept": "application/json, text/event-stream",
      "MCP-Protocol-Version": "2025-03-26",
      ...extra,
    };
    if (this.token) {
      headers["Authorization"] = `Bearer ${this.token}`;
    }
    if (this.cfClientId) {
      headers["CF-Access-Client-Id"] = this.cfClientId;
    }
    if (this.cfClientSecret) {
      headers["CF-Access-Client-Secret"] = this.cfClientSecret;
    }
    return headers;
  }

  async call(toolName, args = {}) {
    const id = this.rpcId++;
    const res = await this.fetch(`${this.hub}/mcp`, {
      method: "POST",
      headers: this.getHeaders(),
      body: JSON.stringify({
        jsonrpc: "2.0",
        id,
        method: "tools/call",
        params: { name: toolName, arguments: args },
      }),
    });

    if (!res.ok) {
      const errText = await res.text();
      throw new Error(`CommHub MCP HTTP ${res.status}: ${errText.slice(0, 300)}`);
    }

    const raw = await res.text();
    // MCP Streamable HTTP response can be SSE (lines starting with data:) or raw JSON
    const lines = raw.split("\n").filter((l) => l.startsWith("data:"));
    const envelope = lines.length ? JSON.parse(lines.at(-1).slice(5).trim()) : JSON.parse(raw);

    if (envelope.error) {
      throw new Error(`MCP error ${envelope.error.code}: ${envelope.error.message}`);
    }

    const text = envelope.result?.content?.[0]?.text;
    if (text === undefined) return envelope.result;
    try {
      return JSON.parse(text);
    } catch {
      return text;
    }
  }

  // --- Requirements Kanban & Accounting Tools ---

  async upsertRequirement({
    externalRef,
    externalUrl = null,
    name,
    description = null,
    priority = null,
    checklist = null,
    column = null,
  }) {
    const args = { external_ref: externalRef, name };
    if (externalUrl !== null) args.external_url = externalUrl;
    if (description !== null) args.description = description;
    if (priority !== null) args.priority = priority;
    if (checklist !== null) args.checklist = checklist;
    if (column !== null) args.column = column;
    return this.call("requirements_upsert_by_external_ref", args);
  }

  async getRequirement(id) {
    return this.call("requirements_get", { id: String(id) });
  }

  async updateRequirement(id, patch = {}) {
    return this.call("requirements_update", {
      id: String(id),
      ...patch,
    });
  }

  async toggleChecklist(id, itemId, checked = true) {
    return this.call("requirements_checklist_toggle", {
      id: String(id),
      item_id: String(itemId),
      checked,
    });
  }

  async addComment(id, text) {
    return this.call("requirements_comment", {
      id: String(id),
      text: String(text),
    });
  }

  async getEvents({ requirementId = null, limit = 50, cursor = null } = {}) {
    const args = { limit };
    if (requirementId) args.requirement_id = String(requirementId);
    if (cursor) args.cursor = cursor;
    return this.call("requirements_events", args);
  }

  // --- Node Presence & Task Dispatching Tools ---

  async reportStatus({ agent = "agent-fleet", status = "online", currentTask = null, queueDepth = 0 } = {}) {
    return this.call("report_status", {
      agent,
      status,
      current_task: currentTask,
      queue_depth: queueDepth,
    });
  }

  async getAllStatus() {
    return this.call("get_all_status", {});
  }

  async sendTask({ target, text, replyTo = null, timeoutSeconds = 300 }) {
    return this.call("send_task", {
      target,
      text,
      reply_to: replyTo,
      timeout_seconds: timeoutSeconds,
    });
  }

  async sendReply({ taskId, reply, status = "replied" }) {
    return this.call("send_reply", {
      task_id: taskId,
      reply,
      status,
    });
  }

  // --- SSE Doorbell (/events/<alias>) ---

  async subscribeEvents(alias, onEvent, { signal = null } = {}) {
    const url = `${this.hub}/events/${encodeURIComponent(alias)}`;
    const res = await this.fetch(url, {
      method: "GET",
      headers: this.getHeaders({ Accept: "text/event-stream" }),
      signal,
    });

    if (!res.ok) {
      throw new Error(`SSE subscribe HTTP ${res.status}`);
    }

    if (!res.body) {
      throw new Error("No response body for SSE stream");
    }

    const reader = res.body.getReader();
    const decoder = new TextDecoder();
    let buffer = "";

    try {
      while (!signal?.aborted) {
        const { value, done } = await reader.read();
        if (done) break;
        buffer += decoder.decode(value, { stream: true });
        const lines = buffer.split("\n\n");
        buffer = lines.pop() || "";
        for (const block of lines) {
          const dataLine = block.split("\n").find((l) => l.startsWith("data:"));
          if (dataLine) {
            try {
              const data = JSON.parse(dataLine.slice(5).trim());
              onEvent(data);
            } catch {
              /* ignore parse error on keepalive or heartbeat comments */
            }
          }
        }
      }
    } finally {
      reader.releaseLock();
    }
  }
}
