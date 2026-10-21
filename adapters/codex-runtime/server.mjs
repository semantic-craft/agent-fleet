import { createServer } from 'node:http';
import { dirname, isAbsolute } from 'node:path';
import { mkdirSync, lstatSync, chmodSync } from 'node:fs';
import { WebSocketServer, WebSocket } from 'ws';

/** A fresh private Unix endpoint; never unlink/replace an existing socket. */
export async function serve({socketPath,manager,onClose=()=>{}}) {
  if(!isAbsolute(socketPath))throw new Error('absolute socketPath required');
  mkdirSync(dirname(socketPath),{recursive:true,mode:0o700});
  const dir=lstatSync(dirname(socketPath));
  if(dir.isSymbolicLink()||!dir.isDirectory()||dir.uid!==process.getuid()||(dir.mode&0o077))throw new Error('private socket directory required');
  const server=createServer(async(req,r)=>{
    if(req.method==='GET'&&req.url==='/health'){r.setHeader('content-type','application/json');r.end(JSON.stringify({ok:true,service:"codex-owned-runtime",pid:process.pid,units:manager.status()}));return;}
    if(req.method==='POST'&&req.url==='/shutdown'){
      if(manager.draining){r.writeHead(409);r.end('already draining');return;}
      manager.draining=true;
      if(manager.status().some(u=>u.state!=='released')){manager.draining=false;r.writeHead(409);r.end('owned workers remain; shutdown refused');return;}
      r.setHeader('content-type','application/json');r.end(JSON.stringify({ok:true,pid:process.pid}));setImmediate(()=>close().catch(()=>{}));return;
    }
    r.writeHead(404);r.end();
  });
  const wss=new WebSocketServer({noServer:true,maxPayload:8*1024*1024});let client;
  server.on('upgrade',(req,socket,head)=>{
    if(req.url!=='/rpc'||(client&&client.readyState!==WebSocket.CLOSED)){socket.destroy();return;}
    wss.handleUpgrade(req,socket,head,ws=>wss.emit('connection',ws));
  });
  const send=p=>{if(client?.readyState===WebSocket.OPEN)client.send(JSON.stringify(p));};
  manager.on('packet',send);
  wss.on('connection',ws=>{
    client=ws;let initialized=false,initializing=false;
    ws.on('message',async raw=>{
      let p;
      try {
        p=JSON.parse(raw.toString());
        if(!p||typeof p!=='object')throw new Error('invalid packet');
        if(!p.method){if(!initialized)throw new Error('initialize required');manager.reply(p.id, p.error?{error:p.error}:{result:p.result});return;}
        if(p.method==='initialized'){if(!initialized)throw new Error('initialize required');for(const pending of manager.pendingPackets())ws.send(JSON.stringify(pending));return;}
        if(p.id===undefined)throw new Error('request id required');
        let result;
        if(p.method==='initialize'){if(initialized||initializing)throw new Error('already initialized');initializing=true;result=await manager.initialize();initialized=true;}
        else {if(!initialized)throw new Error('initialize required');result=await manager.call(p.method,p.params??{});}
        if(ws.readyState===WebSocket.OPEN)ws.send(JSON.stringify({id:p.id,result}));
      }catch(e){if(p?.id!==undefined&&ws.readyState===WebSocket.OPEN)ws.send(JSON.stringify({id:p.id,error:e.native??{code:e.code??-32001,message:e.message}}));}
    });
    // Disconnect deliberately does not close workers or replay input.
    ws.on('close',()=>{if(client===ws)client=undefined;});ws.on('error',()=>{});
  });
  await new Promise((resolve,reject)=>{server.once('error',reject);server.listen(socketPath,resolve);});chmodSync(socketPath,0o600);
  const close=async()=>{
    manager.draining=true;
    if(manager.status().some(u=>u.state!=='released')){manager.draining=false;throw new Error('owned workers remain; service close refused');}
    manager.off('packet',send);for(const c of wss.clients)c.terminate();await new Promise(resolve=>server.close(resolve));wss.close();clearInterval(manager.timer);await onClose();
  };
  return {server,wss,close};
}
