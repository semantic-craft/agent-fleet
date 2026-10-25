/** Explicit caller preparation; no OAuth, prompt, default change or hidden retry. */
export const FLEET_PRESET='fleet-agenvo';
const id=value=>typeof value==='string'&&/^[a-zA-Z0-9_-]{1,160}$/.test(value);
export function fleetCallerCreate({sessionId,workspaceId}) {
 if(!id(sessionId)||!id(workspaceId))throw Error('invalid_caller_identity');
 return Object.freeze({method:'session/create',params:Object.freeze({request:Object.freeze({sessionId,workspaceId,agentPreset:FLEET_PRESET})})});
}
/** After unknown admission, only observe the same ID. Absence never means safe retry. */
export async function observeFleetCaller(call,{sessionId}) {
 if(!id(sessionId))return {status:'blocked',code:'invalid_caller_identity'};
 try {
  const reply=await call('session/projections',{request:{sessionId}});
  if(reply?.error || (reply?.nativeIds?.sessionId!==undefined&&reply.nativeIds.sessionId!==sessionId))return {status:'blocked',code:'caller_observation_unavailable',sessionId};
  const value=reply?.result;
  if(value?.kind!=='sequenced'||!Number.isSafeInteger(value.asOfSeq)||value.asOfSeq<0)return {status:'blocked',code:'caller_observation_unavailable',sessionId};
  if(value.values?.agentPreset!==FLEET_PRESET)return {status:'blocked',code:'caller_preset_mismatch',sessionId};
  return {status:'observed',code:'caller_preset_matches',sessionId,seq:value.asOfSeq};
 } catch{return {status:'blocked',code:'caller_observation_unavailable',sessionId};}
}

/** Optional public host discovery injection; old transports without it are blocked. */
export async function checkFleetCaller(call,{sessionId,readCommands}) {
 if(typeof readCommands!=='function')return {status:'blocked',code:'command_discovery_unsupported',sessionId};
 const observed=await observeFleetCaller(call,{sessionId});
 if(observed.status!=='observed')return observed;
 try {
  const commands=await readCommands(sessionId);
  if(!Array.isArray(commands)||!['agenvo-status','agenvo-authorize'].every(name=>commands.some(c=>c?.name===name)))return {status:'blocked',code:'caller_commands_unavailable',sessionId};
  return {...observed,status:'ready',code:'caller_status_command_available'};
 } catch{return {status:'blocked',code:'command_discovery_unavailable',sessionId};}
}
