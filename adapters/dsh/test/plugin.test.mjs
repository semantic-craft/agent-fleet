import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import plugin, { DEFAULTS, apply, name } from '../index.js';

const dir = path.join(import.meta.dirname, '..');

test('manifest, bundle patch and locale match the DSH plugin contract', () => {
  const pkg = JSON.parse(fs.readFileSync(path.join(dir, 'package.json'), 'utf8'));
  assert.equal(pkg.name, name);
  assert.equal(pkg.dsh.manifestVersion, 1);
  assert.equal(pkg.dsh.bundle.patch, './cordis.patch.yml');
  for (const f of pkg.files) assert.ok(fs.existsSync(path.join(dir, f)), f);
  assert.match(fs.readFileSync(path.join(dir, 'cordis.patch.yml'), 'utf8'), new RegExp(`name: ${name}\\b`));
  for (const l of ['en', 'zh-CN']) assert.ok(JSON.parse(fs.readFileSync(path.join(dir, 'locale', `${l}.json`), 'utf8')).meta.title);
  assert.equal(plugin.apply, apply);
});

test('apply only mounts the native bridge, under the agent-fleet-dsh state dir', () => {
  const effects = [];
  apply({ effect: (fn) => effects.push(fn), get: () => undefined, logger: () => ({}) }, {});
  assert.equal(effects.length, 1);
  assert.ok(DEFAULTS.stateDir.endsWith('/.local/state/agent-fleet-dsh'));
  const off = [];
  apply({ effect: (fn) => off.push(fn) }, { nativeBridge: false });
  assert.equal(off.length, 0);
});
