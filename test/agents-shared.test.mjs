import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const read = (path) => readFileSync(new URL('../' + path, import.meta.url), 'utf8');
const agenvoAsCanon = /agenvo.*正本|正本.*agenvo|agenvo 为/i;

test('root AGENTS.md is the shared cross-repo body', () => {
  const text = read('AGENTS.md');
  assert.match(text, /官方 Agent Network（ANet）Research/);
  assert.match(text, /docs\/anet\.md/);
  assert.match(text, /https:\/\/docs\.agentnetwork\.org\.cn/);
  assert.match(text, /历史遗留/);
  assert.match(text, /Hermes/);
  assert.match(text, /LCU 自定义 MCP/);
  assert.match(text, /CommHub/);
  assert.match(text, /anet\.sonori\.dev/);
  assert.match(text, /helios-codex/);
  assert.match(text, /worktree 不遗留（2026-10-29/);
  assert.match(text, /不改 `main` 的工作区/);
  assert.match(text, /archive\/<worktree名>/);
  assert.match(text, /git worktree remove/);
  assert.match(text, /DSH 是同等能力的后备/);
  assert.doesNotMatch(text, agenvoAsCanon);
  assert.doesNotMatch(text, /EGM/i);
  assert.doesNotMatch(text, /先读什么/);
});

test('docs/AGENTS.shared.md only points at the root body', () => {
  const text = read('docs/AGENTS.shared.md').trim();
  assert.match(text, /\.\.\/AGENTS\.md/);
  assert.ok(text.split('\n').length <= 2, text);
  assert.doesNotMatch(text, /Agent Network/);
  assert.doesNotMatch(text, agenvoAsCanon);
  assert.doesNotMatch(text, /EGM/i);
});
