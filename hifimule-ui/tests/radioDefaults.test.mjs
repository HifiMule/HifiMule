import assert from 'node:assert/strict';
import test from 'node:test';
import { build } from 'esbuild';

const bundle = await build({
    entryPoints: [new URL('../src/components/PlaybackSelectionSettings.ts', import.meta.url).pathname],
    bundle: true, platform: 'node', format: 'esm', write: false,
    plugins: [{ name: 'radio-settings-dependencies', setup(build) {
        build.onResolve({ filter: /^\.\.\/(rpc|i18n|serverIdentity)$/ }, args => ({ path: args.path, namespace: 'test' }));
        build.onLoad({ filter: /.*/, namespace: 'test' }, ({ path }) => ({ loader: 'js', contents:
            path.endsWith('i18n') ? 'export const t = key => key;'
            : path.endsWith('serverIdentity') ? 'export const formatServerIdentity = () => ({label: "Music"});'
            : `export const playbackStartSelection = () => globalThis.startRadio();
               export const playbackCancelSelectionStart = async () => {};
               export const playbackGetSelectionConfig = async () => {};
               export const playbackSaveSelectionConfig = async config => globalThis.saveRadio(config);
               export const playbackSelectionOptions = async () => {};
               export const serverList = async () => [];`
        }));
    }}],
});
const { PlaybackSelectionSettings } = await import(`data:text/javascript;base64,${Buffer.from(bundle.outputFiles[0].contents).toString('base64')}`);
function settings(sources = []) {
    return Object.assign(Object.create(PlaybackSelectionSettings.prototype), {
        config: { schemaVersion: 1, sources, ordering: ['random'], seed: 0, maxTracks: 16 },
        servers: [], busy: false, cancelling: false, saving: false, dirty: false,
        disposed: false, request: 0, editRevision: 0,
        add: {}, save: {}, start: {}, cancel: {}, status: {}, seed: {value: '0'}, ordering: {value: 'random'},
    });
}

test('saved empty sources can start Radio; busy, saving, cancellation and dirty guards apply', () => {
    const view = settings();
    view.updateActions();
    assert.equal(view.start.disabled, false);
    for (const guard of ['busy', 'saving', 'cancelling', 'dirty']) {
        view[guard] = true; view.updateActions(); assert.equal(view.start.disabled, true);
        view[guard] = false;
    }
});

test('automatic Radio start leaves empty configuration intact and reports missing music server', async () => {
    const view = settings();
    globalThis.startRadio = async () => {};
    await view.startSelection();
    assert.equal(view.status.textContent, 'playback.selection.started');
    assert.deepEqual(view.config.sources, []);
    globalThis.startRadio = async () => { throw new Error('PLAYBACK_SELECTION_NO_MUSIC_SERVER'); };
    await view.startSelection();
    assert.equal(view.status.textContent, 'playback.selection.no_music_server');
    assert.equal(view.start.disabled, false);
});

test('clearing sources must be saved before automatic start, and invalid explicit refs stay guarded', async () => {
    const view = settings(); view.dirty = true; view.updateActions();
    assert.equal(view.start.disabled, true);
    let saved; globalThis.saveRadio = async config => { saved = config; };
    await view.saveConfig();
    assert.deepEqual(saved.sources, []);
    assert.equal(view.start.disabled, false);
    view.config.sources = [{ serverId: 'portable', kind: 'playlist', ref: '' }];
    view.dirty = true; view.updateActions();
    assert.equal(view.save.disabled, true);
    await view.saveConfig();
    assert.equal(view.status.textContent, 'playback.selection.invalid');
});
