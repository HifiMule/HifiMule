import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import vm from 'node:vm';
import ts from '../../hifimule-ui/node_modules/typescript/lib/typescript.js';
const allModes = ['artists', 'albums', 'playlists', 'tracks', 'genres', 'recentlyAdded', 'frequentlyPlayed', 'recentlyPlayed', 'favorites'];
const catalog = JSON.parse(readFileSync(new URL('../../hifimule-i18n/catalog.json', import.meta.url)));
function harness(locale = 'en') {
  const document = { activeElement: null };
  class Element {
    children = []; attributes = {}; listeners = {}; className = ''; textContent = ''; hidden = false;
    updateComplete = Promise.resolve(); scrolls = 0; dataset = {};
    constructor(tag) { this.tagName = tag; if (tag === 'sl-button') { this.button = new Element('button'); this.shadowRoot = { querySelector: () => this.button }; } }
    get isConnected() { return this === root || !!this.parentElement?.isConnected; }
    get firstElementChild() { return this.children[0] ?? null; }
    get nextElementSibling() { const a = this.parentElement?.children ?? []; return a[a.indexOf(this)+1] ?? null; }
    set innerHTML(value) { this.replaceChildren(); }
    get innerHTML() { return ''; }
    setAttribute(k,v) { this.attributes[k] = String(v); }
    getAttribute(k) { return this.attributes[k] ?? null; }
    append(...nodes) { for (const n of nodes) this.appendChild(n); }
    appendChild(n) { this.insertBefore(n, null); return n; }
    insertBefore(n, ref) { n.remove(); n.parentElement = this; const i = ref ? this.children.indexOf(ref) : this.children.length; this.children.splice(i,0,n); }
    replaceChildren(...nodes) { for (const n of [...this.children]) n.remove(); this.append(...nodes); }
    remove() { if (this === document.activeElement) document.activeElement = null; if (this.parentElement) this.parentElement.children = this.parentElement.children.filter(n => n !== this); this.parentElement = null; }
    contains(n) { return this === n || this.children.some(c => c.contains(n)); }
    addEventListener(k,f) { (this.listeners[k] ??= []).push(f); }
    async click() { for (const f of this.listeners.click ?? []) await f(); }
    focus() { document.activeElement = this; }
    scrollIntoView() { this.scrolls++; }
    querySelectorAll(s) { const match = n => s.startsWith('.') ? n.className.split(' ').includes(s.slice(1)) : s === 'span' ? n.tagName === 'span' : s.includes('[data-mode') ? n.tagName === 'sl-button' && n.getAttribute('data-mode') !== null && (!s.includes('=') || n.getAttribute('data-mode') === s.split('"')[1]) : s.includes('[data-view') ? n.getAttribute('data-view') !== null : s === 'sl-button' ? n.tagName === s : false; return this.children.flatMap(n => [...(match(n) ? [n] : []), ...n.querySelectorAll(s)]); }
    querySelector(s) { return this.querySelectorAll(s)[0] ?? null; }
  }
  const root = new Element('div');
  document.createElement = tag => new Element(tag);
  document.getElementById = id => id === 'browse-mode-bar' ? root : null;
  const exports = {};
  const input = readFileSync(new URL('../../hifimule-ui/src/library.ts', import.meta.url), 'utf8') + '\nexport const probe = { state, renderModeBar, switchMode, setViewMode };';
  const source = ts.transpileModule(input, {compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText;
  vm.runInNewContext(source, { exports, document, console, requestAnimationFrame: f => f(),
    require: name => name === './i18n' ? {t:key=>catalog[locale][key] ?? key} : {},
    setTimeout, clearTimeout, window: {} });
  const {probe} = exports;
  probe.state.availableModes = [...allModes];
  const render = async () => { probe.renderModeBar(); await Promise.resolve(); await Promise.resolve(); };
  const buttons = () => root.querySelectorAll('sl-button[data-mode]');
  return { ...probe, render, root, buttons, document, Element };
}
test('capability reconciliation preserves order, supported nodes, focus and single handlers', async () => {
  const h = harness(); await h.render(); const original = h.buttons(); original[1].focus();
  await h.render(); assert.equal(h.document.activeElement, original[1]);
  h.state.availableModes = ['albums','tracks']; await h.render();
  assert.deepEqual(h.buttons(), [original[1],original[3]]);
  assert.equal(h.document.activeElement, original[1]);
  h.state.availableModes = [...allModes].reverse(); await h.render();
  assert.deepEqual(h.buttons().map(b=>b.getAttribute('data-mode')), [...allModes].reverse());
  assert.equal(h.buttons().find(b=>b.getAttribute('data-mode')==='albums'),original[1]);
  assert.equal(h.document.activeElement,original[1]);
  for(const b of h.buttons()) assert.equal(b.listeners.click.length,1);
});
test('localized visible names, decorative icons and actual inner pressed states stay aligned', async () => {
  for (const locale of ['en','fr','es','de']) {
    const h=harness(locale); await h.render();
    for(const b of h.buttons()) {
      const mode=b.getAttribute('data-mode');
      assert.equal(b.querySelector('span')?.textContent,catalog[locale]['library.mode.'+mode]);
      const icon=b.children.find(n=>n.tagName==='sl-icon'); assert.ok(icon); assert.equal(icon.getAttribute('aria-hidden'),'true');
      assert.equal(b.button.getAttribute('aria-pressed'),String(mode==='artists'));
    }
    h.state.browseMode='favorites'; h.state.loading=true; await h.render();
    for(const b of h.buttons()){assert.equal(b.disabled,true);assert.equal(b.button.getAttribute('aria-pressed'),String(b.getAttribute('data-mode')==='favorites'));}
  }
});
test('removed focus falls back without taking focus from outside the bar', async () => {
  const h=harness();await h.render();h.buttons()[0].focus();h.state.availableModes=['albums'];h.state.browseMode='albums';await h.render();
  assert.equal(h.document.activeElement,h.buttons()[0]);
  const outside=new h.Element('input');outside.focus();h.state.availableModes=['tracks'];h.state.browseMode='tracks';await h.render();assert.equal(h.document.activeElement,outside);
  h.state.availableModes=[];await h.render();assert.equal(h.buttons().length,0);assert.equal(h.root.querySelector('.view-toggle-group').hidden,true);
});
test('view toggle retains identity and focus, hides for loading and Tracks, and restores preference', async () => {
  const h=harness();await h.render();const group=h.root.querySelector('.view-toggle-group');const list=group.children[1];list.focus();await h.render();
  assert.equal(h.root.querySelector('.view-toggle-group'),group);assert.equal(h.document.activeElement,list);
  await list.click();await h.render();assert.equal(h.state.listViewMode,'list');assert.equal(h.document.activeElement,list);
  h.state.loading=true;await h.render();assert.equal(group.hidden,true);
  h.state.loading=false;h.state.browseMode='tracks';await h.render();assert.equal(group.hidden,true);
  h.state.browseMode='albums';await h.render();assert.equal(group.hidden,false);assert.equal(group.children[1],list);assert.equal(list.button.getAttribute('aria-pressed'),'true');
});
test('mode activation uses the existing reset path, and same-mode/loading clicks are no-ops', async () => {
  const h=harness();await h.render();h.state.breadcrumbStack=[{id:'a',name:'A'}];h.state.selectedIds.add('selection');
  await h.buttons()[0].click();assert.equal(h.state.breadcrumbStack.length,1);assert.equal(h.state.selectedIds.size,1);
  h.state.loading=true;await h.buttons()[1].click();assert.equal(h.state.browseMode,'artists');
  h.state.loading=false;await h.buttons()[1].click();assert.equal(h.state.browseMode,'albums');assert.equal(h.state.breadcrumbStack.length,0);assert.equal(h.state.selectedIds.size,0);
});

test('loading retains the focused mode with disabled semantics and rejects activation', async () => {
  const h=harness();await h.render();const button=h.buttons()[1];button.focus();h.state.loading=true;await h.render();
  assert.equal(h.document.activeElement,button);assert.equal(button.disabled,false);
  assert.equal(button.button.getAttribute('aria-disabled'),'true');await button.click();assert.equal(h.state.browseMode,'artists');
  h.state.loading=false;await h.render();assert.equal(button.button.getAttribute('aria-disabled'),'false');
});

test('detached controls cannot issue unsupported navigation after source changes', async () => {
  const h=harness();await h.render();const removed=h.buttons()[1];h.state.availableModes=['artists'];await h.render();
  await removed.click();assert.equal(h.state.browseMode,'artists');
});
