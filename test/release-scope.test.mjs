import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync, rmSync, symlinkSync, readdirSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { treeHashes, rollbackPlan, releaseStateName } from '../scripts/release-lib.mjs';

const A = 'a'.repeat(40), B = 'b'.repeat(40), C = 'c'.repeat(40);
const dirs = ['codex-app-server', 'herdr', 'dsh', 'dispatch'];
const releaseURL = new URL('../scripts/release.mjs', import.meta.url).href;
const cli = new URL('../scripts/fleet.mjs', import.meta.url).pathname;
function fixture(t) {
  const home = mkdtempSync(join(tmpdir(), 'fleet-scope-'));
  t.after(() => rmSync(home, { recursive: true, force: true }));
  const root = join(home, '.local/share/agent-fleet/releases/agenvo');
  const agents = join(home, 'Library/LaunchAgents'), bin = join(home, 'bin'), calls = join(home, 'calls');
  mkdirSync(agents, { recursive: true }); mkdirSync(bin);
  const json = (p, value) => writeFileSync(p, JSON.stringify(value));
  for (const sha of [A,B,C]) {
    for (const d of dirs) {
      const folder = join(root, sha, 'apps', d); mkdirSync(folder, { recursive: true });
      writeFileSync(join(folder, 'cli.js'), '// fixture '+sha);
    }
    json(join(root, sha, 'release.json'), { files: treeHashes(join(root, sha)) });
  }
  const plist = (d) => join(agents, `io.agenvo.connector.${d}.plist`);
  const status = (d) => join(home, '.config/agenvo', d, 'status.json');
  for (const d of dirs) {
    json(plist(d), { Label: `io.agenvo.connector.${d}`, ProgramArguments: [process.execPath, join(root,A,'apps',d,'cli.js')] });
    mkdirSync(join(home,'.config/agenvo',d), {recursive:true}); json(status(d), {state:'online',pid:100});
  }
  // An unrelated symlink must never be recursively hashed by scoped release.
  symlinkSync(bin, join(agents, 'unrelated-directory'));
  const common = `import fs from 'node:fs'; import path from 'node:path'; const a=process.argv.slice(2);\n`;
  const executable = (name, body) => writeFileSync(join(bin,name), `#!${process.execPath}\n${common}${body}`, {mode:0o755});
  executable('git', `if(a.includes('remote')) console.log('origin fixture (fetch)'); else if(!a.includes('fetch')&&!a.includes('merge-base')) process.exit(90);`);
  executable('plutil', `const f=a.at(-1); const d=JSON.parse(fs.readFileSync(f)); if(a[0]==='-convert') console.log(JSON.stringify(d)); else if(a[0]==='-replace'){d.ProgramArguments=JSON.parse(a[3]);fs.writeFileSync(f,JSON.stringify(d));}else process.exit(90);`);
  executable('launchctl', `fs.appendFileSync(${JSON.stringify(calls)},JSON.stringify(a)+'\\n'); if(a[0]==='bootstrap'&&!fs.existsSync(path.join(${JSON.stringify(home)},'skip-online'))){const p=JSON.parse(fs.readFileSync(a[2]));const parts=p.ProgramArguments[1].split('/');const d=parts[parts.lastIndexOf('apps')+1];const s=path.join(${JSON.stringify(home)},'.config/agenvo',d,'status.json');const prev=JSON.parse(fs.readFileSync(s));fs.writeFileSync(s,JSON.stringify({state:'online',pid:prev.pid+1}));}`);
  const run = (opts) => spawnSync(process.execPath, ['--input-type=module','-e', `import {release} from ${JSON.stringify(releaseURL)}; await release('agenvo',${JSON.stringify({home,cfg:{agenvo:{checkout:home,remote:'fixture',branch:'main'}},...opts})});`], {env:{...process.env,PATH:bin},encoding:'utf8',stdio:['ignore','pipe','pipe']});
  return {home,root,agents,calls,plist,status,run};
}
const read = (p) => readFileSync(p,'utf8');
const success = (r) => assert.equal(r.status,0,r.stderr);

test('no-TTY Codex release and rollback touch only one connector and independent state', t => {
  const f=fixture(t), others=dirs.slice(1).flatMap(d=>[f.plist(d),f.status(d)]);
  const before=others.map(read), shared=join(f.root,'current.json'); writeFileSync(shared,'{"sentinel":true}');
  const r=f.run({sha:B,connector:'codex'}); success(r); t.diagnostic(r.stdout.trim());
  assert.deepEqual(others.map(read),before); assert.equal(read(shared),'{"sentinel":true}');
  const record=JSON.parse(read(join(f.root,'current-codex.json')));
  assert.equal(record.connector,'codex'); assert.deepEqual(readdirSync(record.backup),['io.agenvo.connector.codex-app-server.plist']);
  const calls=read(f.calls).trim().split('\n').map(JSON.parse);
  assert.equal(calls.length,2); assert.ok(calls.every(a=>a.at(-1).includes('codex-app-server')));
  success(f.run({rollback:true,connector:'codex'}));
  assert.ok(read(f.plist(dirs[0])).includes(A)); assert.deepEqual(others.map(read),before);
  assert.equal(read(shared),'{"sentinel":true}');
  assert.equal(read(f.calls).trim().split('\n').length,4);
});

for(const direction of ['scoped-after-full','full-after-scoped']) test(`stale rollback rejected without mutation: ${direction}`,t=>{
  const f=fixture(t);
  success(f.run({sha:B,...(direction==='full-after-scoped'?{connector:'codex'}:{})}));
  success(f.run({sha:C,...(direction==='scoped-after-full'?{connector:'codex'}:{})}));
  const paths=dirs.flatMap(d=>[f.plist(d),f.status(d)]), before=paths.map(read), calls=read(f.calls);
  const r=f.run({rollback:true,...(direction==='full-after-scoped'?{connector:'codex'}:{})});
  assert.notEqual(r.status,0); assert.match(r.stderr,/stale rollback/); assert.deepEqual(paths.map(read),before); assert.equal(read(f.calls),calls);
  t.diagnostic(`${direction}: ${r.stderr.split('\n').find(l=>l.startsWith('Error:'))}; no launchctl or plist mutation`);
});

test('scope and unsupported CLI combinations fail before config or production access',()=>{
  assert.equal(releaseStateName('agenvo','codex'),'current-codex.json');
  for(const args of [['release','dsh','--connector','codex'],['doctor','--connector','codex'],['release','agenvo','--connector','herdr'],['release','agenvo','--connector','codex','--rollback','--sha',B]]) {
    const r=spawnSync(process.execPath,[cli,...args],{encoding:'utf8',stdio:['ignore','pipe','pipe']});
    assert.notEqual(r.status,0); assert.match(r.stderr,/--connector|--rollback/);
  }
});

test('a full rollback record cannot be consumed by a scoped rollback',()=>{
  const root='/fixture/releases/agenvo', record={sha:B,previous:[{label:'example.connector',programArg:'/fixture/apps/codex/cli.js'}]};
  assert.throws(()=>rollbackPlan('agenvo','codex',record,[],[],root),/scope/);
});

 test('rollback health failure retains scoped retry record and retry verifies new PID',t=>{
  const f=fixture(t); success(f.run({sha:B,connector:'codex'}));
  writeFileSync(join(f.home,'skip-online'),'');
  const failed=f.run({rollback:true,connector:'codex',connectorTimeoutMs:1});
  assert.notEqual(failed.status,0); assert.match(failed.stderr,/connectors not online after restart/);
  assert.equal(JSON.parse(read(join(f.root,'current-codex.json'))).rollbackPending,true);
  rmSync(join(f.home,'skip-online'));
  success(f.run({rollback:true,connector:'codex'}));
  assert.equal(JSON.parse(read(join(f.root,'current-codex.json'))).sha,null);
});

test('rollback can restore a legacy checkout dist entry and verify its connector',t=>{
  const f=fixture(t), old=join(f.home,'checkout/apps/codex-app-server/dist/cli.js');
  mkdirSync(join(f.home,'checkout/apps/codex-app-server/dist'),{recursive:true}); writeFileSync(old,'// legacy');
  const p=JSON.parse(read(f.plist(dirs[0]))); p.ProgramArguments[1]=old; writeFileSync(f.plist(dirs[0]),JSON.stringify(p));
  success(f.run({sha:B,connector:'codex'}));
  const r=f.run({rollback:true,connector:'codex'}); success(r); assert.match(r.stdout,/codex-app-server: online pid/);
  assert.equal(JSON.parse(read(f.plist(dirs[0]))).ProgramArguments[1],old);
});
