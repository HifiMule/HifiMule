import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import vm from 'node:vm';
import ts from '../../hifimule-ui/node_modules/typescript/lib/typescript.js';

class Element {
  children = []; dataset = {}; attributes = {}; listeners = new Map();
  textContent = ''; hidden = false; disabled = false; isConnected = true; className = '';
  constructor(tag) { this.tagName = tag; }
  get classList() {
    return {
      add: (...names) => {
        const classes = new Set(this.className.split(/\s+/).filter(Boolean));
        for (const name of names) classes.add(name);
        this.className = [...classes].join(' ');
      },
      contains: name => this.className.split(/\s+/).includes(name),
    };
  }
  setAttribute(key, value) { this.attributes[key] = String(value); }
  getAttribute(key) { return this.attributes[key] ?? null; }
  append(...children) { for (const child of children) { if (child.parentElement) child.remove(); child.parentElement = this; this.children.push(child); } }
  replaceChildren(...children) { for (const child of this.children) child.parentElement = null; this.children = []; this.append(...children); }
  remove() { if (!this.parentElement) return; this.parentElement.children = this.parentElement.children.filter(child => child !== this); this.parentElement = null; }
  addEventListener(name, listener) { this.listeners.set(name, listener); }
  removeEventListener() {}
  focus() { document.activeElement = this; }
  async click() { await this.listeners.get('click')?.(); }
  querySelectorAll(selector) {
    const own = selector === 'button' && this.tagName === 'button' ? [this] : [];
    return own.concat(this.children.flatMap(child => child.querySelectorAll(selector)));
  }
}
const document = { activeElement: null, createElement: tag => new Element(tag) };
function text(element) { return element.textContent + element.children.map(text).join(' '); }

function load(relative, mocks) {
  const exports = {};
  const source = ts.transpileModule(readFileSync(new URL(relative, import.meta.url), 'utf8'), {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
  }).outputText;
  vm.runInNewContext(source, { exports, document, window: { addEventListener() {}, removeEventListener() {} }, console,
    setTimeout: () => 1, clearTimeout() {}, require: name => mocks[name] ?? {} });
  return exports;
}

test('destination hub renders Playback first and explicit selection never steals focus', async () => {
  const calls = [];
  const state = { destinationRevision: '4', destinations: [
    { kind: 'playback', selected: true },
    { kind: 'device', path: '/media/player', deviceId: 'd1', name: 'Player', icon: null, selected: false },
    { kind: 'pendingDevice', pendingId: 'p1', name: 'Blank', selected: false },
  ], deviceDiscoveryIssues: [] };
  const { DestinationHub } = load('../../hifimule-ui/src/components/DestinationHub.ts', {
    '../rpc': { getDaemonState: async () => state, destinationSelect: async selection => calls.push(selection) },
    '../i18n': { t: key => key }, './InitDeviceModal': { InitDeviceModal: class {} },
  });
  const container = new Element('nav');
  const hub = new DestinationHub(container, () => {});
  await hub.refresh();
  const buttons = container.querySelectorAll('button');
  assert.equal(buttons[0].dataset.destinationKind, 'playback');
  buttons[1].focus();
  await hub.applyState({ ...state, destinationRevision: '5', destinations: state.destinations.map((item, index) => ({ ...item, selected: index === 1 })) }, true);
  assert.equal(document.activeElement, buttons[1]);
  assert.equal(container.attributes['aria-label'], 'destination.group');
  await buttons[0].click();
  assert.equal(JSON.stringify(calls), JSON.stringify([{ kind: 'playback' }]));
  hub.destroy();
});

test('playback store accepts only a new owner or strictly newer decimal sequence', () => {
  const { PlaybackStore } = load('../../hifimule-ui/src/state/playback.ts', { '../rpc': {} });
  const store = new PlaybackStore(async () => { throw new Error('unused'); });
  const base = { instanceId: 'i', sessionId: 's', stateSequence: '9' };
  assert.equal(store.accept(base), true);
  assert.equal(store.accept({ ...base, stateSequence: '8' }), false);
  assert.equal(store.accept({ ...base, stateSequence: '10' }), true);
  assert.equal(store.accept({ ...base, instanceId: 'new', stateSequence: '1' }), true);
});

test('playback destination keeps Preview separate and describes one bounded canonical page', async () => {
  let subscriber; let metadataCalls = 0; let browseCalls = 0;
  const snapshot = { instanceId: 'i', sessionId: 's', queueRevision: '7', stateSequence: '1', mode: 'preview',
    preview: { auditionId: 'a' }, playback: { metadata: { title: 'Audition' } } };
  const { PlaybackDestination } = load('../../hifimule-ui/src/components/PlaybackDestination.ts', {
    '../state/playback': { playbackStore: { subscribe(callback) { subscriber = callback; return () => {}; }, refresh: async () => snapshot } },
    '../rpc': {
      serverList: async () => [{ serverId: 'srv', name: 'Living room' }],
      playbackListOccurrences: async () => ({ occurrences: [
        { occurrenceId: 'o1', source: { serverId: 'srv', trackId: 't' } },
        { occurrenceId: 'o2', source: { serverId: 'srv', trackId: 't' } },
      ], nextCursor: null, totalOccurrenceCount: 2 }),
      playbackDescribeOccurrences: async (_observed, ids) => { metadataCalls++; assert.deepEqual(Array.from(ids), ['o1', 'o2']); return ids.map(id => ({ occurrenceId: id, source: { serverId: 'srv', trackId: 't' }, title: 'Song', artist: 'Artist', album: null, durationMs: 1, status: 'available' })); },
    },
    '../i18n': { t: (key, values) => values?.title ? `${key}:${values.title}` : values?.count != null ? `${key}:${values.count}` : key },
    '../serverIdentity': { formatServerIdentity: server => ({ label: server.name }) },
  });
  const container = new Element('section');
  container.className = 'content layout-host';
  const destination = new PlaybackDestination(container, () => { browseCalls++; });
  subscriber(snapshot);
  for (let turn = 0; turn < 12; turn++) await Promise.resolve();
  assert.equal(metadataCalls, 1);
  assert.equal(container.classList.contains('content'), true);
  assert.equal(container.classList.contains('layout-host'), true);
  assert.equal(container.classList.contains('playback-destination'), true);
  assert.match(text(container), /playback\.queue\.preview:Audition/);
  assert.equal((text(container).match(/Song/g) ?? []).length, 2);
  const browse = container.querySelectorAll('button').find(button => button.textContent === 'playback.queue.back_to_library');
  assert.ok(browse, 'Playback must always expose a native return-to-library action');
  browse.focus();
  subscriber({ ...snapshot, stateSequence: '2' });
  for (let turn = 0; turn < 12; turn++) await Promise.resolve();
  const rerenderedBrowse = container.querySelectorAll('button').find(button => button.textContent === 'playback.queue.back_to_library');
  assert.equal(rerenderedBrowse, browse, 'queue refresh must preserve the focused library action node');
  assert.equal(document.activeElement, browse);
  await browse.click();
  assert.equal(browseCalls, 1);
  destination.destroy();
});

test('disposed playback destination ignores a late rejected page', async () => {
  let subscriber; let rejectPage; let refreshCalls = 0;
  const snapshot = { instanceId: 'i', sessionId: 's', queueRevision: '7', stateSequence: '1', mode: 'main',
    playback: { metadata: null } };
  const { PlaybackDestination } = load('../../hifimule-ui/src/components/PlaybackDestination.ts', {
    '../state/playback': { playbackStore: { subscribe(callback) { subscriber = callback; return () => {}; }, refresh: async () => { refreshCalls++; return snapshot; } } },
    '../rpc': {
      serverList: async () => [],
      playbackListOccurrences: async () => new Promise((_resolve, reject) => { rejectPage = reject; }),
    },
    '../i18n': { t: key => key }, '../serverIdentity': { formatServerIdentity: () => ({ label: '' }) },
  });
  const container = new Element('section');
  const destination = new PlaybackDestination(container, () => {});
  subscriber(snapshot);
  for (let turn = 0; turn < 4; turn++) await Promise.resolve();
  destination.destroy();
  rejectPage(new Error('late failure'));
  for (let turn = 0; turn < 12; turn++) await Promise.resolve();
  assert.doesNotMatch(text(container), /playback\.queue\.recoverable_error/);
  assert.equal(refreshCalls, 0);
});

test('playback destination keeps the library return action in recoverable error state', async () => {
  let subscriber; let refreshCalls = 0; let browseCalls = 0;
  const snapshot = { instanceId: 'i', sessionId: 's', queueRevision: '7', stateSequence: '1', mode: 'main',
    playback: { metadata: null } };
  const { PlaybackDestination } = load('../../hifimule-ui/src/components/PlaybackDestination.ts', {
    '../state/playback': { playbackStore: { subscribe(callback) { subscriber = callback; return () => {}; }, refresh: async () => { refreshCalls++; return snapshot; } } },
    '../rpc': { serverList: async () => [], playbackListOccurrences: async () => { throw new Error('stale'); } },
    '../i18n': { t: key => key }, '../serverIdentity': { formatServerIdentity: () => ({ label: '' }) },
  });
  const container = new Element('section');
  const destination = new PlaybackDestination(container, () => { browseCalls++; });
  subscriber(snapshot);
  for (let turn = 0; turn < 12; turn++) await Promise.resolve();
  assert.match(text(container), /playback\.queue\.recoverable_error/);
  const browse = container.querySelectorAll('button').find(button => button.textContent === 'playback.queue.back_to_library');
  assert.ok(browse, 'queue recovery must not trap the user away from the library');
  await browse.click();
  assert.equal(browseCalls, 1);
  assert.equal(refreshCalls, 1);
  destination.destroy();
});

test('main wires local library browsing without changing the daemon destination', () => {
  const source = readFileSync(new URL('../../hifimule-ui/src/main.ts', import.meta.url), 'utf8');
  assert.match(source, /new PlaybackDestination\(playback,\s*showLibrarySurface\)/);
  const helper = source.match(/function showLibrarySurface\(\): void \{([\s\S]*?)\n\}/)?.[1] ?? '';
  assert.match(helper, /library\.hidden = false/);
  assert.match(helper, /browse\.hidden = false/);
  assert.match(helper, /playback\.hidden = true/);
  assert.match(helper, /activePlaybackDestination\?\.destroy\(\)/);
  assert.match(helper, /activePlaybackDestination = null/);
  assert.match(helper, /playback\?\.replaceChildren\(\)/);
  assert.match(helper, /browse\?\.querySelector<HTMLElement>\('button:not\(\[disabled\]\)'\)/);
  assert.match(helper, /focusTarget\.focus\(\)/);
  assert.match(helper, /library\.focus\(\)/);
  assert.doesNotMatch(helper, /destinationSelect|playbackStore|activePlaybackControls|activeBasketSidebar/);
  assert.match(source, /new DestinationHub\(destinationContainer, \(\) => \{ void refreshDestinationView\(\); \}\)/,
    'activating the selected Playback destination must restore its queue');
});

test('library return action has complete locale parity', () => {
  const catalog = JSON.parse(readFileSync(new URL('../../hifimule-i18n/catalog.json', import.meta.url), 'utf8'));
  assert.deepEqual(Object.keys(catalog).sort(), ['de', 'en', 'es', 'fr']);
  for (const locale of Object.keys(catalog)) {
    assert.equal(typeof catalog[locale]['playback.queue.back_to_library'], 'string');
    assert.ok(catalog[locale]['playback.queue.back_to_library'].trim().length > 0);
  }
});

test('destination CSS makes hidden siblings authoritative and bounds Playback scrolling', () => {
  const css = readFileSync(new URL('../../hifimule-ui/src/styles.css', import.meta.url), 'utf8')
    .replace(/\/\*[\s\S]*?\*\//g, '');
  const hiddenRule = css.match(/\.library-view\s*>\s*#browse-mode-bar\[hidden\]\s*,\s*\.library-view\s*>\s*#library-content\[hidden\]\s*,\s*\.library-view\s*>\s*#playback-destination-container\[hidden\]\s*\{([\s\S]*?)\}/);
  assert.ok(hiddenRule, 'destination sibling hidden rule must exist');
  assert.match(hiddenRule[1], /display:\s*none\s*!important/);

  const playbackHost = css.match(/#playback-destination-container\s*\{([\s\S]*?)\}/);
  assert.ok(playbackHost, 'Playback host sizing rule must exist');
  assert.match(playbackHost[1], /flex:\s*1\s+1\s+0/);
  assert.match(playbackHost[1], /min-width:\s*0/);
  assert.match(playbackHost[1], /min-height:\s*0/);
  assert.match(playbackHost[1], /overflow-y:\s*auto/);
  assert.match(playbackHost[1], /overflow-x:\s*hidden/);
  assert.match(playbackHost[1], /overscroll-behavior:\s*contain/);
  assert.match(playbackHost[1], /box-sizing:\s*border-box/);

  const componentRule = css.match(/\.playback-destination\s*\{([\s\S]*?)\}/);
  assert.ok(componentRule, 'Playback component rule must exist');
  assert.doesNotMatch(componentRule[1], /overflow(?:-[xy])?\s*:/, 'component must not create a nested scroller');
  const rowRule = css.match(/\.playback-destination__row\s*,\s*\.playback-destination__empty\s*\{([\s\S]*?)\}/);
  assert.ok(rowRule, 'Playback row wrapping rule must exist');
  assert.match(rowRule[1], /overflow-wrap:\s*anywhere/);

  const actionsRule = css.match(/\.playback-destination__heading-actions\s*\{([\s\S]*?)\}/);
  assert.ok(actionsRule, 'Playback heading actions must have a responsive layout');
  assert.match(actionsRule[1], /min-width:\s*0/);
  assert.match(actionsRule[1], /flex-wrap:\s*wrap/);
  const browseRule = css.match(/\.playback-destination__browse\s*\{([\s\S]*?)\}/);
  assert.ok(browseRule, 'library return action must tolerate narrow translated labels');
  assert.match(browseRule[1], /overflow-wrap:\s*anywhere/);
});
