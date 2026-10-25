import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createCallerRouter } from '../scripts/caller-routes.mjs';
import { lintRoutes } from '../scripts/lint-routes.mjs';
const read = p => readFileSync(new URL('../'+p, import.meta.url),'utf8');
test('task booleans cannot manufacture native host capabilities or external ownership',()=>{
 const router=createCallerRouter({nativePlatform:{verified:true},agenvo:()=>{throw Error('must not dispatch');}});
 assert.equal(router.resolve({caller:'dot',target:'codex',verified:true}).status,'unsupported');
 for(const caller of ['dsh','grokbot']) assert.equal(router.resolve({caller,target:'codex',ownership:true,verified:true}).code,'codex_execution_capability_unavailable');
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

test('same-host registration exposes execution without certifying Desktop or either caller',()=>{
 const codexExecution=Object.fromEntries(['observeTarget','create','read','startTurn'].map(k=>[k,()=>{throw Error('resolver must not dispatch');}]));
 codexExecution.mode='same-host';
 const router=createCallerRouter({agenvo:()=>{},codexExecution});
 codexExecution.mode='cross-backend'; // registration snapshot cannot drift
 for(const caller of ['dsh','grokbot']) {
  const result=router.resolve({caller,target:'codex',desktop:true,verified:true});
  assert.equal(result.status,'available');assert.equal(result.execution,'same-host');
  assert.equal(result.acceptance,'unverified');assert.equal(result.desktop,'unverified');
 }
});
test('task topology cannot override host; missing methods and cross-backend still block',()=>{
 for(const codexExecution of [undefined,{mode:'same-host',verified:true},{mode:'cross-backend'}]) {
  const result=createCallerRouter({agenvo:()=>{},codexExecution}).resolve({caller:'dsh',target:'codex',mode:'same-host'});
  assert.equal(result.status,'blocked');
 }
});

test('lint rejects reintroducing universal writer gate across consumers',()=>{
 assert.ok(lintRoutes(p=>read(p)+(p==='docs/new-mac.md'?'\n在 `thread/start` 之前，必须已有':'')).some(x=>x.endsWith(':blanket_writer_gate')));
});

test('sanitized live baseline never promotes caller or Desktop acceptance',()=>{
 const evidence=JSON.parse(read('docs/evidence/codex-same-host.json'));
 assert.equal(evidence.source,'direct-agenvo-baseline');
 assert.equal(evidence.sameThread,true);assert.equal(evidence.toolCalls,0);
 assert.deepEqual(evidence.turns.map(t=>t.status),['completed','completed']);
 assert.deepEqual(evidence.turns.map(t=>t.text),['CODEX_HOST_AQUAMARINE_A1','CODEX_HOST_AQUAMARINE_A2']);
 assert.equal(evidence.dshInitiated,false);assert.equal(evidence.grokbotInitiated,false);
 assert.equal(evidence.desktopVisible,'unverified');assert.equal(evidence.crossBackendHandoff,'unverified');
 assert.doesNotMatch(JSON.stringify(evidence), /[0-9a-f]{8}-[0-9a-f]{4}-|\/Users\/|Bearer /i);
});

test('lint catches old writer assumptions at root and copied operator entry points',()=>{
 for(const [path,old] of [['AGENTS.md','writer 未验通只阻断这条路线'],['README.md','──待验 writer handoff──'],['prompts/operator.md','在创建前验证原生 ownership 路径'],['lessons.md','→ 按已验证原生 ownership 路径交接→'],['docs/new-mac.md','Desktop 未见可见会话不算执行']]) {
  assert.ok(lintRoutes(p=>read(p)+(p===path?'\n'+old:'')).includes(path+':blanket_writer_gate'),path);
 }
});
