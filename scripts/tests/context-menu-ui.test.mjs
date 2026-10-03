import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import vm from 'node:vm';
import ts from '../../hifimule-ui/node_modules/typescript/lib/typescript.js';

function harness() {
  class Element extends EventTarget {
    children = []; style = {}; attributes = {}; classes = new Set();
    offsetWidth = 180; offsetHeight = 40;
    classList = { add: name => this.classes.add(name) };
    setAttribute(name, value) { this.attributes[name] = value; }
    appendChild(child) { this.children.push(child); child.parent = this; }
    remove() { if (this.parent) this.parent.children = this.parent.children.filter(child => child !== this); }
    contains(target) { return target === this || this.children.some(child => child.contains(target)); }
    focus() { document.activeElement = this; }
    click() { this.dispatchEvent(new Event('click')); }
  }
  const document = new EventTarget();
  document.body = new Element(); document.createElement = () => new Element();
  const window = Object.assign(new EventTarget(), { innerWidth: 500, innerHeight: 400 });
  const frames = []; const actions = [];
  const source = readFileSync(new URL('../../hifimule-ui/src/components/MediaCard.ts', import.meta.url), 'utf8').replace(/^import .*;\r?\n/gm, '');
  const compiled = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS } }).outputText;
  const context = vm.createContext({ exports: {}, document, window, t: key => key, requestAnimationFrame: fn => frames.push(fn) });
  vm.runInContext(compiled, context);
  const card = context.exports.MediaCard;
  card.openAddToPlaylistDialog = (ids, name) => actions.push({ ids: Array.from(ids), name });
  return { card, document, window, actions, reveal: () => frames.splice(0).forEach(fn => fn()) };
}

test('shared card/list/track context menu reveals within viewport and activates playlist action', () => {
  const h = harness();
  h.card.showItemContextMenu(490, 390, 'track-1', 'Track'); h.reveal();
  const menu = h.document.body.children[0];
  assert.equal(menu.style.left, '312px'); assert.equal(menu.style.top, '352px');
  assert.ok(menu.classes.has('is-open')); assert.equal(h.document.activeElement, menu.children[0]);
  menu.children[0].click();
  assert.deepEqual(h.actions, [{ ids: ['track-1'], name: 'Track' }]);
  assert.equal(h.document.body.children.length, 0);
});

test('keyboard activation and dismissal preserve menu behavior', () => {
  for (const key of ['Enter', ' ', 'Escape', 'Tab']) {
    const h = harness(); h.card.showItemContextMenu(20, 30, 'album-1', 'Album'); h.reveal();
    const event = new Event('keydown', { cancelable: true }); event.key = key;
    h.document.dispatchEvent(event);
    assert.equal(h.document.body.children.length, 0);
    assert.equal(h.actions.length, ['Enter', ' '].includes(key) ? 1 : 0);
  }
});

test('scroll, resize, outside click and replacement dismiss only the active menu', () => {
  for (const type of ['scroll', 'resize', 'click']) {
    const h = harness(); h.card.showItemContextMenu(20, 30, 'one', 'One'); h.reveal();
    (type === 'click' ? h.document : h.window).dispatchEvent(new Event(type));
    assert.equal(h.document.body.children.length, 0);
  }
  const h = harness(); h.card.showItemContextMenu(20, 30, 'one', 'One'); h.reveal();
  h.card.showItemContextMenu(20, 30, 'two', 'Two'); h.reveal();
  assert.equal(h.document.body.children.length, 1);
  h.document.body.children[0].children[0].click();
  assert.equal(h.actions[0].ids[0], 'two');
});
