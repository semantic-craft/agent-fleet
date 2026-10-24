/** Bounded caller-side policy over native DSH endpoints; no credentials or scheduler. */
import { createHash } from 'node:crypto';
const clone = x => JSON.parse(JSON.stringify(x));
const freeze = x => { if (x && typeof x === 'object') { Object.values(x).forEach(freeze); Object.freeze(x); } return x; };
const digest = x => createHash('sha256').update(JSON.stringify(x)).digest('hex');
const fault = code => Object.assign(new Error(code), { code });
export const HARNESS_VERSION = 'bounded-dsh-v2';
export class TaskSpec {
  constructor({ objective, acceptance, sideEffects = 'read-only', budget = {}, requiredCapabilities = [] }) {
    if (typeof objective !== 'string' || !objective.trim()) throw fault('objective_required');
    if (!['read-only', 'sandbox', 'write'].includes(sideEffects)) throw fault('invalid_side_effects');
    if (!acceptance || !['exact-text', 'nonempty-text'].includes(acceptance.kind) || (acceptance.kind === 'exact-text' && typeof acceptance.text !== 'string')) throw fault('acceptance_required');
    const limits = { timeoutMs: 600000, maxCalls: 256, maxPages: 128, maxPolls: 60, pollMs: 10000, readRetries: 2, ...budget };
    if (Object.values(limits).some(v => !Number.isSafeInteger(v) || v < 0) || !limits.timeoutMs || !limits.maxCalls) throw fault('invalid_budget');
    Object.assign(this, clone({ objective, acceptance, sideEffects, budget: limits, requiredCapabilities })); freeze(this);
  }
}
export class RunContext {
  constructor({ requestId, sessionId, harnessVersion = HARNESS_VERSION, lessonsVersion, phase = 'production', expected = {} }) {
    if (!requestId || !sessionId || !lessonsVersion || harnessVersion !== HARNESS_VERSION || !['discovery', 'heldout', 'production'].includes(phase)) throw fault('invalid_frozen_context');
    Object.assign(this, clone({ requestId, sessionId, harnessVersion, lessonsVersion, phase, expected })); freeze(this);
  }
}
export class RunResult {
  constructor(value) { Object.assign(this, clone(value)); freeze(this); }
}
export class ProviderAdapter {
  constructor() { this.capabilities = Object.freeze(['observe', 'submit']); }
}
export class DshProviderAdapter extends ProviderAdapter {
  #call;
  constructor(call) { super(); this.#call = call; }
  async invoke(method, request) {
    const envelope = await this.#call(method, method === 'session/modelCatalog' ? {} : { request });
    if (envelope?.error) {
      // Never expose remote errors: they may contain headers or private prompt content.
      const status = envelope.error.status ?? envelope.error.statusCode;
      throw Object.assign(fault('remote_error'), { status, remoteCode: envelope.error.code });
    }
    if (!envelope || !Object.hasOwn(envelope, 'result')) throw fault('invalid_envelope');
    return envelope.result;
  }
}
export class BaseHarness {
  constructor() { this.id = 'base'; Object.freeze(this); }
  policy(task) { return { objective: task.objective, requireTool: false }; }
  verify(task, evidence) { return task.acceptance.kind === 'exact-text' ? evidence.text === task.acceptance.text : evidence.text.trim().length > 0; }
}
export class ReadOnlyVerificationHarness extends BaseHarness {
  policy(task) {
    if (task.sideEffects !== 'read-only') throw fault('verification_requires_read_only');
    return { objective: `${task.objective}\nUse only bounded read-only inspection. Report the requested evidence; do not modify state.`, requireTool: true };
  }
  verify(task, evidence) { return evidence.toolEvents > 0 && super.verify(task, evidence); }
}
// Frozen classes are selected as complete policies. No live evolution or ancestor agent calls.
export function selectHarness({ task, context, specialist = false, baseResult }) {
  if (baseResult?.status === 'success') return { reuse: baseResult };
  if (baseResult && (context.phase === 'production' || !['read-only', 'sandbox'].includes(task.sideEffects))) throw fault('fallback_replay_forbidden');
  return { harness: specialist ? new ReadOnlyVerificationHarness() : new BaseHarness() };
}
const systemSources = new Set(['agent-instructions', 'runtime-context', 'skill-catalog', 'time-context']);
// Verified against native user-approval.setPolicy: only the precise read-only transition.
function readOnlyPolicyNotice(event, events, own) {
  const data = event.data;
  if (event.type !== 'user/message' || event.seq >= own.seq || data?.role !== 'user' || data?.source?.kind !== 'user-approval' || Object.keys(data.source).length !== 1) return false;
  const content = data.content;
  if (!Array.isArray(content) || content.length !== 1 || Object.keys(content[0]).length !== 2 || content[0].type !== 'text' || content[0].text !== 'The approval policy changed from "never" to "ask" (changed by the user).') return false;
  const changes = events.filter(e => ['permission/preset', 'sandbox/mode', 'approval/policy'].includes(e.type));
  const tail = changes.slice(-3);
  if (tail.length !== 3) return false;
  const [preset, sandbox, approval] = tail;
  return preset.type === 'permission/preset' && preset.data?.preset === 'read-only'
    && sandbox.type === 'sandbox/mode' && sandbox.data?.mode === 'read-only' && sandbox.seq === preset.seq + 1
    && approval.type === 'approval/policy' && approval.data?.policy === 'ask' && approval.seq === sandbox.seq + 1 && approval.seq < event.seq
    && events.filter(e => e.type === 'user/message' && e.data?.source?.kind === 'user-approval').length === 1;
}
function correlate(events, context) {
  const own = events.find(e => e.type === 'user/message' && e.data?.source?.rpcId === context.requestId);
  if (!own) return null;
  const header = events.find(e => e.seq > own.seq && e.type === 'request/header');
  if (!header) return null;
  const start = events.filter(e => e.seq < header.seq && e.type === 'turn/start').at(-1);
  if (!start || !Number.isSafeInteger(start.data?.turn)) throw fault('missing_turn_identity');
  const turn = start.data.turn;
  const end = events.find(e => e.seq > header.seq && e.type === 'turn/end' && e.data?.turn === turn);
  if (!end) return null;
  const range = events.filter(e => e.seq >= start.seq && e.seq <= end.seq);
  if (range.some(e => e.type === 'user/message' && e.seq !== own.seq && !systemSources.has(e.data?.source?.kind) && !readOnlyPolicyNotice(e, events.filter(x => x.seq <= end.seq), own))) throw fault('foreign_input');
  if (range.some(e => e.type === 'turn/start' && e.seq !== start.seq)) throw fault('ambiguous_turn');
  const headers = range.filter(e => e.type === 'request/header');
  for (const h of headers) {
    const actual = h.data?.header?.config;
    for (const key of ['provider', 'model', 'reasoningEffort']) if (context.expected[key] !== undefined && actual?.[key] !== context.expected[key]) throw fault('model_drift');
  }
  const text = range.filter(e => e.type === 'assistant/message' && e.data?.turn === turn)
    .flatMap(e => e.data?.message?.content ?? []).filter(c => c.type === 'text').map(c => c.text).join('\n');
  return { turn, inputSeq: own.seq, endSeq: end.seq, reason: end.data?.reason?.kind, text,
    toolEvents: range.filter(e => e.type.startsWith('tool/')).length, headers: headers.length,
    models: headers.map(h => Object.fromEntries(['provider', 'model', 'reasoningEffort'].map(k => [k, typeof h.data?.header?.config?.[k] === 'string' ? h.data.header.config[k].slice(0, 160) : null]))) };
}
/** Journal is caller-owned and must survive retry; use durable storage before process restart.
 * Entry values contain hashes/sequence evidence, never prompts or authentication. */
const journalOwners = new WeakMap();
export class HarnessRunner {
  #adapter; #journal; #now; #sleep; #active = new Set();
  constructor(adapter, { journal = new Map(), now = Date.now, sleep = ms => new Promise(r => setTimeout(r, ms)) } = {}) {
    this.#adapter = adapter; this.#journal = journal; this.#now = now; this.#sleep = sleep;
    if (!journalOwners.has(journal)) journalOwners.set(journal, new Set());
    this.#active = journalOwners.get(journal);
  }
  async run(task, context, options = {}) { return this.#execute(task, context, options, false); }
  async prepareExternalRun(task, context, options = {}) {
    const prepared = await this.#execute(task, context, options, true);
    return prepared.result ? prepared : freeze({ result: prepared, dispatchAllowed: false });
  }
  async #execute(task, context, { harness = new BaseHarness(), signal, requestCancellation = false, exclusiveSession = false } = {}, prepareExternal = false) {
    const key = `${context.sessionId}:${context.requestId}`;
    const fingerprint = digest({ task, context, policy: harness.constructor.name });
    const result = (status, code, extra = {}) => new RunResult({ status, code, requestId: context.requestId, sessionId: context.sessionId, harnessVersion: context.harnessVersion, lessonsVersion: context.lessonsVersion, phase: context.phase, accepted: entry?.accepted === true, completed: false, ...extra });
    let entry = this.#journal.get(key);
    if (entry && entry.fingerprint !== fingerprint) return result('blocked', 'idempotency_conflict');
    if (this.#active.has(key)) return result('unknown', 'observation_in_progress');
    if (entry?.final) return entry.final;
    if (requestCancellation && !exclusiveSession) return result('blocked', 'exclusive_session_required');
    if (requestCancellation && !entry?.attempted) return result('blocked', 'no_submitted_request');
    this.#active.add(key);
    let calls = 0;
    const deadline = this.#now() + task.budget.timeoutMs;
    const guard = () => { if (signal?.aborted) throw fault('observation_aborted'); if (this.#now() >= deadline) throw fault('deadline');  };
    const bounded = async operation => {
      guard();
      let timer, abort;
      try {
        const value = await Promise.race([operation(), new Promise((_, reject) => {
          timer = setTimeout(() => reject(fault('deadline')), Math.max(1, deadline - this.#now()));
          abort = () => reject(fault('observation_aborted'));
          signal?.addEventListener('abort', abort, { once: true });
        })]);
        guard();
        return value;
      } finally { clearTimeout(timer); signal?.removeEventListener('abort', abort); }
    };
    const invoke = async (method, request, read = true) => {
      for (let attempt = 0; ; attempt++) {
        guard(); if (calls >= task.budget.maxCalls) throw fault('call_budget'); calls++;
        try { return await bounded(() => this.#adapter.invoke(method, request)); }
        catch (e) {
          if (!read || attempt >= task.budget.readRetries || !(e.status === 429 || (e.status >= 500 && e.status <= 599))) throw e;
          await bounded(() => this.#sleep(Math.min(2000, 100 * 2 ** attempt)));
        }
      }
    };
    const address = { kind: 'session', sessionId: context.sessionId };
    try {
      if (!(task instanceof TaskSpec) || !(context instanceof RunContext) || ![BaseHarness.prototype, ReadOnlyVerificationHarness.prototype].includes(Object.getPrototypeOf(harness))) throw fault('unreviewed_policy');
      if (task.requiredCapabilities.some(c => !this.#adapter.capabilities.includes(c))) throw fault('unsupported_capability');
      const policy = harness.policy(task);
      const inspectConfiguration = async () => {
        const projection = await invoke('session/projections', { sessionId: context.sessionId });
        const values = projection?.values;
        if (!values) throw fault('configuration_unavailable');
        let selection = values.modelSelection?.next ?? values.modelSelection?.lastUsed;
        if (values.modelSelection?.next === null && values.modelSelection?.lastUsed === null) {
          const catalog = await invoke('session/modelCatalog', {});
          selection = catalog?.default;
          if (!selection) throw fault('configuration_unavailable');
        }
        for (const k of ['provider', 'model', 'reasoningEffort']) if (context.expected[k] !== undefined && selection?.[k] !== context.expected[k]) throw fault('model_drift');
        if (context.expected.preset !== undefined && values.agentPreset !== context.expected.preset) throw fault('preset_drift');
        const normalized = selection ? Object.fromEntries(['provider', 'model', 'reasoningEffort'].map(k => [k, selection[k] ?? null])) : null;
        const fingerprint = digest({ preset: values.agentPreset ?? null, selection: normalized });
        if (entry?.configuration && entry.configuration !== fingerprint) throw fault('configuration_drift');
        if (context.expected.configurationFingerprint && context.expected.configurationFingerprint !== fingerprint) throw fault('configuration_drift');
        return fingerprint;
      };
      const configuration = await inspectConfiguration();
      if (!entry) {
        const first = await invoke('session/follow', { address, maxMessages: 1 });
        if (!Number.isSafeInteger(first.cursor)) throw fault('invalid_cursor');
        entry = { fingerprint, base: first.cursor, attempted: false, accepted: false, configuration };
        this.#journal.set(key, entry);
      }
      if (prepareExternal) {
        if (requestCancellation || (entry.attempted && !entry.external)) return result('blocked', 'already_submitted_locally');
        const dispatchAllowed = !entry.attempted;
        guard();
        entry.attempted = true;
        entry.external = true;
        this.#journal.set(key, entry);
        return freeze({ result: result('prepared', 'external_submission_reserved'), dispatchAllowed,
          descriptor: { method: 'session/prompt', params: { request: { requestId: context.requestId, sessionId: context.sessionId, mode: 'queue', content: [{ type: 'text', text: policy.objective }] } } } });
      }
      if (!entry.attempted) {
        guard(); entry.attempted = true; this.#journal.set(key, entry);
        try {
          await invoke('session/prompt', { requestId: context.requestId, sessionId: context.sessionId, mode: 'queue', content: [{ type: 'text', text: policy.objective }] }, false);
          entry.accepted = true; this.#journal.set(key, entry);
        } catch { return result('unknown', 'submission_outcome_unknown'); }
      }
      for (let poll = 0; poll < task.budget.maxPolls; poll++) {
        const snap = await invoke('session/follow', { address, maxMessages: 1 });
        if (!Number.isSafeInteger(snap.cursor) || snap.cursor < entry.base) throw fault('cursor_regression');
        let before = snap.cursor + 1;
        const events = [];
        for (let page = 0; before > entry.base + 1; page++) {
          if (page >= task.budget.maxPages) throw fault('page_budget');
          const data = await invoke('session/page', { address, beforeSeq: before, throughSeq: snap.cursor, maxMessages: 1 });
          // maxMessages limits message windows, not administrative events within a window.
          const batch = (data.records ?? []).map(r => r.event).sort((a, b) => a.seq - b.seq);
          if (!batch.length || batch.some(e => !Number.isSafeInteger(e.seq)) || batch.at(-1).seq !== before - 1 || batch.some((e, i) => i > 0 && e.seq !== batch[i - 1].seq + 1)) throw fault('incomplete_history');
          events.push(...batch.filter(e => e.seq > entry.base).reverse());
          before = batch[0].seq;
        }
        events.reverse();
        if (events.some(e => e.type === 'user/message' && e.data?.source?.rpcId === context.requestId)) {
          entry.accepted = true; this.#journal.set(key, entry);
        }
        const evidence = correlate(events, context);
        if (!evidence && requestCancellation) {
          const own = events.find(e => e.type === 'user/message' && e.data?.source?.rpcId === context.requestId);
          const header = own && events.find(e => e.seq > own.seq && e.type === 'request/header');
          const start = header && events.filter(e => e.seq < header.seq && e.type === 'turn/start').at(-1);
          if (!own || !header || !start || !Number.isSafeInteger(start.data?.turn)) return result('blocked', 'queued_cancellation_unsupported');
          if (events.some(e => e.seq >= start.seq && e.seq !== own.seq && e.type === 'user/message' && !systemSources.has(e.data?.source?.kind) && !readOnlyPolicyNotice(e, events, own)) || events.some(e => e.seq > start.seq && e.type === 'turn/start')) return result('blocked', 'cancellation_identity_ambiguous');
          if (!entry.cancelAttempted) {
            entry.cancelAttempted = true; this.#journal.set(key, entry);
            try { await invoke('session/cancel', { sessionId: context.sessionId }, false); }
            catch { return result('unknown', 'cancellation_outcome_unknown'); }
          }
        }
        if (evidence) {
          await inspectConfiguration();
          entry.accepted = true;
          const { text, ...safe } = evidence;
          const successful = evidence.reason === 'completed' && harness.verify(task, evidence);
          guard();
          const status = successful ? 'success' : evidence.reason === 'cancelled' ? 'cancelled' : 'failed';
          const final = result(status, successful ? 'verified' : 'terminal_not_verified', { completed: evidence.reason === 'completed', evidence: { ...safe, outputHash: digest(text), outputLength: text.length } });
          entry.final = final; this.#journal.set(key, entry); return final;
        }
        await bounded(() => this.#sleep(task.budget.pollMs));
      }
      return result('timeout', 'poll_budget');
    } catch (e) {
      const code = ['observation_aborted', 'deadline', 'call_budget', 'page_budget', 'model_drift', 'foreign_input', 'ambiguous_turn', 'missing_turn_identity', 'unreviewed_policy', 'unsupported_capability', 'configuration_unavailable', 'configuration_drift', 'preset_drift', 'verification_requires_read_only', 'invalid_cursor', 'cursor_regression', 'incomplete_history'].includes(e.code) ? e.code : 'read_unavailable';
      return result(code === 'deadline' ? 'timeout' : code === 'observation_aborted' || code === 'read_unavailable' ? 'unknown' : 'blocked', code);
    } finally { this.#active.delete(key); }
  }
}

// Policy integrity against accidental monkey-patching, not an untrusted-JavaScript sandbox.
Object.freeze(BaseHarness.prototype);
Object.freeze(ReadOnlyVerificationHarness.prototype);
