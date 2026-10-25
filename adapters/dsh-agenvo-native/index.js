import {createCredentialAuth,credentialBindingId} from './oauth.js';
import {registerStatus} from './status.js';
import {connectScoped} from './transport.js';
import {registerHostAuthorization,browserOpener} from './action.js';
export const name='agent-fleet-agenvo-native';
export const inject=['tools','credentials','authorization','commands'];
// Bound network time while the credential backend holds its refresh lock.
export function boundedAuthFetch(fetchImpl=globalThis.fetch){
 return (input,init={})=>fetchImpl(input,{...init,signal:AbortSignal.any([
  AbortSignal.timeout(15000), ...(init.signal?[init.signal]:[]),
  ...(input instanceof Request?[input.signal]:[]),
 ])});
}
function bindingFrom(config){
 if(!config || Object.keys(config).some(k=>!['serverUrl','issuer','resource','scope'].includes(k)))throw new Error('unsupported_native_config');
 return Object.freeze({...config});
}
/** Host-only composition. Never expose authorization control as a model tool. */
export async function createHostAuthorization(ctx,config){
 const binding=bindingFrom(config);
 const [sdk,{credentialKey}]=await Promise.all([import('@modelcontextprotocol/client'),import('@deepseek-ai/dsh-credentials')]);
 return createCredentialAuth({credentials:ctx.credentials,key:credentialKey(name,credentialBindingId(binding)),binding,refreshAuthorization:sdk.refreshAuthorization,fetchFn:boundedAuthFetch()});
}
export async function apply(ctx,config){
 const binding=bindingFrom(config);
 const [sdk,scope,McpClient,{credentialKey},{runNativeCommand}]=await Promise.all([import('@modelcontextprotocol/client'),import('@deepseek-ai/dsh-scope'),import('@deepseek-ai/dsh-mcp-client'),import('@deepseek-ai/dsh-credentials'),import('@deepseek-ai/dsh-native-command')]);
 if(!scope.scopeOf(ctx))throw Error('scoped_context_required');
 const key=credentialKey(name,credentialBindingId(binding));
 const authorization=await createHostAuthorization(ctx,binding);
 let disposed=false;
 ctx.effect(()=>()=>{disposed=true;},'agent-fleet.authorization-owner');
 const connect=connectionGate(()=>connectScoped(ctx,{relayUrl:binding.serverUrl,authProvider:authorization.authProvider},{...sdk,scopeOf:scope.scopeOf,createMcpToolDefinition:McpClient.createMcpToolDefinition}),()=>disposed);
 ctx.effect(()=>registerStatus(ctx,{key,connectionState:connect.state}),'agent-fleet.status-command');
 const dispose=registerHostAuthorization(ctx,{key,oauthController:authorization,auth:sdk.auth,openBrowser:browserOpener({issuer:binding.issuer,runNativeCommand}),connect,platformSupported:process.platform==='darwin',hasGrant:async()=>await ctx.credentials.readRecord(key)!==undefined});
 ctx.effect(()=>dispose,'agent-fleet.authorization-action');
 // Missing grant leaves a discoverable human command, not a failed plugin or automatic OAuth flow.
 if(await ctx.credentials.readRecord(key)!==undefined){
  // Preserve the human retry surface if the existing grant cannot connect.
  // The explicit command reports a sanitized outcome; never launch OAuth here.
  await connect().catch(()=>{});
 }
}

/** Coalesce explicit connection requests only; a closed connection never auto-retries. */
export function connectionGate(open,isDisposed){
 let connection,connecting;
 const ensure=async()=>{
  if(isDisposed())throw Error('authorization_owner_disposed');
  if(connection && !connection.closed)return;
  connection=undefined;
  if(!connecting)connecting=Promise.resolve().then(open).then(async value=>{
   if(isDisposed()){await value.dispose();throw Error('authorization_owner_disposed');}
   if(value.closed)throw Error('connection_closed');
   connection=value;
  }).finally(()=>{connecting=undefined;});
  return connecting;
 };
 ensure.state=()=>isDisposed()?'disposed':connecting?'connecting':connection&&!connection.closed?'connected':'disconnected';
 return ensure;
}
