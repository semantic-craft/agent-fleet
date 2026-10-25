import {createCredentialAuth,credentialBindingId} from './oauth.js';
import {connectScoped} from './transport.js';
export const name='agent-fleet-agenvo-native';
export const inject=['tools','credentials'];
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
 const [sdk,scope,McpClient]=await Promise.all([import('@modelcontextprotocol/client'),import('@deepseek-ai/dsh-scope'),import('@deepseek-ai/dsh-mcp-client')]);
 const authorization=await createHostAuthorization(ctx,binding);
 await connectScoped(ctx,{relayUrl:binding.serverUrl,authProvider:authorization.authProvider},{...sdk,scopeOf:scope.scopeOf,createMcpToolDefinition:McpClient.createMcpToolDefinition});
}
