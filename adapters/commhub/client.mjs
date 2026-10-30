// CommHub Streamable HTTP MCP Client & Doorbell Listener (#128, #129, #131, #132)
import { readFileSync } from "node:fs";
import { hostname } from "node:os";
import { randomUUID } from "node:crypto";

export const PROTOCOL = "2025-03-26";
export const REPLY_MAX_CHARS = 10_000;

export function truncateReply(text) {
  const s = String(text ?? "");
  if (s.length <= REPLY_MAX_CHARS) return s;
  const note = "\n\n[agent-fleet: reply truncated to the hub's 10000-character limit]";
  return s.slice(0, REPLY_MAX_CHARS - note.length) + note;
}

function parseEnvelope(raw) {
  try {
    return JSON.parse(raw);
  } catch {
    /* SSE-framed body */
  }
  for (const line of (raw || "").split(/\r?\n/)) {
    const m = line.match(/^data:\s*(\{.*\})\s*$/);
    if (m) return JSON.parse(m[1]);
  }
  return null;
}

export class CommHubClient {
  constructor({
    hub = process.env.ANET_HUB || "http://127.0.0.1:9200",
    token = process.env.ANET_NODE_TOKEN || null,
    tokenFile = process.env.ANET_NODE_TOKEN_FILE || null,
    cfClientId = process.env.CF_ACCESS_CLIENT_ID || null,
    cfClientSecret = process.env.CF_ACCESS_CLIENT_SECRET || null,
    alias = null,
    resumeId = null,
    networkId = null,
    clientVersion = "agent-fleet",
    timeoutMs = 30_000,
    fetchImpl = globalThis.fetch,
  } = {}) {
    this.hub = hub.replace(/\/$/, "");
    this.token = token || (tokenFile ? readFileSync(tokenFile, "utf8").trim() : null);
    this.cfClientId = cfClientId;
    this.cfClientSecret = cfClientSecret;
    this.alias = alias;
    this.resumeId = resumeId;
    this.networkId = networkId;
    this.clientVersion = clientVersion;
    this.timeoutMs = timeoutMs;
    this.fetch = fetchImpl;
    this.sessionId = null;
    this.rpcId = 0;
  }

  getHeaders(extra = {}) {
    const headers = {
      "Content-Type": "application/json",
      "Accept": "application/json, text/event-stream",
      "MCP-Protocol-Version": PROTOCOL,
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
    if (this.sessionId) {
      headers["mcp-session-id"] = this.sessionId;
    }
    return headers;
  }

  async initialize() {
    this.sessionId = null;
    const res = await this.fetch(`${this.hub}/mcp`, {
      method: "POST",
      headers: this.getHeaders(),
      body: JSON.stringify({
        jsonrpc: "2.0",
        id: ++this.rpcId,
        method: "initialize",
        params: {
          protocolVersion: PROTOCOL,
          capabilities: {},
          clientInfo: {
            name: "agent-fleet",
            version: this.clientVersion ?? "0",
          },
        },
      }),
      signal: AbortSignal.timeout(this.timeoutMs),
    });

    if (!res.ok) {
      const errText = await res.text();
      throw new Error(`initialize: HTTP ${res.status}: ${errText.slice(0, 300)}`);
    }

    this.sessionId = res.headers?.get?.("mcp-session-id") ?? null;
    await res.text();
    return this.sessionId;
  }

  async callOnce(toolName, args) {
    const res = await this.fetch(`${this.hub}/mcp`, {
      method: "POST",
      headers: this.getHeaders(),
      body: JSON.stringify({
        jsonrpc: "2.0",
        id: ++this.rpcId,
        method: "tools/call",
        params: {
          name: toolName,
          arguments: {
            ...(this.networkId ? { network_id: this.networkId } : {}),
            ...args,
          },
        },
      }),
      signal: AbortSignal.timeout(this.timeoutMs),
    });
    const raw = await res.text();
    return { res, raw };
  }

  async call(toolName, args = {}) {
    if (!this.sessionId) {
      await this.initialize();
    }

    let { res, raw } = await this.callOnce(toolName, args);

    // Hub restart recovery: re-initialize once and retry on 400 or 404
    if (res.status === 404 || res.status === 400) {
      await this.initialize();
      ({ res, raw } = await this.callOnce(toolName, args));
    }

    if (res.status === 401) {
      throw new Error(`${toolName}: unauthorized (node token rejected)`);
    }

    const envelope = parseEnvelope(raw);
    if (!envelope) {
      throw new Error(`${toolName}: empty response (HTTP ${res.status})`);
    }

    if (envelope.error) {
      throw new Error(`MCP error ${envelope.error.code ?? ""}: ${envelope.error.message}`);
    }

    const text = envelope.result?.content?.[0]?.text;
    let payload = envelope.result;
    if (typeof text === "string") {
      try {
        payload = JSON.parse(text);
      } catch {
        payload = { ok: true, text };
      }
    }

    if (payload && payload.ok === false && payload.queued === true) {
      return { ...payload, ok: true, queued: true };
    }

    if (payload && payload.ok === false) {
      throw new Error(`${toolName}: ${payload.error ?? payload.message ?? "failed"}`);
    }

    return payload;
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
    status = null,
    networkId = null,
  }) {
    const args = { external_ref: externalRef, name };
    if (externalUrl !== null) args.external_url = externalUrl;
    if (description !== null) args.description = description;
    if (priority !== null) args.priority = priority;
    if (checklist !== null) args.checklist = checklist;
    const col = column ?? status;
    if (col !== null) args.status = col;
    if (networkId || this.networkId) args.network_id = networkId || this.networkId;
    return this.call("requirements_upsert_by_external_ref", args);
  }

  async getRequirement(id, { networkId = null } = {}) {
    const args = { id: String(id) };
    if (networkId || this.networkId) args.network_id = networkId || this.networkId;
    return this.call("requirements_get", args);
  }

  async updateRequirement(id, patch = {}, { networkId = null } = {}) {
    const args = { id: String(id), ...patch };
    if (args.column && !args.status) {
      args.status = args.column;
      delete args.column;
    }
    if (networkId || this.networkId) args.network_id = networkId || this.networkId;
    return this.call("requirements_update", args);
  }

  async toggleChecklist(id, itemId, doneOrChecked = true, { networkId = null } = {}) {
    const isDone = typeof doneOrChecked === "object"
      ? Boolean(doneOrChecked.done ?? doneOrChecked.checked ?? true)
      : Boolean(doneOrChecked);
    const args = {
      id: String(id),
      item_id: String(itemId),
      done: isDone,
    };
    if (networkId || this.networkId) args.network_id = networkId || this.networkId;
    return this.call("requirements_checklist_toggle", args);
  }

  async addComment(id, text, { networkId = null } = {}) {
    const args = {
      id: String(id),
      text: String(text),
    };
    if (networkId || this.networkId) args.network_id = networkId || this.networkId;
    return this.call("requirements_comment", args);
  }

  async getEvents({ requirementId = null, limit = 50, cursor = null, networkId = null } = {}) {
    const args = { limit: Number(limit) };
    if (requirementId) args.requirement_id = String(requirementId);
    if (cursor) args.cursor = cursor;
    if (networkId || this.networkId) args.network_id = networkId || this.networkId;
    return this.call("requirements_events", args);
  }

  // --- Node Presence & Task Dispatching Tools ---

  async reportStatus({
    alias = null,
    resumeId = null,
    status = "idle",
    task = null,
    agent = "agent-fleet",
    hostname: host = null,
    projectDir = null,
    version = null,
    networkId = null,
    ...rest
  } = {}) {
    let mappedStatus = status;
    if (status === "online") mappedStatus = "idle";
    else if (status === "busy") mappedStatus = "working";

    const destAlias = alias || this.alias || "agent-fleet";
    const resId = resumeId || this.resumeId || `fleet-${randomUUID()}`;
    const args = {
      resume_id: String(resId),
      alias: String(destAlias),
      status: mappedStatus,
      agent,
      hostname: host || hostname(),
      project_dir: projectDir || process.cwd(),
      version: version || this.clientVersion,
      ...rest,
    };
    if (task) args.task = String(task).slice(0, 200);
    if (networkId || this.networkId) args.network_id = networkId || this.networkId;
    return this.call("report_status", args);
  }

  async getAllStatus({ networkId = null } = {}) {
    const args = {};
    if (networkId || this.networkId) args.network_id = networkId || this.networkId;
    return this.call("get_all_status", args);
  }

  async sendTask({
    alias = null,
    target = null,
    task = null,
    text = null,
    priority = "normal",
    context = null,
    fromSession = null,
    ttlSeconds = null,
    timeoutSeconds = null,
    networkId = null,
    parentTaskId = null,
    meta = null,
    force = null,
  } = {}) {
    const destAlias = alias || target;
    const taskContent = task || text;
    if (!destAlias) throw new Error("sendTask requires alias (or target)");
    if (!taskContent) throw new Error("sendTask requires task (or text)");

    const args = {
      alias: String(destAlias),
      task: String(taskContent),
      priority,
    };
    if (context !== null && context !== undefined) args.context = String(context);
    if (fromSession !== null && fromSession !== undefined) args.from_session = String(fromSession);
    else if (this.alias) args.from_session = String(this.alias);
    const ttl = ttlSeconds ?? timeoutSeconds;
    if (ttl !== null && ttl !== undefined) args.ttl_seconds = Number(ttl);
    if (networkId || this.networkId) args.network_id = networkId || this.networkId;
    if (parentTaskId !== null && parentTaskId !== undefined) args.parent_task_id = String(parentTaskId);
    if (meta !== null && meta !== undefined) args.meta = meta;
    if (force !== null && force !== undefined) args.force = Boolean(force);
    return this.call("send_task", args);
  }

  async sendReply({
    inReplyTo = null,
    taskId = null,
    text = null,
    reply = null,
    status = "replied",
    fromSession = null,
    alias = null,
    networkId = null,
    meta = null,
    threadId = null,
    turnId = null,
  } = {}) {
    const id = inReplyTo ?? taskId;
    const content = text ?? reply;
    const safeText = truncateReply(content ?? "");
    const args = {
      text: safeText,
      status,
    };
    if (id !== null && id !== undefined) args.in_reply_to = String(id);
    if (fromSession !== null && fromSession !== undefined) args.from_session = String(fromSession);
    else if (this.alias) args.from_session = String(this.alias);
    if (alias !== null && alias !== undefined) args.alias = String(alias);
    if (networkId || this.networkId) args.network_id = networkId || this.networkId;
    if (meta !== null && meta !== undefined) args.meta = meta;
    if (threadId !== null && threadId !== undefined) args.thread_id = String(threadId);
    if (turnId !== null && turnId !== undefined) args.turn_id = String(turnId);
    return this.call("send_reply", args);
  }

  async getInbox({ alias = null, limit = 10, networkId = null, humanFirst = null } = {}) {
    const inboxAlias = alias || this.alias;
    if (!inboxAlias) throw new Error("getInbox requires alias");
    const args = { alias: String(inboxAlias), limit: Number(limit) };
    if (networkId || this.networkId) args.network_id = networkId || this.networkId;
    if (humanFirst !== null && humanFirst !== undefined) args.human_first = Boolean(humanFirst);
    return this.call("get_inbox", args);
  }

  async ackInbox({ alias = null, messageId, networkId = null } = {}) {
    const ackAlias = alias || this.alias;
    if (!ackAlias) throw new Error("ackInbox requires alias");
    if (!messageId) throw new Error("ackInbox requires messageId");
    const args = { alias: String(ackAlias), message_id: String(messageId) };
    if (networkId || this.networkId) args.network_id = networkId || this.networkId;
    return this.call("ack_inbox", args);
  }

  // --- Node Rules & Capabilities Tools (#131) ---

  async writeNodeRulesFile({ nodeId = null, alias = null, content, networkId = null }) {
    const args = { content: String(content) };
    if (nodeId) args.node_id = nodeId;
    if (alias) args.alias = alias;
    if (networkId || this.networkId) args.network_id = networkId || this.networkId;
    return this.call("write_node_rules_file", args);
  }

  async readNodeRulesFile({ nodeId = null, alias = null, networkId = null } = {}) {
    const args = {};
    if (nodeId) args.node_id = nodeId;
    if (alias) args.alias = alias;
    if (networkId || this.networkId) args.network_id = networkId || this.networkId;
    return this.call("read_node_rules_file", args);
  }

  // --- SSE Doorbell (/events/<alias>) ---

  async* events(alias, signal = null) {
    const url = `${this.hub}/events/${encodeURIComponent(alias)}`;
    const res = await this.fetch(url, {
      method: "GET",
      headers: this.getHeaders({ Accept: "text/event-stream" }),
      signal,
    });

    if (!res.ok || !res.body) {
      throw new Error(`events: HTTP ${res.status}`);
    }

    const reader = res.body.getReader();
    const decoder = new TextDecoder();
    let buffer = "";

    try {
      while (!signal?.aborted) {
        const { value, done } = await reader.read();
        if (done) return;
        buffer += decoder.decode(value, { stream: true });
        const lines = buffer.split(/\r?\n/);
        buffer = lines.pop() ?? "";
        for (const line of lines) {
          if (!line.startsWith("data: ")) continue;
          try {
            yield JSON.parse(line.slice(6));
          } catch {
            /* ignore malformed frame */
          }
        }
      }
    } finally {
      try {
        await reader.cancel();
      } catch {
        /* already closed */
      }
    }
  }

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
        const normalized = buffer.replace(/\r\n/g, "\n");
        const lines = normalized.split("\n\n");
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
