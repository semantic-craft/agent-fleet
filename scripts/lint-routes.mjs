import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { resolve } from 'node:path';
import { routeTable } from './caller-routes.mjs';

const LEGACY_ROUTING = 'docs/legacy/caller-routing.md';
const LEGACY_HANDOFF = 'docs/legacy/codex-desktop-handoff.md';

export function lintRoutes(read) {
  const errors = [];
  if (!read(LEGACY_ROUTING).includes('native-thread-selection.md')) errors.push(`${LEGACY_ROUTING}:explicit_selection_entry_missing: use docs/native-thread-selection.md`);
  const canonical = read(LEGACY_ROUTING).split('<!-- caller-routes:start -->\n')[1]?.split('\n<!-- caller-routes:end -->')[0];
  if (canonical !== routeTable()) errors.push('caller_route_table_drift');
  const contradictory = /远程通路只有 Agenvo|所有远程派活只走 Agenvo|都在 ChatGPT 里用自然语言下达|每个任务截两张图：ChatGPT/;
  for (const path of ['README.md','docs/new-mac.md','docs/runbook.md','prompts/operator.md','prompts/acceptance-codex.md']) {
    const text = read(path);
    if (!text.includes('caller-routing.md')) errors.push(`${path}:routing_reference_missing`);
    if (contradictory.test(text)) errors.push(`${path}:contradictory_route`);
  }
  // Root AGENTS.md is the cross-repo canon, not a caller-routing copy.
  if (contradictory.test(read('AGENTS.md'))) errors.push('AGENTS.md:contradictory_route');
  const blanketWriter = /writer 未验通只阻断这条路线|──待验 writer handoff──|在创建前验证原生 ownership 路径|Desktop 未见可见会话不算执行|→ 按已验证原生 ownership 路径交接→|当前未验证，报告 `blocked_native_writer_release` 并停止，不能先调|在 `thread\/start` 之前，必须已有/;
  const blanketUnsubscribe = /创建前门槛[^\n。]*thread\/unsubscribe|^目标 connector 必须暴露 `thread\/unsubscribe`/m;
  for (const path of ['README.md','prompts/operator.md','lessons.md','docs/codex-desktop-handoff.md','docs/architecture.md','docs/runbook.md','docs/new-mac.md','prompts/acceptance-codex.md', LEGACY_HANDOFF]) {
    const text = read(path);
    if (blanketWriter.test(text)) errors.push(`${path}:blanket_writer_gate`);
    if (blanketUnsubscribe.test(text)) errors.push(`${path}:blanket_unsubscribe_gate`);
  }
  // Unsubscribe and same-host baseline stay in the archive. Current docs point there instead of repeating the procedure.
  const handoff = read(LEGACY_HANDOFF);
  if (!handoff.includes('独立同 host 执行基线') || !handoff.includes('不以 `thread/unsubscribe` 或 watcher 交接检查作为创建前置')) errors.push(`${LEGACY_HANDOFF}:same_host_baseline_missing`);
  return errors;
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const errors = lintRoutes(path => readFileSync(new URL('../'+path, import.meta.url), 'utf8'));
  if (errors.length) { console.error(errors.join('\n')); process.exitCode = 1; }
  else console.log('caller routing: OK');
}
