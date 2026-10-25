import {mkdir,readFile,writeFile,copyFile} from 'node:fs/promises';
import {dirname,join,resolve} from 'node:path';
import {fileURLToPath,pathToFileURL} from 'node:url';
import {credentialBindingId} from './oauth.js';
const source=dirname(fileURLToPath(import.meta.url));
export async function preparePackage(binding,destination){
 if(!binding||Object.keys(binding).some(k=>!['serverUrl','issuer','resource','scope'].includes(k)))throw Error('nonsecret_binding_only');
 credentialBindingId(binding);
 const endpoint=new URL(binding.serverUrl);
 if(endpoint.pathname!=='/mcp')throw Error('mcp_endpoint_required');
 // Destination must not exist; no overwrite of installed packages or config.
 await mkdir(destination,{recursive:false,mode:0o700});
 const pkg=JSON.parse(await readFile(join(source,'package.json'),'utf8'));
 pkg.dsh={bundle:{patch:'./cordis.patch.yml'}};
 pkg.files=[...pkg.files,'cordis.patch.yml','binding.json','LICENSE'];
 for(const file of pkg.files.filter(f=>!['cordis.patch.yml','binding.json','LICENSE'].includes(f)))await copyFile(join(source,file),join(destination,file));
 await copyFile(join(source,'../../LICENSE'),join(destination,'LICENSE'));
 const patch=[{insert:[{id:'preset-fleet-agenvo',name:'@deepseek-ai/dsh-agent-preset',config:{id:'fleet-agenvo',name:'Fleet Agenvo',plugins:[{id:'fleet-agenvo-caller',name:pkg.name,config:binding}]}}]}];
 await writeFile(join(destination,'package.json'),JSON.stringify(pkg,null,2)+'\n');
 await writeFile(join(destination,'cordis.patch.yml'),JSON.stringify(patch,null,2)+'\n');
 await writeFile(join(destination,'binding.json'),JSON.stringify(binding,null,2)+'\n',{mode:0o600});
 return {package:pkg.name,preset:'fleet-agenvo',destination};
}
if(process.argv[1]&&import.meta.url===pathToFileURL(resolve(process.argv[1])).href){
 const [input,destination,...extra]=process.argv.slice(2);
 if(!input||!destination||extra.length)throw Error('usage: node prepare.mjs binding.json NEW-staging-directory');
 console.log(JSON.stringify(await preparePackage(JSON.parse(await readFile(input,'utf8')),resolve(destination))));
}
