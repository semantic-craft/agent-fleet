import test from 'node:test';
import assert from 'node:assert/strict';
import {existsSync, readFileSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
const root=new URL('../patches/codex-handoff-probe/',import.meta.url);
test('handoff probe no longer ships the removed connector patch files',()=>{
 const m=JSON.parse(readFileSync(new URL('manifest.json',root),'utf8'));
 assert.equal(m.format,1);assert.equal(m.deployed,false);assert.equal(m.liveValidated,false);
 assert.equal(m.removed,'2026-11-02');
 assert.equal(m.patch,undefined);assert.equal(m.license,undefined);assert.equal(m.notice,undefined);
 assert.match(m.source.base,/^[a-f0-9]{40}$/);assert.match(m.source.candidate,/^[a-f0-9]{40}$/);
 for(const name of ['agenvo.patch','agenvo-license.txt','agenvo-notice.txt']) {
  assert.equal(existsSync(fileURLToPath(new URL(name,root))),false,name);
 }
});
test('probe plan does not equate scoped code or elapsed time with a live handoff',()=>{
 const doc=readFileSync(new URL('../docs/codex-handoff-probe.md',import.meta.url),'utf8');
 for(const marker of ['not deployed','same production app-server','no live launch is ready','30 minutes','thread/unsubscribe','SIGINT','temporary test pairing','operator to stop']) assert.ok(doc.includes(marker),marker);
 assert.match(doc,/--handoff-probe-thread-id/);
});
