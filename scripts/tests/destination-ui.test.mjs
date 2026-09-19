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
  querySelector(selector) {
    const all = [this, ...this.children.flatMap(child => [child, ...child.querySelectorAll('*')])];
    const occurrence = selector.match(/data-occurrence-id="([^"]+)"/)?.[1];
    const action = selector.match(/data-queue-action="([^"]+)"/)?.[1];
    return all.find(node => (!occurrence || node.dataset.occurrenceId === occurrence)
      && (!action || node.dataset.queueAction === action)) ?? null;
  }
}
const document = { activeElement: null, createElement: tag => new Element(tag) };
function text(element) { return element.textContent + element.children.map(text).join(' '); }
function findClass(element, className) {
  if (element.classList.contains(className)) return element;
  for (const child of element.children) { const found = findClass(child, className); if (found) return found; }
  return null;
}
function occurrencePage(occurrences, section = 'upcoming', nextCursor = null, count = occurrences.length) {
  return { occurrences, nextCursor, totalOccurrenceCount: count, section, sectionCount: occurrences.length,
    mainCurrentOccurrenceId: 'current', precedingOccurrenceId: null, followingOccurrenceIds: [], endOfSection: !nextCursor };
}

function load(relative, mocks, runtime = {}) {
  const exports = {};
  const source = ts.transpileModule(readFileSync(new URL(relative, import.meta.url), 'utf8'), {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
  }).outputText;
  vm.runInNewContext(source, { exports, document, window: { addEventListener() {}, removeEventListener() {} }, console,
    CSS: { escape: value => String(value) },
    setTimeout: () => 1, clearTimeout() {}, require: name => mocks[name] ?? {}, ...runtime });
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
    '../state/basket': { basketStore: { flushPendingSave: async () => {} } },
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
  for (let turn = 0; turn < 12; turn++) await Promise.resolve();
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
    mainCurrent: { occurrenceId: 'current' },
    preview: { auditionId: 'a' }, playback: { metadata: { title: 'Audition' } } };
  const { PlaybackDestination } = load('../../hifimule-ui/src/components/PlaybackDestination.ts', {
    '../state/playback': { playbackStore: { subscribe(callback) { subscriber = callback; return () => {}; }, refresh: async () => snapshot } },
    '../rpc': {
      serverList: async () => [{ serverId: 'srv', name: 'Living room' }],
      playbackListOccurrences: async (_observed, _cursor, _limit, options) => options.section === 'history' ? occurrencePage([], 'history') : occurrencePage([
        { occurrenceId: 'o1', source: { serverId: 'srv', trackId: 't' } },
        { occurrenceId: 'o2', source: { serverId: 'srv', trackId: 't' } },
      ], 'upcoming', null, 3),
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
  assert.equal(metadataCalls, 1, "transport changes must reuse page metadata");
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
  const retry = container.querySelectorAll('button').find(button => button.textContent === 'playback.retry');
  await retry.click();
  for (let turn = 0; turn < 12; turn++) await Promise.resolve();
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

async function settle() { for (let turn = 0; turn < 24; turn++) await Promise.resolve(); }
function deferred() { let resolve, reject; const promise = new Promise((a, b) => { resolve = a; reject = b; }); return { promise, resolve, reject }; }
function clockHarness() {
  const timers = new Map(); let id = 0;
  return { timers, runtime: { setTimeout: callback => { timers.set(++id, callback); return id; }, clearTimeout: id => timers.delete(id) },
    async tick() { const entry = timers.entries().next().value; if (entry) { timers.delete(entry[0]); entry[1](); } await settle(); } };
}
const queueSnapshot = { instanceId: 'i', sessionId: 's', queueRevision: '1', stateSequence: '1', mode: 'main',
  mainCurrent: { occurrenceId: 'current', source: { serverId: 'srv', trackId: 'track' } },
  playback: { status: 'active', metadata: null } };
function queueHarness(rpc = {}, storeOverride) {
  const clock = clockHarness(); let subscriber; let metadataCalls = 0;
  const store = storeOverride ?? { subscribe(cb) { subscriber = cb; return () => {}; }, refresh: async () => queueSnapshot };
  const customList = rpc.playbackListOccurrences;
  const customDescribe = rpc.playbackDescribeOccurrences;
  const customServerList = rpc.serverList;
  const rest = { ...rpc }; delete rest.playbackListOccurrences; delete rest.playbackDescribeOccurrences; delete rest.serverList;
  const { PlaybackDestination } = load('../../hifimule-ui/src/components/PlaybackDestination.ts', {
    '../state/playback': { playbackStore: store },
    '../rpc': { serverList: customServerList ?? (async () => []),
      playbackListOccurrences: async (s, cursor, limit, options) => options?.section === 'history'
        ? occurrencePage([], 'history')
        : customList ? customList(s, cursor, limit, options)
          : occurrencePage([{ occurrenceId: cursor ?? 'first' }], 'upcoming', cursor ? null : 'second', 101),
      playbackDescribeOccurrences: customDescribe ?? (async (_s, ids) => { metadataCalls++; return ids.map(id => ({ occurrenceId: id, source: { serverId: 'srv' }, title: id, status: 'available' })); }),
      isPlaybackQueueConflict: () => false, ...rest },
    '../i18n': { t: (key, values) => values?.id ? `${key}:${values.id}` : key },
    '../serverIdentity': { formatServerIdentity: s => ({ label: s.name }) },
  }, clock.runtime);
  const container = new Element('section'); const component = new PlaybackDestination(container, () => {});
  return { component, container, clock, receive: s => subscriber(s), metadataCalls: () => metadataCalls,
    current: findClass(container, 'playback-destination__current'),
    button: key => container.querySelectorAll('button').find(b => b.textContent === key) };
}

test('current occurrence requires live transport and matching loaded metadata', async () => {
  const h = queueHarness();
  h.receive({ ...queueSnapshot, playback: { status: 'paused', metadata: null } }); await settle();
  const metadataCalls = h.metadataCalls();
  assert.equal(h.current.textContent, 'playback.queue.no_current',
    'a restored active pointer without loaded metadata must not display its UUID');
  let sequence = 2;
  for (const status of ['idle', 'stopped', 'completed', 'error']) {
    h.receive({ ...queueSnapshot, stateSequence: String(sequence++), playback: {
      status, metadata: { title: 'Stale', source: queueSnapshot.mainCurrent.source },
    } });
    await settle();
    assert.equal(h.current.textContent, 'playback.queue.no_current', `status ${status} must hide current metadata`);
  }
  for (const source of [{ serverId: 'other', trackId: 'track' }, { serverId: 'srv', trackId: 'other' }]) {
    h.receive({ ...queueSnapshot, stateSequence: String(sequence++), playback: {
      status: 'paused', metadata: { title: 'Wrong source', source },
    } });
    await settle();
    assert.equal(h.current.textContent, 'playback.queue.no_current');
  }
  for (const status of ['loading', 'active', 'paused']) {
    h.receive({ ...queueSnapshot, stateSequence: String(sequence++), playback: { status, metadata: null } });
    await settle();
    assert.equal(h.current.textContent, 'playback.queue.no_current');
    h.receive({ ...queueSnapshot, stateSequence: String(sequence++), playback: {
      status, metadata: { title: 'Loaded', source: queueSnapshot.mainCurrent.source },
    } });
    await settle();
    assert.match(h.current.textContent, /playback\.queue\.current_occurrence:Loaded/);
    assert.doesNotMatch(h.current.textContent, /current_occurrence:current/);
  }
  h.receive({ ...queueSnapshot, stateSequence: String(sequence++), playback: { status: 'paused', metadata: null } });
  await settle();
  assert.equal(h.current.textContent, 'playback.queue.no_current', 'metadata removal must clear stale current text');
  assert.equal(h.metadataCalls(), metadataCalls, 'transport-only changes must not reload queue metadata');
  h.component.destroy();
});

test('queue paging keeps focus and ignores transport-only updates and repeated Next clicks', async () => {
  const second = deferred(); let pageCalls = 0;
  const h = queueHarness({ playbackListOccurrences: async (_s, cursor) => {
    pageCalls++; return cursor ? second.promise : { occurrences: [{ occurrenceId: 'first' }], nextCursor: 'second', totalOccurrenceCount: 101 };
  } });
  h.receive(queueSnapshot); await settle();
  const next = h.button('playback.queue.next'); next.focus();
  await next.click(); await next.click(); await settle();
  h.receive({ ...queueSnapshot, stateSequence: '2', mode: 'preview', playback: { status: 'active', metadata: { title: 'Audition' } } }); await settle();
  assert.equal(pageCalls, 2);
  second.resolve({ occurrences: [{ occurrenceId: 'second' }], nextCursor: null, totalOccurrenceCount: 101 }); await settle();
  assert.equal(h.metadataCalls(), 2);
  assert.equal(h.button('playback.queue.next'), next);
  assert.equal(document.activeElement, next);
  assert.match(text(h.container), /second/);
  assert.equal(h.button('playback.queue.previous').getAttribute('aria-disabled'), 'false');
  h.component.destroy();
});

test('obsolete queue page errors cannot replace a newer queue or reset its paging', async () => {
  const old = deferred();
  const h = queueHarness({ playbackListOccurrences: async s => s.queueRevision === '1' ? old.promise :
    { occurrences: [{ occurrenceId: 'new-queue' }], nextCursor: 'next-new', totalOccurrenceCount: 200 } });
  h.receive(queueSnapshot); await settle();
  h.receive({ ...queueSnapshot, queueRevision: '2', stateSequence: '2' }); await settle();
  old.reject(new Error('old failed page')); await settle();
  assert.match(text(h.container), /new-queue/);
  assert.doesNotMatch(text(h.container), /recoverable_error/);
  assert.equal(h.button('playback.queue.next').getAttribute('aria-disabled'), 'false');
  h.component.destroy();
});

test('paused queue recovers with an equal real-store snapshot and bounded retry', async () => {
  const clock = clockHarness();
  const { PlaybackStore } = load('../../hifimule-ui/src/state/playback.ts', { '../rpc': {} }, clock.runtime);
  const store = new PlaybackStore(async () => queueSnapshot); store.accept(queueSnapshot);
  let calls = 0;
  const h = queueHarness({ playbackListOccurrences: async () => {
    if (++calls === 1) throw new Error('temporary');
    return { occurrences: [{ occurrenceId: 'recovered' }], nextCursor: null, totalOccurrenceCount: 1 };
  } }, store);
  await settle(); assert.equal(calls, 1);
  await h.clock.tick();
  assert.equal(calls, 2);
  assert.match(text(h.container), /recovered/);
  assert.equal(h.button('playback.retry').hidden, true);
  h.component.destroy();
  assert.equal(h.clock.timers.size, 0);
});

test('failed queue retries stop after three automatic attempts and expose manual retry', async () => {
  let calls = 0;
  const h = queueHarness({ playbackListOccurrences: async () => { calls++; throw new Error('offline'); } });
  h.receive(queueSnapshot); await settle();
  for (let i = 0; i < 5; i++) await h.clock.tick();
  assert.equal(calls, 4); assert.equal(h.clock.timers.size, 0);
  assert.equal(h.button('playback.retry').hidden, false);
  h.component.destroy();
});

test('late and retried server labels update cached paused rows without reloading metadata', async () => {
  const labels = deferred(); let calls = 0;
  const h = queueHarness({ serverList: async () => { if (++calls === 1) throw new Error('temporary'); return labels.promise; } });
  h.receive(queueSnapshot); await settle();
  assert.match(text(h.container), /source_unavailable/);
  await h.clock.tick(); labels.resolve([{ serverId: 'srv', name: 'Living Room' }]); await settle();
  assert.match(text(h.container), /Living Room/);
  assert.equal(h.metadataCalls(), 1);
  h.component.destroy();
});

test('destination switches flush outgoing basket and retain one poll timer and stable issue nodes', async () => {
  const clock = clockHarness(); const calls = []; const flush = deferred();
  const state = { destinationRevision: '1', destinations: [{ kind: 'playback', selected: false }, { kind: 'device', path: '/a', name: 'A', selected: true }],
    deviceDiscoveryIssues: [{ discoveryId: 'broken', revision: '1', code: 'DEVICE_READ_FAILED', displayName: 'Broken' }] };
  const { DestinationHub } = load('../../hifimule-ui/src/components/DestinationHub.ts', {
    '../rpc': { getDaemonState: async () => state, destinationSelect: async () => { calls.push('select'); state.destinations[0].selected = true; state.destinations[1].selected = false; } },
    '../state/basket': { basketStore: { flushPendingSave: async () => { calls.push('flush'); await flush.promise; } } },
    '../i18n': { t: key => key },
  }, clock.runtime);
  const container = new Element('nav'); const hub = new DestinationHub(container, () => {}); await settle();
  const issue = container.children[1].children[0];
  await hub.refresh(); assert.equal(container.children[1].children[0], issue);
  assert.equal(clock.timers.size, 1);
  await container.querySelectorAll('button')[0].click(); await settle();
  assert.deepEqual(calls, ['flush']);
  flush.resolve(); await settle(); assert.deepEqual(calls, ['flush', 'select']);
  for (let i = 0; i < 3; i++) { await container.querySelectorAll('button')[0].click(); await settle(); }
  assert.equal(clock.timers.size, 1);
  assert.equal(container.children[1].children[0], issue);
  hub.destroy(); assert.equal(clock.timers.size, 0);
});

test('basket flush failure prevents destination selection', async () => {
  let selections = 0;
  const { DestinationHub } = load('../../hifimule-ui/src/components/DestinationHub.ts', {
    '../rpc': { getDaemonState: async () => ({ destinationRevision: '1', destinations: [
      { kind: 'playback', selected: false }, { kind: 'device', path: '/a', name: 'A', selected: true }], deviceDiscoveryIssues: [] }), destinationSelect: async () => { selections++; } },
    '../state/basket': { basketStore: { flushPendingSave: async () => { throw new Error('disk failure'); } } }, '../i18n': { t: key => key },
  });
  const container = new Element('nav'); const hub = new DestinationHub(container, () => {}); await settle();
  await container.querySelectorAll('button')[0].click(); await settle();
  assert.equal(selections, 0); assert.match(text(container), /destination.selection_failed/);
  hub.destroy();
});

test('hub ignores superseded and disposed state requests', async () => {
  const requests = []; const clock = clockHarness();
  const { DestinationHub } = load('../../hifimule-ui/src/components/DestinationHub.ts', {
    '../rpc': { getDaemonState: () => { const req = deferred(); requests.push(req); return req.promise; } }, '../i18n': { t: key => key },
  }, clock.runtime);
  const container = new Element('nav'); const hub = new DestinationHub(container, () => {});
  const latest = hub.refresh();
  requests[1].resolve({ destinationRevision: '2', destinations: [{ kind: 'playback', selected: true }], deviceDiscoveryIssues: [] }); await latest;
  requests[0].resolve({ destinationRevision: '1', destinations: [], deviceDiscoveryIssues: [] }); await settle();
  assert.equal(container.querySelectorAll('button').length, 1); assert.equal(clock.timers.size, 1);
  const pending = hub.refresh(); hub.destroy(); requests[2].resolve({ destinationRevision: '3', destinations: [], deviceDiscoveryIssues: [] }); await pending;
  assert.equal(container.querySelectorAll('button').length, 1); assert.equal(clock.timers.size, 0);
});

test('partial offline queue metadata has an explicit retry without progress-driven requests', async () => {
  let calls = 0;
  const h = queueHarness({ playbackDescribeOccurrences: async () => [{ occurrenceId: 'first', source: { serverId: 'srv' },
    status: ++calls === 1 ? 'sourceUnavailable' : 'available', title: calls === 1 ? null : 'Recovered song' }] });
  h.receive(queueSnapshot); await settle();
  assert.equal(h.button('playback.retry').hidden, false);
  h.receive({ ...queueSnapshot, stateSequence: '2' }); await settle(); assert.equal(calls, 1);
  await h.button('playback.retry').click(); await settle();
  assert.match(text(h.container), /Recovered song/); assert.equal(h.button('playback.retry').hidden, true);
  h.component.destroy();
});

test('editable upcoming rows submit occurrence moves without blind conflict replay', async () => {
  const moves = []; let subscriber; let revision = '1';
  const snapshot = () => ({ ...queueSnapshot, queueRevision: revision });
  const { PlaybackDestination } = load('../../hifimule-ui/src/components/PlaybackDestination.ts', {
    '../state/playback': { playbackStore: { subscribe(cb) { subscriber = cb; return () => {}; }, refresh: async () => snapshot() } },
    '../rpc': {
      serverList: async () => [], isPlaybackQueueConflict: () => false,
      playbackListOccurrences: async (_s, _cursor, _limit, options) => options.section === 'history'
        ? occurrencePage([], 'history')
        : { ...occurrencePage([
          { occurrenceId: 'a', source: { serverId: 'srv', trackId: 'a' } },
          { occurrenceId: 'b', source: { serverId: 'srv', trackId: 'b' } },
          { occurrenceId: 'c', source: { serverId: 'srv', trackId: 'c' } },
        ]), followingOccurrenceIds: [], endOfSection: true },
      playbackDescribeOccurrences: async (_s, ids) => ids.map(id => ({ occurrenceId: id, source: { serverId: 'srv' }, title: id, status: 'available' })),
      playbackMoveUpcoming: async (observed, occurrenceId, beforeOccurrenceId) => {
        moves.push({ revision: observed.queueRevision, occurrenceId, beforeOccurrenceId }); revision = '2';
      },
      playbackRemoveUpcoming: async () => {},
    },
    '../i18n': { t: key => key }, '../serverIdentity': { formatServerIdentity: s => ({ label: s.name }) },
  });
  const container = new Element('section'); const component = new PlaybackDestination(container, () => {});
  subscriber(snapshot()); await settle();
  const moveDown = container.querySelectorAll('button').filter(button => button.textContent === 'playback.queue.move_down')[0];
  await moveDown.click(); await settle();
  assert.deepEqual(moves, [{ revision: '1', occurrenceId: 'a', beforeOccurrenceId: 'c' }]);
  assert.match(text(container), /playback\.queue\.moved/);
  component.destroy();
});
