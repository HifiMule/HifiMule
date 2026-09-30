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
  get options() { return this.tagName === 'select' ? this.children : undefined; }
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
  focus() { this.ownerDocument.activeElement = this; this.listeners.get('focus')?.(); }
  blur() { this.ownerDocument.activeElement = null; this.listeners.get('blur')?.(); }
  addEventListener(key, handler) { this.listeners.set(key, handler); }
  click() { if (!this.disabled) { this.listeners.get('click')?.(); this.onclick?.(); } }
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
  const document = { activeElement: null, createElement: tag => { const element = new Element(tag); element.ownerDocument = document; return element; } };
  const Option = class extends Element {
    constructor(label, value) { super('option'); this.textContent = label; this.value = value; }
  };
  vm.runInNewContext(source, { exports, document, Option, setTimeout, clearTimeout, require: name => ({
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
  const errors = ['PLAYBACK_SELECTION_EMPTY', 'PLAYBACK_SELECTION_SETUP', 'PLAYBACK_SELECTION_SOURCE_UNAVAILABLE', 'PLAYBACK_SELECTION_PREPARATION_FAILED'];
  const { settings, root } = load({
    playbackGetSelectionConfig: async () => config,
    serverList: async () => [server],
    playbackSelectionOptions: async () => ({ supported: true, options: [{ id: 'list', name: 'List' }] }),
    playbackStartSelection: async () => { throw new Error(errors.shift()); },
    playbackCancelSelectionStart: async () => {},
  });
  await tick();
  for (const expected of ['empty', 'invalid', 'unavailable', 'preparation_failed']) {
    button(root, 'playback.selection.start').click();
    await tick();
    assert.equal(status(root).textContent, `playback.selection.${expected}`);
  }
  settings.destroy();
});

test('saving a seed edit preserves advanced ordering and track cap', async () => {
  let saved;
  const advanced = { ...config, ordering: ['favorite', 'quality'], maxTracks: 37 };
  const { settings, root } = load({
    playbackGetSelectionConfig: async () => advanced,
    serverList: async () => [server],
    playbackSelectionOptions: async () => ({ supported: true, options: [{ id: 'list', name: 'List' }] }),
    playbackSaveSelectionConfig: async value => { saved = value; },
  });
  await tick();
  const seed = root.find(node => node.tagName === 'input' && node.type === 'number');
  seed.value = '8'; seed.listeners.get('input')();
  button(root, 'playback.selection.save').click();
  await tick();
  assert.deepEqual(Array.from(saved.ordering), ['favorite', 'quality']);
  assert.equal(saved.maxTracks, 37);
  settings.destroy();
});

test('source options can be paged beyond the first page', async () => {
  const offsets = [];
  const { settings, root } = load({
    playbackGetSelectionConfig: async () => config,
    serverList: async () => [server],
    playbackSelectionOptions: async (_serverId, _kind, offset) => {
      offsets.push(offset);
      return offset === 0
        ? { supported: true, options: Array.from({ length: 400 }, (_, index) => ({ id: index === 0 ? 'list' : `id-${index}`, name: `Source ${index}` })), hasMore: true }
        : { supported: true, options: [{ id: 'later', name: 'Later' }], hasMore: false };
    },
  });
  await tick(); await tick();
  const input = root.find(node => node.attributes.role === 'combobox');
  input.focus();
  input.blur(); // A native button takes focus when More is clicked.
  assert.equal(button(root, 'playback.selection.more').disabled, false);
  button(root, 'playback.selection.more').click();
  await tick();
  assert.deepEqual(offsets, [0, 400]);
  assert.ok(root.all(node => node.attributes.role === 'option').some(option => option.textContent.includes('Later')));
  assert.equal(root.find(node => node.attributes.role === 'listbox').hidden, false);
  settings.destroy();
});

test('autocomplete searches beyond the initial page and saves the selected artist ID', async () => {
  let saved;
  const { settings, root } = load({
    playbackGetSelectionConfig: async () => ({ ...config, sources: [{ serverId: 'portable', kind: 'artist', ref: 'early' }] }),
    serverList: async () => [server],
    playbackSelectionOptions: async (_server, _kind, _offset, query) => ({ supported: true,
      options: query === 'Zebra' ? [{ id: 'late', name: 'Zebra' }] : [{ id: 'early', name: 'Alpha' }], hasMore: false }),
    playbackSaveSelectionConfig: async value => { saved = value; },
  });
  await tick();
  const input = root.find(node => node.attributes.role === 'combobox');
  assert.ok(input, 'source must be a searchable combobox');
  input.value = 'Zebra'; input.listeners.get('input')();
  assert.equal(button(root, 'playback.selection.save').disabled, true, 'unselected text cannot be saved as an ID');
  await new Promise(resolve => setTimeout(resolve, 350)); await tick();
  const match = root.find(node => node.attributes.role === 'option' && node.textContent === 'Zebra');
  assert.ok(match); match.click();
  button(root, 'playback.selection.save').click(); await tick();
  assert.equal(saved.sources[0].ref, 'late');
  assert.equal(input.value, 'Zebra');
  settings.destroy();
});

test('a source added from the library displays its resolved name outside the options page', async () => {
  const { settings, root } = load({
    playbackGetSelectionConfig: async () => ({ ...config, sources: [{ serverId: 'portable', kind: 'artist', ref: 'late' }] }),
    serverList: async () => [server],
    playbackSelectionOptions: async (_server, _kind, _offset, _query, ref) => ({ supported: true,
      options: [{ id: 'early', name: 'Alpha' }], selected: ref === 'late' ? { id: 'late', name: 'Zebra' } : null }),
  });
  await tick();
  await tick();
  assert.equal(root.find(node => node.attributes.role === 'combobox')?.value, 'Zebra');
  settings.destroy();
});

test('typing invalidates an earlier options response before the debounce finishes', async () => {
  const old = deferred();
  const { settings, root } = load({
    playbackGetSelectionConfig: async () => ({ ...config, sources: [{ serverId: 'portable', kind: 'artist', ref: 'late' }] }),
    serverList: async () => [server],
    playbackSelectionOptions: (_server, _kind, _offset, query) => query ? Promise.resolve({ supported: true, options: [] }) : old.promise,
  });
  await tick();
  const input = root.find(node => node.attributes.role === 'combobox');
  assert.ok(input);
  input.value = 'New query'; input.listeners.get('input')();
  old.resolve({ supported: true, options: [{ id: 'late', name: 'Old name' }], selected: { id: 'late', name: 'Old name' } });
  await tick();
  assert.equal(input.value, 'New query');
  assert.equal(button(root, 'playback.selection.save').disabled, true);
  settings.destroy();
});

test('keyboard selection distinguishes duplicate names and remains editable after saving', async () => {
  const saved = [];
  const { settings, root } = load({
    playbackGetSelectionConfig: async () => ({ ...config, sources: [{ serverId: 'portable', kind: 'artist', ref: 'first' }] }),
    serverList: async () => [server],
    playbackSelectionOptions: async () => ({ supported: true, options: [{ id: 'first', name: 'Same name' }, { id: 'second', name: 'Same name' }] }),
    playbackSaveSelectionConfig: async value => saved.push(value),
  });
  await tick(); await tick();
  const input = root.find(node => node.attributes.role === 'combobox');
  const key = value => input.listeners.get('keydown')({ key: value, preventDefault() {} });
  key('ArrowDown'); key('Enter');
  button(root, 'playback.selection.save').click(); await tick();
  key('ArrowDown'); key('ArrowDown'); key('Enter');
  button(root, 'playback.selection.save').click(); await tick();
  assert.deepEqual(saved.map(value => value.sources[0].ref), ['first', 'second']);
  assert.deepEqual(root.all(node => node.attributes.role === 'option').map(node => node.textContent), ['Same name (first)', 'Same name (second)']);
  settings.destroy();
});
