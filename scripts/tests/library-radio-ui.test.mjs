import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import vm from 'node:vm';
import ts from '../../hifimule-ui/node_modules/typescript/lib/typescript.js';

class Element {
  children = []; attributes = {}; listeners = {}; disabled = false;
  appendChild(child) { this.children.push(child); }
  setAttribute(key, value) { this.attributes[key] = value; }
  addEventListener(key, handler) { this.listeners[key] = handler; }
}
function load(overrides = {}) {
  const calls = [], toasts = [], exports = {};
  const config = { schemaVersion: 1, sources: [{ serverId: 'old', kind: 'genre', ref: 'genre' }], ordering: ['quality','random'], seed: 83, maxTracks: 42 };
  const rpc = {
    playbackGetSelectionConfig: async () => { calls.push('read'); return config; },
    playbackSaveSelectionConfig: async value => { calls.push(['save', value]); },
    playbackStartSelection: async () => { calls.push('start'); }, ...overrides,
  };
  const source = ts.transpileModule(readFileSync(new URL('../../hifimule-ui/src/components/RadioPlayButton.ts', import.meta.url), 'utf8'),
    { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText;
  vm.runInNewContext(source, { exports, document: { createElement: () => new Element() }, require: name => ({
    '../rpc': rpc, '../i18n': { t: (key, args) => key + (args ? ` ${args.title}` : '') }, '../toast': { showToast: (...args) => toasts.push(args) },
  })[name] });
  return { create: exports.createRadioPlayButton, calls, toasts, config };
}
const artist = { type: 'MusicArtist', serverId: 'original-server', id: 'original-artist', basketId: 'favorite:artist', name: 'Artist' };
const click = button => button.listeners.click({ stopPropagation() {} });

test('artist, playlist and genre replace sources while preserving settings, save before start and capture identity', async () => {
  for (const item of [artist, { ...artist, type: 'Playlist', playablePlaylist: true, childCount: 3 }, { ...artist, type: 'MusicGenre', id: 'original-genre' }]) {
    const h = load(), copy = { ...item }, tooltip = h.create(copy), button = tooltip.children[0];
    copy.serverId = 'changed'; copy.id = 'changed';
    assert.equal(button.name, 'broadcast');
    assert.equal(tooltip.attributes.content, button.label);
    let stopped = 0;
    button.listeners.mousedown({ stopPropagation() { stopped++; } });
    await button.listeners.click({ stopPropagation() { stopped++; } });
    assert.equal(stopped, 2);
    assert.deepEqual(JSON.parse(JSON.stringify(h.calls)), ['read', ['save', { ...h.config, sources: [{ serverId: item.serverId, kind: item.type === 'Playlist' ? 'playlist' : item.type === 'MusicGenre' ? 'genre' : 'artist', ref: item.id }] }], 'start']);
    assert.equal(button.disabled, false);
  }
});
test('ineligible items have no radio action', () => {
  const h = load();
  for (const item of [{ ...artist, serverId: undefined }, { ...artist, basketType: 'BookAuthor' }, { ...artist, type: 'MusicAlbum' }, { ...artist, type: 'Audio' }, { ...artist, type: 'Playlist' }, { ...artist, type: 'Playlist', playablePlaylist: true, childCount: 0 }]) assert.equal(h.create(item), null);
  assert.deepEqual(h.calls, []);
});
test('read and save failures prevent startup, release busy state and allow retry', async () => {
  for (const failure of ['playbackGetSelectionConfig','playbackSaveSelectionConfig']) {
    let fails = true, starts = 0;
    const h = load({ [failure]: async () => { if (fails) throw Error('failed'); return h.config; }, playbackStartSelection: async () => { starts++; } });
    const button = h.create(artist).children[0];
    await click(button);
    assert.equal(starts, 0); assert.equal(button.disabled, false);
    assert.equal(h.toasts[0][0], failure === 'playbackGetSelectionConfig' ? 'playback.selection.load_failed' : 'playback.selection.save_failed');
    fails = false; await click(button); assert.equal(starts, 1);
  }
});
test('startup failures retain saved source and report translated guidance', async () => {
  for (const [code, key] of [['PLAYBACK_SELECTION_EMPTY','playback.selection.empty'], ['PLAYBACK_BUSY','playback.selection.busy'], ['OUTPUT_MISSING','playback.output.choose'], ['PLAYBACK_SELECTION_PREPARATION_FAILED','playback.selection.preparation_failed'], ['SERVER_UNAVAILABLE','playback.selection.unavailable']]) {
    const h = load({ playbackStartSelection: async () => { throw { data: { code } }; } });
    const button = h.create(artist).children[0]; await click(button);
    assert.equal(h.calls[1][0], 'save'); assert.equal(h.toasts[0][0], key); assert.equal(button.disabled, false);
  }
});
test('concurrent card and row activations cannot interleave replacement or startup', async () => {
  let resolveRead;
  const h = load({ playbackGetSelectionConfig: () => new Promise(resolve => { resolveRead = resolve; }) });
  const first = h.create(artist).children[0], second = h.create({ ...artist, id: 'second' }).children[0];
  const pending = click(first); await click(second); await click(first);
  assert.equal(h.calls.length, 0); resolveRead(h.config); await pending;
  assert.equal(h.calls.length, 2); assert.equal(h.calls[0][1].sources[0].ref, artist.id);
  const retry = click(second); resolveRead(h.config); await retry;
  assert.equal(h.calls[2][1].sources[0].ref, 'second');
});
test('all locales include item-specific labels and both layouts use the shared action', () => {
  const catalog = JSON.parse(readFileSync(new URL('../../hifimule-i18n/catalog.json', import.meta.url), 'utf8'));
  for (const locale of ['en','fr','es','de']) for (const kind of ['artist','playlist','genre']) assert.ok(catalog[locale][`playback.radio.start_${kind}`]?.includes('{title}'));
  for (const path of ['library.ts','components/MediaCard.ts']) assert.match(readFileSync(new URL(`../../hifimule-ui/src/${path}`, import.meta.url), 'utf8'), /createRadioPlayButton\(item/);
  const library = readFileSync(new URL('../../hifimule-ui/src/library.ts', import.meta.url), 'utf8');
  assert.match(library, /serverId: basketStore.getActiveServerId\(\) \?\? undefined/);
  assert.match(library, /playablePlaylist: !state.isBookLibrary/);
});
