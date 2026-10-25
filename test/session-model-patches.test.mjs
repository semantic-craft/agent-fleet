import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
const root = new URL('../patches/dsh-session-only/', import.meta.url);
test('reviewed source candidates retain fixed bases and exact patch/license bytes',()=>{
 const manifest=JSON.parse(readFileSync(new URL('manifest.json',root),'utf8'));
 assert.equal(manifest.deployed,false);
 assert.equal(manifest.runtime.base,'1f81c047fc48fa190a683e469daa699857e9afe8');
 assert.equal(manifest.agenvo.base,'cd10fb92e1da731b307b85f8e94e8db8b6b8808a');
 for(const entry of [manifest.runtime,manifest.agenvo]) {
   for(const key of ['patch','license']) {
     assert.match(entry[key].file,/^[a-z-]+\.(patch|txt)$/);
     const bytes=readFileSync(new URL(entry[key].file,root));
     assert.equal(createHash('sha256').update(bytes).digest('hex'),entry[key].sha256);
   }
   const patch=readFileSync(new URL(entry.patch.file,root),'utf8');
   assert.ok(patch.includes('selectModelForSession'));
   assert.equal((/\/Users\/[a-z]/).test(patch),false);
 }
});
