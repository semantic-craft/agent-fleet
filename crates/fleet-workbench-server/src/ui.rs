/// Self-contained Agents-Anywhere v2 + Agent-Fleet Unified Workbench Web UI.
/// Served at `/` when `--static-dir` is not supplied. Implements the `contracts/protocol/1.1`
/// WebSocket subscriber with a `33 ms` coalescing buffer, monotonic `updatedSeq` tracking,
/// `DeliveryTier::ThreadVisible` sidebar badges, and interactive notice approval cards.
pub const WORKBENCH_INDEX_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>Agent-Fleet Unified Workbench (v2)</title>
  <style>
    :root {
      --bg: #0b0f17;
      --panel: #131a26;
      --panel-alt: #192231;
      --border: #28354a;
      --text: #e6edf3;
      --muted: #8b949e;
      --accent: #4c9aff;
      --green: #2ea043;
      --green-bg: rgba(46, 160, 67, 0.16);
      --amber: #d29922;
      --amber-bg: rgba(210, 153, 34, 0.16);
      --purple: #a371f7;
      --purple-bg: rgba(163, 113, 247, 0.16);
      --mono: "SF Mono", "JetBrains Mono", Menlo, Consolas, monospace;
      --sans: -apple-system, BlinkMacSystemFont, "SF Pro Text", "Segoe UI", sans-serif;
    }
    * { box-sizing: border-box; }
    body {
      margin: 0;
      background: var(--bg);
      color: var(--text);
      font-family: var(--sans);
      font-size: 14px;
      line-height: 1.45;
    }
    header {
      display: flex;
      align-items: center;
      justify-content: space-between;
      padding: 12px 20px;
      background: var(--panel);
      border-bottom: 1px solid var(--border);
    }
    .brand {
      display: flex;
      align-items: center;
      gap: 12px;
      font-weight: 600;
      font-size: 16px;
    }
    .pill {
      display: inline-flex;
      align-items: center;
      gap: 6px;
      padding: 2px 9px;
      border-radius: 999px;
      font-size: 12px;
      font-family: var(--mono);
      font-weight: 500;
      border: 1px solid var(--border);
      background: var(--panel-alt);
    }
    .pill.green {
      color: #56d364;
      background: var(--green-bg);
      border-color: rgba(46, 160, 67, 0.45);
    }
    .pill.amber {
      color: #e3b341;
      background: var(--amber-bg);
      border-color: rgba(210, 153, 34, 0.45);
    }
    .pill.purple {
      color: #d2a8ff;
      background: var(--purple-bg);
      border-color: rgba(163, 113, 247, 0.45);
    }
    .layout {
      display: grid;
      grid-template-columns: 320px 1fr 340px;
      height: calc(100vh - 53px);
    }
    .col {
      border-right: 1px solid var(--border);
      overflow-y: auto;
      padding: 16px;
      display: flex;
      flex-direction: column;
      gap: 14px;
    }
    .col:last-child { border-right: none; }
    h2 {
      margin: 0;
      font-size: 12px;
      text-transform: uppercase;
      letter-spacing: 0.06em;
      color: var(--muted);
    }
    .card {
      background: var(--panel);
      border: 1px solid var(--border);
      border-radius: 8px;
      padding: 12px;
    }
    .provider-grid {
      display: flex;
      flex-direction: column;
      gap: 8px;
    }
    .provider-row {
      display: flex;
      align-items: center;
      justify-content: space-between;
      padding: 8px 10px;
      background: var(--panel-alt);
      border: 1px solid var(--border);
      border-radius: 6px;
    }
    .provider-title {
      font-weight: 600;
      font-size: 13px;
    }
    .provider-sub {
      font-size: 11px;
      color: var(--muted);
      font-family: var(--mono);
    }
    .session-item {
      padding: 10px;
      border-radius: 6px;
      border: 1px solid var(--border);
      background: var(--panel-alt);
      cursor: pointer;
      margin-bottom: 8px;
    }
    .session-item.active {
      border-color: var(--accent);
      box-shadow: 0 0 0 1px var(--accent) inset;
    }
    .timeline-list {
      display: flex;
      flex-direction: column;
      gap: 10px;
      flex: 1;
    }
    .timeline-item {
      padding: 12px 14px;
      border-radius: 8px;
      border: 1px solid var(--border);
      background: var(--panel);
    }
    .timeline-item.user {
      border-left: 3px solid var(--accent);
    }
    .timeline-item.assistant {
      border-left: 3px solid var(--green);
    }
    .item-meta {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 8px;
      margin-bottom: 6px;
      font-size: 11px;
      color: var(--muted);
      font-family: var(--mono);
    }
    .item-content {
      white-space: pre-wrap;
      word-break: break-word;
      font-size: 13.5px;
    }
    form {
      display: flex;
      flex-direction: column;
      gap: 8px;
    }
    input, textarea, button {
      font-family: inherit;
      font-size: 13px;
      border-radius: 6px;
      border: 1px solid var(--border);
      background: var(--panel-alt);
      color: var(--text);
      padding: 8px 10px;
    }
    button {
      background: var(--accent);
      color: #fff;
      border-color: transparent;
      font-weight: 600;
      cursor: pointer;
    }
    button.approve-btn {
      background: var(--green);
      margin-top: 8px;
    }
    .notice-card {
      border: 1px solid rgba(210, 153, 34, 0.55);
      background: rgba(210, 153, 34, 0.08);
      border-radius: 8px;
      padding: 12px;
      margin-bottom: 10px;
    }
  </style>
</head>
<body>
  <header>
    <div class="brand">
      <span>Agent-Fleet Unified Workbench</span>
      <span class="pill green" id="health-pill">v2 Ready</span>
      <span class="pill purple" id="connector-pill">Connector: waiting</span>
    </div>
    <div style="display:flex;gap:8px;align-items:center;">
      <span class="pill" id="coalesce-pill">WS Buffer: 33ms</span>
      <span class="pill green" id="seq-pill">cursor: seq:0</span>
    </div>
  </header>
  <div class="layout">
    <div class="col">
      <h2>Runtime Providers (6 Canonical Peers)</h2>
      <div class="provider-grid" id="providers-list"></div>

      <h2>Create &amp; Start Session</h2>
      <div class="card">
        <form id="create-session-form">
          <input id="target-input" name="target" value="zcode/demo-workbench" placeholder="host/slug (e.g. zcode/demo-workbench)" />
          <input id="title-input" name="title" value="Demo Workbench Session" placeholder="Session title" />
          <textarea id="prompt-input" name="prompt" rows="2" placeholder="Initial prompt">Verify unified Rust workbench timeline and sidebar mount</textarea>
          <button type="submit" id="create-session-btn">Create &amp; Mount Sidebar Session</button>
        </form>
      </div>

      <h2>Mounted Sessions</h2>
      <div id="sessions-list"></div>
    </div>

    <div class="col">
      <div style="display:flex;justify-content:space-between;align-items:center;">
        <h2 id="active-session-heading">Live Session Timeline</h2>
        <span class="pill" id="ws-status-pill">WS: idle</span>
      </div>
      <div class="timeline-list" id="timeline-list"></div>
      <div class="card">
        <form id="followup-form">
          <input id="followup-input" placeholder="Send human follow-up turn to active desktop thread..." />
          <button type="submit" id="followup-send-btn">Send Turn to Desktop Host</button>
        </form>
      </div>
    </div>

    <div class="col">
      <h2>Runtime Notices &amp; Approvals</h2>
      <div id="notices-list"></div>
      <h2>Telemetry &amp; Sequence Log</h2>
      <div class="card" style="font-family:var(--mono);font-size:11px;white-space:pre-wrap;" id="telemetry-log">coalesce_window_ms=33
dedup_durable_events=true</div>
    </div>
  </div>

  <script>
    const COALESCE_WINDOW_MS = 33;
    let activeSessionId = null;
    let activeWs = null;
    let highestUpdatedSeq = 0;
    const seenDurableEventIds = new Set();
    let coalesceQueue = [];
    let coalesceTimer = null;

    function logTelemetry(line) {
      const el = document.getElementById('telemetry-log');
      const lines = (el.textContent + '\n' + line).trim().split('\n').slice(-18);
      el.textContent = lines.join('\n');
    }

    async function refreshOverview() {
      try {
        const [pRes, sRes] = await Promise.all([
          fetch('/api/v2/providers'),
          fetch('/api/v2/sessions')
        ]);
        const pData = await pRes.json();
        const sData = await sRes.json();
        renderProviders(pData);
        renderSessions(sData.sessions || []);
      } catch (err) {
        logTelemetry('overview_err=' + err.message);
      }
    }

    function renderProviders(data) {
      const connPill = document.getElementById('connector-pill');
      const connectors = data.connectors || [];
      if (connectors.length > 0) {
        const c = connectors[0];
        connPill.textContent = `Connector: ${c.connectorId} (gen ${c.generation})`;
        connPill.className = 'pill green';
      }
      const list = document.getElementById('providers-list');
      const providers = data.providers || [];
      list.innerHTML = providers.map(p => {
        const ch = (p.capabilities && p.capabilities.dispatch_channel) || 'local';
        const pillClass = ch === 'anet' ? 'amber' : 'green';
        return `<div class="provider-row" data-provider="${p.provider_id}">
          <div>
            <div class="provider-title">${p.display_name} <span style="color:var(--muted);font-weight:400;">(${p.provider_id})</span></div>
            <div class="provider-sub">${p.bundle_id}</div>
          </div>
          <span class="pill ${pillClass}">${ch}</span>
        </div>`;
      }).join('');
    }

    function renderSessions(sessions) {
      const list = document.getElementById('sessions-list');
      if (!activeSessionId && sessions.length > 0) {
        selectSession(sessions[0].sessionId);
      }
      list.innerHTML = sessions.map(s => {
        const isActive = s.sessionId === activeSessionId ? 'active' : '';
        const tier = s.deliveryTier || 'thread-visible';
        return `<div class="session-item ${isActive}" data-session-id="${s.sessionId}" onclick="selectSession('${s.sessionId}')">
          <div style="display:flex;justify-content:space-between;align-items:center;">
            <strong>${s.runtime}/${s.projectSlug}</strong>
            <span class="pill green">${tier}</span>
          </div>
          <div class="provider-sub" style="margin-top:4px;">${s.title}</div>
          <div class="provider-sub">id: ${s.sessionId} | seq:${s.latestUpdatedSeq}</div>
        </div>`;
      }).join('');
    }

    async function selectSession(sessionId) {
      const changed = activeSessionId !== sessionId;
      activeSessionId = sessionId;
      document.getElementById('active-session-heading').textContent = `Timeline: ${sessionId}`;
      const snapRes = await fetch(`/api/v2/sessions/${encodeURIComponent(sessionId)}/snapshot`);
      if (snapRes.ok) {
        const snap = await snapRes.json();
        highestUpdatedSeq = Math.max(highestUpdatedSeq, snap.updatedSeq || 0);
        document.getElementById('seq-pill').textContent = `cursor: seq:${highestUpdatedSeq}`;
        renderTimelineItems(snap.items || []);
        renderNotices(snap.notices || []);
      }
      if (changed) {
        connectSessionWs(sessionId);
      }
    }

    function connectSessionWs(sessionId) {
      if (activeWs) {
        try { activeWs.close(); } catch (_) {}
      }
      const proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
      const url = `${proto}//${location.host}/api/v2/sessions/${encodeURIComponent(sessionId)}/ws`;
      const ws = new WebSocket(url);
      activeWs = ws;
      document.getElementById('ws-status-pill').textContent = 'WS: connecting';
      ws.onopen = () => {
        document.getElementById('ws-status-pill').textContent = 'WS: connected (1.1)';
        document.getElementById('ws-status-pill').className = 'pill green';
        logTelemetry(`ws_connected session=${sessionId}`);
      };
      ws.onmessage = (ev) => {
        try {
          const env = JSON.parse(ev.data);
          enqueueCoalescedEvent(env);
        } catch (e) {}
      };
      ws.onclose = () => {
        document.getElementById('ws-status-pill').textContent = 'WS: closed';
      };
    }

    function enqueueCoalescedEvent(env) {
      const isLiveState = env.type === 'runtime.capability.updated' || env.type === 'runtime.state.updated';
      if (!isLiveState && env.type && env.type.startsWith('timeline.item_')) {
        if (seenDurableEventIds.has(env.eventId)) {
          return;
        }
        seenDurableEventIds.add(env.eventId);
      }
      coalesceQueue.push(env);
      if (!coalesceTimer) {
        coalesceTimer = setTimeout(flushCoalescedEvents, COALESCE_WINDOW_MS);
      }
    }

    async function flushCoalescedEvents() {
      coalesceTimer = null;
      const batch = coalesceQueue.splice(0, coalesceQueue.length);
      if (batch.length === 0) return;
      for (const env of batch) {
        if (typeof env.updatedSeq === 'number' && env.updatedSeq > highestUpdatedSeq) {
          highestUpdatedSeq = env.updatedSeq;
        }
      }
      document.getElementById('seq-pill').textContent = `cursor: seq:${highestUpdatedSeq}`;
      logTelemetry(`coalesced_flush count=${batch.length} cursor=seq:${highestUpdatedSeq}`);
      if (activeSessionId) {
        await selectSession(activeSessionId);
      }
    }

    function renderTimelineItems(items) {
      const container = document.getElementById('timeline-list');
      container.innerHTML = items.map(item => {
        const roleClass = item.role === 'user' ? 'user' : 'assistant';
        const itemId = item.itemId ?? item.item_id ?? '';
        const orderSeq = item.orderSeq ?? item.order_seq ?? 0;
        const updatedSeq = item.updatedSeq ?? item.updated_seq ?? 0;
        const kind = item.kind ?? item.type ?? 'message';
        const tier = item.deliveryTier ?? item.delivery_tier ?? 'thread-visible';
        const peerSender = item.peerSender ?? item.peer_sender;
        const peerBadge = peerSender
          ? `<span class="pill purple">peer: ${peerSender}</span>`
          : '';
        return `<div class="timeline-item ${roleClass}" data-item-id="${itemId}" data-updated-seq="${updatedSeq}">
          <div class="item-meta">
            <span>#${orderSeq} [${item.role}/${kind}] ${peerBadge}</span>
            <span><span class="pill green">${tier}</span> seq:${updatedSeq}</span>
          </div>
          <div class="item-content">${escapeHtml(item.content || '')}</div>
        </div>`;
      }).join('');
    }

    function renderNotices(notices) {
      const container = document.getElementById('notices-list');
      if (notices.length === 0) {
        container.innerHTML = '<div class="card" style="color:var(--muted);">No pending notices</div>';
        return;
      }
      container.innerHTML = notices.map(n => {
        const noticeId = n.noticeId || n.notice_id;
        const status = n.status || 'open';
        const pillClass = status === 'resolved' ? 'green' : 'amber';
        const btn = status === 'open'
          ? `<button class="approve-btn" id="approve-notice-${noticeId}" onclick="respondNotice('${noticeId}', 'approved')">Approve (${n.kind})</button>`
          : '';
        return `<div class="notice-card" data-notice-id="${noticeId}" data-notice-status="${status}">
          <div style="display:flex;justify-content:space-between;align-items:center;">
            <strong>${escapeHtml(n.title || noticeId)}</strong>
            <span class="pill ${pillClass}" id="notice-status-${noticeId}">${status}</span>
          </div>
          <div style="margin-top:6px;font-size:13px;">${escapeHtml(n.prompt || '')}</div>
          ${btn}
        </div>`;
      }).join('');
    }

    async function respondNotice(noticeId, decision) {
      if (!activeSessionId) return;
      const res = await fetch(`/api/v2/sessions/${encodeURIComponent(activeSessionId)}/runtime/notices/${encodeURIComponent(noticeId)}/respond`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ decision })
      });
      if (res.ok) {
        const body = await res.json();
        logTelemetry(`notice_responded id=${noticeId} status=${body.status}`);
        await selectSession(activeSessionId);
      }
    }

    document.getElementById('create-session-form').addEventListener('submit', async (ev) => {
      ev.preventDefault();
      const target = document.getElementById('target-input').value.trim();
      const title = document.getElementById('title-input').value.trim();
      const prompt = document.getElementById('prompt-input').value.trim();
      const res = await fetch('/api/v2/sessions/create-and-start', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ target, title, prompt })
      });
      const data = await res.json();
      if (data.ok && data.sessionId) {
        logTelemetry(`created_session id=${data.sessionId} tier=${data.deliveryTier}`);
        await refreshOverview();
        await selectSession(data.sessionId);
      } else {
        logTelemetry(`create_rejected err=${data.error || 'unknown'}`);
      }
    });

    document.getElementById('followup-form').addEventListener('submit', async (ev) => {
      ev.preventDefault();
      if (!activeSessionId) return;
      const input = document.getElementById('followup-input');
      const prompt = input.value.trim();
      if (!prompt) return;
      input.value = '';
      const res = await fetch(`/api/v2/sessions/${encodeURIComponent(activeSessionId)}/messages`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ prompt, role: 'user' })
      });
      if (res.ok) {
        await selectSession(activeSessionId);
      }
    });

    function escapeHtml(str) {
      return str.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
    }

    refreshOverview();
    setInterval(refreshOverview, 1500);
  </script>
</body>
</html>
"#;
