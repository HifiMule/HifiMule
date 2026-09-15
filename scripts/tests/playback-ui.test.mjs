import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import test from 'node:test';
import ts from '../../hifimule-ui/node_modules/typescript/lib/typescript.js';

// Exercise the real component; replace only the native RPC, clock and browser
// boundary. Removing a DOM node also removes its focus in this test boundary.
function harness(initial, control = async () => {}) {
  const document = { activeElement: null };
  class Element {
    children = []; dataset = {}; attributes = {}; listeners = new Map();
    textContent = ''; hidden = false; disabled = false; isConnected = true;
    constructor(tag) { this.tagName = tag; }
    setAttribute(key, value) { this.attributes[key] = value; }
    append(...children) { this.children.push(...children); }
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
        const nested = child.querySelector(selector); if (nested) return nested;
      }
      return null;
    }
    focus() { document.activeElement = this; }
    async click() { if (!this.disabled) await this.listeners.get('click')?.(); }
  }
  document.createElement = tag => new Element(tag);
  const timers = new Map(); let timerId = 0; let calls = 0; let snapshot = initial;
  const listeners = new Map();
  const window = {
    addEventListener: (type, callback) => listeners.set(callback, type),
    removeEventListener: (_type, callback) => listeners.delete(callback),
  };
  const exports = {};
  const source = ts.transpileModule(readFileSync(new URL('../../hifimule-ui/src/components/PlaybackControls.ts', import.meta.url), 'utf8'), {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
  }).outputText;
  vm.runInNewContext(source, {
    exports, document, window, console,
    setTimeout: callback => { timers.set(++timerId, callback); return timerId; },
    clearTimeout: id => timers.delete(id),
    require: name => name === '../rpc'
      ? { playbackGetSession: async () => { calls++; return snapshot; }, playbackControl: control }
      : { t: key => key },
  });
  const container = new Element('section');
  const component = new exports.PlaybackControls(container);
  return {
    container, component, document, timers, listeners,
    setSnapshot: value => { snapshot = value; }, calls: () => calls,
    async tick() {
      await Promise.resolve(); await Promise.resolve();
      const entry = timers.entries().next().value;
      if (entry) { timers.delete(entry[0]); entry[1](); }
      await Promise.resolve(); await Promise.resolve();
    },
  };
}
function snapshot(state = 'buffering', sequence = '1', error = null) {
  return {
    schemaVersion: 1, instanceId: 'instance', sessionId: 'session', queueRevision: '1',
    stateSequence: sequence, generationId: 'generation', state, positionMs: 0,
    current: { occurrenceId: 'occurrence', source: { serverId: 'server', trackId: 'track' } },
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
test('primary transport retains keyboard focus when Pause becomes Resume', async () => {
  const h = harness(snapshot('playing')); await h.tick();
  const primary = h.container.querySelector('[data-playback-action="pause"]'); primary.focus();
  h.setSnapshot(snapshot('paused', '2')); await h.tick();
  assert.equal(h.document.activeElement, h.container.querySelector('[data-playback-action="resume"]'));
  assert.ok(h.document.activeElement);
  h.component.destroy();
});
test('output loss explains that retry opens the current default output', async () => {
  const h = harness(snapshot('paused', '1', { code: 'OUTPUT_LOST', retryable: true })); await h.tick();
  assert.match(text(h.container), /playback.resume_default_output/);
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
