/** One-shot human authorization surface, not a relay or persistent proxy. */
import {createCredentialAuth,credentialBindingId} from './oauth.js';
import {boundedAuthFetch,name} from './index.js';
import {registerAuthorizationFlow,CALLBACK_URL} from './flow.js';
export const CLIENT_NAME='Agent Fleet DSH Native';
export async function authorizeWithHost({binding,credentialsFile,notify,signal},deps){
 const {Context,LocalCredentialProvider,AuthorizationService,credentialKey,auth,refreshAuthorization}=deps;
 const key=credentialKey(name,credentialBindingId(binding));const ctx=new Context();
 try{
  await ctx.plugin(LocalCredentialProvider,{path:credentialsFile,watch:false});
  await ctx.plugin(AuthorizationService);
  let result;
  await ctx.plugin({name,inject:['authorization','credentials'],async apply(owner){
   const oauthController=createCredentialAuth({credentials:owner.credentials,key,binding,refreshAuthorization,fetchFn:deps.fetchFn??boundedAuthFetch()});
   const dispose=registerAuthorizationFlow({authorization:owner.authorization,key,oauthController,auth,
    clientMetadata:{client_name:CLIENT_NAME,redirect_uris:[CALLBACK_URL],response_types:['code'],grant_types:['authorization_code','refresh_token'],token_endpoint_auth_method:'none'},
    ...(deps.createServerImpl?{createServerImpl:deps.createServerImpl}:{})});
   owner.effect(()=>dispose,'fleet.authorization-flow');
   result=await owner.authorization.begin({key,method:'oauth',signal,interaction:{notify,prompt:async()=>{throw Error('unexpected_authorization_prompt');}}});
  }});
  if(!result || !['authorized','cancelled'].includes(result.status))throw Error('authorization_did_not_settle');
  return result;
 }finally{await ctx.fiber.dispose();}
}
