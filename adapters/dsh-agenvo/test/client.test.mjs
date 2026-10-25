import test from 'node:test';
import assert from 'node:assert/strict';
import {realpathSync,mkdtempSync,mkdirSync,writeFileSync,chmodSync,rmSync,symlinkSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {buildConfig,verifyPaths} from '../config.js';
import * as plugin from '../index.js';
const config=()=>({relayUrl:'https://relay.example.com/mcp',nodePath:'/fixture/node',proxyPath:'/fixture/proxy.js',oauthDirectory:'/fixture/dsh-only',callbackPort:13345});
test('standard caller configuration has no credentials, install command or new relay',()=>{
 const c=buildConfig(config());assert.equal(c.transport,'stdio');assert.equal(c.serverName,'agenvo');
 assert.deepEqual(c.args,['/fixture/proxy.js','https://relay.example.com/mcp','13345','--transport','http-only','--host','127.0.0.1']);
 assert.deepEqual(c.env,{MCP_REMOTE_CONFIG_DIR:'/fixture/dsh-only'});assert.equal(c.failOnStartupError,true);assert.equal(c.reconnect.enabled,false);
});
test('reject arbitrary auth headers, command args and secret URLs before mount',()=>{
 for(const extra of [{headers:{}},{token:'secret'},{args:[]},{env:{}},{verified:true}]) assert.throws(()=>buildConfig({...config(),...extra}),/invalid_config/);
 for(const relayUrl of ['http://example.com/mcp','https://u:p@example.com/mcp','https://example.com/mcp?token=x','https://example.com/mcp#x','https://example.com/admin']) assert.throws(()=>buildConfig({...config(),relayUrl}),/invalid_relay/);
 for(const callbackPort of [0,443,65536,1.1]) assert.throws(()=>buildConfig({...config(),callbackPort}),/invalid_callback_port/);
});
test('namespace exports preserve inject and refuse unscoped Host mount',async()=>{
 assert.equal('default' in plugin,false);assert.deepEqual(plugin.inject,['tools']);
 let count=0;await assert.rejects(plugin.mount({plugin:()=>count++},config(),{McpClient:{},scopeOf:()=>undefined,verify:()=>count++}),/scoped_context_required/);assert.equal(count,0);
});
test('official Config and child lifecycle own registration, startup rejection propagates once',async()=>{
 const key={},calls=[];const McpClient={Config:c=>({...c,resolved:true})};
 const ctx={plugin:async(p,c)=>{calls.push([p,c]);throw Error('fixture_connect_failed');}};
 await assert.rejects(plugin.mount(ctx,config(),{McpClient,scopeOf:()=>key,verify:()=>{}}),/fixture_connect_failed/);
 assert.equal(calls.length,1);assert.equal(calls[0][0],McpClient);assert.equal(calls[0][1].resolved,true);
});
test('metadata validation never creates caches and rejects shared permissions or symlinks',()=>{
 const root=realpathSync(mkdtempSync(join(tmpdir(),'fleet-client-')));
 try {
  const nodePath=join(root,'node'),proxyPath=join(root,'proxy'),oauthDirectory=join(root,'auth');
  writeFileSync(nodePath,'fixture');writeFileSync(proxyPath,'fixture');mkdirSync(oauthDirectory,{mode:0o700});
  const c=buildConfig({...config(),nodePath,proxyPath,oauthDirectory});verifyPaths(c);
  chmodSync(oauthDirectory,0o755);assert.throws(()=>verifyPaths(c),/private_oauth_directory_required/);chmodSync(oauthDirectory,0o700);
  const parentLink=join(root,'parent-link');symlinkSync(root,parentLink);assert.throws(()=>verifyPaths({...c,command:join(parentLink,'node')}),/reviewed_regular_file_required/);
  const link=join(root,'link');symlinkSync(oauthDirectory,link);assert.throws(()=>verifyPaths({...c,env:{MCP_REMOTE_CONFIG_DIR:link}}),/private_oauth_directory_required/);
 } finally {rmSync(root,{recursive:true,force:true});}
});

test('package remains optional without auto-loading an outbound Host bundle',async()=>{
 const {readFileSync}=await import('node:fs');
 const pkg=JSON.parse(readFileSync(new URL('../package.json',import.meta.url),'utf8'));
 assert.equal(pkg.dsh,undefined);assert.equal(pkg.scripts,undefined);
 for(const file of pkg.files) assert.ok(readFileSync(new URL('../'+file,import.meta.url)).length);
 assert.equal(pkg.peerDependencies['@deepseek-ai/dsh-mcp-client'],'0.2.1-rc.3');
});
