import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import vm from 'node:vm';
import ts from '../../hifimule-ui/node_modules/typescript/lib/typescript.js';

class Element {
  children = []; dataset = {}; attributes = {}; listeners = new Map();
  textContent = ''; hidden = false; disabled = false; className = '';
  connectedRoot = false;
  get isConnected() { return this.connectedRoot || !!this.parentElement?.isConnected; }
  get firstElementChild() { return this.children[0] ?? null; }
  contains(node) { return this === node || this.children.some(child => child.contains(node)); }
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
  replaceChildren(...children) { for (const child of [...this.children]) child.remove(); this.append(...children); }
  remove() { if (!this.parentElement) return; if (this.contains(document.activeElement)) document.activeElement = null; this.parentElement.children = this.parentElement.children.filter(child => child !== this); this.parentElement = null; }
  insertBefore(child, before) { child.remove(); child.parentElement = this; const index = before ? this.children.indexOf(before) : this.children.length; this.children.splice(index, 0, child); }
  addEventListener(name, listener) { this.listeners.set(name, listener); }
  removeEventListener() {}
  focus() { if (this.isConnected) document.activeElement = this; }
  async click() { await this.listeners.get('click')?.(); }
  querySelectorAll(selector) {
    const own = selector === '*' || selector === 'button' && this.tagName === 'button' ? [this] : [];
    return own.concat(this.children.flatMap(child => child.querySelectorAll(selector)));
  }
  querySelector(selector) {
    const all = this.querySelectorAll('*');
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
    setTimeout: () => 1, clearTimeout() {}, require: name => name === '../rpc' ? { isPlaybackQueueConflict: () => false, ...mocks[name] } : mocks[name] ?? {}, ...runtime });
  return exports;
}

test('floating controls are a retained final library child outside both content scrollers', () => {
  const main = readFileSync(new URL('../../hifimule-ui/src/main.ts', import.meta.url), 'utf8');
  const shell = main.slice(main.indexOf('root.innerHTML = `', main.indexOf('function renderMainLayout')));
  assert.ok(shell.indexOf('id="playback-controls-container"') > shell.indexOf('id="playback-destination-container"'));
  assert.ok(shell.indexOf('id="playback-controls-container"') < shell.indexOf('slot="end"'));
});

test('destination hub omits Playback because the bar owns that surface switch', async () => {
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
  const container = new Element('nav'); container.connectedRoot = true;
  const hub = new DestinationHub(container, () => {});
  await hub.refresh();
  const buttons = container.querySelectorAll('button');
  assert.deepEqual(buttons.map(button => button.dataset.destinationKind), ['device', 'pendingDevice']);
  buttons[0].focus();
  await hub.applyState({ ...state, destinationRevision: '5', destinations: state.destinations.map((item, index) => ({ ...item, selected: index === 1 })) }, true);
  assert.equal(document.activeElement, buttons[0]);
  assert.equal(container.attributes['aria-label'], 'destination.group');
  assert.deepEqual(calls, []);
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

test('freshness handles initial hangs, equal heartbeats and superseded instance reads', async () => {
  let now = 0; let timer; const reads = []; const states = [];
  const { PlaybackStore } = load('../../hifimule-ui/src/state/playback.ts', { '../rpc': {} }, {
    Date: class extends Date { static now() { return now; } },
    setTimeout: cb => { timer = cb; return 1; }, clearTimeout: () => { timer = undefined; },
  });
  const store = new PlaybackStore(() => new Promise((resolve, reject) => reads.push({ resolve, reject })));
  store.subscribeConnection(state => states.push(state));
  let heartbeats = 0;
  const off = store.subscribe(() => {}, () => heartbeats++);
  assert.equal(store.connection(), 'connecting');
  now = 2001; timer();
  assert.equal(store.connection(), 'stale');
  const fresh = store.refresh();
  const newer = { instanceId: 'new', sessionId: 's', stateSequence: '1' };
  reads[1].resolve(newer); await fresh;
  reads[0].resolve({ instanceId: 'old', sessionId: 's', stateSequence: '999' });
  await Promise.resolve(); await Promise.resolve();
  assert.equal(store.current(), newer);
  const failed = store.refresh(); reads[2].reject(new Error('offline')); await assert.rejects(failed);
  assert.equal(store.connection(), 'disconnected');
  const equal = store.refresh(); reads[3].resolve(newer); await equal;
  assert.equal(store.connection(), 'fresh'); assert.equal(heartbeats, 2);
  off(); assert.equal(timer, undefined);
});

test('unsubscribe and remount fence a pending read and retain one scheduled poll', async () => {
  const timers = new Map(); let id = 0; const reads = [];
  const { PlaybackStore } = load('../../hifimule-ui/src/state/playback.ts', { '../rpc': {} }, {
    setTimeout: callback => { timers.set(++id, callback); return id; }, clearTimeout: id => timers.delete(id),
  });
  const store = new PlaybackStore(() => new Promise(resolve => reads.push(resolve)));
  const off = store.subscribe(() => {}); assert.equal(timers.size, 1);
  off(); assert.equal(timers.size, 0);
  const seen = []; const off2 = store.subscribe(s => seen.push(s.instanceId));
  const off3 = store.subscribe(() => {}); assert.equal(timers.size, 1); assert.equal(reads.length, 2);
  reads[0]({instanceId:'obsolete',sessionId:'s',stateSequence:'99'}); await settle();
  assert.deepEqual(seen, []);
  reads[1]({instanceId:'current',sessionId:'s',stateSequence:'1'}); await settle();
  assert.deepEqual(seen, ['current']);
  off2(); off3(); assert.equal(timers.size, 0);
});

test('command publication shares ordering and cannot introduce a stale owner or occurrence', async () => {
  let read = {instanceId:'i',sessionId:'s',generationId:'g',current:{occurrenceId:'a'},stateSequence:'1'};
  const { PlaybackStore } = load('../../hifimule-ui/src/state/playback.ts', {'../rpc':{}});
  const store = new PlaybackStore(async () => read);
  await store.refresh(); const observed=store.current();
  const newer={...read,stateSequence:'2',generationId:'seek'};
  assert.equal(store.acceptCommand(newer,observed),true);
  assert.equal(store.current(),newer);
  assert.equal(store.acceptCommand({...read,stateSequence:'99'},observed),false);
  assert.equal(store.acceptCommand({...newer,current:{occurrenceId:'wrong'},stateSequence:'100'},newer),false);
  read={...newer,instanceId:'restarted',stateSequence:'1'}; await store.refresh();
  assert.equal(store.acceptCommand({...newer,stateSequence:'101'},newer),false);
});

test('playback destination renders only queue regions and describes one bounded canonical page', async () => {
  let subscriber; let metadataCalls = 0;
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
      playbackDescribeOccurrences: async (_observed, ids) => { if (ids[0] === 'current') return [{ occurrenceId: 'current', source: { serverId: 'srv' }, title: 'Main', status: 'available' }]; metadataCalls++; assert.deepEqual(Array.from(ids), ['o1', 'o2']); return ids.map(id => ({ occurrenceId: id, source: { serverId: 'srv', trackId: 't' }, title: 'Song', artist: 'Artist', album: null, durationMs: 1, status: 'available' })); },
    },
    '../i18n': { t: (key, values) => values?.title ? `${key}:${values.title}` : values?.count != null ? `${key}:${values.count}` : key },
    '../serverIdentity': { formatServerIdentity: server => ({ label: server.name }) },
  });
  const container = new Element('section'); container.connectedRoot = true;
  container.className = 'content layout-host';
  const destination = new PlaybackDestination(container);
  subscriber(snapshot);
  for (let turn = 0; turn < 12; turn++) await Promise.resolve();
  assert.equal(metadataCalls, 1);
  assert.equal(container.classList.contains('content'), true);
  assert.equal(container.classList.contains('layout-host'), true);
  assert.equal(container.classList.contains('playback-destination'), true);
  assert.doesNotMatch(text(container), /playback\.queue\.preview|playback\.queue\.current_occurrence|destination\.playback/);
  assert.equal((text(container).match(/Song/g) ?? []).length, 2);
  const queueAction = container.querySelectorAll('button').find(button => button.getAttribute('aria-label') === 'playback.queue.move_up');
  assert.equal(queueAction.children[0].tagName, 'sl-icon');
  assert.equal(queueAction.children[0].getAttribute('name'), 'arrow-up');
  assert.equal(queueAction.parentElement.tagName, 'sl-tooltip');
  assert.equal(queueAction.parentElement.getAttribute('placement'), 'top');
  assert.equal(queueAction.parentElement.getAttribute('hoist'), '');
  queueAction.focus();
  subscriber({ ...snapshot, stateSequence: '2' });
  for (let turn = 0; turn < 12; turn++) await Promise.resolve();
  const rerenderedAction = container.querySelectorAll('button').find(button => button.getAttribute('aria-label') === 'playback.queue.move_up');
  assert.equal(metadataCalls, 1, "transport changes must reuse page metadata");
  assert.equal(rerenderedAction, queueAction, 'transport refresh must preserve the focused queue action node');
  assert.equal(document.activeElement, queueAction);
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
  const container = new Element('section'); container.connectedRoot = true;
  const destination = new PlaybackDestination(container, () => {});
  subscriber(snapshot);
  for (let turn = 0; turn < 4; turn++) await Promise.resolve();
  destination.destroy();
  rejectPage(new Error('late failure'));
  for (let turn = 0; turn < 12; turn++) await Promise.resolve();
  assert.doesNotMatch(text(container), /playback\.queue\.recoverable_error/);
  assert.equal(refreshCalls, 0);
});

test('playback destination keeps retry available in recoverable error state', async () => {
  let subscriber; let refreshCalls = 0;
  const snapshot = { instanceId: 'i', sessionId: 's', queueRevision: '7', stateSequence: '1', mode: 'main',
    playback: { metadata: null } };
  const { PlaybackDestination } = load('../../hifimule-ui/src/components/PlaybackDestination.ts', {
    '../state/playback': { playbackStore: { subscribe(callback) { subscriber = callback; return () => {}; }, refresh: async () => { refreshCalls++; return snapshot; } } },
    '../rpc': { serverList: async () => [], playbackListOccurrences: async () => { throw new Error('stale'); } },
    '../i18n': { t: key => key }, '../serverIdentity': { formatServerIdentity: () => ({ label: '' }) },
  });
  const container = new Element('section'); container.connectedRoot = true;
  const destination = new PlaybackDestination(container);
  subscriber(snapshot);
  for (let turn = 0; turn < 12; turn++) await Promise.resolve();
  assert.match(text(container), /playback\.queue\.recoverable_error/);
  assert.equal(container.querySelectorAll('button').some(button => button.textContent === 'playback.queue.back_to_library'), false);
  const retry = container.querySelectorAll('button').find(button => button.textContent === 'playback.retry');
  await retry.click();
  for (let turn = 0; turn < 12; turn++) await Promise.resolve();
  assert.equal(refreshCalls, 1);
  destination.destroy();
});

test('main wires the bar-owned Library/Playing switch and updates the shared title', () => {
  const source = readFileSync(new URL('../../hifimule-ui/src/main.ts', import.meta.url), 'utf8');
  assert.match(source, /new PlaybackDestination\(playback\)/);
  assert.match(source, /activePlaybackControls\?\.setSurface\(surface\)/);
  assert.match(source, /title\.textContent = t\(playing \? 'playback\.playing_title' : 'ui\.library\.title'\)/);
  assert.match(source, /serverHub\.hidden = playing/);
  assert.match(source, /if \(surface === 'library'\) showLibrarySurface\(\)/);
  assert.match(source, /destinationSelect\(\{ kind: 'playback' \}\)/);
});

test('bar surface labels have complete locale parity', () => {
  const catalog = JSON.parse(readFileSync(new URL('../../hifimule-i18n/catalog.json', import.meta.url), 'utf8'));
  assert.deepEqual(Object.keys(catalog).sort(), ['de', 'en', 'es', 'fr']);
  for (const locale of Object.keys(catalog)) {
    for (const key of ['playback.browse_library', 'playback.show_playing', 'playback.playing_title']) {
      assert.equal(typeof catalog[locale][key], 'string');
      assert.ok(catalog[locale][key].trim().length > 0);
    }
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

  assert.doesNotMatch(css, /\.playback-destination__(?:heading|browse|current|preview)/);
});

async function settle() { await new Promise(resolve => setImmediate(resolve)); }
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
      playbackDescribeOccurrences: async (observed, ids) => {
        if (ids.length === 1 && ids[0] === observed.mainCurrent?.occurrenceId) return [{ occurrenceId: ids[0], source: observed.mainCurrent.source, title: 'Canonical current', status: 'available' }];
        return customDescribe ? customDescribe(observed, ids) : (metadataCalls++, ids.map(id => ({ occurrenceId: id, source: { serverId: 'srv' }, title: id, status: 'available' })));
      },
      isPlaybackQueueConflict: () => false, ...rest },
    '../i18n': { t: (key, values) => values?.id ? `${key}:${values.id}` : key },
    '../serverIdentity': { formatServerIdentity: s => ({ label: s.name }) },
  }, clock.runtime);
  const container = new Element('section'); container.connectedRoot = true; const component = new PlaybackDestination(container);
  return { component, container, clock, receive: s => subscriber(s), metadataCalls: () => metadataCalls,
    button: key => container.querySelectorAll('button').find(b => b.textContent === key || b.getAttribute('aria-label') === key) };
}

test('transport-only updates do not add a duplicate current occurrence or reload queue metadata', async () => {
  const h = queueHarness();
  h.receive({ ...queueSnapshot, playback: { status: 'paused', metadata: null } }); await settle();
  assert.equal(findClass(h.container, 'playback-destination__current'), null);
  const calls = h.metadataCalls();
  for (const status of ['idle', 'stopped', 'completed', 'error', 'active']) {
    h.receive({ ...queueSnapshot, stateSequence: '2', mode: 'preview', playback: {
      status, metadata: { title: 'Audition', source: { serverId: 'other', trackId: 'other' } },
    } });
    await settle();
    assert.doesNotMatch(text(h.container), /Canonical current|Audition|current_occurrence/);
  }
  assert.equal(h.metadataCalls(), calls);
  h.component.destroy();
});

test('queue paging keeps focus and ignores transport-only updates and repeated Next clicks', async () => {
  const second = deferred(); let pageCalls = 0;
  const h = queueHarness({ playbackListOccurrences: async (_s, cursor) => {
    pageCalls++; return cursor ? second.promise : occurrencePage([{ occurrenceId: 'first' }], 'upcoming', 'second', 101);
  } });
  h.receive(queueSnapshot); await settle();
  const next = h.button('playback.queue.next'); next.focus();
  await next.click(); await next.click(); await settle();
  h.receive({ ...queueSnapshot, stateSequence: '2', mode: 'preview', playback: { status: 'active', metadata: { title: 'Audition' } } }); await settle();
  assert.equal(pageCalls, 2);
  second.resolve(occurrencePage([{ occurrenceId: 'second' }])); await settle();
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
    occurrencePage([{ occurrenceId: 'new-queue' }], 'upcoming', 'next-new', 200) });
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
    return occurrencePage([{ occurrenceId: 'recovered' }]);
  } }, store);
  await settle(); assert.equal(calls, 1);
  await h.clock.tick();
  assert.equal(calls, 2);
  assert.match(text(h.container), /recovered/);
  assert.equal(h.button('playback.retry').hidden, true);
  h.component.destroy();
  assert.equal(h.component.regions.upcoming.retryTimer, undefined);
});

test('failed queue retries stop after three automatic attempts and expose manual retry', async () => {
  let calls = 0;
  const h = queueHarness({ playbackListOccurrences: async () => { calls++; throw new Error('offline'); } });
  h.receive(queueSnapshot); await settle();
  for (let i = 0; i < 5; i++) await h.clock.tick();
  assert.equal(calls, 4); assert.equal(h.component.regions.upcoming.retryTimer, undefined);
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
  const container = new Element('nav'); container.connectedRoot = true; const hub = new DestinationHub(container, () => {}); await settle();
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
  const container = new Element('nav'); container.connectedRoot = true; const hub = new DestinationHub(container, () => {}); await settle();
  await container.querySelectorAll('button')[0].click(); await settle();
  assert.equal(selections, 0); assert.match(text(container), /destination.selection_failed/);
  hub.destroy();
});

test('hub ignores superseded and disposed state requests', async () => {
  const requests = []; const clock = clockHarness();
  const { DestinationHub } = load('../../hifimule-ui/src/components/DestinationHub.ts', {
    '../rpc': { getDaemonState: () => { const req = deferred(); requests.push(req); return req.promise; } }, '../i18n': { t: key => key },
  }, clock.runtime);
  const container = new Element('nav'); container.connectedRoot = true; const hub = new DestinationHub(container, () => {});
  const latest = hub.refresh();
  requests[1].resolve({ destinationRevision: '2', destinations: [{ kind: 'playback', selected: true }], deviceDiscoveryIssues: [] }); await latest;
  requests[0].resolve({ destinationRevision: '1', destinations: [], deviceDiscoveryIssues: [] }); await settle();
  assert.equal(container.querySelectorAll('button').length, 0); assert.equal(container.hidden, true); assert.equal(clock.timers.size, 1);
  const pending = hub.refresh(); hub.destroy(); requests[2].resolve({ destinationRevision: '3', destinations: [], deviceDiscoveryIssues: [] }); await pending;
  assert.equal(container.querySelectorAll('button').length, 0); assert.equal(clock.timers.size, 0);
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
  const container = new Element('section'); container.connectedRoot = true; const component = new PlaybackDestination(container, () => {});
  subscriber(snapshot()); await settle();
  const moveDown = container.querySelectorAll('button').filter(button => button.getAttribute('aria-label') === 'playback.queue.move_down')[0];
  await moveDown.click(); await settle();
  assert.deepEqual(moves, [{ revision: '1', occurrenceId: 'a', beforeOccurrenceId: 'c' }]);
  assert.match(text(container), /playback\.queue\.moved/);
  component.destroy();
});

function liveQueueHarness(overrides = {}) {
  const clock = clockHarness();
  let snapshot = { ...queueSnapshot };
  const { PlaybackStore } = load('../../hifimule-ui/src/state/playback.ts', { '../rpc': {} }, clock.runtime);
  const store = new PlaybackStore(async () => snapshot);
  store.accept(snapshot);
  const realRpc = load('../../hifimule-ui/src/rpc.ts', { './i18n': { t: key => key } });
  const describe = async (_observed, ids) => ids.map(id => ({ occurrenceId: id,
    source: { serverId: 'srv', trackId: id }, title: `Title ${id}`, status: 'available' }));
  const { PlaybackDestination } = load('../../hifimule-ui/src/components/PlaybackDestination.ts', {
    '../state/playback': { playbackStore: store },
    '../rpc': { serverList: async () => [], isPlaybackQueueConflict: error => error.conflict === true || realRpc.isPlaybackQueueConflict(error),
      playbackDescribeOccurrences: describe,
      playbackListOccurrences: async (_s, _c, _l, options) => occurrencePage([], options.section), ...overrides },
    '../i18n': { t: (key, values) => values?.id ? `${key}:${values.id}` : key },
    '../serverIdentity': { formatServerIdentity: server => ({ label: server.name }) },
  }, clock.runtime);
  const container = new Element('section'); container.connectedRoot = true;
  const component = new PlaybackDestination(container, () => {});
  const region = name => findClass(container, `playback-destination__region--${name}`);
  return { component, container, clock, store, describe, region, RpcError: realRpc.RpcError,
    snapshot: () => snapshot,
    update(next, notify = false) {
      snapshot = { ...snapshot, ...next, stateSequence: String(Number(snapshot.stateSequence) + 1) };
      if (notify) store.accept(snapshot);
    },
    action: (id, action) => container.querySelector(`[data-occurrence-id="${id}"][data-queue-action="${action}"]`),
    button: (section, label) => region(section).querySelectorAll('button').find(button => button.textContent === label || button.getAttribute('aria-label') === label),
  };
}
const sourceOccurrence = id => ({ occurrenceId: id, source: { serverId: 'srv', trackId: id } });

test('real refresh broadcasts keep mutation focus pending until the located upcoming metadata arrives', async () => {
  const located = deferred(); const requests = []; let h;
  h = liveQueueHarness({
    playbackListOccurrences: async (snapshot, _cursor, _limit, options) => {
      requests.push([snapshot.queueRevision, options.section, options.aroundOccurrenceId]);
      if (options.section === 'history') return occurrencePage([], 'history');
      return occurrencePage(['a', 'b', 'c'].map(sourceOccurrence));
    },
    playbackDescribeOccurrences: async (snapshot, ids) => {
      if (snapshot.queueRevision === '2' && ids[0] === 'a') return located.promise;
      return ids.map(id => ({ occurrenceId: id, source: { serverId: 'srv' }, title: id, status: 'available' }));
    },
    playbackMoveUpcoming: async () => h.update({ queueRevision: '2' }),
  });
  await settle();
  const focused = h.action('a', 'moveDown'); focused.focus(); await focused.click(); await settle();
  assert.equal(document.activeElement, focused, 'history completion must not redirect focus');
  assert.deepEqual(requests.filter(([revision, section]) => revision === '2' && section === 'upcoming'),
    [['2', 'upcoming', 'a']], 'subscriber notification must not race a duplicate first-page request');
  located.resolve(await h.describe(h.snapshot(), ['a', 'b', 'c'])); await settle();
  assert.equal(document.activeElement, focused);
  assert.equal(focused.isConnected, true);
  h.component.destroy();
});

test('metadata responses are fenced per region after description and echoed anchors are validated', async () => {
  const oldMetadata = deferred(); let first = true;
  const h = liveQueueHarness({
    playbackListOccurrences: async (snapshot, _cursor, _limit, options) => {
      if (options.section === 'history') return occurrencePage([], 'history');
      return occurrencePage([sourceOccurrence(snapshot.queueRevision === '1' ? 'old' : 'new')]);
    },
    playbackDescribeOccurrences: async (_snapshot, ids) => {
      if (ids[0] === 'old' && first) { first = false; return oldMetadata.promise; }
      return ids.map(id => ({ occurrenceId: id, source: { serverId: 'srv' }, title: id, status: 'available' }));
    },
  });
  await settle();
  h.update({ queueRevision: '2' }, true); await settle();
  oldMetadata.resolve([{ occurrenceId: 'old', source: { serverId: 'srv' }, title: 'OBSOLETE', status: 'available' }]); await settle();
  assert.match(text(h.region('upcoming')), /new/);
  assert.doesNotMatch(text(h.region('upcoming')), /OBSOLETE/);
  h.component.destroy();

  const wrong = liveQueueHarness({ playbackListOccurrences: async (_s, _c, _l, options) => ({
    ...occurrencePage([sourceOccurrence('wrong')], options.section), mainCurrentOccurrenceId: 'other-current',
  }) });
  await settle();
  assert.doesNotMatch(text(wrong.region('upcoming')), /Title wrong/);
  assert.equal(wrong.container.querySelectorAll('button').find(button => button.textContent === 'playback.retry').hidden, false);
  wrong.component.destroy();
});

test('removing the final upcoming occurrence detaches its action and focuses the connected upcoming heading', async () => {
  let removed = false; let h;
  h = liveQueueHarness({
    playbackListOccurrences: async (_s, _c, _l, options) => occurrencePage(
      options.section === 'upcoming' && !removed ? [sourceOccurrence('only')] : [], options.section),
    playbackRemoveUpcoming: async () => { removed = true; h.update({ queueRevision: '2' }); },
  });
  await settle();
  const action = h.action('only', 'remove'); action.focus(); await action.click(); await settle();
  assert.equal(action.isConnected, false);
  assert.equal(document.activeElement, h.region('upcoming').children[0]);
  assert.equal(document.activeElement.isConnected, true);
  h.component.destroy();
});

test('label and metadata refresh preserve keyed action nodes and their connected focus', async () => {
  const labels = deferred(); let available = false;
  const h = liveQueueHarness({
    serverList: () => labels.promise,
    playbackListOccurrences: async (_s, _c, _l, options) => occurrencePage(
      options.section === 'upcoming' ? [sourceOccurrence('a')] : [], options.section),
    playbackDescribeOccurrences: async (_s, ids) => ids.map(id => ({ occurrenceId: id,
      source: { serverId: 'srv' }, title: id, status: available ? 'available' : 'sourceUnavailable' })),
  });
  await settle();
  const action = h.action('a', 'remove'); action.focus();
  labels.resolve([{ serverId: 'srv', name: 'Bedroom' }]); await settle();
  assert.equal(h.action('a', 'remove'), action); assert.equal(document.activeElement, action);
  assert.match(text(h.region('upcoming')), /Bedroom/);
  available = true;
  await h.container.querySelectorAll('button').find(button => button.textContent === 'playback.retry').click(); await settle();
  assert.equal(h.action('a', 'remove'), action); assert.equal(document.activeElement, action);
  assert.equal(action.isConnected, true);
  h.component.destroy();
});

test('deep located pages retain direct backward navigation and exact Next/Previous round trips', async () => {
  const ids = Array.from({ length: 900 }, (_value, index) => `track-${index}`);
  const requests = []; let h;
  h = liveQueueHarness({
    playbackListOccurrences: async (_s, cursor, limit, options) => {
      if (options.section === 'history') return occurrencePage([], 'history');
      const start = options.aroundOccurrenceId
        ? Math.max(0, ids.indexOf(options.aroundOccurrenceId) - Math.floor(limit / 2))
        : Number(cursor ?? 0);
      requests.push(start);
      const end = Math.min(ids.length, start + limit);
      return { ...occurrencePage(ids.slice(start, end).map(sourceOccurrence), 'upcoming', end < ids.length ? String(end) : null),
        precedingOccurrenceId: ids[start - 1] ?? null, followingOccurrenceIds: ids.slice(end, end + 2), endOfSection: end === ids.length };
    },
    playbackMoveUpcoming: async (_s, id, anchor) => {
      ids.splice(ids.indexOf(id), 1); ids.splice(anchor ? ids.indexOf(anchor) : ids.length, 0, id);
      h.update({ queueRevision: '2' });
    },
  });
  await settle();
  for (let index = 0; index < 5; index++) { await h.button('upcoming', 'playback.queue.next').click(); await settle(); }
  const moved = h.action('track-550', 'moveDown'); moved.focus(); await moved.click(); await settle();
  const locatedStart = requests.at(-1);
  assert.ok(locatedStart > 400);
  assert.equal(h.button('upcoming', 'playback.queue.previous').getAttribute('aria-disabled'), 'false');
  assert.equal(document.activeElement, h.action('track-550', 'moveDown'));
  await h.button('upcoming', 'playback.queue.next').click(); await settle();
  await h.button('upcoming', 'playback.queue.previous').click(); await settle();
  assert.equal(requests.at(-1), locatedStart, 'Previous must return to the centered page, not the first page');
  const count = requests.length;
  await h.button('upcoming', 'playback.queue.previous').click(); await settle();
  assert.equal(requests.length, count + 1, 'backward navigation must not walk all intervening pages');
  assert.ok(requests.at(-1) < locatedStart && requests.at(-1) > 0);
  assert.ok(h.region('upcoming').querySelectorAll('button').length <= 302);
  h.component.destroy();
});

test('history success cannot hide upcoming errors or unavailable metadata and history failures retry independently', async () => {
  const history = deferred(); let failing = true;
  const h = liveQueueHarness({ playbackListOccurrences: async (_s, _c, _l, options) => {
    if (options.section === 'history') return history.promise;
    if (failing) throw new Error('offline');
    return occurrencePage([sourceOccurrence('a')]);
  } });
  await settle(); history.resolve(occurrencePage([], 'history')); await settle();
  const retry = h.container.querySelectorAll('button').find(button => button.textContent === 'playback.retry');
  assert.equal(retry.hidden, false); assert.match(text(h.container), /recoverable_error/);
  failing = false; await retry.click(); await settle(); assert.equal(retry.hidden, true);
  h.component.destroy();

  let historyCalls = 0;
  const independent = liveQueueHarness({ playbackListOccurrences: async (_s, _c, _l, options) => {
    if (options.section === 'history' && ++historyCalls === 1) throw new Error('history offline');
    return occurrencePage([], options.section);
  } });
  await settle(); await independent.clock.tick(); await independent.clock.tick();
  assert.equal(historyCalls, 2);
  assert.doesNotMatch(text(independent.container), /recoverable_error/);
  independent.component.destroy();
});

test('current occurrence changes never trigger duplicate current metadata requests', async () => {
  const calls = [];
  const h = liveQueueHarness({ playbackDescribeOccurrences: async (_s, ids) => {
    calls.push([...ids]);
    return ids.map(id => ({ occurrenceId: id, source: { serverId: 'srv' }, title: null, status: 'sourceUnavailable' }));
  } });
  await settle();
  h.update({ mainCurrent: { occurrenceId: 'new-current', source: { serverId: 'srv', trackId: 'new' } } }, true); await settle();
  h.update({ mode: 'preview', playback: { metadata: { title: 'Audition' }, status: 'paused' } }, true); await settle();
  assert.deepEqual(calls, []);
  assert.doesNotMatch(text(h.container), /current_occurrence|Audition/);
  h.component.destroy();
});

test('a superseded same-identity description cannot overwrite a newer located page or its error state', async () => {
  const stale = deferred(); let calls = 0;
  const h = liveQueueHarness({
    playbackListOccurrences: async (_s, _c, _l, options) => occurrencePage(
      options.section === 'upcoming' ? [sourceOccurrence(++calls === 1 ? 'old' : 'located')] : [], options.section),
    playbackDescribeOccurrences: async (_s, ids) => ids[0] === 'old' ? stale.promise
      : ids.map(id => ({ occurrenceId: id, source: { serverId: 'srv' }, title: id, status: 'available' })),
  });
  await settle();
  // Exercise overlapping requests against the production region loader without
  // changing owner/revision/anchor: identity checks alone cannot reject the old result.
  await h.component.loadRegion(h.component.regions.upcoming);
  assert.match(text(h.region('upcoming')), /located/);
  stale.resolve([{ occurrenceId: 'old', source: { serverId: 'srv' }, title: 'STALE PAGE', status: 'sourceUnavailable' }]);
  await settle();
  assert.match(text(h.region('upcoming')), /located/);
  assert.doesNotMatch(text(h.region('upcoming')), /STALE PAGE/);
  assert.equal(h.container.querySelectorAll('button').find(button => button.textContent === 'playback.retry').hidden, true);
  h.component.destroy();
});

test('scoped read conflicts refresh immediately without replay and contain refresh failure', async () => {
  let calls = 0; let h;
  h = liveQueueHarness({ playbackListOccurrences: async (_s, _c, _l, options) => {
    if (options.section === 'history') return occurrencePage([], 'history');
    if (++calls === 1) {
      await Promise.resolve();
      h.update({ queueRevision: '2' });
      throw Object.assign(new Error('stale anchor'), { conflict: true });
    }
    return occurrencePage([sourceOccurrence('authoritative')]);
  } });
  await settle();
  assert.match(text(h.region('upcoming')), /authoritative/);
  assert.equal(calls, 2);
  assert.equal(h.component.regions.upcoming.retryTimer, undefined);
  h.component.destroy();

  let fail = false;
  const broken = liveQueueHarness({ playbackListOccurrences: async (_s, _c, _l, options) => {
    if (fail && options.section === 'upcoming') throw Object.assign(new Error('conflict'), { conflict: true });
    return occurrencePage([], options.section);
  } });
  await settle();
  fail = true; broken.store.refresh = async () => { throw new Error('offline'); };
  await broken.component.loadRegion(broken.component.regions.upcoming); await settle();
  assert.match(text(broken.container), /conflict_refresh_failed/);
  assert.equal(broken.container.querySelectorAll('button').find(button => button.textContent === 'playback.retry').hidden, false);
  broken.component.destroy();
});

test('queue mutation remains busy through authoritative metadata loading and ignores repeated actions', async () => {
  const admitted = deferred(); const described = deferred(); let mutations = 0; let h;
  h = liveQueueHarness({
    playbackListOccurrences: async (_s, _c, _l, options) => occurrencePage(
      options.section === 'upcoming' ? ['a', 'b', 'c'].map(sourceOccurrence) : [], options.section),
    playbackDescribeOccurrences: async (snapshot, ids) => {
      if (snapshot.queueRevision === '2' && ids.includes('a')) return described.promise;
      return ids.map(id => ({ occurrenceId: id, source: { serverId: 'srv' }, title: id, status: 'available' }));
    },
    playbackMoveUpcoming: async () => {
      ++mutations; await admitted.promise; h.update({ queueRevision: '2' });
    },
    playbackRemoveUpcoming: async () => { assert.fail('a second action cannot enter while the first is pending'); },
  });
  await settle();
  const action = h.action('a', 'moveDown'); action.focus(); await action.click(); await settle();
  const body = findClass(h.container, 'playback-destination__body');
  assert.equal(body.getAttribute('aria-busy'), 'true');
  await action.click(); await h.action('b', 'remove').click(); await settle();
  assert.equal(mutations, 1);
  admitted.resolve(); await settle();
  assert.equal(body.getAttribute('aria-busy'), 'true', 'committed edit is still reconciling its page');
  await action.click(); await h.action('b', 'remove').click(); await settle();
  assert.equal(mutations, 1);
  described.resolve(await h.describe(h.snapshot(), ['a', 'b', 'c'])); await settle();
  assert.equal(body.getAttribute('aria-busy'), 'false');
  assert.equal(document.activeElement, action);
  assert.equal(action.isConnected, true);
  h.component.destroy();
});

test('mutation refresh failures release busy state, expose Retry and never replay the edit', async () => {
  for (const conflict of [false, true]) {
    let mutations = 0;
    const h = liveQueueHarness({
      playbackListOccurrences: async (_s, _c, _l, options) => occurrencePage(
        options.section === 'upcoming' ? ['a', 'b'].map(sourceOccurrence) : [], options.section),
      playbackMoveUpcoming: async () => {
        ++mutations;
        if (conflict) throw Object.assign(new Error('private conflict detail'), { conflict: true });
      },
    });
    await settle();
    h.store.refresh = async () => { throw new Error('private refresh failure'); };
    const action = h.action('a', 'moveDown'); action.focus(); await action.click(); await settle();
    assert.equal(mutations, 1);
    assert.equal(findClass(h.container, 'playback-destination__body').getAttribute('aria-busy'), 'false');
    assert.equal(h.container.querySelectorAll('button').find(button => button.textContent === 'playback.retry').hidden, false);
    assert.equal(findClass(h.container, 'playback-destination__mutation-status').textContent,
      conflict ? 'playback.queue.conflict_refresh_failed' : 'playback.queue.moved');
    assert.equal(document.activeElement, action);
    assert.equal(action.isConnected, true);
    await h.container.querySelectorAll('button').find(button => button.textContent === 'playback.retry').click(); await settle();
    assert.equal(mutations, 1, 'Retry reloads state and cannot repeat an accepted or rejected mutation');
    h.component.destroy();
  }
});

test('accepted removal recovers once when its focus survivor advances before the refresh returns', async () => {
  let edits = 0; const located = []; let h;
  h = liveQueueHarness({
    playbackListOccurrences: async (snapshot, cursor, _limit, options) => {
      if (options.section === 'upcoming') {
        located.push([snapshot.queueRevision, cursor, options.aroundOccurrenceId]);
        if (options.aroundOccurrenceId === 'b')
          throw new h.RpcError('Locator is no longer upcoming', 409, { code: 'INVALID_CURSOR' }, null);
      }
      return { ...occurrencePage(options.section === 'upcoming'
        ? (snapshot.queueRevision === '1' ? ['a', 'b', 'c'] : ['c']).map(sourceOccurrence) : [], options.section),
        mainCurrentOccurrenceId: snapshot.mainCurrent.occurrenceId };
    },
    playbackRemoveUpcoming: async () => {
      ++edits;
      h.update({ queueRevision: '2', mainCurrent: sourceOccurrence('b') });
    },
  });
  await settle();
  const action = h.action('a', 'remove'); action.focus(); await action.click(); await settle();
  assert.equal(edits, 1);
  assert.deepEqual(located, [['1', null, null], ['2', null, 'b'], ['2', null, null]]);
  assert.match(text(h.region('upcoming')), /Title c/);
  assert.equal(document.activeElement, h.region('upcoming').children[0]);
  assert.equal(document.activeElement.isConnected, true);
  assert.equal(h.component.regions.upcoming.location.aroundOccurrenceId, null);
  assert.equal(h.component.regions.upcoming.retryTimer, undefined);
  assert.equal(findClass(h.container, 'playback-destination__body').getAttribute('aria-busy'), 'false');
  h.component.destroy();
});

test('invalid scoped cursors reset paging and reload immediately even when the refreshed snapshot is equal', async () => {
  for (const code of ['INVALID_CURSOR', 'INSTANCE_MISMATCH', 'SESSION_MISMATCH']) {
    const cursors = []; let h;
    h = liveQueueHarness({ playbackListOccurrences: async (_snapshot, cursor, _limit, options) => {
      if (options.section === 'history') return occurrencePage([], 'history');
      cursors.push(cursor);
      if (cursor) throw new h.RpcError('Invalid scoped page', 409, { code }, null);
      return occurrencePage([sourceOccurrence('a')], 'upcoming', cursors.length === 1 ? 'obsolete' : null);
    } });
    await settle();
    let refreshes = 0; const refresh = h.store.refresh.bind(h.store);
    h.store.refresh = async () => { ++refreshes; return refresh(); };
    await h.button('upcoming', 'playback.queue.next').click(); await settle();
    assert.deepEqual(cursors, [null, 'obsolete', null], code);
    assert.equal(refreshes, 1);
    assert.equal(h.component.regions.upcoming.previous.length, 0);
    assert.equal(h.button('upcoming', 'playback.queue.previous').getAttribute('aria-disabled'), 'true');
    assert.equal(h.component.regions.upcoming.retryTimer, undefined);
    h.component.destroy();
  }
});
