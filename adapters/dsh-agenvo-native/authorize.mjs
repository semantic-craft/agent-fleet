#!/usr/bin/env node
// Explicit command only: never invoked by install or plugin apply.
import {readFile} from 'node:fs/promises';
import {isAbsolute} from 'node:path';
import {authorizeWithHost} from './setup.js';
const [bindingFile,credentialsFile,...extra]=process.argv.slice(2);
if(!bindingFile||!credentialsFile||extra.length||!isAbsolute(bindingFile)||!isAbsolute(credentialsFile)){
 console.error('usage: node authorize.mjs /absolute/nonsecret-binding.json /absolute/DSH-credentials.yaml');process.exitCode=2;
}else{
 const controller=new AbortController();const cancel=()=>controller.abort();process.once('SIGINT',cancel);process.once('SIGTERM',cancel);
 try{
  const binding=JSON.parse(await readFile(bindingFile,'utf8'));
  const [{Context},{LocalCredentialProvider},{AuthorizationService},{credentialKey},sdk]=await Promise.all([
   import('@deepseek-ai/cordis'),import('@deepseek-ai/dsh-credentials-local'),import('@deepseek-ai/dsh-authorization'),import('@deepseek-ai/dsh-credentials'),import('@modelcontextprotocol/client')]);
  const result=await authorizeWithHost({binding,credentialsFile,signal:controller.signal,notify:notice=>{
   // Intended consent handoff only; never dump provider records or callback/code.
   if(notice.url)process.stdout.write(`Open the formal consent page:\n${notice.url}\n`);
  }},{Context,LocalCredentialProvider,AuthorizationService,credentialKey,...sdk});
  process.stdout.write(`Authorization ${result.status}\n`);
 }catch{console.error('Authorization failed; no raw credential or remote error logged.');process.exitCode=1;}
 finally{process.removeListener('SIGINT',cancel);process.removeListener('SIGTERM',cancel);}
}
