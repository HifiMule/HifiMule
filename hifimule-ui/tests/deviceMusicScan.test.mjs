import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import ts from 'typescript';
const source=readFileSync(new URL('../src/components/InitDeviceModal.ts',import.meta.url),'utf8').replace(/^import .*;\r?\n/gm,'').replace('export class','class');
const script=ts.transpile(source,{target:ts.ScriptTarget.ES2022});
class Element {
    children=new Map();listeners=new Map();hidden=false;loading=false;textContent='';isConnected=true;style={};
    set innerHTML(value){this.html=value;this.children.clear();for(const match of value.matchAll(/id="([^"]+)"/g)){const child=new Element();child.parent=this;child.id=match[1];this.children.set('#'+match[1],child);}}
    get innerHTML(){return (this.html ?? '') + [...this.children.values()].map(child=>child.innerHTML).join('');}
    querySelector(id){return this.children.get(id) ?? [...this.children.values()].map(child=>child.querySelector(id)).find(Boolean);}
    addEventListener(event,fn){this.listeners.set(event,fn);}
    click(){return this.listeners.get('click')?.();}
    remove(){this.parent?.children.delete('#'+this.id);}
    setAttribute(){}
    focus(){}
    prepend(child){child.parent=this;this.children.set('#'+child.id,child);}
    hide(){this.hidden=true;}
}
function setup(rpc){
    const window={setInterval:()=>1,clearInterval:()=>{}};
    const basketEvents=[];const basketStore={setDaemonContext:(...args)=>basketEvents.push(['context',...args]),hydrateFromDaemon:(items)=>basketEvents.push(['hydrate',items])};
    const {InitDeviceModal}=new Function('rpcCall','t','window','document','basketStore','withDeadline','setInterval','clearInterval',`${script}\nreturn {InitDeviceModal};`)(rpc,(key)=>key,window,{createElement:()=>new Element()},basketStore,promise=>promise,window.setInterval,window.clearInterval);
    const modal=Object.create(InitDeviceModal.prototype);modal.escapeHtml=value=>value;modal.dialog=new Element();modal.dialog.innerHTML='<button id="init-device-confirm-btn"></button><button id="init-device-cancel-btn"></button>';
    let completed=0;modal.onComplete=()=>{basketEvents.push(['complete']);completed++;};
    const body=new Element();modal.showMusicScan(body,'captured-scan','device-1');return {modal,body,basketEvents,completed:()=>completed};
}
test('setup asks before import independently of sync settings and declining finishes without starting a scan',async()=>{
    const calls=[];const ui=setup(async(method,params)=>{calls.push({method,params});return {cancelled:true};});
    assert.match(ui.body.innerHTML,/device.scan.ownership/);assert.deepEqual(calls,[]);
    await ui.body.querySelector('#scan-no').click();assert.deepEqual(calls,[{method:'device.music_scan_cancel',params:{scanId:'captured-scan'}}]);assert.equal(ui.completed(),1);
});
test('scan completion has one action and hydrates the basket before closing',async()=>{
    const calls=[];const items=[{type:'album',id:'album-1'}];
    const ui=setup(async(method,params)=>{
        calls.push({method,params});
        if(method==='manifest_get_basket')return {targetDeviceId:'device-1',basketHash:'imported-hash',basketItems:items};
        return {imported:3,unmatched:1,ambiguous:2,incompatible:1};
    });
    assert.equal(ui.modal.dialog.querySelector('#init-device-confirm-btn'),undefined);
    assert.equal(ui.modal.dialog.querySelector('#init-device-cancel-btn'),undefined);
    await ui.body.querySelector('#scan-yes').click();
    assert.equal(ui.completed(),0);assert.deepEqual(ui.basketEvents,[]);
    assert.match(ui.body.innerHTML,/device.scan.result/);
    assert.deepEqual([...ui.body.querySelector('#scan-content').children.keys()],['#scan-done']);
    await ui.body.querySelector('#scan-done').click();
    assert.deepEqual(calls,[{method:'device.music_scan_start',params:{scanId:'captured-scan'}},{method:'manifest_get_basket',params:undefined}]);
    assert.deepEqual(ui.basketEvents,[['context','device-1','imported-hash'],['hydrate',items],['complete']]);
    assert.equal(ui.completed(),1);assert.equal(ui.modal.dialog.hidden,true);
});
test('basket refresh failure can be retried without initializing or scanning again',async()=>{
    const calls=[];let fail=true;
    const ui=setup(async(method)=>{
        calls.push(method);
        if(method==='manifest_get_basket'){
            if(fail)throw new Error('temporarily unavailable');
            return {targetDeviceId:'device-1',basketItems:[]};
        }
        return {imported:1};
    });
    await ui.body.querySelector('#scan-yes').click();
    const done=ui.body.querySelector('#scan-done');await done.click();
    assert.equal(ui.completed(),0);assert.equal(done.disabled,false);
    assert.match(ui.body.querySelector('#scan-error').textContent,/temporarily unavailable/);
    fail=false;await done.click();
    assert.equal(ui.completed(),1);assert.equal(ui.body.querySelector('#scan-error').textContent,'');
    assert.deepEqual(calls,['device.music_scan_start','manifest_get_basket','manifest_get_basket']);
});
test('completion does not replace the basket when the selected destination changed',async()=>{
    const ui=setup(async(method)=>method==='manifest_get_basket'?{targetDeviceId:'another-device',basketItems:[{}]}:{imported:1});
    await ui.body.querySelector('#scan-yes').click();await ui.body.querySelector('#scan-done').click();
    assert.deepEqual(ui.basketEvents,[['complete']]);
});
test('completion waits for basket refresh and ignores repeated clicks while pending',async()=>{
    let resolve;let reads=0;
    const ui=setup(async(method)=>{
        if(method==='manifest_get_basket'){reads++;return new Promise(r=>resolve=r);}
        return {imported:1};
    });
    await ui.body.querySelector('#scan-yes').click();
    const done=ui.body.querySelector('#scan-done');const pending=done.click();await done.click();
    assert.equal(reads,1);assert.equal(ui.completed(),0);assert.equal(done.disabled,true);
    resolve({targetDeviceId:'device-1',basketItems:[]});await pending;assert.equal(ui.completed(),1);
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
    await ui.body.querySelector('#scan-no').click();assert.equal(ui.completed(),0);
    await ui.body.querySelector('#scan-dismiss').click();assert.equal(ui.completed(),1);
});
