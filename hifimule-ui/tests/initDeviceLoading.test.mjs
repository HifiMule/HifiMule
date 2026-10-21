import * as html from '../src/html.ts';
import assert from 'node:assert/strict';
import test from 'node:test';
import vm from 'node:vm';
import {readFileSync} from 'node:fs';
import ts from 'typescript';
const transpile = path => ts.transpileModule(readFileSync(new URL(path, import.meta.url),'utf8'), {compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText;
function harness() {
 const timers=new Map(); let id=0;
 const clock={setTimeout:fn=>{timers.set(++id,fn);return id;},clearTimeout:id=>timers.delete(id)};
 const deadline={exports:{},Error,Promise,...clock}; vm.runInNewContext(transpile('../src/lifecycleDeadline.ts'),deadline);
 const body={isConnected:true,innerHTML:'',contains:()=>false,setAttribute(){},removeAttribute(){},querySelector:()=>null};
 const confirm={disabled:false}; const calls=[]; const renders=[];
 let rpc=()=>new Promise(()=>{});
 const ctx={exports:{},Error,Promise,document:{activeElement:null},require:name=>name==='../html'?html:name==='../rpc'?{rpcCall:method=>{calls.push(method);return rpc(method);}}:name==='../lifecycleDeadline'?deadline.exports:name==='../i18n'?{t:key=>key}: {}};
 vm.runInNewContext(transpile('../src/components/InitDeviceModal.ts'),ctx);
 const modal=new ctx.exports.InitDeviceModal({}); modal.dialog={querySelector:selector=>selector==='.init-device-body'?body:confirm};
 modal.renderContent=(_body,user,profiles)=>renders.push({user,profiles});modal.renderError=(_body,error)=>renders.push({error});
 return {modal,body,confirm,calls,renders,setRpc:fn=>rpc=fn,expire:()=>{for(const fn of [...timers.values()])fn();}};
}
test('credential rejection produces a retryable error, not logged-out content',async()=>{
 const h=harness();h.setRpc(async method=>{if(method==='get_credentials')throw 'Connection lost';return [];});
 await h.modal.loadCredentials();assert.equal(h.renders[0].error,'Connection lost');assert.equal(h.confirm.disabled,true);
 h.setRpc(async method=>method==='get_credentials'?{userId:'listener'}:[]);await h.modal.loadCredentials();assert.equal(h.renders[1].user,'listener');
});
test('hung reads expire and duplicate loading is suppressed',async()=>{
 const h=harness();const pending=h.modal.loadCredentials();await h.modal.loadCredentials();assert.equal(h.calls.length,2);
 h.expire();await pending;assert.equal(h.renders[0].error,'device.init.load_timeout');
});
test('profile timeout falls back without discarding credentials; late response is ignored',async()=>{
 const h=harness();let resolve;h.setRpc(method=>method==='get_credentials'?Promise.resolve({userId:'listener'}):new Promise(done=>resolve=done));
 const pending=h.modal.loadCredentials();for(let i=0;i<10;i++)await Promise.resolve();h.expire();await pending;
 assert.equal(h.renders[0].user,'listener');assert.equal(h.renders[0].profiles[0].id,'passthrough');
 resolve([{id:'late'}]);await Promise.resolve();assert.equal(h.renders.length,1);
});
test('closed dialogs discard pending results',async()=>{
 const h=harness();const pending=h.modal.loadCredentials();h.body.isConnected=false;h.expire();await pending;assert.equal(h.renders.length,0);
});
test('successful response with no credentials still follows login-required path',async()=>{
 const h=harness();h.setRpc(async()=>({}));await h.modal.loadCredentials();assert.equal(h.renders[0].user,null);
});
