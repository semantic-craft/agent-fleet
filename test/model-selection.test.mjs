import test from 'node:test';
import assert from 'node:assert/strict';
import {planModelSelection, reviewModelRestoration} from '../scripts/model-selection.mjs';
const before={provider:'p',model:'a',reasoningEffort:'medium'}, desired={provider:'p',model:'b',reasoningEffort:'low'};
const coordination={authorized:true,allSelectorsPaused:true,newSessionCreationPaused:true};
const build=()=>planModelSelection({before,desired,sessionId:'owned',coordination});
const plan=build(); const make=()=>plan;
const observed={currentDefault:desired,selectedSession:{sessionId:'owned',selection:desired},selectionOutcome:'confirmed',coordination,persistenceBarrier:{kind:'runtime-persistence-barrier',sessionId:'owned',operationId:plan.operationId,settled:true}};
test('selection requires coordination covering both selectors and new sessions',()=>{
 for(const key of Object.keys(coordination)) assert.equal(planModelSelection({before,desired,sessionId:'owned',coordination:{...coordination,[key]:false}}).status,'blocked');
});
test('snapshot is frozen and no plan authorizes automatic mutation',()=>{const p=make();assert.equal(p.mutationAllowed,false);assert.ok(Object.isFrozen(p.before));assert.notEqual(p.before,before);});
test('unknown selection only reads back and never restores blindly',()=>{assert.equal(reviewModelRestoration(make(),{...observed,selectionOutcome:'unknown'}).code,'selection_unknown_read_back');});
test('foreign session and coordination loss block restoration',()=>{
 assert.equal(reviewModelRestoration(make(),{...observed,selectedSession:{sessionId:'foreign',selection:desired}}).status,'blocked');
 assert.equal(reviewModelRestoration(make(),{...observed,coordination:{}}).code,'global_selection_coordination_lost');
});
test('external default drift cannot be overwritten',()=>{assert.equal(reviewModelRestoration(make(),{...observed,currentDefault:{...before,model:'external'}}).code,'external_default_drift');});
test('restore remains operator action until exact default and session readback',()=>{
 assert.equal(reviewModelRestoration(make(),observed).status,'operator_action_required');
 assert.equal(reviewModelRestoration(make(),{...observed,currentDefault:before}).code,'default_restored');
});
test('malformed selections fail closed',()=>{assert.equal(planModelSelection({before:{...before,secret:'never'},desired,sessionId:'owned',coordination}).status,'blocked');});

test('catalog original value does not prove asynchronous default writes settled',()=>{assert.equal(reviewModelRestoration(make(),{...observed,currentDefault:before,persistenceBarrier:undefined}).code,'default_persistence_unproven');});

test('old same-session barrier cannot complete a fresh selection operation',()=>{assert.equal(reviewModelRestoration(build(),observed).code,'default_persistence_unproven');});
