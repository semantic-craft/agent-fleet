/** Human slash-command authorization. No consent URL enters session events. */
import {registerAuthorizationFlow, CALLBACK_URL} from './flow.js';
export const CLIENT_NAME = 'Agent Fleet DSH Native';
export const COMMAND = 'agenvo-authorize';
export const CLIENT_METADATA = Object.freeze({client_name:CLIENT_NAME,redirect_uris:[CALLBACK_URL],response_types:['code'],grant_types:['authorization_code','refresh_token'],token_endpoint_auth_method:'none'});
export function browserOpener({issuer, runNativeCommand, platform=process.platform}) {
 return async (value, signal) => {
  let url;
  try { url=new URL(value); } catch { throw Error('authorization_browser_url_invalid'); }
  if(url.protocol!=='https:' || url.origin!==new URL(issuer).origin || url.username || url.password || url.hash) throw Error('authorization_browser_url_invalid');
  if(platform!=='darwin') throw Error('authorization_browser_platform_unsupported');
  try { await runNativeCommand('/usr/bin/open',[url.href],signal,'visible'); }
  catch { throw Error('authorization_browser_open_failed'); }
 };
}
export function registerHostAuthorization(ctx,{key,oauthController,auth,openBrowser,connect,createServerImpl,platformSupported=true,hasGrant=async()=>false}) {
 let disposed=false, active;
 const unregisterCommand=ctx.commands.register({name:COMMAND,description:'Authorize Agenvo in your browser',recordInput:false,
  async handler({rawInput,signal}) {
   if(disposed || active || signal.aborted || rawInput.trim() || !platformSupported)return {kind:'error',text:'Authorization unavailable or invalid command input.'};
   const controller=new AbortController(); active=controller;
   const combined=AbortSignal.any([signal,controller.signal]);
   let opening, unregisterFlow, browserFailed=false;
   try {
    if(await hasGrant()) {
     if(disposed || combined.aborted)return {kind:'error',text:'Authorization cancelled.'};
     await connect();
     if(disposed || combined.aborted)return {kind:'error',text:'Connection result arrived after cancellation; authorization may remain saved.'};
     return {kind:'success',text:'Existing Agenvo authorization connected.'};
    }
    if(disposed || combined.aborted)return {kind:'error',text:'Authorization cancelled.'};
    // Global key ownership lasts only this explicit attempt. Other scopes may
    // register commands, but simultaneous attempts fail at the native seam.
    unregisterFlow=registerAuthorizationFlow({authorization:ctx.authorization,key,oauthController,auth,clientMetadata:CLIENT_METADATA,...(createServerImpl?{createServerImpl}:{})});
    const outcome=await ctx.authorization.begin({key,method:'oauth',signal:combined,interaction:{
     notify(notice) {
      if(opening || typeof notice.url!=='string'){browserFailed=true;controller.abort();return;}
      opening=Promise.resolve().then(()=>openBrowser(notice.url,combined)).catch(()=>{browserFailed=true;controller.abort();});
     },
     prompt:async()=>{throw Error('authorization_prompt_unsupported');},
    }});
    await opening;
    if(browserFailed)return {kind:'error',text:'Could not open authorization browser. Start a new attempt.'};
    if(disposed || combined.aborted || outcome.status!=='authorized')return {kind:'error',text:'Authorization cancelled or expired.'};
    try { await connect(); }
    catch { return {kind:'error',text:'Authorization saved; Agenvo connection failed. No task was submitted.'}; }
    if(disposed || combined.aborted)return {kind:'error',text:'Connection result arrived after cancellation; authorization may remain saved.'};
    return {kind:'success',text:'Agenvo authorized and connected.'};
   } catch { return {kind:'error',text:'Authorization failed. Start a new attempt.'}; }
   finally {controller.abort();unregisterFlow?.();if(active===controller)active=undefined;}
  }
 });
 return ()=>{disposed=true;active?.abort();unregisterCommand();};
}
