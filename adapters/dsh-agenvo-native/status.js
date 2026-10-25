/** Presence-only status. Never reads record values, refreshes or connects. */
export function registerStatus(ctx,{key,connectionState}) {
 let disposed=false;
 const remove=ctx.commands.register({name:'agenvo-status',description:'Show local Agenvo credential presence and connection status',recordInput:false,
  async handler({rawInput,signal}) {
   if(disposed || signal.aborted || rawInput.trim())return {kind:'error',text:'Status unavailable or invalid command input.'};
   try {
    const info=await ctx.credentials.describeRecord(key);
    if(disposed || signal.aborted) return {kind:'error',text:'Status cancelled.'};
    if(typeof info?.configured!=='boolean' || typeof info?.writable!=='boolean')throw Error('invalid');
    const state=connectionState();
    if(!['disconnected','connecting','connected','disposed'].includes(state))throw Error('invalid');
    const kind=['grant','api-key'].includes(info.kind)?info.kind:'unknown';
    return {kind:'success',text:JSON.stringify({configured:info.configured,kind,writable:info.writable,connection:state,expiry:'unknown',grantedScopes:'unknown'})};
   } catch {return {kind:'error',text:'Local authorization status unavailable.'};}
  }
 });
 return ()=>{disposed=true;remove();};
}
