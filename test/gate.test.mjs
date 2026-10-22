import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, writeFileSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

const root = fileURLToPath(new URL('../', import.meta.url));
const last = 'squash 时以本正文作为提交说明。';
function checkBody(t, body) {
  const dir = mkdtempSync(join(root, '.gate-test-'));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const file = join(dir, 'body.txt');
  writeFileSync(file, body);
  // Only system shell utilities are needed; gh and jq are not on this PATH.
  const result = spawnSync('/bin/bash', [join(root, 'scripts/gate.sh'), '--check-body', file], {
    encoding: 'utf8', env: { PATH: '/usr/bin:/bin' },
  });
  assert.ifError(result.error);
  assert.equal(result.stderr, '');
  t.diagnostic(`exit ${result.status}\n${result.stdout.trim()}`);
  return result;
}

test('正文含改为时失败', t => {
  const r = checkBody(t, `改为新的检查方式。\n${last}\n`);
  assert.equal(r.status, 1);
  assert.match(r.stdout, /FAIL  正文含墓碑词: 改为\(1行\)/);
  assert.match(r.stdout, /RESULT FAIL/);
});
test('合并之前只提示，不阻止通过', t => {
  const r = checkBody(t, `合并之前检查正文。\n${last}\n`);
  assert.equal(r.status, 0);
  assert.match(r.stdout, /INFO  正文含提示词: 之前\(1行\)/);
  assert.match(r.stdout, /RESULT PASS/);
});
test('干净正文通过', t => {
  const r = checkBody(t, `本次补充检查。\n${last}\n`);
  assert.equal(r.status, 0);
  assert.match(r.stdout, /PASS  正文无墓碑词/);
  assert.match(r.stdout, /RESULT PASS/);
});
test('末行错误时失败', t => {
  const r = checkBody(t, '本次补充检查。\n错误末行\n');
  assert.equal(r.status, 1);
  assert.match(r.stdout, /FAIL  正文最后一个非空行为「错误末行」/);
  assert.match(r.stdout, /RESULT FAIL/);
});
