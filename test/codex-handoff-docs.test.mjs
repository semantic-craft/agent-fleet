import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
const read=p=>readFileSync(new URL('../'+p,import.meta.url),'utf8');
const section=(s,start,end)=>s.slice(s.indexOf(start),end?s.indexOf(end,s.indexOf(start)):undefined);
test('Desktop handoff retains agent submission and scopes control handback checks',()=>{
 const contract=read('docs/codex-desktop-handoff.md');
 for(const required of ['unknown','desktop_visible','submitted','turn/start','不能单独证明']) assert.ok(contract.includes(required), required);
 assert.doesNotMatch(contract,/awaiting_user_send|由用户按 Send/);
});
test('target machine and brief follow discovery rather than personal hostnames',()=>{
 for(const p of ['docs/architecture.md']) {
  const s=read(p);assert.match(s,/deviceId/);
 }
});
test('new-machine Codex acceptance consumes Desktop flow, not CLI artifacts',()=>{
 const s=section(read('docs/new-mac.md'),'## Codex 派活验收','## 机器角色');
 assert.match(s,/codex-desktop-handoff.md/);assert.match(s,/turn\/start/);
 assert.doesNotMatch(s,/用户按发送/);
 const beforeCli=s.split('Herdr 的模板')[0];
 assert.doesNotMatch(beforeCli,/events-1.jsonl|result-1.md|resume <thread_id>|pane_not_found/);
 assert.match(s,/Claude Code/);assert.match(s,/result-1.json/);assert.match(s,/--resume <session_id>/);
});
test('downstream UI prompt and historical ADR do not revive the old default',()=>{
 const s=read('prompts/acceptance-codex.md');
 assert.match(s,/codex-desktop-handoff.md/);assert.match(s,/turn\/start/);
 assert.doesNotMatch(s,/用户按发送|由用户按发送/);
 assert.doesNotMatch(s,/Codex 经 Herdr 执行|给 Codex 派一个改代码类任务|按步骤 4 的脚本给 Codex/);
 assert.doesNotMatch(read('docs/new-mac.md'),/先保证 Codex 能经 Herdr|Codex 派活依赖这条通路/);
 assert.match(read('lessons.md'),/turn\/start/);
 assert.doesNotMatch(section(read('lessons.md'),'### Codex 派活走 Desktop 空会话','\n### '),/用户在 Codex Desktop 按发送|由本人按发送/);
 assert.match(read('docs/decisions/0003-herdr-for-cli-agents.md'),/不是默认派活或新机验收/);
});

test('all consumers distinguish independent execution from Desktop handback without personal targets',()=>{
 const paths=['docs/codex-desktop-handoff.md','docs/architecture.md','docs/runbook.md','docs/new-mac.md','prompts/acceptance-codex.md','lessons.md'];
 for(const p of paths){
  const s=read(p);
  assert.match(s,/独立同 host 执行基线/,p);
  assert.doesNotMatch(s,/创建前门槛[^\n。]*thread\/unsubscribe/,p);
  assert.match(s,/blocked_native_writer_release/,p);
  assert.doesNotMatch(s,/释放 connector 写锁|由用户发送|用户按发送/,p);
 }
 assert.match(read('docs/codex-desktop-handoff.md'),/kickstart.*不等于放锁/);
});
