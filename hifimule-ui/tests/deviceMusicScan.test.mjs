import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import ts from 'typescript';
const source=readFileSync(new URL('../src/components/InitDeviceModal.ts',import.meta.url),'utf8').replace(/^import .*;\r?\n/gm,'').replace('export class','class');
const script=ts.transpile(source,{target:ts.ScriptTarget.ES2022});
class Element {
    children=new Map();listeners=new Map();hidden=false;loading=true;textContent='';
    set innerHTML(value){this.html=value;this.children.clear();for(const match of value.matchAll(/id="([^"]+)"/g))this.children.set('#'+match[1],new Element());}
    get innerHTML(){return this.html;}
    querySelector(id){return this.children.get(id);}
    addEventListener(event,fn){this.listeners.set(event,fn);}
    click(){return this.listeners.get('click')?.();}
    prepend(){}
    hide(){this.hidden=true;}
}
function setup(rpc){
    const window={setInterval:()=>1,clearInterval:()=>{}};
    const {InitDeviceModal}=new Function('rpcCall','t','window','document',`${script}\nreturn {InitDeviceModal};`)(rpc,(key)=>key,window,{createElement:()=>new Element()});
    const modal=Object.create(InitDeviceModal.prototype);modal.dialog=new Element();modal.dialog.innerHTML='<button id="init-device-confirm-btn"></button><button id="init-device-cancel-btn"></button>';
    let completed=0;modal.onComplete=()=>completed++;
    const body=new Element();modal.showMusicScan(body,'captured-scan');return {modal,body,completed:()=>completed};
}
test('setup asks before import and declining finishes without starting a scan',async()=>{
    const calls=[];const ui=setup(async(method,params)=>calls.push({method,params}));
    assert.match(ui.body.innerHTML,/device.scan.ownership/);assert.deepEqual(calls,[]);
    await ui.body.querySelector('#scan-no').click();assert.deepEqual(calls,[{method:'device.music_scan_cancel',params:{scanId:'captured-scan'}}]);assert.equal(ui.completed(),1);
});
test('accepted scan reports result and refreshes basket only after completion',async()=>{
    const calls=[];const ui=setup(async(method,params)=>{calls.push({method,params});return {imported:3,unmatched:1,ambiguous:2,incompatible:1};});
    await ui.body.querySelector('#scan-yes').click();assert.equal(calls[0].method,'device.music_scan_start');assert.equal(calls[0].params.scanId,'captured-scan');assert.equal(ui.completed(),0);
    assert.match(ui.body.innerHTML,/device.scan.result/);await ui.body.querySelector('#scan-done').click();assert.equal(ui.completed(),1);
});
test('cancelling an active setup scan sends its captured identity and waits for cancellation',async()=>{
    const calls=[];let reject;
    const ui=setup(async(method)=>{calls.push(method);if(method==='device.music_scan_start')return new Promise((_,r)=>reject=r);return {cancelled:true};});
    const running=ui.body.querySelector('#scan-yes').click();await ui.body.querySelector('#scan-cancel').click();assert.equal(ui.completed(),0);
    reject(new Error('cancelled'));await running;assert.deepEqual(calls,['device.music_scan_start','device.music_scan_cancel']);assert.equal(ui.completed(),1);
});
test('failed scan remains retryable without claiming completion',async()=>{
    const ui=setup(async()=>{throw new Error('device disconnected');});await ui.body.querySelector('#scan-yes').click();assert.equal(ui.completed(),0);assert.ok(ui.body.querySelector('#scan-yes'));
});

test('cancel racing a committed import preserves the successful result',async()=>{
    let resolve;
    const ui=setup(async(method)=>method==='device.music_scan_start'?new Promise(r=>resolve=r):{committed:true,cancelled:false});
    const running=ui.body.querySelector('#scan-yes').click();await ui.body.querySelector('#scan-cancel').click();
    resolve({imported:1,unmatched:0,ambiguous:0,incompatible:0});await running;
    assert.equal(ui.completed(),0);assert.match(ui.body.innerHTML,/device.scan.result/);
    await ui.body.querySelector('#scan-done').click();assert.equal(ui.completed(),1);
});
test('expired proposal can be dismissed without initializing the device again',async()=>{
    const ui=setup(async()=>{throw new Error('Music scan expired');});
    await ui.body.querySelector('#scan-yes').click();
    await ui.body.querySelector('#scan-no').click();assert.equal(ui.completed(),1);
});
