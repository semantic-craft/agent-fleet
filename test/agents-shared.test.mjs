import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, lstatSync, readlinkSync } from 'node:fs';

const root = new URL('..', import.meta.url);
const read = (path) => readFileSync(new URL(path, root), 'utf8');
const agenvoAsCanon = /agenvo.*正本|正本.*agenvo|agenvo 为/i;

function assertSharedRules(text) {
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
}

test('docs/AGENTS.shared.md is the editable shared body', () => {
  assertSharedRules(read('docs/AGENTS.shared.md'));
});

test('root AGENTS.md symlinks to the shared body', () => {
  const path = new URL('AGENTS.md', root);
  assert.equal(lstatSync(path).isSymbolicLink(), true);
  assert.equal(readlinkSync(path), 'docs/AGENTS.shared.md');
  const viaRoot = read('AGENTS.md');
  assert.equal(viaRoot, read('docs/AGENTS.shared.md'));
  assertSharedRules(viaRoot);
});
