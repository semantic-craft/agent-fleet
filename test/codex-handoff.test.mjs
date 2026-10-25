import test from 'node:test';
import assert from 'node:assert/strict';
import {checkCodexHandoff as check} from '../scripts/codex-handoff.mjs';
const fixture = () => {
 const target = {deviceId:'device-a', instanceId:'codex-a', runtimeVersion:'fixture-runtime', adapterRevision:'fixture-rev', desktopVersion:'fixture-desktop'};
 return {phase:'create', target, capability:{...target, supported:true, status:'verified', nativeMethod:'fixture-only-handoff', evidenceRef:'fixture:canary',
 canary:{create:true,nativeHandoff:true,desktopVisible:true,agentSubmit:true,resultReadback:true,desktopCanTakeOverAfterSubmit:true}},
 brief:{deviceId:target.deviceId,path:'/fixture/brief.md',readback:true,evidenceRef:'fixture:brief'}, admission:'not_submitted',threadId:'thread-a',
 ownership:{...target,threadId:'thread-a',verified:true,nativeMethod:'fixture-only-handoff',evidenceRef:'fixture:ownership'},
 desktop:{...target,threadId:'thread-a',visible:true,evidenceRef:'fixture:desktop'}};
};
test('unsupported current path blocks before creation despite online/visible flags',()=>{
 assert.equal(check().allowed,false);
 const f=fixture();delete f.capability;f.online=true;f.connectorRestarted=true;
 assert.equal(check(f).reason,'blocked_native_writer_release');
});
test('every combination identity and canary stage is required before creation',()=>{
 const good=fixture();assert.deepEqual(check(good),{allowed:true,action:'thread/start'});
 for(const key of ['deviceId','instanceId','runtimeVersion','adapterRevision','desktopVersion','nativeMethod','evidenceRef']) {
  const f=fixture();delete f.capability[key];assert.equal(check(f).allowed,false,key);
 }
 for(const key of Object.keys(good.capability.canary)) {
  const f=fixture();f.capability.canary[key]=false;assert.equal(check(f).allowed,false,key);
 }
 for(const key of ['deviceId','instanceId','runtimeVersion','adapterRevision','desktopVersion']) {
  const f=fixture();f.capability[key]='different';assert.equal(check(f).allowed,false,key);
 }
});
test('brief must be read back on the selected device, not the caller host',()=>{
 for(const change of [{deviceId:'device-b'},{readback:false},{path:'relative'},{evidenceRef:''}]) {
  const f=fixture();Object.assign(f.brief,change);assert.equal(check(f).reason,'blocked_target_brief');
 }
});
test('visibility, ownership and agent submission are distinct thread-scoped conditions',()=>{
 const f=fixture();f.phase='submit';assert.deepEqual(check(f),{allowed:true,action:'turn/start'});
 for(const field of ['ownership','desktop']) for(const key of ['deviceId','instanceId','runtimeVersion','adapterRevision','desktopVersion','threadId','evidenceRef']) {
  const bad=structuredClone(f);bad[field][key]='';assert.equal(check(bad).allowed,false,`${field}.${key}`);
 }
 const noOwner=structuredClone(f);noOwner.ownership.verified=false;assert.equal(check(noOwner).reason,'blocked_thread_ownership');
 const hidden=structuredClone(f);hidden.desktop.visible=false;assert.equal(check(hidden).reason,'blocked_desktop_visibility');
});
test('unknown or already admitted writes never become create or submit actions',()=>{
 for(const phase of ['create','submit']) for(const admission of [undefined,'unknown','accepted','completed']) {
  assert.equal(check({...fixture(),phase,admission}).reason,'observe_only');
 }
});
