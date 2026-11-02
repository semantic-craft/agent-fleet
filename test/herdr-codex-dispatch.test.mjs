import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const read = (p) => readFileSync(new URL('../' + p, import.meta.url), 'utf8');
const fleet = JSON.parse(read('fleet.example.json'));
const contexts = fleet.machines[0].instances.map((i) => i.context);
const adr = read('docs/decisions/0003-herdr-for-cli-agents.md');

test('fleet example instance contexts are anet placeholders without model params', () => {
  assert.ok(contexts.length >= 3);
  for (const context of contexts) {
    assert.match(context, /docs\/anet\.md/);
    assert.doesNotMatch(context, /gpt-6-astra|modelProvider|model_provider|gemini-4-argon|magpie|codex exec/);
  }
  assert.match(adr, /-c model_provider="magpie"/);
});
