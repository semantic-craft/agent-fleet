import test from 'node:test';
import assert from 'node:assert/strict';
import { prepareNativeThread, checkNativeThreadRequest, checkNativeThreadObservation } from '../scripts/native-thread-selection.mjs';

const selection = { model: 'deepseek/deepseek-flash', thinking: 'high' };
const intent = { kind: 'independent-thread', selection,
  request: { title: 'A', prompt: 'Read the assigned worktree.', target: { type: 'project', projectId: 'project-a', environment: { type: 'local' } } } };
const catalog = [{ model: selection.model, efforts: ['low', 'high'] }];

test('replay omitted create_thread model: reject before dispatch; generated request keeps explicit selection', () => {
  assert.throws(() => checkNativeThreadRequest(intent, 'create_thread', intent.request), /model_selection_mismatch/);
  const args = prepareNativeThread(intent, catalog);
  assert.equal(args.model, selection.model);
  assert.equal(args.thinking, 'high');
  assert.equal(checkNativeThreadRequest(intent, 'create_thread', args).status, 'ready');
});
test('independent thread cannot be fulfilled by spawn_agent even with the requested model', () => {
  assert.throws(() => checkNativeThreadRequest(intent, 'spawn_agent', {model: selection.model, reasoning_effort: 'high'}), /independent_thread_requires_create_thread/);
});
test('a stale catalog, missing effort, or conflicting request cannot silently pick defaults', () => {
  assert.throws(() => prepareNativeThread(intent, []), /model_unavailable/);
  assert.throws(() => prepareNativeThread(intent, [{model: selection.model, efforts: ['low']}]), /effort_unavailable/);
  assert.throws(() => prepareNativeThread({...intent, selection:{model:selection.model}}, catalog), /explicit_selection_required/);
  assert.throws(() => prepareNativeThread({...intent,request:{...intent.request,model:'gpt-6-astra'}},catalog), /selection_has_one_owner/);
});
test('same-thread execution evidence is separate from creation, UI and business acceptance', () => {
  const request = prepareNativeThread(intent,catalog);
  assert.throws(() => checkNativeThreadObservation(intent,'thread-a',{threadId:'thread-a',status:'active'}), /turn_evidence_required/);
  assert.throws(() => checkNativeThreadObservation(intent,'thread-a',{threadId:'thread-b',turnId:'turn-b',model:request.model,thinking:'high',executed:true}), /thread_identity_mismatch/);
  assert.throws(() => checkNativeThreadObservation(intent,'thread-a',{threadId:'thread-a',turnId:'turn-a',model:'gpt-6-astra',thinking:'medium',executed:true}), /model_selection_mismatch/);
  const result=checkNativeThreadObservation(intent,'thread-a',{threadId:'thread-a',turnId:'turn-a',model:request.model,thinking:'high',executed:true});
  assert.equal(result.status,'execution-observed');
  assert.equal(result.desktop,'unverified');
  assert.equal(result.business,'unverified');
});
