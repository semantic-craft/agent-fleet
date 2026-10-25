// Installed-shape regression: actual prepare output, profile symlink and official
// DSH package resolver/Commands/Authorization. No OAuth, browser or model call.
import assert from 'node:assert/strict';
import {mkdtemp,readFile,writeFile,mkdir,readdir,symlink,rm,realpath} from 'node:fs/promises';
import {createRequire} from 'node:module';
import {dirname,join,resolve} from 'node:path';
import {pathToFileURL} from 'node:url';
import {tmpdir} from 'node:os';
import {spawnSync} from 'node:child_process';
import {preparePackage} from '../prepare.mjs';
const root=resolve(process.argv[2]);
const official=new Map();
for(const group of await readdir(join(root,'packages'),{withFileTypes:true})){
 if(!group.isDirectory())continue;
 for(const item of await readdir(join(root,'packages',group.name),{withFileTypes:true})){
  if(!item.isDirectory())continue;
  const dir=join(root,'packages',group.name,item.name);
  try{const pkg=JSON.parse(await readFile(join(dir,'package.json'),'utf8'));official.set(pkg.name,{dir,pkg});}catch(e){if(e.code!=='ENOENT')throw e;}
 }
}
const require=createRequire(join(root,'packages/mcp/mcp-client/package.json'));
for(const name of ['@deepseek-ai/cordis','@modelcontextprotocol/client']){
 let dir=dirname(require.resolve(name));
 for(;;){try{const pkg=JSON.parse(await readFile(join(dir,'package.json'),'utf8'));if(pkg.name===name){official.set(name,{dir,pkg});break;}}catch(e){if(e.code!=='ENOENT')throw e;} const up=dirname(dir);assert.notEqual(up,dir);dir=up;}
}
const load=async name=>{const {dir,pkg}=official.get(name);return import(pathToFileURL(join(dir,pkg.main??'index.js')))};
const {Context}=await import(pathToFileURL(require.resolve('@deepseek-ai/cordis')));
const {PluginPackages}=await load('@deepseek-ai/dsh-app-boot');
const {default:Commands}=await load('@deepseek-ai/dsh-commands');
const {default:Authorization}=await load('@deepseek-ai/dsh-authorization');
const {LocalCredentialProvider}=await load('@deepseek-ai/dsh-credentials-local');
const {createScope}=await load('@deepseek-ai/dsh-scope');
const temporary=await realpath(await mkdtemp(join(tmpdir(),'fleet-packaged-host-')));
const stage=join(temporary,'artifact'), profile=join(temporary,'home/profiles/test');
const binding={serverUrl:'https://relay.example/mcp',issuer:'https://relay.example/',resource:'https://relay.example/mcp',scope:'runtime:approved'};
const ctx=new Context();let network=0;
const originalFetch=globalThis.fetch;
globalThis.fetch=async()=>{network++;throw Error('network_forbidden_in_packaged_test')};
try{
 await preparePackage(binding,stage);
 const pkg=JSON.parse(await readFile(join(stage,'package.json'),'utf8'));
 assert.equal(pkg.scripts,undefined);
 const patch=JSON.parse(await readFile(join(stage,pkg.dsh.bundle.patch),'utf8'));
 assert.equal(patch.length,1);assert.equal(patch[0].insert.length,1);
 const preset=patch[0].insert[0];assert.equal(preset.config.id,'fleet-agenvo');
 assert.equal(preset.config.plugins.length,1);assert.equal(preset.config.plugins[0].name,pkg.name);
 assert.deepEqual(preset.config.plugins[0].config,binding);
 assert.ok(!pkg.files.includes('authorize.mjs'),'invalid standalone auth entry must not ship');
 await mkdir(join(profile,'node_modules'),{recursive:true});
 await symlink(stage,join(profile,'node_modules',pkg.name),'dir');
 await writeFile(join(profile,'package.json'),JSON.stringify({type:'module',dependencies:{[pkg.name]:'link:'+stage},dsh:{profile:{bundles:[pkg.name]}}}));
 // Reproduce the prior failure before installing the official Host resolver.
 const plain=spawnSync(process.execPath,['--input-type=module','-e',"try { import.meta.resolve('@deepseek-ai/cordis'); process.exitCode=2; } catch(e) { if(e.code!=='ERR_MODULE_NOT_FOUND')throw e; }"],{cwd:stage,encoding:'utf8',env:{PATH:process.env.PATH}});
 assert.equal(plain.status,0,plain.stderr);
 const entries=Object.keys(pkg.peerDependencies).map(name=>{const found=official.get(name);assert.ok(found,'official peer '+name);assert.equal(found.pkg.version,pkg.peerDependencies[name],'fixed peer '+name);return {name,packageDir:found.dir,version:found.pkg.version,declarer:join(found.dir,'package.json'),scope:'installation'}});
 entries.push({name:pkg.name,packageDir:stage,version:pkg.version,declarer:join(profile,'package.json'),scope:'profile'});
 await ctx.plugin(PluginPackages,{resolution:{profilesDir:dirname(profile),profileDir:profile,localPackageNames:[pkg.name],linkedRoots:[{name:pkg.name,realPath:stage}],entries}});
 await ctx.plugin(LocalCredentialProvider,{path:join(temporary,'synthetic-credentials.yaml'),watch:false});
 await ctx.plugin(Authorization);await ctx.plugin(Commands);
 ctx.provide('tools',{register(){throw Error('tools_must_not_register_without_grant')}});
 const probe=join(profile,'probe.mjs');
 await writeFile(probe,`export {default as unused} from 'node:assert'; export * as plugin from '${pkg.name}'; export const peers=await Promise.all(${JSON.stringify(Object.keys(pkg.peerDependencies))}.map(name=>import(name)));`);
 const imported=await import(pathToFileURL(probe));
 assert.equal(imported.peers.length,Object.keys(pkg.peerDependencies).length);
 assert.equal(imported.plugin.name,pkg.name);
 const agent={};const isolated=createScope(ctx,agent);
 const fiber=await isolated.ctx.plugin(imported.plugin,preset.config.plugins[0].config);
 assert.equal(network,0,'passive package load must never start OAuth/MCP');
 const names=ctx.commands.list(agent).map(x=>x.name);
 assert.ok(names.includes('agenvo-authorize'),'official UI registry must discover command');
 assert.ok(names.includes('agenvo-status'),'actual package apply must register status');
 assert.equal(ctx.commands.list({}).length,0,'unrelated scope must not see command');
 assert.equal(ctx.authorization.list().length,0,'no flow or OAuth attempt before explicit command');
 await fiber.dispose();await isolated.dispose();
 assert.equal(ctx.commands.list(agent).length,0,'unload removes command');
 assert.equal(ctx.authorization.list().length,0,'unload removes flow');
 assert.equal(network,0);
 console.log('packaged profile symlink + official resolver/peer imports + scoped command/flow discovery/unload passed; zero network/grant/browser/model');
}finally{await ctx.fiber.dispose();globalThis.fetch=originalFetch;await rm(temporary,{recursive:true,force:true});}
