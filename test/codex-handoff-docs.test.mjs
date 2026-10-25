import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
const read=p=>readFileSync(new URL('../'+p,import.meta.url),'utf8');
const skill=read('skills/delegate-via-agenvo/SKILL.md');
const section=(s,start,end)=>s.slice(s.indexOf(start),end?s.indexOf(end,s.indexOf(start)):undefined);
const desktop=section(skill,'## Codex Desktop','## 一轮一个 tab');
test('Desktop handoff gates before creation and does not prescribe connector restart',()=>{
 assert.match(desktop,/创建前先确认 native writer/);
 assert.match(desktop,/blocked_native_writer_release/);
 assert.doesNotMatch(desktop,/kickstart -k|释放该 connector 写锁/);
 assert.match(desktop,/由用户在 Desktop 按发送/);
 const contract=read('docs/codex-desktop-handoff.md');
 for(const required of ['unknown','awaiting_user_send','不能单独证明','不证明 native writer']) assert.ok(contract.includes(required));
});
test('target machine and brief follow discovery rather than personal hostnames',()=>{
 for(const p of ['skills/delegate-via-agenvo/SKILL.md','docs/architecture.md']) {
  const s=read(p);assert.match(s,/deviceId/);
 }
 assert.match(desktop,/同一 deviceId/);assert.match(desktop,/目标机器回读/);
});
test('new-machine Codex acceptance consumes Desktop flow, not CLI artifacts',()=>{
 const s=section(read('docs/new-mac.md'),'## Codex 派活验收','## 机器角色');
 assert.match(s,/codex-desktop-handoff.md/);assert.match(s,/用户按发送/);
 const beforeCli=s.split('Herdr 的模板')[0];
 assert.doesNotMatch(beforeCli,/events-1.jsonl|result-1.md|resume <thread_id>|pane_not_found/);
 assert.match(s,/Claude Code/);assert.match(s,/result-1.json/);assert.match(s,/--resume <session_id>/);
 assert.match(skill,/const agent = \(D\) => `claude -p/);
});
test('downstream UI prompt and historical ADR do not revive the old default',()=>{
 const s=read('prompts/acceptance-codex.md');
 assert.match(s,/codex-desktop-handoff.md/);assert.doesNotMatch(s,/Codex 经 Herdr 执行|给 Codex 派一个改代码类任务|按步骤 4 的脚本给 Codex/);
 assert.doesNotMatch(read('docs/new-mac.md'),/先保证 Codex 能经 Herdr|Codex 派活依赖这条通路/);
 assert.doesNotMatch(read('lessons.md'),/释放 connector 写锁/);
 assert.match(read('docs/decisions/0003-herdr-for-cli-agents.md'),/不是默认派活或新机验收/);
});
