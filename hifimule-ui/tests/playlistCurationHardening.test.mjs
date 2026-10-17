import assert from 'node:assert/strict';
import test from 'node:test';
import vm from 'node:vm';
import { readFileSync } from 'node:fs';
import ts from 'typescript';

const source = readFileSync(new URL('../src/components/PlaylistCurationView.ts', import.meta.url), 'utf8');
const compiled = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText;
const catalog = JSON.parse(readFileSync(new URL('../../hifimule-i18n/catalog.json', import.meta.url), 'utf8'));
function harness(message = '') {
    let language = 'en';
    const exports = {};
    const container = { innerHTML: '', replaceChildren(...children) { this.children = children; } };
    vm.runInNewContext(compiled, {
        exports,
        require(name) {
            if (name === '../rpc') return { fetchBrowsePlaylist: async () => { throw new Error(message); } };
            if (name === '../i18n') return { t: (key, params = {}) => Object.entries(params).reduce((text, [key, value]) => text.replaceAll('{'+key+'}', value), catalog[language][key] ?? key) };
            return {};
        },
        document: { createElement: tag => ({ tag, style: {}, attributes: {}, setAttribute(key, value) { this.attributes[key] = value; } }) },
        Error,
    });
    return { view: new exports.PlaylistCurationView(container, 'id', 'name', () => {}, true), container, language: value => { language = value; } };
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
    view.selectedArtist = ''; view.selectedAlbum = '';
    for (const locale of ['en', 'fr', 'es', 'de']) {
        language(locale);
        assert.deepEqual(Array.from(view.getTrackIdsByArtist('')), ['missing']);
        assert.deepEqual(Array.from(view.getTrackIdsByAlbum('', '')), ['missing']);
        assert.equal(view.getTracksForPanel()[0].id, 'missing');
        assert.ok(view.buildArtistPanelHtml([''], '').includes(catalog[locale]['playlist.curation.unknown_artist']));
        assert.ok(view.buildAlbumPanelHtml([''], '').includes(catalog[locale]['playlist.curation.unknown_album']));
    }
});
