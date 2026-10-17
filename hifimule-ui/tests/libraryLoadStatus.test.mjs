import assert from 'node:assert/strict';
import test from 'node:test';
import vm from 'node:vm';
import { readFileSync } from 'node:fs';
import ts from 'typescript';
const source=ts.transpileModule(readFileSync(new URL('../src/components/libraryLoadStatus.ts',import.meta.url),'utf8'),{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText;
function harness(){
 const frames=[],exports={};
 const element=()=>({attributes:{},textContent:'',isConnected:true,setAttribute(k,v){this.attributes[k]=v;}});
 const content=element();content.before=node=>{content.sibling=node;};
 vm.runInNewContext(source,{exports,document:{createElement:element},requestAnimationFrame:fn=>frames.push(fn),require:()=>({t:(key,values)=>`${key}${values?.count??''}`})});
 return {content,status:exports.libraryLoadStatus(content),again:()=>exports.libraryLoadStatus(content),flush:()=>{while(frames.length)frames.shift()();}};
}
test('live region survives content replacement and repeated access',()=>{
 const h=harness(),region=h.content.sibling;
 h.status.start();h.flush();assert.equal(region.textContent,'library.loading.pending');
 h.content.innerHTML='new results';assert.equal(h.again(),h.status);assert.equal(h.content.sibling,region);
 h.status.complete(42);h.flush();assert.equal(region.textContent,'library.loading.complete42');assert.equal(h.content.attributes['aria-busy'],'false');
});
test('fast completion suppresses obsolete loading; failure cannot become success in finally',()=>{
 const h=harness();h.status.start();h.status.complete(0);h.flush();assert.equal(h.content.sibling.textContent,'library.loading.empty0');
 h.status.start();h.status.fail();h.status.complete(42);h.flush();assert.equal(h.content.sibling.textContent,'library.loading.failed');assert.equal(h.content.attributes['aria-busy'],'false');
 h.status.start();h.status.complete(1);h.flush();assert.equal(h.content.sibling.textContent,'library.loading.complete1');
});
test('detached view never announces queued results',()=>{
 const h=harness();h.status.start();h.status.complete(2);h.content.isConnected=false;h.flush();assert.equal(h.content.sibling.textContent,'');
});
