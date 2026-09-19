import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import vm from 'node:vm';
import ts from '../../hifimule-ui/node_modules/typescript/lib/typescript.js';

function harness() {
  const saved = new Map();
  const toasts = [];
  let timer = 0;
  class TestCustomEvent extends Event {
    constructor(type, init = {}) { super(type); this.detail = init.detail; }
  }
  const window = {
    setTimeout: () => ++timer,
    clearTimeout: () => {},
    dispatchEvent: event => { if (event.type === 'toast') toasts.push(event.detail); return true; },
  };
  const localStorage = {
    getItem: key => saved.get(key) ?? null,
    setItem: (key, value) => saved.set(key, String(value)),
  };
  const exports = {};
  const source = ts.transpileModule(readFileSync(new URL('../../hifimule-ui/src/state/basket.ts', import.meta.url), 'utf8'), {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
  }).outputText;
  vm.runInNewContext(source, {
    exports, window, localStorage, console, EventTarget, CustomEvent: TestCustomEvent,
    require: name => name === '../i18n'
      ? { t: key => key }
      : { rpcCall: async () => [] },
  });
  return { store: exports.basketStore, toasts };
}

const item = id => ({ id, name: id, type: 'Audio', childCount: 1, sizeTicks: 1, sizeBytes: 1 });

test('user basket mutations require a selected physical target while reconciliation remains trusted', () => {
  const { store, toasts } = harness();
  store.setActiveServerId('server-a');

  assert.equal(store.add(item('blocked')), false);
  assert.equal(store.has('blocked'), false);
  assert.equal(toasts.length, 1);

  store.setPhysicalTargetAvailable(true);
  assert.equal(store.add(item('kept')), true);
  assert.equal(store.has('kept'), true);

  store.setPhysicalTargetAvailable(false);
  assert.equal(store.remove('kept'), false);
  assert.equal(store.toggle(item('other')), false);
  assert.equal(store.clear(), false);
  assert.equal(store.has('kept'), true);

  store.hydrateFromDaemon([item('hydrated')]);
  assert.equal(store.has('kept'), false);
  assert.equal(store.has('hydrated'), true);
  assert.equal(store.removeItemsForServer('server-a'), 0);
  store.clearForDevice();
  assert.equal(store.getItems().length, 0);
});

test('every existing basket mutation surface consumes the shared physical-target admission API', () => {
  const sources = [
    '../../hifimule-ui/src/library.ts',
    '../../hifimule-ui/src/components/MediaCard.ts',
    '../../hifimule-ui/src/components/TracksBrowseView.ts',
    '../../hifimule-ui/src/components/BasketSidebar.ts',
  ].map(path => readFileSync(new URL(path, import.meta.url), 'utf8'));
  for (const source of sources) {
    assert.match(source, /admitPhysicalTargetMutation\(|hasPhysicalTarget\(/);
  }
});
