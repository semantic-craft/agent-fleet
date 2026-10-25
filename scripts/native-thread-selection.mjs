/** Native Desktop request contract; no scheduler, runtime connection or global settings.
 * The host owns discovery and execution. This cannot intercept direct platform tool calls.
 */
import { readFileSync } from 'node:fs';
import { pathToFileURL } from 'node:url';

const fail = (code, fix) => { throw new Error(`${code}: ${fix}`); };
const text = v => typeof v === 'string' && v.trim().length > 0;
function selected(intent) {
  if (intent?.kind !== 'independent-thread')
    fail('independent_thread_required', 'Use this contract for user-requested independent Desktop chats only.');
  if (!text(intent.selection?.model) || !text(intent.selection?.thinking))
    fail('explicit_selection_required', 'Put the user-selected model and thinking in selection; do not infer either from a prompt or global default.');
  return intent.selection;
}
export function checkNativeThreadRequest(intent, tool, args) {
  const selection = selected(intent);
  if (tool !== 'create_thread')
    fail('independent_thread_requires_create_thread', 'Use the native create_thread tool; spawn_agent creates a child, not an independent Desktop chat.');
  if (args?.model !== selection.model || args?.thinking !== selection.thinking)
    fail('model_selection_mismatch', 'Pass selection.model and selection.thinking as top-level create_thread arguments.');
  return { status: 'ready', tool };
}
export function prepareNativeThread(intent, catalog) {
  const selection = selected(intent);
  if (!intent.request || 'model' in intent.request || 'thinking' in intent.request)
    fail('selection_has_one_owner', 'Keep model and thinking only in selection; request contains the native target, title and prompt.');
  const model = catalog?.find(x => x.model === selection.model);
  if (!model) fail('model_unavailable', 'Discover the target host model catalog; do not substitute a model.');
  if (!model.efforts?.includes(selection.thinking))
    fail('effort_unavailable', 'Select a supported effort from the target catalog without silently changing the requested effort.');
  const args = { ...structuredClone(intent.request), model: selection.model, thinking: selection.thinking };
  checkNativeThreadRequest(intent, 'create_thread', args);
  return args;
}
export function checkNativeThreadObservation(intent, threadId, observation) {
  const selection = selected(intent);
  if (!text(threadId) || observation?.threadId !== threadId)
    fail('thread_identity_mismatch', 'Read the exact thread ID returned by create_thread.');
  if (!text(observation.turnId) || observation.executed !== true)
    fail('turn_evidence_required', 'Read a native turn with an actual reply or executed tool; active/accepted alone is insufficient.');
  if (observation.model !== selection.model || observation.thinking !== selection.thinking)
    fail('model_selection_mismatch', 'Compare the observed native turn model/effort with selection; retain mismatches as failures.');
  return { status: 'execution-observed', threadId, turnId: observation.turnId,
    model: observation.model, thinking: observation.thinking, desktop: 'unverified', business: 'unverified' };
}

// Files only: avoids shell interpolation of prompts. Observations are supplied evidence,
// not authenticated by this offline validator. The caller must retain source references.
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    const [action, file] = process.argv.slice(2);
    const data = JSON.parse(readFileSync(file, 'utf8'));
    let result;
    if (action === 'prepare') result = prepareNativeThread(data.intent, data.catalog);
    else if (action === 'check-request') result = checkNativeThreadRequest(data.intent, data.tool, data.args);
    else if (action === 'check-observation') result = checkNativeThreadObservation(data.intent, data.threadId, data.observation);
    else fail('action_required', 'Use prepare, check-request or check-observation with a JSON file.');
    console.log(JSON.stringify(result, null, 2));
  } catch (error) { console.error(error.message); process.exitCode = 1; }
}
