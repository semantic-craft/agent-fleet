import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createCallerRouter } from '../scripts/caller-routes.mjs';
import { lintRoutes } from '../scripts/lint-routes.mjs';
const read = p => readFileSync(new URL('../'+p, import.meta.url),'utf8');
test('task booleans cannot manufacture native host capabilities or external ownership',()=>{
 const router=createCallerRouter({nativePlatform:{verified:true},agenvo:()=>{throw Error('must not dispatch');}});
 assert.equal(router.resolve({caller:'dot',target:'codex',verified:true}).status,'unsupported');
 for(const caller of ['dsh','grokbot']) assert.equal(router.resolve({caller,target:'codex',ownership:true,verified:true}).code,'native_writer_unverified');
});
test('executable native registration enables only native caller, not external callers',()=>{
 const nativePlatform=Object.fromEntries(['discoverEnvironment','create','read','continue'].map(k=>[k,()=>{}]));
 const router=createCallerRouter({nativePlatform,agenvo:()=>{}});
 assert.equal(router.resolve({caller:'dot',target:'codex'}).status,'available');
 assert.equal(router.resolve({caller:'dsh',target:'codex'}).status,'blocked');
 assert.equal(router.resolve({caller:'grokbot',target:'dsh'}).status,'available');
 assert.equal(router.resolve({caller:'unknown',target:'codex'}).code,'caller_unknown');
});
test('route lint rejects changed acceptance and copyable legacy routing',()=>{
 assert.deepEqual(lintRoutes(read),[]);
 assert.ok(lintRoutes(p=>read(p).replace('partial-maintenance-evidence','verified')).includes('caller_route_table_drift'));
 assert.ok(lintRoutes(p=>read(p)+(p==='prompts/operator.md'?'\n所有远程派活只走 Agenvo':'')).some(x=>x.endsWith('contradictory_route')));
});
