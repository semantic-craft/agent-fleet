import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const read = (p) => readFileSync(new URL('../' + p, import.meta.url), 'utf8');
const fleet = JSON.parse(read('fleet.example.json'));
const herdr = fleet.machines[0].instances.find((i) => i.id === 'herdr-laptop').context;
const codex = fleet.machines[0].instances.find((i) => i.id === 'codex-laptop').context;
const agentLine = herdr.slice(herdr.indexOf('- agent 命令：'), herdr.indexOf('\n- 读结果'));
const adr = read('docs/decisions/0003-herdr-for-cli-agents.md');

test('Codex Desktop and explicit Codex CLI retain their routing parameters', () => {
  assert.match(codex, /model=gpt-6-astra，modelProvider=magpie/);
  assert.match(codex, /effort=medium/);
  assert.match(herdr, /const agent=\(D\)=>`codex exec --json -o "\$\{D\}\/result-\$\{n\}\.md" -C "\$PWD" -s danger-full-access -m <模型> -c model_provider="magpie"/);
  assert.match(agentLine, /Codex 见脚本，追问在 model_provider="magpie" 后加 resume <thread_id>/);
  assert.match(adr, /-c model_provider="magpie"/);
});
