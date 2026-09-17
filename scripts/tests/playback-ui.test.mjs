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
          playbackListOutputs: outputRpc.list ?? (async () => ({ instanceId: snapshot.instanceId, outputs: snapshot.output?.selected ? [snapshot.output.selected] : [], output: snapshot.output })),
          playbackSelectOutput: outputRpc.select ?? (async () => snapshot),
        }
      : { t: key => key },
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
    playback: { status: error ? 'error' : ({ playing: 'active', buffering: 'loading', paused: 'paused' })[state], metadata: { title: 'Track' }, error },
  };
}
function text(element) { return element.textContent + element.children.map(text).join(' '); }

test('loading playback exposes Pause rather than a second Resume', async () => {
  const h = harness(snapshot()); await h.tick();
  assert.ok(h.container.querySelector('[data-playback-action="pause"]'));
  assert.equal(h.container.querySelector('[data-playback-action="resume"]'), null);
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

test('output strings have four-locale parity and no current-default recovery instruction', () => {
  const catalog = JSON.parse(readFileSync(new URL('../../hifimule-i18n/catalog.json', import.meta.url), 'utf8'));
  const keys = Object.keys(catalog.en).filter(key => key.startsWith('playback.output.') || key.startsWith('playback.error.OUTPUT_'));
  for (const language of ['en', 'fr', 'es', 'de']) {
    for (const key of keys) assert.ok(catalog[language][key], `${language}: ${key}`);
    assert.equal(catalog[language]['playback.resume_default_output'], undefined);
    assert.ok(catalog[language]['playback.resume_selected_output'].includes('{name}'));
  }
});
