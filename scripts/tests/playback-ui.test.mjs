import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import test from 'node:test';
import ts from '../../hifimule-ui/node_modules/typescript/lib/typescript.js';

// Exercise the real component; replace only the native RPC, clock and browser
// boundary. Removing a DOM node also removes its focus in this test boundary.
function harness(initial, control = async () => {}, outputRpc = {}) {
  const document = { activeElement: null };
  class Element {
    children = []; dataset = {}; attributes = {}; listeners = new Map();
    textContent = ''; value = ''; hidden = false; disabled = false; isConnected = true;
    constructor(tag) { this.tagName = tag; }
    setAttribute(key, value) { this.attributes[key] = value; }
    append(...children) { for (const child of children) { child.remove(); child.parent = this; this.children.push(child); } }
    remove() { if (this.parent) { this.parent.children = this.parent.children.filter(child => child !== this); this.parent = null; } }
    appendChild(child) { this.append(child); return child; }
    contains(node) { return this === node || this.children.some(child => child.contains(node)); }
    replaceChildren(...children) {
      if (this.children.some(child => child.contains(document.activeElement))) document.activeElement = null;
      this.children = children;
    }
    addEventListener(name, listener) { this.listeners.set(name, listener); }
    querySelector(selector) {
      const action = selector.match(/data-playback-action="(.*?)"/)?.[1];
      for (const child of this.children) {
        if (action && child.dataset.playbackAction === action) return child;
        if (selector === child.tagName) return child;
        const nested = child.querySelector(selector); if (nested) return nested;
      }
      return null;
    }
    focus() { document.activeElement = this; }
    async click() { if (!this.disabled) await this.listeners.get('click')?.(); }
    async change(value) { this.value = value; await this.listeners.get('change')?.(); }
  }
  document.createElement = tag => new Element(tag);
  const timers = new Map(); let timerId = 0; let calls = 0; let snapshot = initial;
  const listeners = new Map();
  const window = {
    addEventListener: (type, callback) => listeners.set(callback, type),
    removeEventListener: (_type, callback) => listeners.delete(callback),
  };
  let now = 10000;
  const exports = {};
  const source = ts.transpileModule(readFileSync(new URL('../../hifimule-ui/src/components/PlaybackControls.ts', import.meta.url), 'utf8'), {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
  }).outputText;
  vm.runInNewContext(source, {
    exports, document, window, console, Date: class extends Date { static now() { return now; } },
    setTimeout: callback => { timers.set(++timerId, callback); return timerId; },
    clearTimeout: id => timers.delete(id),
    require: name => name === '../rpc'
      ? { playbackGetSession: async () => { calls++; return snapshot; }, playbackControl: control,
          serverList: async () => [{ id: 'local-server', serverId: 'server', url: 'https://music.example', serverType: 'jellyfin', username: 'alexis', name: 'Salon', icon: null, selected: true }],
          playbackListOutputs: outputRpc.list ?? (async () => ({ instanceId: snapshot.instanceId, outputs: snapshot.output?.selected ? [snapshot.output.selected] : [], output: snapshot.output })),
          playbackSelectOutput: outputRpc.select ?? (async () => snapshot),
          playbackSeek: outputRpc.seek ?? (async () => snapshot),
        }
      : name === '../serverIdentity'
        ? { formatServerIdentity: server => ({ label: server.name || 'Jellyfin' }) }
        : { t: (key, values) => values?.source ? `${key}: ${values.source}` : key },
  });
  const container = new Element('section');
  const component = new exports.PlaybackControls(container);
  return {
    container, component, document, timers, listeners,
    setSnapshot: value => { snapshot = value; }, calls: () => calls, advance: ms => { now += ms; },
    async tick() {
      for (let turn = 0; turn < 12; turn++) await Promise.resolve();
      const entry = timers.entries().next().value;
      if (entry) { timers.delete(entry[0]); entry[1](); }
      for (let turn = 0; turn < 12; turn++) await Promise.resolve();
    },
  };
}
function snapshot(state = 'buffering', sequence = '1', error = null) {
  return {
    schemaVersion: 1, instanceId: 'instance', sessionId: 'session', queueRevision: '1',
    stateSequence: sequence, generationId: 'generation', state, positionMs: 0,
    current: { occurrenceId: 'occurrence', source: { serverId: 'server', trackId: 'track' } },
    output: { revision: '1', selected: { outputId: 'headphones', displayName: 'Headphones', detail: 'USB', available: true, isDefault: false }, pending: null, active: null, status: 'available', error },
    playback: { status: error ? 'error' : ({ playing: 'active', buffering: 'loading', paused: 'paused' })[state], canGoNext: false, metadata: { title: 'Track' },
      durationMs: null, seek: { available: false, reason: 'playback.seek.unavailable' }, error },
  };
}
function text(element) { return element.textContent + element.children.map(text).join(' '); }

function albumButtonHarness() {
  const calls = []; const toasts = [];
  class Element {
    listeners = new Map(); disabled = false; name = ''; label = '';
    children = []; dataset = {}; attributes = {};
    appendChild(child) { child.parent = this; this.children.push(child); return child; }
    setAttribute(name, value) { this.attributes[name] = value; }
    addEventListener(name, listener) { this.listeners.set(name, listener); }
    async dispatchEvent(name, detail = {}) {
      const event = { target: this, defaultPrevented: false, stopped: false, ...detail,
        stopPropagation() { this.stopped = true; },
        preventDefault() { this.defaultPrevented = true; } };
      for (let node = this; node && !event.stopped; node = node.parent) {
        await node.listeners.get(name)?.(event);
      }
      return event;
    }
    async dispatch(name) { return (await this.dispatchEvent(name)).stopped; }
  }
  const exports = {};
  const source = ts.transpileModule(readFileSync(new URL('../../hifimule-ui/src/components/AlbumPlayButton.ts', import.meta.url), 'utf8'), {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
  }).outputText;
  vm.runInNewContext(source, {
    exports, document: { createElement: () => new Element() },
    require: name => name === '../rpc'
      ? { playbackPlayAlbum: async (serverId, albumId) => calls.push({ serverId, albumId }) }
      : name === '../toast'
        ? { showToast: (...args) => toasts.push(args) }
        : { t: (_key, values) => `Play ${values.title}` },
  });
  const viewExports = {};
  const viewSource = ts.transpileModule(readFileSync(new URL('../../hifimule-ui/src/components/TracksBrowseView.ts', import.meta.url), 'utf8'), {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
  }).outputText;
  vm.runInNewContext(viewSource, {
    exports: viewExports, document: { createElement: () => new Element() },
    require: name => name === './AlbumPlayButton' ? exports : {},
  });
  const selections = [];
  const view = Object.create(viewExports.TracksBrowseView.prototype);
  view.selectedAlbumId = null;
  view.selectAlbum = id => selections.push(id);
  // Mount the real row and helper together; only DOM/browser activation is replaced.
  const container = new Element();
  const mountRow = album => container.appendChild(view.buildAlbumRow(album));
  return { create: exports.createAlbumPlayButton, calls, toasts, mountRow, selections };
}

test('mounted album row leaves nested keyboard activation to its play button', async () => {
  const h = albumButtonHarness();
  const row = h.mountRow({ id: 'album', serverId: 'portable-server', name: 'Album' });
  const button = row.children[0];
  for (const key of [' ', 'Enter']) {
    const event = await button.dispatchEvent('keydown', { key });
    assert.equal(event.defaultPrevented, false, `${key} must retain button activation`);
    assert.deepEqual(h.selections, []);
    // Browser/Shoelace generates a click for an uncancelled activation key.
    await button.dispatch('click');
  }
  assert.deepEqual(h.calls, [
    { serverId: 'portable-server', albumId: 'album' },
    { serverId: 'portable-server', albumId: 'album' },
  ]);
  assert.deepEqual(h.selections, []);
  for (const key of [' ', 'Enter']) {
    assert.equal((await row.dispatchEvent('keydown', { key })).defaultPrevented, true);
  }
  assert.deepEqual(h.selections, ['album', 'album']);
  assert.equal(h.calls.length, 2);
});

test('loading playback exposes Pause rather than a second Resume', async () => {
  const h = harness(snapshot()); await h.tick();
  assert.ok(h.container.querySelector('[data-playback-action="pause"]'));
  assert.equal(h.container.querySelector('[data-playback-action="resume"]'), null);
  assert.equal(h.container.querySelector('[data-playback-action="retry"]').hidden, true);
  h.component.destroy();
});
test('Shoelace cannot override hidden transport actions', () => {
  const styles = readFileSync(new URL('../../hifimule-ui/src/styles.css', import.meta.url), 'utf8');
  assert.match(styles, /\.playback-controls sl-button\[hidden\]\s*\{\s*display:\s*none\s*!important;\s*\}/);
});
test('transport errors use the full player row before wrapping', () => {
  const styles = readFileSync(new URL('../../hifimule-ui/src/styles.css', import.meta.url), 'utf8');
  assert.match(styles, /\.playback-controls\s*\{[\s\S]*?flex-wrap:\s*wrap;/);
  assert.match(styles, /\.playback-controls__error\s*\{[^}]*flex:\s*1 0 100%;/);
});
test('album actions capture portable source identity and preserve parent navigation', async () => {
  const h = albumButtonHarness();
  const button = h.create('complete-album', 'portable-server-a', 'Album A');
  assert.equal(button.label, 'Play Album A');
  assert.equal(button.disabled, false);
  assert.equal(await button.dispatch('mousedown'), true);
  assert.equal(await button.dispatch('click'), true);
  assert.deepEqual(h.calls, [{ serverId: 'portable-server-a', albumId: 'complete-album' }]);

  const unavailable = h.create('album-without-source', undefined, 'Unavailable');
  assert.equal(unavailable.disabled, true);
  await unavailable.dispatch('click');
  assert.equal(h.calls.length, 1);
});
test('playback source renders the configured server label instead of its portable UUID', async () => {
  const h = harness(snapshot()); await h.tick();
  assert.match(text(h.container), /playback\.source: Salon/);
  assert.doesNotMatch(text(h.container), /playback\.source: server(?:\s|$)/);
  h.component.destroy();
});
test('audio output selection is tucked behind a compact icon control', async () => {
  const h = harness(snapshot()); await h.tick();
  const dropdown = h.container.querySelector('sl-dropdown');
  const outputButton = h.container.querySelector('sl-icon-button');
  assert.ok(dropdown);
  assert.ok(outputButton);
  assert.equal(outputButton.attributes.label, 'playback.output.label');
  assert.ok(dropdown.contains(h.container.querySelector('select')));
  h.component.destroy();
});
test('audio output panel expands left from its right-aligned trigger', () => {
  const styles = readFileSync(new URL('../../hifimule-ui/src/styles.css', import.meta.url), 'utf8');
  assert.match(styles, /\.playback-controls__output-dropdown::part\(panel\)\s*\{[\s\S]*width: min\(42rem, calc\(100vw - 2rem\)\)/);
  assert.match(styles, /\.playback-controls__output-dropdown::part\(panel\)\s*\{[\s\S]*max-width: calc\(100vw - 2rem\)/);
});
test('primary transport retains keyboard focus when Pause becomes Resume', async () => {
  const h = harness(snapshot('playing')); await h.tick();
  const primary = h.container.querySelector('[data-playback-action="pause"]'); primary.focus();
  h.setSnapshot(snapshot('paused', '2')); await h.tick();
  assert.equal(h.document.activeElement, h.container.querySelector('[data-playback-action="resume"]'));
  assert.ok(h.document.activeElement);
  h.component.destroy();
});
test('output loss resumes only the selected output', async () => {
  const h = harness(snapshot('paused', '1', { code: 'OUTPUT_LOST', retryable: true })); await h.tick();
  assert.match(text(h.container), /playback.resume_selected_output/);
  h.component.destroy();
});
test('transport rejection is caught and shown without exposing raw diagnostics', async () => {
  const h = harness(snapshot('playing'), async () => { throw { data: { code: 'PERSISTENCE_FAILED' }, message: 'private raw diagnostic' }; });
  await h.tick(); await h.container.querySelector('[data-playback-action="stop"]').click();
  assert.match(text(h.container), /playback.command_error/);
  assert.doesNotMatch(text(h.container), /private raw/);
  h.component.destroy();
});
test('Next follows authoritative capability and Retry appears only for retryable failure', async () => {
  const actions = [];
  const withNext = snapshot('paused'); withNext.playback.canGoNext = true;
  const h = harness(withNext, async action => actions.push(action)); await h.tick();
  const next = h.container.querySelector('[data-playback-action="next"]');
  assert.equal(next.disabled, false); await next.click();
  assert.deepEqual(actions, ['next']);
  const failed = snapshot('paused', '2', { code: 'SOURCE_UNAVAILABLE', retryable: true });
  failed.playback.canGoNext = false; h.setSnapshot(failed); await h.tick();
  assert.equal(h.container.querySelector('[data-playback-action="next"]').disabled, true);
  const retry = h.container.querySelector('[data-playback-action="retry"]');
  assert.equal(retry.hidden, false); await retry.click();
  assert.deepEqual(actions, ['next', 'retry']);
  h.component.destroy();
});
test('completed final occurrence keeps Next unavailable and ignores an older poll response', async () => {
  const completed = snapshot('paused', '9');
  completed.playback.status = 'completed';
  completed.playback.canGoNext = false;
  completed.playback.metadata.title = 'Final track';
  const h = harness(completed); await h.tick();
  assert.equal(h.container.querySelector('[data-playback-action="next"]').disabled, true);
  assert.match(text(h.container), /Final track/);

  const stale = snapshot('playing', '8');
  stale.playback.metadata.title = 'Stale track';
  stale.playback.canGoNext = true;
  h.setSnapshot(stale); await h.tick();
  assert.match(text(h.container), /Final track/);
  assert.doesNotMatch(text(h.container), /Stale track/);
  assert.equal(h.container.querySelector('[data-playback-action="next"]').disabled, true);
  h.component.destroy();
});
test('destroy cancels polling and removes pagehide listeners', async () => {
  const h = harness(snapshot()); await h.tick(); const before = h.calls();
  h.component.destroy(); await h.tick();
  assert.equal(h.calls(), before); assert.equal(h.timers.size, 0); assert.equal(h.listeners.size, 0);
});
test('detached control stops its polling lifecycle', async () => {
  const h = harness(snapshot()); await h.tick(); h.container.isConnected = false;
  await h.tick(); const before = h.calls(); await h.tick();
  assert.equal(h.calls(), before); assert.equal(h.listeners.size, 0);
});


test('output selection leaves Pause and Stop usable and never calls Resume', async () => {
  let finish; const pending = new Promise(resolve => { finish = resolve; });
  const actions = []; const choices = [];
  const h = harness(snapshot('playing'), async action => actions.push(action), {
    select: async (...args) => { choices.push(args); return pending; },
  });
  await h.tick();
  const select = h.container.querySelector('select'); select.focus();
  await select.change('replacement');
  assert.equal(choices[0][0], 'replacement');
  assert.equal(h.container.querySelector('[data-playback-action="pause"]').disabled, false);
  assert.equal(h.container.querySelector('[data-playback-action="stop"]').disabled, false);
  await h.container.querySelector('[data-playback-action="stop"]').click();
  assert.deepEqual(actions, ['stop']);
  finish(snapshot('paused', '2')); await h.tick();
  assert.deepEqual(actions, ['stop']);
  h.component.destroy();
});

test('selector stays mounted and focused across polling and duplicate-name discovery', async () => {
  const one = { outputId: 'one', displayName: 'USB audio', detail: 'USB · 1', available: true };
  const two = { outputId: 'two', displayName: 'USB audio', detail: 'USB · 2', available: true };
  const h = harness(snapshot(), async () => {}, { list: async () => ({ instanceId: 'instance', outputs: [one, two] }) });
  await h.tick();
  const select = h.container.querySelector('select');
  assert.match(text(select), /USB · 1/); assert.match(text(select), /USB · 2/);
  select.focus();
  h.setSnapshot(snapshot('paused', '2')); await h.tick();
  assert.equal(h.document.activeElement, select);
  assert.equal(h.container.querySelector('select'), select);
  h.component.destroy();
});

test('focus discovery updates option nodes without requiring blur or restarting playback', async () => {
  const one = { outputId: 'headphones', displayName: 'Headphones', detail: 'USB', available: true };
  const two = { outputId: 'second', displayName: 'New output', detail: 'USB', available: true };
  let outputs = [one];
  const h = harness(snapshot(), async () => { assert.fail('discovery cannot start transport'); }, {
    list: async () => ({ instanceId: 'instance', outputs }),
  });
  await h.tick();
  const select = h.container.querySelector('select');
  const original = select.children.find(option => option.value === one.outputId);
  select.focus();
  outputs = [{ ...one, displayName: 'Renamed' }, two];
  await select.listeners.get('focus')(); await h.tick();
  assert.equal(h.document.activeElement, select);
  assert.equal(select.children.find(option => option.value === one.outputId), original);
  assert.match(text(select), /Renamed/);
  assert.match(text(select), /New output/);
  outputs = [one];
  h.advance(1000); await h.tick(); // Pick up the asynchronous worker without another focus event.
  assert.equal(select.children.some(option => option.value === two.outputId), false);
  assert.equal(h.document.activeElement, select);
  h.component.destroy();
});

test('unavailable output disables Resume while keeping output choice available without a track', async () => {
  const unavailable = snapshot('paused'); unavailable.output.selected.available = false;
  const h = harness(unavailable); await h.tick();
  assert.equal(h.container.querySelector('[data-playback-action="resume"]').disabled, true);
  unavailable.current = null; unavailable.stateSequence = '2'; h.setSnapshot(unavailable); await h.tick();
  assert.equal(h.container.querySelector('select').hidden, false);
  h.component.destroy();
});

test('last-resort output remains selectable and warns that routing is best effort', async () => {
  const fallback = { outputId: 'fallback', displayName: 'Built-in Audio', detail: 'Analog', available: true, isDefault: true, identityConfidence: 'fallback', isVirtual: false };
  const h = harness(snapshot(), async () => {}, { list: async () => ({ instanceId: 'instance', outputs: [fallback] }) });
  await h.tick();
  const option = h.container.querySelector('select').children.find(child => child.value === 'fallback');
  assert.equal(option.disabled, false);
  assert.match(text(option), /playback\.output\.fallback/);
  h.component.destroy();
});

test('output and seek strings have four-locale parity and no current-default recovery instruction', () => {
  const catalog = JSON.parse(readFileSync(new URL('../../hifimule-i18n/catalog.json', import.meta.url), 'utf8'));
  const keys = Object.keys(catalog.en).filter(key => key.startsWith('playback.output.')
    || key.startsWith('playback.error.OUTPUT_') || key.startsWith('playback.seek.')
    || key.startsWith('seek.') || ['playback.play_album', 'playback.next', 'playback.retry'].includes(key));
  for (const language of ['en', 'fr', 'es', 'de']) {
    for (const key of keys) assert.ok(catalog[language][key], `${language}: ${key}`);
    assert.equal(catalog[language]['playback.resume_default_output'], undefined);
    assert.ok(catalog[language]['playback.resume_selected_output'].includes('{name}'));
  }
});

test('seek slider previews locally and submits only the committed change', async () => {
  const calls = [];
  const initial = snapshot('paused');
  initial.playback.durationMs = 10_000;
  initial.playback.seek = { available: true, mechanism: 'ffmpeg-post-open-media-time-seek' };
  const h = harness(initial, async () => {}, { seek: async (position, observed) => {
    calls.push([position, observed.generationId]);
    return { ...initial, stateSequence: '2', generationId: 'seek-generation',
      playback: { ...initial.playback, pendingSeek: { operationId: 'seek', requestedPositionMs: position, priorCommittedPositionMs: 0 } } };
  }});
  await h.tick();
  const slider = h.container.querySelector('input');
  slider.value = '4000';
  await slider.listeners.get('input')();
  assert.equal(calls.length, 0);
  await slider.listeners.get('change')();
  for (let turn = 0; turn < 12; turn++) await Promise.resolve();
  assert.deepEqual(calls, [[4000, 'generation']]);
  assert.match(text(h.container), /playback.seek.pending/);
  h.component.destroy();
});

test('repeated scrubs coalesce to the latest target while seek RPC is pending', async () => {
  const calls = [];
  let releaseFirst;
  const initial = snapshot('paused');
  initial.playback.durationMs = 10_000;
  initial.playback.seek = { available: true };
  const h = harness(initial, async () => {}, { seek: (position, observed) => {
    calls.push([position, observed.generationId]);
    const response = { ...initial, stateSequence: String(calls.length + 1), generationId: `seek-${calls.length}` };
    if (calls.length === 1) return new Promise(resolve => { releaseFirst = () => resolve(response); });
    return Promise.resolve(response);
  }});
  await h.tick();
  const slider = h.container.querySelector('input');
  await slider.change('2000');
  await slider.change('3000');
  await slider.change('4000');
  assert.deepEqual(calls, [[2000, 'generation']]);
  releaseFirst();
  for (let turn = 0; turn < 24; turn++) await Promise.resolve();
  assert.deepEqual(calls, [[2000, 'generation'], [4000, 'seek-1']]);
  h.component.destroy();
});

test('authoritative clock advances only while fresh and reanchors backward', async () => {
  const active = snapshot('playing');
  active.positionMs = 2_000;
  active.playback.durationMs = 10_000;
  active.playback.seek = { available: true };
  const h = harness(active); await h.tick();
  const slider = h.container.querySelector('input');
  h.advance(500); h.component.renderTimeline();
  assert.equal(slider.value, '2500');
  h.advance(400); h.component.renderTimeline();
  assert.equal(slider.value, '2750');
  const backward = { ...active, positionMs: 1_000, stateSequence: '2', generationId: 'seek-generation' };
  h.setSnapshot(backward); await h.tick();
  assert.equal(slider.value, '1000');
  const paused = { ...backward, state: 'paused', stateSequence: '3', playback: { ...backward.playback, status: 'paused' } };
  h.setSnapshot(paused); await h.tick(); h.advance(700); h.component.renderTimeline();
  assert.equal(slider.value, '1000');
  h.component.destroy();
});


test('progress polls preserve a dragged thumb and keep elapsed authoritative', async () => {
  const initial = snapshot('playing');
  initial.positionMs = 1000;
  initial.playback.durationMs = 10000;
  initial.playback.seek = { available: true };
  const h = harness(initial); await h.tick();
  const slider = h.container.querySelector('input'); slider.focus();
  slider.value = '7000'; slider.listeners.get('input')();
  assert.equal(h.component.elapsed.textContent, '0:01');
  h.setSnapshot({ ...initial, stateSequence: '2', positionMs: 2000 }); await h.tick();
  assert.equal(slider.value, '7000');
  assert.equal(h.component.elapsed.textContent, '0:02');
  assert.equal(h.document.activeElement, slider);
  h.component.destroy();
});

test('queued scrub is abandoned when its occurrence or session is replaced', async () => {
  for (const replaceSession of [false, true]) {
    let release;
    const pending = new Promise(resolve => { release = resolve; });
    const calls = [];
    const initial = snapshot('paused');
    initial.playback.durationMs = 10000;
    initial.playback.seek = { available: true };
    const h = harness(initial, async () => {}, { seek: async (position, observed) => {
      calls.push([position, observed.current.occurrenceId]); return pending;
    }});
    await h.tick();
    const first = h.component.submitSeek(4000);
    await h.component.submitSeek(7000);
    const replacement = { ...initial, stateSequence: '9', generationId: 'replacement',
      sessionId: replaceSession ? 'other-session' : initial.sessionId,
      current: replaceSession ? initial.current : { ...initial.current, occurrenceId: 'other-track' } };
    h.setSnapshot(replacement); await h.tick();
    release({ ...initial, stateSequence: '2' }); await first;
    for (let turn = 0; turn < 12; turn++) await Promise.resolve();
    assert.deepEqual(calls, [[4000, 'occurrence']]);
    assert.equal(h.component.snapshot, replacement);
    h.component.destroy();
  }
});

test('seek rejection renders without a state change and retry clears the error', async () => {
  const initial = snapshot('paused');
  initial.playback.durationMs = 10000;
  initial.playback.seek = { available: true };
  let reject = true;
  const h = harness(initial, async () => {}, { seek: async () => {
    if (reject) throw { data: { code: 'INVALID_SEEK' } };
    return initial;
  }});
  await h.tick(); await h.component.submitSeek(11000);
  assert.match(text(h.container), /playback.error.INVALID_SEEK/);
  await h.tick(); assert.match(text(h.container), /playback.error.INVALID_SEEK/);
  reject = false; await h.component.submitSeek(1000);
  assert.doesNotMatch(text(h.container), /playback.error.INVALID_SEEK/);
  h.component.destroy();
});
