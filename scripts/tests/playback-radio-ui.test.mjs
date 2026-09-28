import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import vm from 'node:vm';
import ts from '../../hifimule-ui/node_modules/typescript/lib/typescript.js';

class Element {
  constructor(tagName) { this.tagName = tagName; }
  children = []; attributes = {}; listeners = new Map(); dataset = {}; hidden = false; disabled = false;
  classList = { add() {} };
  append(...items) { this.children.push(...items); }
  replaceChildren(...items) { this.children = items; }
  setAttribute(name, value) { this.attributes[name] = value; }
  addEventListener(name, handler) { this.listeners.set(name, handler); }
  click() { this.listeners.get('click')?.(); }
  find(predicate) { return predicate(this) ? this : this.children.map(child => child.find(predicate)).find(Boolean); }
  focus() {}
}

function load() {
  const source = ts.transpileModule(readFileSync(new URL('../../hifimule-ui/src/components/PlaybackDestination.ts', import.meta.url), 'utf8'),
    { compilerOptions: { module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022 } }).outputText;
  const exports = {};
  const calls = [];
  const playbackStore = { subscribe:() => () => {},refresh:async () => ({}) };
  const mocks = {
    playbackControl:async action => { calls.push(action); },
    playbackListOccurrences:async (_snapshot,_cursor,_limit,{section}) => ({ section,mainCurrentOccurrenceId:null,occurrences:[],sectionCount:0,nextCursor:null }),
    serverList:async () => [],
  };
  const document = { createElement:tag => new Element(tag) };
  vm.runInNewContext(source,{exports,document,console,setTimeout,clearTimeout,require:name => ({
    '../rpc':mocks,
    '../i18n':{t:key => key},
    '../state/playback':{playbackStore},
    '../serverIdentity':{formatServerIdentity:() => ({label:'server'})},
    './PlaybackSelectionSettings':{PlaybackSelectionSettings:class { element = new Element('section'); destroy() {} }},
  })[name]});
  const root = new Element('div');
  return { destination:new exports.PlaybackDestination(root),root,calls };
}

test('Radio waiting state is live, explained and has an accessible retry', async () => {
  const {destination,root,calls} = load();
  const snapshot = { instanceId:'instance',sessionId:'session',queueRevision:'2',mainCurrent:null,
    queueKind:'radio',radio:{logicalId:'logical',center:null,status:'waiting',reason:'radio.exhausted'},
    current:{occurrenceId:'current',source:{serverId:'one',trackId:'first'}},generationId:'generation' };
  await destination.receive(snapshot);
  const status = root.find(node => node.textContent === 'playback.radio.exhausted');
  const retry = root.find(node => node.textContent === 'playback.radio.retry');
  assert.equal(status.attributes['aria-live'],'polite');
  assert.equal(status.attributes.role,'status');
  assert.equal(status.hidden,false);
  assert.equal(retry.hidden,false);
  assert.equal(retry.type,'button');
  retry.click();
  await Promise.resolve();
  assert.deepEqual(calls,['retry']);
  destination.destroy();
});

test('accepted artist transition keeps its evidence explanation after reconnect', async () => {
  const {destination,root} = load();
  const snapshot = { instanceId:'instance',sessionId:'session',queueRevision:'3',mainCurrent:null,
    queueKind:'radio',radio:{logicalId:'logical',center:{serverId:'one',artistId:'artist'},status:'ready',
      reason:'radio.similarArtist',transition:{center:{serverId:'one',artistId:'artist'},kind:'similarArtist',reason:'radio.similarArtist'}},
    current:{occurrenceId:'current',source:{serverId:'one',trackId:'first'}},generationId:'generation' };
  await destination.receive(snapshot);
  const status = root.find(node => node.textContent === 'playback.radio.similarArtist');
  const retry = root.find(node => node.textContent === 'playback.radio.retry');
  assert.equal(status?.attributes['aria-live'],'polite');
  assert.equal(retry.hidden,true);
  destination.destroy();
});

test('a short transition refill explains both its evidence and waiting state', async () => {
  const {destination,root} = load();
  const snapshot = { instanceId:'instance',sessionId:'session',queueRevision:'4',mainCurrent:null,
    queueKind:'radio',radio:{logicalId:'logical',center:{serverId:'one',artistId:'artist'},status:'waiting',
      reason:'radio.exhausted',transition:{center:{serverId:'one',artistId:'artist'},kind:'similarArtist',reason:'radio.similarArtist'}},
    current:{occurrenceId:'current',source:{serverId:'one',trackId:'first'}},generationId:'generation' };
  await destination.receive(snapshot);
  const status = root.find(node => node.textContent === 'playback.radio.similarArtist playback.radio.exhausted');
  assert.equal(status?.attributes['aria-live'],'polite');
  assert.equal(status?.attributes.role,'status');
  destination.destroy();
});

test('Radio transition and cycle explanations exist in all four locales', () => {
  const catalog = JSON.parse(readFileSync(new URL('../../hifimule-i18n/catalog.json', import.meta.url), 'utf8'));
  for (const locale of ['en','fr','es','de']) {
    for (const reason of ['snapshotUnavailable','sharedTrackCredit','similarArtist','newStartingPoint','newCycle','cyclePending','exhausted','sourceFailure']) {
      assert.ok(catalog[locale][`playback.radio.${reason}`]?.trim(), `${locale}: ${reason}`);
    }
  }
});
