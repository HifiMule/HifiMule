import assert from 'node:assert/strict';
import test from 'node:test';
import vm from 'node:vm';
import { readFileSync } from 'node:fs';
import ts from 'typescript';

for (const [file, factory, args, icon] of [
    ['TrackPlayButton', 'createTrackPlayButton', ['server', 'track', '夜の音楽 🎵'], 'play-fill'],
    ['TrackPlayButton', 'createTrackPlayButton', ['server', 'part', '夜の音楽 🎵', 'part'], 'play-fill'],
    ['AlbumPlayButton', 'createAlbumPlayButton', ['album', 'server', '夜の音楽 🎵'], 'play-fill'],
    ['PlaylistPlayButton', 'createPlaylistPlayButton', ['playlist', 'server', '夜の音楽 🎵'], 'play-fill'],
    ['TrackQueueButton', 'createTrackQueueButton', ['server', 'track', '夜の音楽 🎵'], 'list-ul'],
    ['TrackPreviewButton', 'createTrackPreviewButton', ['server', 'track', '夜の音楽 🎵'], 'soundwave'],
]) {
    test(`${file}: rapid activation is serialized and failure permits retry`, async () => {
        const requests = [], errors = [], exports = {};
        const document = { createElement: () => ({
            listeners: {}, attributes: {}, dataset: {},
            addEventListener(type, fn) { this.listeners[type] = fn; },
            setAttribute(key, value) { this.attributes[key] = value; },
            removeAttribute(key) { delete this.attributes[key]; },
        }) };
        const rpc = new Proxy({}, { get: () => (...source) => new Promise((resolve, reject) => requests.push({source, resolve, reject})) });
        const source = ts.transpileModule(readFileSync(new URL(`../src/components/${file}.ts`, import.meta.url), 'utf8'), {
            compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
        }).outputText;
        vm.runInNewContext(source, { exports, document, require: name => (name === '../rpc' || name === '../state/queue') ? rpc
            : name === '../i18n' ? { t: (_, values) => values.title } : { showToast: error => errors.push(error) } });
        const button = exports[factory](...args);
        const click = () => button.listeners.click({stopPropagation() {}});
        const first = click();
        await Promise.all(Array.from({length: 10}, click));
        assert.equal(requests.length, 1);
        assert.equal(button.disabled, true);
        assert.equal(button.attributes['aria-busy'], 'true');
        assert.equal(button.name, 'hourglass-split');
        requests[0].reject(new Error('Offline'));
        if (file === 'TrackQueueButton') await assert.rejects(first, /Offline/);
        else await first;
        assert.deepEqual(errors, file === 'TrackQueueButton' ? [] : ['Offline']);
        assert.equal(button.disabled, false);
        assert.equal(button.attributes['aria-busy'], undefined);
        assert.equal(button.name, icon);
        const retry = click();
        assert.equal(requests.length, 2);
        requests[1].resolve(); await retry;
        assert.equal(button.disabled, false);
        assert.equal(button.name, icon);
        assert.equal(button.label, '夜の音楽 🎵');
    });
}
