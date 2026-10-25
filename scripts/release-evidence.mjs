#!/usr/bin/env node
import { readFileSync, lstatSync, readdirSync, realpathSync } from 'node:fs';
import { resolve, join, relative, sep } from 'node:path';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';
// Explicit read-only artifact inspection, not authentication or running-process proof.
export function verifyRelease(directory, revision) {
  const blocked = code => ({status:'blocked',code});
  if (!/^[a-f0-9]{40}$/.test(revision ?? '')) return blocked('revision_required');
  try {
    if (lstatSync(directory).isSymbolicLink()) throw Error();
    const root=realpathSync(directory), m=join(root,'release.json');
    if (!lstatSync(m).isFile() || lstatSync(m).isSymbolicLink()) throw Error();
    const manifest=JSON.parse(readFileSync(m,'utf8'));
    if(manifest.sha!==revision || !manifest.files || typeof manifest.files!=='object' || Array.isArray(manifest.files)) throw Error();
    const entries=Object.entries(manifest.files);
    if(!entries.length || entries.length>100000) throw Error();
    for(const [name,hash] of entries){
      if(!name || name.includes('\\') || name==='release.json' || name.split('/').some(x=>!x||x==='.'||x==='..') || !/^[a-f0-9]{64}$/.test(hash)) throw Error();
      const p=resolve(root,name);if(!p.startsWith(root+sep)) throw Error();
      let cursor=root;for(const part of name.split('/')){cursor=join(cursor,part);if(lstatSync(cursor).isSymbolicLink())throw Error();}
      if(!lstatSync(p).isFile()||createHash('sha256').update(readFileSync(p)).digest('hex')!==hash)throw Error();
    }
    const walk=dir=>{for(const item of readdirSync(dir,{withFileTypes:true})){
      const p=join(dir,item.name);if(item.isSymbolicLink())throw Error();
      if(item.isDirectory())walk(p);else if(!item.isFile()||(relative(root,p)!=='release.json'&&!Object.hasOwn(manifest.files,relative(root,p).split(sep).join('/'))))throw Error();
    }};walk(root);
    return {status:'verified-files',revision,files:entries.length,sourceAuthenticityProven:false,runningProcessProven:false};
  }catch{return blocked('release_evidence_invalid');}
}
if(process.argv[1]&&resolve(process.argv[1])===fileURLToPath(import.meta.url)){
 if(process.argv.length!==4){console.error('usage: node scripts/release-evidence.mjs RELEASE_DIRECTORY EXPECTED_REVISION');process.exitCode=2;}
 else{const result=verifyRelease(process.argv[2],process.argv[3]);console.log(JSON.stringify(result));if(result.status!=='verified-files')process.exitCode=1;}
}
