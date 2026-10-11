import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import test from 'node:test';
import ts from 'typescript';

const source=readFileSync(new URL('../src/rpc.ts',import.meta.url),'utf8')
    .replace(/^import .*;\r?\n/gm,'').replace(/export /g,'');
const script=ts.transpile(source,{target:ts.ScriptTarget.ES2022});

test('shared state RPC requests compact polling and honors explicit legacy access',async()=>{
    const calls=[];
    const api=new Function('invoke',`${script}\nreturn {rpcCall,getDaemonState};`)(async(method,payload)=>{calls.push({method,payload});return {destinationRevision:'1',destinations:[]};});
    await api.getDaemonState();
    await api.rpcCall('get_daemon_state');
    await api.rpcCall('get_daemon_state',{includeManifest:true});
    assert.deepEqual(calls.map(call=>call.payload.params),[{includeManifest:false},{includeManifest:false},{includeManifest:true}]);
});
