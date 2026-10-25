import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { resolve } from 'node:path';
import { routeTable } from './caller-routes.mjs';
export function lintRoutes(read) {
  const errors = [];
  const canonical = read('docs/caller-routing.md').split('<!-- caller-routes:start -->\n')[1]?.split('\n<!-- caller-routes:end -->')[0];
  if (canonical !== routeTable()) errors.push('caller_route_table_drift');
  for (const path of ['README.md','AGENTS.md','docs/new-mac.md','docs/runbook.md','skills/delegate-via-agenvo/SKILL.md','prompts/operator.md','prompts/acceptance-codex.md']) {
    const text = read(path);
    if (!text.includes('caller-routing.md')) errors.push(`${path}:routing_reference_missing`);
    if (/远程通路只有 Agenvo|所有远程派活只走 Agenvo|都在 ChatGPT 里用自然语言下达|每个任务截两张图：ChatGPT/.test(text)) errors.push(`${path}:contradictory_route`);
  }
  for (const path of ['README.md','AGENTS.md','prompts/operator.md','lessons.md','docs/codex-desktop-handoff.md','docs/architecture.md','docs/runbook.md','docs/new-mac.md','skills/delegate-via-agenvo/SKILL.md','prompts/acceptance-codex.md']) {
    const text = read(path);
    if ((!text.includes('同 host') || !text.includes('跨 backend')) && !text.includes('thread/unsubscribe')) errors.push(`${path}:execution_topology_missing`);
    if (/writer 未验通只阻断这条路线|──待验 writer handoff──|在创建前验证原生 ownership 路径|Desktop 未见可见会话不算执行|→ 按已验证原生 ownership 路径交接→|当前未验证，报告 `blocked_native_writer_release` 并停止，不能先调|在 `thread\/start` 之前，必须已有/.test(text)) errors.push(`${path}:blanket_writer_gate`);
  }
  return errors;
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const errors = lintRoutes(path => readFileSync(new URL('../'+path, import.meta.url), 'utf8'));
  if (errors.length) { console.error(errors.join('\n')); process.exitCode = 1; }
  else console.log('caller routing: OK');
}
