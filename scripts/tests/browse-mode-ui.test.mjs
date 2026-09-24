import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import vm from 'node:vm';
import ts from '../../hifimule-ui/node_modules/typescript/lib/typescript.js';
const allModes = ['artists', 'albums', 'playlists', 'tracks', 'genres', 'recentlyAdded', 'frequentlyPlayed', 'recentlyPlayed', 'favorites'];
const catalog = JSON.parse(readFileSync(new URL('../../hifimule-i18n/catalog.json', import.meta.url)));
function harness(locale = 'en') {
  const document = { activeElement: null };
  const audit = [];
  let content;
  let modesResult = [...allModes];
  let modesError = null;
  let podcastShowsError = null;
  let podcastEpisodesError = null;
  let podcastShowFetches = 0;
  const basketStore = {
    addEventListener() {},
    removeEventListener(type, handler) { audit.push(`remove:basket:${handler.name || 'handler'}`); },
  };
  class Element {
    children = []; attributes = {}; listeners = {}; className = ''; textContent = ''; hidden = false;
    updateComplete = Promise.resolve(); scrolls = 0; dataset = {};
    constructor(tag) { this.tagName = tag; if (tag === 'sl-button') { this.button = new Element('button'); this.shadowRoot = { querySelector: () => this.button }; } }
    get isConnected() { return this === root || !!this.parentElement?.isConnected; }
    get firstElementChild() { return this.children[0] ?? null; }
    get nextElementSibling() { const a = this.parentElement?.children ?? []; return a[a.indexOf(this)+1] ?? null; }
    set innerHTML(value) { if (this === content) audit.push('replace:innerHTML'); this.replaceChildren(); }
    get innerHTML() { return ''; }
    setAttribute(k,v) { this.attributes[k] = String(v); }
    getAttribute(k) { return this.attributes[k] ?? null; }
    append(...nodes) { for (const n of nodes) this.appendChild(n); }
    appendChild(n) { this.insertBefore(n, null); return n; }
    insertBefore(n, ref) { n.remove(); n.parentElement = this; const i = ref ? this.children.indexOf(ref) : this.children.length; this.children.splice(i,0,n); }
    replaceChildren(...nodes) { if (this === content) audit.push('replace:children'); for (const n of [...this.children]) n.remove(); this.append(...nodes); }
    remove() { if (this === document.activeElement) document.activeElement = null; if (this.parentElement) this.parentElement.children = this.parentElement.children.filter(n => n !== this); this.parentElement = null; }
    contains(n) { return this === n || this.children.some(c => c.contains(n)); }
    addEventListener(k,f) { (this.listeners[k] ??= []).push(f); }
    removeEventListener(k,f) { audit.push(`remove:${k}:${f.name || 'handler'}`); this.listeners[k] = (this.listeners[k] ?? []).filter(listener => listener !== f); }
    async click() { for (const f of this.listeners.click ?? []) await f(); }
    focus() { document.activeElement = this; }
    scrollIntoView() { this.scrolls++; }
    querySelectorAll(s) { const match = n => s.startsWith('.') ? n.className.split(' ').includes(s.slice(1)) : s === 'span' ? n.tagName === 'span' : s.includes('[data-mode') ? n.tagName === 'sl-button' && n.getAttribute('data-mode') !== null && (!s.includes('=') || n.getAttribute('data-mode') === s.split('"')[1]) : s.includes('[data-view') ? n.getAttribute('data-view') !== null : s === 'sl-button' ? n.tagName === s : false; return this.children.flatMap(n => [...(match(n) ? [n] : []), ...n.querySelectorAll(s)]); }
    querySelector(s) { return this.querySelectorAll(s)[0] ?? null; }
  }
  const root = new Element('div');
  content = new Element('div');
  document.createElement = tag => new Element(tag);
  document.getElementById = id => id === 'browse-mode-bar' ? root : id === 'library-content' ? content : null;
  const exports = {};
  const played = [];
  const input = readFileSync(new URL('../../hifimule-ui/src/library.ts', import.meta.url), 'utf8') + '\nexport const probe = { state, renderModeBar, switchMode, setViewMode, initLibraryView, mapAlbums, mapAlbumTracks, loadPodcastView, openPodcastShow, renderPodcastError };';
  const source = ts.transpileModule(input, {compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText;
  vm.runInNewContext(source, { exports, document, console, requestAnimationFrame: f => f(),
    require: name => name === './i18n' ? {t:(key,params={})=>(catalog[locale][key] ?? key).replace(/\{(\w+)\}/g,(_,name)=>String(params[name] ?? ''))}
      : name === './rpc' ? {fetchBrowseModes: async()=>{if(modesError)throw modesError;return modesResult;}, serverList: async()=>[],
        fetchPodcastShows: async()=>{if(podcastShowsError)throw podcastShowsError;return {shows:[{id:'show-opaque',title:'Talks',description:null,coverArtId:null,episodeCount:1}],total:51};},
        fetchPodcastShow: async()=>{podcastShowFetches++;if(podcastEpisodesError)throw podcastEpisodesError;return {show:{id:'show-opaque',title:'Talks',description:null,coverArtId:null,episodeCount:60},episodes:Array.from({length:60},(_,i)=>({id:i===0?'episode-opaque':`episode-${i}`,showId:'show-opaque',title:i===0?'First':`Episode ${i}`,description:null,durationSeconds:60,publishedAt:'2026-01-01',coverArtId:null})),total:60,possiblyTruncated:true};},
        playbackPlayEpisode: async(serverId,episodeId)=>played.push([serverId,episodeId])}
      : name === './state/basket' ? {basketStore} : {},
    setTimeout, clearTimeout, window: {} });
  const {probe} = exports;
  probe.state.availableModes = [...allModes];
  const render = async () => { probe.renderModeBar(); await Promise.resolve(); await Promise.resolve(); };
  const buttons = () => root.querySelectorAll('sl-button[data-mode]');
  return { ...probe, render, root, content, buttons, document, Element, audit, played,
    setModesResult(value) { modesResult = value; },
    setModesError(value) { modesError = value; },
    setPodcastShowsError(value) { podcastShowsError = value; },
    setPodcastEpisodesError(value) { podcastEpisodesError = value; },
    getPodcastShowFetches() { return podcastShowFetches; },
  };
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
test('book scope uses localized mode and preserves primary author and part order', async () => {
  for (const locale of ['en', 'fr', 'es', 'de']) {
    const h = harness(locale);
    h.state.isBookLibrary = true;
    h.state.availableModes = ['albums'];
    h.state.browseMode = 'albums';
    await h.render();
    assert.equal(h.buttons()[0].querySelector('span').textContent, catalog[locale]['library.books.mode']);
    const [book] = h.mapAlbums([{id:'opaque',name:'Book',artistName:'Primary author',presentationCredits:[{name:'Narrator',role:'narrator'}],trackCount:2}]);
    assert.equal(book.type, 'Book');
    assert.match(book.subtitle, /Primary author/);
    assert.match(book.subtitle, /Narrator/);
    const parts = h.mapAlbumTracks([{id:'one',title:'File 1',trackNumber:null,duration:1},{id:'two',title:'File 2',trackNumber:null,duration:1}]);
    assert.equal(parts[0].type, 'BookPart');
    assert.match(parts[0].subtitle, /1/);
    assert.match(parts[1].subtitle, /2/);
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
  h.state.loading=true;await h.render();assert.equal(group.hidden,true);assert.equal(h.document.activeElement,h.root);
  h.state.loading=false;h.state.browseMode='tracks';await h.render();assert.equal(group.hidden,true);
  h.state.browseMode='albums';await h.render();assert.equal(group.hidden,false);assert.equal(group.children[1],list);assert.equal(list.button.getAttribute('aria-pressed'),'true');
});

test('hiding Grid/List moves only its own focus to an enabled mode or the browse bar', async () => {
  const tracks=harness();await tracks.render();const tracksGroup=tracks.root.querySelector('.view-toggle-group');tracksGroup.children[0].focus();
  tracks.state.browseMode='tracks';await tracks.render();
  assert.equal(tracks.document.activeElement,tracks.buttons().find(button=>button.getAttribute('data-mode')==='tracks'));

  const loading=harness();await loading.render();loading.root.querySelector('.view-toggle-group').children[0].focus();loading.state.loading=true;await loading.render();
  assert.equal(loading.document.activeElement,loading.root);

  const empty=harness();await empty.render();empty.root.querySelector('.view-toggle-group').children[1].focus();empty.state.availableModes=[];await empty.render();
  assert.equal(empty.document.activeElement,empty.root);

  const external=harness();await external.render();const outside=new external.Element('input');outside.focus();external.state.browseMode='tracks';await external.render();
  assert.equal(external.document.activeElement,outside);
});

test('library initialization tears down list and basket listeners before replacing content', async () => {
  for (const failure of [null, new Error('capability failure')]) {
    const h=harness();
    const scroll=function scrollHandler(){};
    const listBasket=function listBasketHandler(){};
    const gridBasket=function gridBasketHandler(){};
    h.content.__listScrollHandler=scroll;
    h.content.__listBasketHandler=listBasket;
    h.content.__gridBasketHandler=gridBasket;
    h.content.addEventListener('scroll',scroll);
    h.setModesResult([]);
    h.setModesError(failure);
    await h.initLibraryView();
    const firstReplacement=h.audit.findIndex(event=>event.startsWith('replace:'));
    assert.ok(firstReplacement>=3, h.audit.join(','));
    assert.deepEqual(h.audit.slice(0,3),[
      'remove:scroll:scrollHandler',
      'remove:basket:listBasketHandler',
      'remove:basket:gridBasketHandler',
    ]);
    assert.equal(h.content.__listScrollHandler,undefined);
    assert.equal(h.content.__listBasketHandler,undefined);
    assert.equal(h.content.__gridBasketHandler,undefined);
  }
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

test('podcast view presents shows and episodes with a typed play action', async () => {
  const h = harness('en');
  h.state.browseMode = 'podcasts';
  h.state.podcastServerId = 'server-opaque';
  await h.loadPodcastView();
  const walk = node => [node, ...node.children.flatMap(walk)];
  let labels = walk(h.content).map(node => node.textContent).filter(Boolean);
  assert.ok(labels.includes('Talks'));
  assert.ok(!labels.includes('Book'));
  const show = walk(h.content).find(node => node.tagName === 'button' && node.textContent === 'Talks');
  assert.ok(show);
  await show.click();
  await new Promise(resolve => setTimeout(resolve, 0));
  labels = walk(h.content).map(node => node.textContent).filter(Boolean);
  assert.ok(labels.includes('First'));
  const play = walk(h.content).find(node => node.tagName === 'button' && node.textContent === catalog.en['library.podcast.play']);
  assert.ok(play);
  await play.click();
  await new Promise(resolve => setTimeout(resolve, 0));
  assert.deepEqual(h.played, [['server-opaque','episode-opaque']]);
});

test('podcast stale and permission states use localized accessible error text', () => {
  const h = harness('fr');
  const walk = node => [node, ...node.children.flatMap(walk)];
  h.renderPodcastError({code:-4,data:{errorCode:'STALE_CONFIGURATION'}});
  assert.ok(walk(h.content).some(node => node.textContent === catalog.fr['library.podcast.stale']));
  h.renderPodcastError({data:{errorCode:'PROVIDER_FORBIDDEN'}});
  assert.ok(walk(h.content).some(node => node.textContent === catalog.fr['library.podcast.permission']));
});

test('failed show Load more preserves rows and episode pages stay local', async () => {
  const h = harness('en');
  const walk = node => [node, ...node.children.flatMap(walk)];
  h.state.browseMode = 'podcasts';
  await h.loadPodcastView();
  h.setPodcastShowsError(new Error('offline'));
  await h.loadPodcastView(true);
  assert.ok(walk(h.content).some(node => node.textContent === 'Talks'));
  assert.ok(walk(h.content).some(node => node.textContent === catalog.en['library.podcast.unavailable']));
  h.setPodcastShowsError(null);
  await h.openPodcastShow('show-opaque');
  h.setPodcastEpisodesError(new Error('offline'));
  await h.openPodcastShow('show-opaque', true);
  assert.ok(walk(h.content).some(node => node.textContent === 'First'));
  assert.ok(walk(h.content).some(node => node.textContent === 'Episode 59'));
  assert.ok(walk(h.content).some(node => node.textContent === catalog.en['library.podcast.back']));
  assert.ok(walk(h.content).some(node => node.textContent === catalog.en['library.podcast.truncated']));
  assert.equal(h.getPodcastShowFetches(), 1);
});

test('podcast browse mode has localized label in every shipped language', async () => {
  for (const locale of ['en','fr','es','de']) {
    const h = harness(locale);
    h.state.availableModes = ['podcasts'];
    h.state.browseMode = 'podcasts';
    await h.render();
    assert.equal(h.buttons()[0].querySelector('span').textContent, catalog[locale]['library.mode.podcasts']);
  }
});
