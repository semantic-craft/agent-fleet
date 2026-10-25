import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdtemp,readFile,rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';import {join} from 'node:path';
import {preparePackage} from '../prepare.mjs';
const binding={serverUrl:'https://relay.example/mcp',issuer:'https://relay.example/',resource:'https://relay.example/mcp',scope:'runtime:approved'};
test('staged package adds one opt-in preset, never changes defaults, refuses overwrite/secret fields',async()=>{
 const root=await mkdtemp(join(tmpdir(),'fleet-pack-'));
 try{
  const out=join(root,'package');await preparePackage(binding,out);
  const patch=JSON.parse(await readFile(join(out,'cordis.patch.yml'),'utf8'));assert.equal(patch.length,1);assert.equal(patch[0].insert.length,1);assert.equal(patch[0].insert[0].config.id,'fleet-agenvo');assert.equal(patch[0].insert[0].config.plugins.length,1);assert.equal(JSON.stringify(patch).includes('selectedDefault'),false);
  const pkg=JSON.parse(await readFile(join(out,'package.json'),'utf8'));assert.equal(pkg.scripts,undefined);assert.equal(pkg.dsh.bundle.patch,'./cordis.patch.yml');
  await assert.rejects(preparePackage(binding,out),/EEXIST/);
  await assert.rejects(preparePackage({...binding,token:'synthetic'},join(root,'bad')),/nonsecret_binding_only/);
 }finally{await rm(root,{recursive:true,force:true});}
});
