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
  return errors;
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const errors = lintRoutes(path => readFileSync(new URL('../'+path, import.meta.url), 'utf8'));
  if (errors.length) { console.error(errors.join('\n')); process.exitCode = 1; }
  else console.log('caller routing: OK');
}
