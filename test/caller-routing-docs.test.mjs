import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
const read=p=>readFileSync(new URL('../'+p,import.meta.url),'utf8');
test('entry points distinguish native capability from the required external route',()=>{
 for(const path of ['README.md','AGENTS.md','docs/architecture.md','docs/runbook.md','docs/new-mac.md','prompts/acceptance-codex.md','prompts/operator.md']) {
  const text=read(path);
  assert.match(text,/原生平台任务|原生任务工具/,path);
  assert.match(text,/DSH[／→]|DSH、GrokBot/,path);
  assert.match(text,/GrokBot/,path);
  assert.match(text,/caller-routing.md/,path);
  assert.doesNotMatch(text,/远程通路只有 Agenvo|所有远程派活只走 Agenvo|派 Codex／默认编码 →|\*\*Codex Desktop\*\*（默认编码线）/,path);
 }
});
test('external guards keep their scope without denying native Codex or replacing external needs',()=>{
 const contract=read('docs/codex-desktop-handoff.md');
 assert.match(contract,/dot.*原生平台任务/);assert.match(contract,/blocked 不表示 Codex 整体不可用/);
 assert.match(contract,/DSH→Agenvo→Codex Desktop、GrokBot→Agenvo→Codex Desktop/);
 assert.match(contract,/blocked_native_writer_release/);assert.match(contract,/turn\/start/);
 const probe=read('docs/codex-handoff-probe.md');
 assert.match(probe,/required external DSH\/GrokBot/);assert.match(probe,/cannot replace external callers/);
 assert.match(read('docs/decisions/0006-single-remote-path.md'),/不排除 dot.*原生平台任务/);
});
test('native prerequisites and three separate evidence rows prevent cross-route success claims',()=>{
 const text=read('docs/caller-routing.md');
 for(const term of ['电脑连接授权','跨机先发现环境','schema','不自动跨入口接续','blocked 是待解决','同会话继续','结果返回']) assert.ok(text.includes(term),term);
 const rows=text.split('## 按三个调用方分别验收')[1].split('\n').filter(x=>/^\| (dot → 原生平台|DSH → Agenvo|GrokBot → Agenvo)/.test(x));
 assert.equal(rows.length,3);
 assert.match(rows[0],/尚未全部验收/);
 for(const row of rows.slice(1)) {assert.match(row,/未验/);assert.match(row,/待解决，非完成/);}
 assert.match(text,/也要从各自调用方.*分别验证/);
 assert.match(text,/离线 guard、probe mock 或 dot 原生任务成功均不能/);
});

test('copied prompts retain caller routing and require actual external callers',()=>{
 const operator=read('prompts/operator.md').split('```text')[1];
 for(const phrase of ['原生平台任务工具','DSH→Agenvo→Codex Desktop','GrokBot→Agenvo→Codex Desktop','同入口继续和回读']) assert.ok(operator.includes(phrase),phrase);
 assert.doesNotMatch(operator,/Codex exec 线程|改代码的任务.*全部/);
 const acceptance=read('prompts/acceptance-codex.md').split('```text')[1];
 assert.match(acceptance,/分别由 DSH 和 GrokBot 发出/);
 assert.match(acceptance,/不能用 ChatGPT 代理测试充当通过/);
 assert.doesNotMatch(acceptance,/都在 ChatGPT 里用自然语言下达|每个任务截两张图：ChatGPT|本次是 Desktop 空线程交接/);
});
