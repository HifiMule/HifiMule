import * as html from '../src/html.ts';
import assert from 'node:assert/strict';
import test from 'node:test';
import vm from 'node:vm';
import { readFileSync } from 'node:fs';
import ts from 'typescript';

const source = readFileSync(new URL('../src/components/PlaylistCurationView.ts', import.meta.url), 'utf8');
const compiled = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText;
const catalog = JSON.parse(readFileSync(new URL('../../hifimule-i18n/catalog.json', import.meta.url), 'utf8'));
function harness(message = '', { tracks, rpcCall = async () => {} } = {}) {
    let language = 'en';
    const exports = {};
    const controls = new Map();
    const container = {
        innerHTML: '',
        replaceChildren(...children) { this.children = children; },
        querySelector(selector) { return controls.get(selector)?.[0] ?? null; },
        querySelectorAll(selector) { return controls.get(selector) ?? []; },
    };
    vm.runInNewContext(compiled, {
        exports,
        require(name) {
            if (name === '../html') return html;
            if (name === '../rpc') return {
                fetchBrowsePlaylist: async () => {
                    if (tracks) return { tracks };
                    throw new Error(message);
                },
                rpcCall,
            };
            if (name === '../i18n') return { t: (key, params = {}) => Object.entries(params).reduce((text, [key, value]) => text.replaceAll('{'+key+'}', value), catalog[language][key] ?? key) };
            return {};
        },
        document: { createElement: tag => ({ tag, style: {}, attributes: {}, setAttribute(key, value) { this.attributes[key] = value; } }) },
        requestAnimationFrame: callback => callback(),
        Error,
    });
    return { view: new exports.PlaylistCurationView(container, 'id', 'name', () => {}, true), container, controls, language: value => { language = value; } };
}

function row(dataset = {}, classes = '') {
    const names = new Set(classes.split(' '));
    const handlers = new Map();
    const button = { attributes: {}, setAttribute(key, value) { this.attributes[key] = value; }, closest() { return null; } };
    return {
        dataset, button,
        classList: { contains: name => names.has(name), toggle(name, enabled) { enabled ? names.add(name) : names.delete(name); } },
        querySelector: () => button,
        addEventListener(name, handler) { handlers.set(name, [...(handlers.get(name) ?? []), handler]); },
        async click() { for (const handler of handlers.get('click') ?? []) await handler({ target: button, stopPropagation() {} }); },
        scrollIntoView() {},
    };
}

const playlist = [
    { id: 'b1', title: 'B first', artistName: 'Beta', albumName: 'Greatest Hits', duration: 60, sizeBytes: 1024 },
    { id: 'a1', title: 'A first', artistName: 'Alpha', albumName: 'Greatest Hits', duration: 120, sizeBytes: 2048 },
    { id: 'other', title: 'Other', artistName: 'Alpha', albumName: 'Another Album' },
    { id: 'b2', title: 'B second', artistName: 'Beta', albumName: 'Greatest Hits' },
];
const ids = tracks => Array.from(tracks, track => track.id);
const pairs = albums => Array.from(albums, ({ artist, album }) => [artist, album]);
function panels(controls) {
    const album = { innerHTML: '' };
    const tracks = { innerHTML: '' };
    controls.set('#curation-album-panel', [album]);
    controls.set('#curation-track-panel', [tracks]);
    return { album, tracks };
}
test('playlist load errors remain literal text, including markup', async () => {
    const message = '<img src=x onerror="alert(1)"> & <strong>failure</strong>';
    const { view, container } = harness(message);
    await view.load();
    const alert = container.children[0];
    assert.equal(alert.tag, 'sl-alert');
    assert.equal(alert.textContent, message);
    assert.equal(alert.innerHTML, undefined);
    assert.equal(alert.attributes.variant, 'danger');
});
test('missing metadata stays distinct from real names and filtering survives language changes', () => {
    const { view, language } = harness();
    view.tracks = [{ id: 'missing' }, { id: 'named', artistName: 'Unknown Artist', albumName: 'Unknown Album' }];
    assert.equal(view.buildArtistIndex().size, 2);
    view.selectedArtist = ''; view.selectedAlbum = { artist: '', album: '' };
    for (const locale of ['en', 'fr', 'es', 'de']) {
        language(locale);
        assert.deepEqual(Array.from(view.getTrackIdsByArtist('')), ['missing']);
        assert.deepEqual(Array.from(view.getTrackIdsByAlbum('', '')), ['missing']);
        assert.equal(view.getTracksForPanel()[0].id, 'missing');
        assert.ok(view.buildArtistPanelHtml([''], '').includes(catalog[locale]['playlist.curation.unknown_artist']));
        assert.ok(view.buildAlbumPanelHtml([{ artist: '', album: '' }], '').includes(catalog[locale]['playlist.curation.unknown_album']));
    }
});

test('opening a playlist immediately shows sorted albums from every artist and retains playlist order', async () => {
    const { view, container } = harness('', { tracks: playlist });
    await view.load();
    assert.equal(view.selectedArtist, null);
    assert.equal(view.selectedAlbum, null);
    assert.deepEqual(pairs(view.getAlbumsForPanel(view.buildArtistIndex())), [
        ['Alpha', 'Another Album'], ['Alpha', 'Greatest Hits'], ['Beta', 'Greatest Hits'],
    ]);
    assert.match(container.innerHTML, /curation-all-albums curation-album-focused[\s\S]*?aria-pressed="true"/);
    assert.match(container.innerHTML, /Greatest Hits · Alpha/);
    assert.match(container.innerHTML, /Greatest Hits · Beta/);
    for (const artist of ['Alpha', 'Beta']) {
        const removeLabel = catalog.en['playlist.curation.remove_album_named'].replace('{name}', `Greatest Hits · ${artist}`);
        assert.ok(container.innerHTML.includes(`label="${html.escapeAttr(removeLabel)}"`));
    }
    assert.ok(!container.innerHTML.includes(catalog.en['playlist.curation.select_artist']));
    assert.deepEqual(ids(view.getTracksForPanel()), ['b1', 'a1', 'other', 'b2']);
    assert.match(container.innerHTML, /3m 0s/);
    assert.match(container.innerHTML, /3.0 KB/);
});

test('full and partial rendering use the same global and artist-scoped album rows', () => {
    const { view, container, controls } = harness();
    view.tracks = playlist;
    for (const artist of [null, 'Alpha']) {
        view.selectedArtist = artist;
        view.render();
        const expected = view.buildAlbumPanelHtml(view.getAlbumsForPanel(view.buildArtistIndex()), artist);
        assert.ok(container.innerHTML.includes(expected));
        const { album } = panels(controls);
        view.updateAlbumsAndTracks();
        assert.equal(album.innerHTML, expected);
        if (artist === 'Alpha') assert.ok(!album.innerHTML.includes('data-artist="Beta"'));
    }
});

test('album row activation filters by artist/album pair and updates pressed states with absolute track positions', async () => {
    const { view, controls } = harness();
    view.tracks = playlist;
    const { tracks } = panels(controls);
    const all = row({}, 'curation-all-albums');
    const alpha = row({ artist: 'Alpha', album: 'Greatest Hits' });
    const beta = row({ artist: 'Beta', album: 'Greatest Hits' });
    controls.set('.curation-all-albums', [all]);
    controls.set('.curation-album-row', [all, alpha, beta]);
    view.bindAlbumPanelEvents();
    await alpha.click();
    assert.equal(view.selectedArtist, null);
    assert.deepEqual(ids(view.getTracksForPanel()), ['a1']);
    assert.equal(alpha.button.attributes['aria-pressed'], 'true');
    assert.equal(beta.button.attributes['aria-pressed'], 'false');
    assert.equal(all.button.attributes['aria-pressed'], 'false');
    assert.match(tracks.innerHTML, /#2<\/span>/);
    await beta.click();
    assert.deepEqual(ids(view.getTracksForPanel()), ['b1', 'b2']);
    assert.equal(alpha.button.attributes['aria-pressed'], 'false');
    assert.equal(beta.button.attributes['aria-pressed'], 'true');
    await beta.click();
    assert.equal(view.selectedAlbum, null);
    assert.equal(all.button.attributes['aria-pressed'], 'true');
    assert.deepEqual(ids(view.getTracksForPanel()), ids(playlist));
});

test('artist changes clear album selection; All albums retains artist scope and All artists resets both', async () => {
    const { view, controls } = harness();
    view.tracks = playlist;
    const { album } = panels(controls);
    const allArtists = row({}, 'curation-all-artists');
    const alpha = row({ artist: 'Alpha' });
    const beta = row({ artist: 'Beta' });
    controls.set('.curation-all-artists', [allArtists]);
    controls.set('.curation-artist-row', [allArtists, alpha, beta]);
    view.bindArtistPanelEvents();
    view.selectedAlbum = { artist: 'Beta', album: 'Greatest Hits' };
    await alpha.click();
    assert.equal(view.selectedArtist, 'Alpha');
    assert.equal(view.selectedAlbum, null);
    assert.equal(alpha.button.attributes['aria-pressed'], 'true');
    assert.deepEqual(ids(view.getTracksForPanel()), ['a1', 'other']);
    assert.ok(!album.innerHTML.includes('data-artist="Beta"'));
    const allAlbums = row({}, 'curation-all-albums');
    controls.set('.curation-all-albums', [allAlbums]);
    controls.set('.curation-album-row', [allAlbums]);
    view.bindAlbumPanelEvents();
    view.selectedAlbum = { artist: 'Alpha', album: 'Greatest Hits' };
    await allAlbums.click();
    assert.equal(view.selectedArtist, 'Alpha');
    assert.deepEqual(ids(view.getTracksForPanel()), ['a1', 'other']);
    view.selectedAlbum = { artist: 'Alpha', album: 'Greatest Hits' };
    await beta.click();
    assert.equal(view.selectedAlbum, null);
    assert.deepEqual(ids(view.getTracksForPanel()), ['b1', 'b2']);
    view.selectedAlbum = { artist: 'Beta', album: 'Greatest Hits' };
    await allArtists.click();
    assert.equal(view.selectedArtist, null);
    assert.equal(view.selectedAlbum, null);
    assert.deepEqual(ids(view.getTracksForPanel()), ids(playlist));
    assert.match(album.innerHTML, /data-artist="Alpha"/);
    assert.match(album.innerHTML, /data-artist="Beta"/);
});

test('global missing-metadata groups keep raw identity and escape both labels and attributes', () => {
    const { view } = harness();
    view.tracks = [
        { id: 'missing' },
        { id: 'named', artistName: 'Unknown Artist', albumName: 'Unknown Album' },
        { id: 'markup', artistName: '<img src=x onerror="bad">', albumName: '<script>bad</script>' },
    ];
    const albums = view.getAlbumsForPanel(view.buildArtistIndex());
    assert.equal(albums.length, 3);
    view.selectedAlbum = { artist: '', album: '' };
    assert.deepEqual(ids(view.getTracksForPanel()), ['missing']);
    view.selectedAlbum = { artist: 'Unknown Artist', album: 'Unknown Album' };
    assert.deepEqual(ids(view.getTracksForPanel()), ['named']);
    const markup = view.buildAlbumPanelHtml(albums, null);
    assert.ok(markup.includes('data-artist=""'));
    assert.ok(markup.includes('data-album=""'));
    assert.ok(markup.includes('&lt;img'));
    assert.ok(markup.includes('&lt;script&gt;'));
    assert.ok(!markup.includes('<img'));
    assert.ok(!markup.includes('<script>'));
});

test('global album removal events target only one pair and retain a surviving same-title selection', async () => {
    const calls = [];
    const { view, controls } = harness('', { rpcCall: async (method, params) => calls.push([method, params]) });
    view.tracks = playlist;
    view.selectedAlbum = { artist: 'Beta', album: 'Greatest Hits' };
    const remove = row({ artist: 'Alpha', album: 'Greatest Hits' });
    controls.set('.curation-remove-album', [remove]);
    view.bindAlbumPanelEvents();
    await remove.click();
    assert.equal(calls[0][0], 'playlist.removeTracks');
    assert.deepEqual(Array.from(calls[0][1].trackIds), ['a1']);
    assert.deepEqual(ids(view.tracks), ['b1', 'other', 'b2']);
    assert.equal(view.selectedAlbum.artist, 'Beta');
    assert.deepEqual(ids(view.getTracksForPanel()), ['b1', 'b2']);
    assert.equal(view.getAlbumsForPanel(view.buildArtistIndex()).length, 2);
});

test('failed album removal restores tracks and the selected pair; mutation guards preserve selection', async () => {
    const { view, container, controls } = harness('', { rpcCall: async () => { throw new Error('offline'); } });
    view.tracks = playlist;
    view.selectedAlbum = { artist: 'Beta', album: 'Greatest Hits' };
    const error = { style: {} };
    controls.set('#curation-error', [error]);
    view.isReordering = true;
    await view.removeAlbum('Beta', 'Greatest Hits');
    assert.deepEqual(ids(view.tracks), ids(playlist));
    assert.equal(view.selectedAlbum.artist, 'Beta');
    view.isReordering = false;
    await view.removeAlbum('Beta', 'Greatest Hits');
    assert.deepEqual(ids(view.tracks), ids(playlist));
    assert.equal(view.selectedAlbum.artist, 'Beta');
    assert.match(container.innerHTML, /Greatest Hits · Beta/);
    assert.match(error.textContent, /offline/);
});

test('playlist mutations refresh albums, clear vanished selections and keep All albums on an empty playlist', async () => {
    const { view, container } = harness();
    view.tracks = playlist.slice();
    view.selectedAlbum = { artist: 'Beta', album: 'Greatest Hits' };
    view.tracks.push({ id: 'new', title: 'New track', artistName: 'Gamma', albumName: 'New Album' });
    view.render();
    assert.equal(view.selectedAlbum.artist, 'Beta');
    assert.match(container.innerHTML, /New Album · Gamma/);
    await view.removeAlbum('Beta', 'Greatest Hits');
    assert.equal(view.selectedAlbum, null);
    assert.ok(!container.innerHTML.includes('data-artist="Beta"'));
    view.selectedArtist = 'Gamma';
    view.selectedAlbum = { artist: 'Gamma', album: 'New Album' };
    await view.removeArtist('Gamma');
    assert.equal(view.selectedArtist, null);
    assert.equal(view.selectedAlbum, null);
    await view.doRemove(ids(view.tracks));
    assert.match(container.innerHTML, /curation-all-albums curation-album-focused/);
    assert.ok(container.innerHTML.includes(catalog.en['playlist.curation.no_artists']));
    assert.ok(container.innerHTML.includes(catalog.en['playlist.curation.no_tracks']));
    assert.equal(view.getAlbumsForPanel(view.buildArtistIndex()).length, 0);
});

test('reordering a global album filter swaps its absolute playlist positions and sends the complete order', async () => {
    const calls = [];
    const { view, controls } = harness('', { rpcCall: async (method, params) => calls.push([method, params]) });
    view.tracks = playlist;
    view.selectedAlbum = { artist: 'Beta', album: 'Greatest Hits' };
    const { tracks } = panels(controls);
    await view.moveTrack(1, -1);
    assert.deepEqual(ids(view.tracks), ['b2', 'a1', 'other', 'b1']);
    assert.deepEqual(ids(view.getTracksForPanel()), ['b2', 'b1']);
    assert.equal(calls[0][0], 'playlist.reorder');
    assert.deepEqual(Array.from(calls[0][1].trackIds), ['b2', 'a1', 'other', 'b1']);
    assert.match(tracks.innerHTML, /#1<\/span>[\s\S]*B second[\s\S]*#4<\/span>[\s\S]*B first/);
});
