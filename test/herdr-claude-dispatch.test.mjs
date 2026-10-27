import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const read = (p) => readFileSync(new URL('../' + p, import.meta.url), 'utf8');
const fleet = JSON.parse(read('fleet.example.json'));
const herdr = fleet.machines[0].instances.find((i) => i.id === 'herdr-laptop').context;
const codex = fleet.machines[0].instances.find((i) => i.id === 'codex-laptop').context;
const agentLine = herdr.slice(herdr.indexOf('- agent 命令：'), herdr.indexOf('\n- 读结果'));
const claude = agentLine.slice(agentLine.indexOf('Claude Code：'), agentLine.indexOf('agy：'));
const agy = agentLine.slice(agentLine.indexOf('agy：'), agentLine.indexOf('dsh headless：'));
const adr = read('docs/decisions/0003-herdr-for-cli-agents.md');

const claudeCommand = 'claude -p --dangerously-skip-permissions --model opus --effort medium --output-format json < "${D}/brief-${n}.md" > "${D}/result-${n}.json" 2> "${D}/stderr-${n}.log"';

test('Herdr Claude dispatch uses official opus and medium effort', () => {
  assert.match(claude, new RegExp(claudeCommand.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')));
  assert.match(claude, /追问保留 --model opus --effort medium 并加 --resume <session_id>/);
  assert.match(claude, /--model sonnet --effort medium/);
  assert.doesNotMatch(claude, /deepseek|3425|minimax|magpie|ANTHROPIC_BASE_URL|exit 97/i);
  assert.doesNotMatch(herdr, /deepseek\/deepseek-flash|127\.0\.0\.1:3425|minimax-code/);
  assert.doesNotMatch(adr, /deepseek\/deepseek-flash|127\.0\.0\.1:3425|minimax-code|ANTHROPIC_BASE_URL/);
  assert.match(adr, /--model opus --effort medium/);
  assert.match(adr, /--resume <session_id>/);
  assert.match(adr, /--model sonnet/);
});

test('Codex Desktop, Codex CLI, and agy command lines stay as they were', () => {
  assert.match(codex, /model=gpt-6-astra，modelProvider=magpie/);
  assert.match(codex, /effort=medium/);
  assert.match(herdr, /const agent=\(D\)=>`codex exec --json -o "\$\{D\}\/result-\$\{n\}\.md" -C "\$PWD" -s danger-full-access -m <模型> -c model_provider="magpie"/);
  assert.match(agentLine, /Codex 见脚本，追问在 model_provider="magpie" 后加 resume <thread_id>/);
  assert.equal(agy, 'agy：agy -p "$(cat "${D}/brief-${n}.md")" --output-format json --dangerously-skip-permissions --model <模型> > "${D}/result-${n}.json" 2> "${D}/stderr-${n}.log"，追问加 --conversation <conversation_id>，桌面版看不到这类会话。');
  assert.doesNotMatch(agy, /magpie/);
  assert.match(adr, /-c model_provider="magpie"/);
});
