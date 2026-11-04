import test from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { workdir, readStatus } from '../scripts/anet-workdir.mjs';
import { hubState } from '../scripts/anet-hub-state.mjs';

const status = (dir = '/project') => JSON.stringify({ auto_reply: { work_dir: dir }, hub: { state: 'Registered', url: 'https://anet.sonori.dev' }, secret: 'DO_NOT_PRINT' });

test('get/assert read official work_dir, never cache cwd or bare autoreply', () => {
  const run = args => { assert.deepEqual(args, ['--id', 'turing', 'status', '--json']); return status(); };
  assert.equal(workdir(['--id', 'turing', 'get'], run), '/project');
  assert.equal(workdir(['assert', '--require', '/project/', '--id', 'turing'], run), '/project');
  assert.throws(() => workdir(['get', '--require', '/other'], () => status()), /HOST_CWD_WRONG/);
  assert.throws(() => workdir(['get'], () => '{"cwd":"/project"}'), /ABSOLUTE_WORK_DIR_REQUIRED/);
});

test('set writes selected identity then verifies readback; stale state fails', () => {
  const calls = [];
  const run = args => { calls.push(args); return status(); };
  assert.equal(workdir(['--id', 'agy', 'set', '/project'], run), '/project');
  assert.deepEqual(calls, [['--id', 'agy', 'autoreply', 'set', '--work-dir', '/project'], ['--id', 'agy', 'status', '--json']]);
  assert.throws(() => workdir(['set', '/other'], run), /HOST_CWD_WRONG/);
  assert.throws(() => workdir(['set', 'relative'], () => assert.fail('must not run')), /ABSOLUTE/);
});

test('invalid arguments and daemon errors fail without diagnostic/secret output', () => {
  for (const args of [['assert'], ['get', '--wat'], ['set'], ['--id'], ['set', '/project', 'extra']]) {
    assert.throws(() => workdir(args, () => assert.fail('must not run')));
  }
  for (const run of [() => 'DO_NOT_PRINT', () => { throw new Error('DO_NOT_PRINT'); }]) {
    assert.throws(() => readStatus([], run), { message: 'ANET_STATUS_UNAVAILABLE' });
  }
  const result = spawnSync(process.execPath, ['scripts/anet-workdir.mjs', 'assert'], { encoding: 'utf8' });
  assert.equal(result.status, 1);
  assert.equal(result.stdout, '');
});

test('Hub only prints registered origin; rejects unsafe URLs and states', () => {
  assert.equal(hubState([], () => status()), 'https://anet.sonori.dev');
  for (const hub of [{ state: 'offline' }, { state: 'registered', url: 'https://user:secret@example.org' }, { state: 'registered', url: 'https://example.org?token=secret' }]) {
    assert.throws(() => hubState([], () => JSON.stringify({ hub })));
  }
});

test('current docs distinguish Sonori Hub from retired control-plane routes', () => {
  for (const file of ['README.md', 'docs/anet.md', 'docs/architecture.md']) {
    const text = readFileSync(file, 'utf8');
    assert.match(text, /https:\/\/anet\.sonori\.dev/);
    assert.match(text, /Reason Network/);
    assert.match(text, /自有舰队编排引擎仓/);
    assert.doesNotMatch(text, /不经 `anet\.sonori\.dev`|CommHub MCP、`anet\.sonori\.dev`/);
  }
});
