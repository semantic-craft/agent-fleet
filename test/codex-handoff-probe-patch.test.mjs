import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
const root=new URL('../patches/codex-handoff-probe/',import.meta.url);
test('handoff probe source capsule pins replayable, unlaunched inputs',()=>{
 const m=JSON.parse(readFileSync(new URL('manifest.json',root),'utf8'));
 assert.equal(m.format,1);assert.equal(m.deployed,false);assert.equal(m.liveValidated,false);
 assert.match(m.source.base,/^[a-f0-9]{40}$/);assert.match(m.source.candidate,/^[a-f0-9]{40}$/);
 assert.equal(m.source.repository,'https://github.com/semantic-craft/agenvo');
 for(const entry of [m.patch,m.license,m.notice]) {
  assert.match(entry.file,/^[a-z-]+\.(patch|txt)$/);
  assert.equal(createHash('sha256').update(readFileSync(new URL(entry.file,root))).digest('hex'),entry.sha256);
 }
 const patch=readFileSync(new URL(m.patch.file,root),'utf8');
 assert.match(patch,/diff --git a\/apps\/codex-app-server\/src\/handoff-probe.ts/);
 assert.match(patch,/diff --git a\/tests\/codex-handoff-probe.test.ts/);
 assert.doesNotMatch(patch,/diff --git .*node_modules|diff --git .*dist\//);
});
test('probe plan does not equate scoped code or elapsed time with a live handoff',()=>{
 const doc=readFileSync(new URL('../docs/codex-handoff-probe.md',import.meta.url),'utf8');
 for(const marker of ['not deployed','same production app-server','no live launch is ready','30 minutes','thread/unsubscribe','SIGINT','temporary test pairing','operator to stop']) assert.ok(doc.includes(marker),marker);
 assert.match(doc,/--handoff-probe-thread-id/);
});
