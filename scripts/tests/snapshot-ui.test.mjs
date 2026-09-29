import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import vm from 'node:vm';
import { randomUUID } from 'node:crypto';
import ts from '../../hifimule-ui/node_modules/typescript/lib/typescript.js';

const memory = () => {
  const values = new Map();
  return { getItem: k => values.get(k) ?? null, setItem: (k,v) => values.set(k,v), removeItem: k => values.delete(k) };
};
function load(path, mocks, runtime = {}) {
  const exports = {};
  const source = ts.transpileModule(readFileSync(new URL(`../../hifimule-ui/src/${path}`,import.meta.url),'utf8'), { compilerOptions: { module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022 } }).outputText;
  vm.runInNewContext(source,{exports,require:name=>mocks[name] ?? (name.endsWith('/lifecycleDeadline') ? load('lifecycleDeadline.ts', {}, runtime) : {}),crypto:{randomUUID},localStorage:memory(),console,setTimeout,clearTimeout,...runtime});
  return exports;
}
const observed = () => ({instanceId:randomUUID(),sessionId:randomUUID(),queueRevision:'7',mainCurrent:{occurrenceId:randomUUID()},mode:'main'});
const missing = () => Object.assign(new Error('missing'),{data:{code:'SNAPSHOT_NOT_FOUND'}});
const flush = async () => {for(let i=0;i<20;i++) await Promise.resolve();};

test('save recovery persists exact intent before send and repeats original operation after reload',async()=>{
  const storage=memory(); const calls=[]; const s=observed();
  const rpc={playbackSaveSnapshot:async p=>{calls.push(JSON.parse(JSON.stringify(p)));assert.ok(storage.getItem('hifimule.snapshotSave.v1'));throw new Error('lost reply');},playbackGetSnapshot:async()=>{throw missing();},playbackGetSession:async()=>s};
  const {SnapshotSaveCoordinator}=load('state/snapshotSaves.ts',{'../rpc':rpc});
  const first=new SnapshotSaveCoordinator(storage);
  await first.save(s,'Snapshot 🎵');
  assert.equal(first.state.kind,'unknown'); assert.ok(first.pending);
  const secondModule=load('state/snapshotSaves.ts',{'../rpc':rpc});
  const second=new secondModule.SnapshotSaveCoordinator(storage);
  await second.recover();
  assert.deepEqual(calls[1],calls[0]); assert.ok(second.pending);
  rpc.playbackGetSnapshot=async()=>({snapshotId:randomUUID(),operationId:calls[0].operationId,name:'Snapshot 🎵'});
  await second.recover(); assert.equal(second.state.kind,'saved');assert.equal(second.pending,null);
  assert.equal(storage.getItem('hifimule.snapshotSave.v1'),null);
});

test('not-found while the original capture is in flight retains identity and forbids a replacement',async()=>{
  const storage=memory(), s=observed(), calls=[]; let finish;
  const rpc={playbackSaveSnapshot: p=>{calls.push(p);return calls.length===1 ? new Promise(resolve=>{finish=resolve;}) : Promise.reject(Object.assign(new Error('busy'),{data:{code:'PLAYBACK_BUSY'}}));},playbackGetSnapshot:async()=>{throw missing();},playbackGetSession:async()=>s};
  const {SnapshotSaveCoordinator}=load('state/snapshotSaves.ts',{'../rpc':rpc});
  const first=new SnapshotSaveCoordinator(storage); const running=first.save(s,'Frozen');
  const reopened=new SnapshotSaveCoordinator(storage); await reopened.recover();
  assert.equal(reopened.state.kind,'unknown'); assert.equal(reopened.pending.operationId,calls[0].operationId);
  await reopened.save(observed(),'Replacement'); assert.equal(calls.length,2);
  finish({status:'saved',schemaVersion:1,snapshot:{snapshotId:randomUUID(),operationId:calls[0].operationId,name:'Frozen'}}); await running;
  assert.equal(first.state.kind,'saved');
});

test('new-daemon not-found is checked twice and explains the no-commit outcome',async()=>{
  const storage=memory(),s=observed();let lookups=0,sends=0;
  const rpc={playbackSaveSnapshot:async()=>{sends++;throw new Error('lost');},playbackGetSnapshot:async()=>{lookups++;throw missing();},playbackGetSession:async()=>observed()};
  const {SnapshotSaveCoordinator}=load('state/snapshotSaves.ts',{'../rpc':rpc}); const coordinator=new SnapshotSaveCoordinator(storage);
  await coordinator.save(s,'name');await coordinator.recover();
  assert.equal(lookups,2);assert.equal(sends,1);assert.equal(coordinator.pending,null);assert.equal(coordinator.state.code,'SNAPSHOT_NOT_SAVED');
});

test('definite stale conflict clears the rejected request while storage and transport errors retain it',async()=>{
  for(const code of ['STALE_MAIN_OCCURRENCE','TERMINAL_PENDING','SNAPSHOT_STORAGE_FAILED','DAEMON_STOPPED']) {
    const rpc={playbackSaveSnapshot:async()=>{throw Object.assign(new Error(code),{data:{code}});}};
    const {SnapshotSaveCoordinator}=load('state/snapshotSaves.ts',{'../rpc':rpc});const coordinator=new SnapshotSaveCoordinator(memory());
    await coordinator.save(observed(),'name');
    assert.equal(!!coordinator.pending,['SNAPSHOT_STORAGE_FAILED','DAEMON_STOPPED'].includes(code));
  }
});

test('recovery storage failure never sends and a committed response stays success if clearing storage fails',async()=>{
  let calls=0;
  const rpc={playbackSaveSnapshot:async()=>{calls++;return {schemaVersion:1,status:'saved',snapshot:{snapshotId:randomUUID(),name:'saved'}};}};
  const {SnapshotSaveCoordinator}=load('state/snapshotSaves.ts',{'../rpc':rpc});
  const coordinator=new SnapshotSaveCoordinator({...memory(),setItem:()=>{throw new Error('quota');}});
  await coordinator.save(observed(),'name');assert.equal(calls,0);assert.equal(coordinator.state.code,'SNAPSHOT_RECOVERY_STORAGE');
  const committed=new SnapshotSaveCoordinator({...memory(),removeItem:()=>{throw new Error('unavailable');}});
  await committed.save(observed(),'name');assert.equal(committed.state.kind,'saved');assert.ok(committed.pending);
});

test('a hung save becomes result-unknown without cancelling or replacing its durable intent',async()=>{
  let timeout,finish;
  const rpc={playbackSaveSnapshot:()=>new Promise(resolve=>{finish=resolve;})};
  const {SnapshotSaveCoordinator}=load('state/snapshotSaves.ts',{'../rpc':rpc},{setTimeout:fn=>{timeout=fn;return 1;},clearTimeout(){}});
  const coordinator=new SnapshotSaveCoordinator(memory());const job=coordinator.save(observed(),'hung');const id=coordinator.pending.operationId;
  timeout();await job;assert.equal(coordinator.state.kind,'unknown');assert.equal(coordinator.pending.operationId,id);
  finish({status:'saved',schemaVersion:1,snapshot:{snapshotId:randomUUID()}});await flush();
  assert.equal(coordinator.state.kind,'unknown');assert.equal(coordinator.pending.operationId,id);
});

test('zero-server startup distinguishes saved content, unresolved save, genuine first-run and read failure',async()=>{
  const rpc={playbackListSnapshots:async(cursor,limit)=>{assert.equal(limit,1);return {snapshots:[]};}};
  const saves={pending:null,state:{kind:'idle'}};
  const {localContentRoute}=load('localContentRoute.ts',{'./rpc':rpc,'./state/snapshotSaves':{snapshotSaves:saves}});
  assert.equal(await localContentRoute(0),'onboarding');
  saves.pending={operationId:randomUUID()};assert.equal(await localContentRoute(0),'main');saves.pending=null;
  rpc.playbackListSnapshots=async()=>({snapshots:[{}]});assert.equal(await localContentRoute(0),'main');
  saves.state={kind:'error',code:'SNAPSHOT_RECOVERY_STORAGE'};
  assert.equal(await localContentRoute(0),'main');
  rpc.playbackListSnapshots=async()=>({snapshots:[]});assert.equal(await localContentRoute(0),'error');
  rpc.playbackListSnapshots=async()=>{throw new Error('db busy');};assert.equal(await localContentRoute(0),'error');
  assert.equal(await localContentRoute(1),'main');
});

class Element {
  children=[]; attributes={};listeners=new Map();dataset={};hidden=false;disabled=false;value='';textContent='';
  constructor(tag){this.tagName=tag;}
  append(...children){this.children.push(...children);}
  replaceChildren(...children){this.children=children;}
  setAttribute(k,v){this.attributes[k]=v;}
  addEventListener(k,fn){this.listeners.set(k,fn);}
  focus(){doc.activeElement=this;}
  click(){if(!this.disabled)return this.listeners.get('click')?.();}
}
const doc={createElement:tag=>new Element(tag),activeElement:null};
function all(node){return [node,...node.children.flatMap(all)];}
function text(node){return node.textContent+' '+node.children.map(text).join(' ');}
function component(rpc={}) {
  rpc={playbackListSnapshotPlaylistExports:async()=>[],playbackPlanSnapshotPlaylistExport:async()=>({parts:[]}),playbackStartSnapshotPlaylistExport:async()=>({parts:[]}),playbackRetrySnapshotPlaylistExport:async()=>({parts:[]}),playbackReconcileSnapshotPlaylistExport:async()=>({parts:[]}),...rpc};
  let subscriber,connectionSubscriber,saveSubscriber;let connection='fresh';const changes=[];const s=observed();
  const store={subscribe:fn=>{subscriber=fn;fn(s);return()=>{};},subscribeConnection:fn=>{connectionSubscriber=fn;fn(connection);return()=>{};},connection:()=>connection,refresh:async()=>{}};
  const saves={state:{kind:'idle'},pending:null,canSave:()=>true,subscribe:fn=>{saveSubscriber=fn;fn(saves.state);return()=>{};},save:async()=>{},recover:async()=>{}};
  const {PlaybackSnapshots}=load('components/PlaybackSnapshots.ts',{'../rpc':rpc,'../i18n':{t:(key,args)=>key+(args?' '+JSON.stringify(args):'')},'../state/playback':{playbackStore:store},'../state/snapshotSaves':{snapshotSaves:saves,snapshotErrorCode:e=>e?.data?.code}}, {document:doc});
  const view=new PlaybackSnapshots(show=>changes.push(show));
  return {view,changes,store,saveState:value=>saveSubscriber(value),live:value=>subscriber(value),connection:value=>{connection=value;connectionSubscriber(value);}};
}
const header=(id='saved')=>({snapshotId:id,name:id,createdAt:'2026-09-29T00:00:00Z',entryCount:'10001'});
const entry=(ordinal,id='a')=>({ordinal:String(ordinal),occurrenceId:id+ordinal,source:{serverId:'missing',trackId:'opaque'+ordinal},origin:'upcoming',sourceLabel:'Frozen source',title:null,artist:null,album:null,sourceIcon:null,sourceAvailable:false});

test('stale capture refreshes the live baseline before another explicit save',async()=>{
  const c=component();let finish,calls=0;
  c.store.refresh=()=>{calls++;return new Promise(resolve=>{finish=resolve;});};
  c.saveState({kind:'error',code:'STALE_MAIN_OCCURRENCE'});
  const save=all(c.view.element).find(e=>e.textContent==='playback.snapshots.save');
  assert.equal(calls,1);assert.equal(save.disabled,true);
  finish(observed());await flush();assert.equal(save.disabled,false);c.view.destroy();
});

test('saved pages remain bounded, read-only, focus-stable and independent of live polling',async()=>{
  let requests=0;
  const rpc={playbackListSnapshots:async()=>({snapshots:[header()],nextCursor:null}),playbackListSnapshotEntries:async(id,cursor,limit)=>{requests++;assert.equal(limit,50);const offset=cursor?Number(cursor):0;return {snapshotId:id,entries:Array.from({length:50},(_,i)=>entry(i+offset)),nextCursor:String(offset+50)};}};
  const c=component(rpc);c.view.showList();await flush();
  await all(c.view.element).find(e=>e.tagName==='button'&&e.textContent.startsWith('saved ·')).click();await flush();
  assert.equal(all(c.view.element).filter(e=>e.dataset.occurrenceId).length,50);
  assert.match(text(c.view.element),/Frozen source/);assert.match(text(c.view.element),/sourceUnavailable/);assert.match(text(c.view.element),/opaque0/);
  const next=all(c.view.element).find(e=>e.textContent==='playback.queue.next');next.focus();
  for(let i=0;i<70;i++){await next.click();await flush();}
  assert.equal(all(c.view.element).filter(e=>e.dataset.occurrenceId).length,50);
  assert.ok(c.view.previous.length<=64);assert.equal(doc.activeElement,next);
  assert.ok(all(c.view.element).filter(e=>e.dataset.occurrenceId).every(row=>!all(row).some(e=>e.tagName==='button')));
  const before=requests;c.live({...observed(),mode:'preview'});c.connection('stale');await flush();assert.equal(requests,before);
  assert.equal(all(c.view.element).find(e=>e.textContent==='playback.snapshots.save').disabled,true);
  c.view.destroy();
});

test('failed saved-page navigation leaves Previous pointing to the last loaded page',async()=>{
  const requested=[];
  const c=component({
    playbackListSnapshots:async()=>({snapshots:[header()],nextCursor:null}),
    playbackListSnapshotEntries:async(_id,cursor)=>{
      requested.push(cursor);
      if (cursor==='page-2') throw new Error('temporary read failure');
      return {snapshotId:'saved',entries:[entry(0)],nextCursor:'page-2'};
    },
  });
  c.view.showList();await flush();
  all(c.view.element).find(e=>e.textContent.startsWith('saved ·')).click();await flush();
  const next=all(c.view.element).find(e=>e.textContent==='playback.queue.next');
  const previous=all(c.view.element).find(e=>e.textContent==='playback.queue.previous');
  next.click();await flush();
  assert.equal(next.disabled,true);
  previous.click();await flush();
  assert.deepEqual(requested,[null,'page-2',null]);
  assert.equal(c.view.cursor,null);
  c.view.destroy();
});

test('late saved-page replies are discarded after selection change and destruction',async()=>{
  const pending=[];
  const c=component({playbackListSnapshots:async()=>({snapshots:[header('a'),header('b')],nextCursor:null}),playbackListSnapshotEntries:(id)=>new Promise(resolve=>pending.push({id,resolve}))});
  c.view.showList();await flush();
  all(c.view.element).find(e=>e.textContent.startsWith('a ·')).click();await flush();
  c.view.showList();await flush();all(c.view.element).find(e=>e.textContent.startsWith('b ·')).click();await flush();
  pending[1].resolve({snapshotId:'b',entries:[entry(0,'b')],nextCursor:null});await flush();
  pending[0].resolve({snapshotId:'a',entries:[entry(0,'a')],nextCursor:null});await flush();
  assert.deepEqual(all(c.view.element).filter(e=>e.dataset.occurrenceId).map(e=>e.dataset.occurrenceId),['b0']);
  c.view.showList();await flush();all(c.view.element).find(e=>e.textContent.startsWith('a ·')).click();await flush();c.view.destroy();
  pending[2].resolve({snapshotId:'a',entries:[entry(0,'late')],nextCursor:null});await flush();
  assert.equal(all(c.view.element).filter(e=>e.dataset.occurrenceId).length,0);
});

test('Preview explanation, polite result region and Escape return do not replace focused controls',async()=>{
  const c=component({playbackListSnapshots:async()=>({snapshots:[],nextCursor:null})});
  const input=all(c.view.element).find(e=>e.tagName==='input');input.focus();
  c.live({...observed(),mode:'preview'});assert.match(text(c.view.element),/previewMain/);assert.equal(doc.activeElement,input);
  c.live({...observed(),mainCurrent:null,mode:'preview'});assert.match(text(c.view.element),/previewOnly/);
  assert.ok(all(c.view.element).some(e=>e.attributes['aria-live']==='polite'));
  c.view.showList();await flush();let prevented=false;c.view.element.listeners.get('keydown')({key:'Escape',preventDefault(){prevented=true;}});
  assert.ok(prevented);assert.equal(c.changes.at(-1),false);assert.equal(doc.activeElement.textContent,'playback.snapshots.browse');c.view.destroy();
});

test('all four locales have every snapshot label and preserve interpolation variables',()=>{
  const catalog=JSON.parse(readFileSync(new URL('../../hifimule-i18n/catalog.json',import.meta.url),'utf8'));
  const keys=Object.keys(catalog.en).filter(k=>k.startsWith('playback.snapshots.'));
  assert.ok(keys.length>30);
  for(const locale of ['fr','es','de']) for(const key of keys){assert.ok(catalog[locale][key],`${locale}:${key}`);assert.deepEqual(catalog[locale][key].match(/\{\w+\}/g)?.sort(),catalog.en[key].match(/\{\w+\}/g)?.sort());}
});
