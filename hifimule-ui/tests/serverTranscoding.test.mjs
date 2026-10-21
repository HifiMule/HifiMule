import * as html from '../src/html.ts';
import assert from 'node:assert/strict';
import test from 'node:test';
import vm from 'node:vm';
import { readFileSync } from 'node:fs';
import ts from 'typescript';
function harness(location) {
  const listeners = {}, calls = [], exports = {};
  const save = { loading:false, addEventListener: (_event, cb) => listeners.save = cb };
  const cancel = { disabled:false, addEventListener: (_event, cb) => listeners.cancel = cb };
  const error = { style:{}, textContent:'', open:false };
  const select = {value:'local'};
  const dialog = { innerHTML:'', hidden:false, addEventListener() {}, show() {}, hide() { this.hidden=true; },
    querySelectorAll: () => [], querySelector: key => ({'#server-identity-save':save,'#server-identity-cancel':cancel,'#server-identity-name':{value:'Music'},'#server-transcoding-location':select,'#server-identity-error':error})[key] };
  let reject = false;
  vm.runInNewContext(ts.transpileModule(readFileSync(new URL('../src/components/ServerHub.ts',import.meta.url),'utf8'),{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText,{
    exports, document:{createElement:()=>dialog,body:{appendChild(){}}},customElements:{whenDefined:async()=>{}},
    require:name=>name==='../html'?html:name==='../rpc'?{serverUpdate:async params=>{calls.push(params);if(reject)throw new Error('offline');}}:name==='../serverIdentity'?{SERVER_ICON_OPTIONS:['music-note'],formatServerIdentity:()=>({label:'Music',icon:'music-note',secondaryText:'server'})}:name==='../i18n'?{t:key=>key}:{},
  });
  const hub=Object.create(exports.ServerHub.prototype);hub.servers=[{id:'one',transcodingLocation:location}];hub.refresh=async()=>{};hub.onServerChanged=()=>{};
  hub.handleEdit('one');return {listeners,calls,dialog,save,cancel,error,select,setReject(){reject=true;}};
}
test('encoding selector defaults to server and reflects saved local setting',()=>{
  assert.match(harness(undefined).dialog.innerHTML,/value="server"/);
  assert.match(harness('local').dialog.innerHTML,/value="local"/);
});
test('save submits server identity and encoding together and closes after success',async()=>{
  const h=harness('server');await h.listeners.save();assert.equal(h.calls.length,1);assert.equal(h.calls[0].transcodingLocation,'local');assert.equal(h.calls[0].id,'one');assert.equal(h.calls[0].name,'Music');assert.equal(h.dialog.hidden,true);
});
test('failed update retains the selected encoding and unlocks retry',async()=>{
  const h=harness('server');h.setReject();await h.listeners.save();assert.equal(h.dialog.hidden,false);assert.equal(h.select.value,'local');assert.equal(h.error.open,true);assert.equal(h.save.loading,false);assert.equal(h.cancel.disabled,false);
});
