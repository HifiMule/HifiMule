import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import vm from 'node:vm';
import ts from '../../hifimule-ui/node_modules/typescript/lib/typescript.js';

class Element {
  constructor(tag) { this.tagName = tag; }
  children = []; listeners = new Map(); attributes = {}; parentElement = null;
  textContent = ''; disabled = false; connectedRoot = false; value = '';
  get isConnected() { return this.connectedRoot || Boolean(this.parentElement?.isConnected); }
  append(...children) {
    for (const child of children) { child.parentElement = this; this.children.push(child); }
  }
  replaceChildren(...children) {
    for (const child of this.children) child.parentElement = null;
    this.children = [];
    this.append(...children);
  }
  add(option) { this.append(option); if (!this.value) this.value = option.value; }
  setAttribute(key, value) { this.attributes[key] = value; }
  addEventListener(key, handler) { this.listeners.set(key, handler); }
  click() { if (!this.disabled) this.listeners.get('click')?.(); }
  find(predicate) { return predicate(this) ? this : this.children.map(child => child.find(predicate)).find(Boolean); }
  all(predicate) { return (predicate(this) ? [this] : []).concat(this.children.flatMap(child => child.all(predicate))); }
}

function deferred() {
  let resolve; let reject;
  const promise = new Promise((res, rej) => { resolve = res; reject = rej; });
  return { promise, resolve, reject };
}

function load(mocks) {
  const source = ts.transpileModule(readFileSync(new URL('../../hifimule-ui/src/components/PlaybackSelectionSettings.ts', import.meta.url), 'utf8'), {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
  }).outputText;
  const exports = {};
  const document = { createElement: tag => new Element(tag) };
  const Option = class extends Element {
    constructor(label, value) { super('option'); this.textContent = label; this.value = value; }
  };
  vm.runInNewContext(source, { exports, document, Option, require: name => ({
    '../rpc': mocks,
    '../i18n': { t: key => key },
    '../serverIdentity': { formatServerIdentity: server => ({ label: server.name ?? server.serverId }) },
  })[name], console });
  const settings = new exports.PlaybackSelectionSettings();
  const root = new Element('div'); root.connectedRoot = true; root.append(settings.element);
  return { settings, root };
}

const config = { schemaVersion: 1, sources: [{ serverId: 'portable', kind: 'playlist', ref: 'list' }],
  ordering: ['random'], seed: 7, maxTracks: 16 };
const server = { serverId: 'portable', name: 'Music' };
const tick = async () => { await Promise.resolve(); await Promise.resolve(); await Promise.resolve(); };
const button = (root, key) => root.find(node => node.tagName === 'button' && node.textContent === key);
const status = root => root.find(node => node.attributes.role === 'status');

test('failed cancellation lets the active start settle and restores controls', async () => {
  const start = deferred();
  const cancel = deferred();
  const { settings, root } = load({
    playbackGetSelectionConfig: async () => config,
    serverList: async () => [server],
    playbackSelectionOptions: async () => ({ supported: true, options: [{ id: 'list', name: 'List' }] }),
    playbackStartSelection: () => start.promise,
    playbackCancelSelectionStart: () => cancel.promise,
  });
  await tick();
  button(root, 'playback.selection.start').click();
  assert.equal(button(root, 'playback.selection.cancel').disabled, false);
  button(root, 'playback.selection.cancel').click();
  cancel.reject(new Error('connection lost'));
  await tick();
  assert.equal(status(root).textContent, 'playback.selection.cancel_failed');
  start.resolve();
  await tick();
  assert.equal(status(root).textContent, 'playback.selection.started');
  assert.equal(button(root, 'playback.selection.start').disabled, false);
  settings.destroy();
});

test('successful cancellation ignores an older start result and reports cancellation', async () => {
  const start = deferred();
  const { settings, root } = load({
    playbackGetSelectionConfig: async () => config,
    serverList: async () => [server],
    playbackSelectionOptions: async () => ({ supported: true, options: [{ id: 'list', name: 'List' }] }),
    playbackStartSelection: () => start.promise,
    playbackCancelSelectionStart: async () => {},
  });
  await tick();
  button(root, 'playback.selection.start').click();
  button(root, 'playback.selection.cancel').click();
  await tick();
  start.resolve();
  await tick();
  assert.equal(status(root).textContent, 'playback.selection.cancelled');
  assert.equal(button(root, 'playback.selection.start').disabled, false);
  assert.equal(status(root).attributes['aria-live'], 'polite');
  const selects = root.all(node => node.tagName === 'select');
  assert.ok(selects.every(select => select.parentElement?.tagName === 'label'));
  assert.ok(root.all(node => node.tagName === 'button').every(control => control.type === 'button'));
  assert.equal(button(root, 'playback.selection.remove').attributes['aria-label'], 'playback.selection.remove 1');
  settings.destroy();
});

test('start reports empty, missing setup and unavailable source separately', async () => {
  const errors = ['PLAYBACK_SELECTION_EMPTY', 'PLAYBACK_SELECTION_SETUP', 'PLAYBACK_SELECTION_SOURCE_UNAVAILABLE'];
  const { settings, root } = load({
    playbackGetSelectionConfig: async () => config,
    serverList: async () => [server],
    playbackSelectionOptions: async () => ({ supported: true, options: [{ id: 'list', name: 'List' }] }),
    playbackStartSelection: async () => { throw new Error(errors.shift()); },
    playbackCancelSelectionStart: async () => {},
  });
  await tick();
  for (const expected of ['empty', 'invalid', 'unavailable']) {
    button(root, 'playback.selection.start').click();
    await tick();
    assert.equal(status(root).textContent, `playback.selection.${expected}`);
  }
  settings.destroy();
});
