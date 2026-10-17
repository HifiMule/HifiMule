import assert from 'node:assert/strict';
import test from 'node:test';
import vm from 'node:vm';
import { readFileSync } from 'node:fs';
import ts from 'typescript';

const exports = {};
vm.runInNewContext(ts.transpileModule(readFileSync(new URL('../src/state/libraryCache.ts', import.meta.url), 'utf8'), {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
}).outputText, { exports, require: () => assert.fail('shared cache must not load UI dependencies') });

test('playlist invalidation preserves other pages, including after a server cache reset', () => {
    const { libraryPageCache: cache, invalidatePlaylistsCache } = exports;
    const album = { items: [], total: 0 };
    cache.set('albums:root', album);
    cache.set('playlists:root', { items: [], total: 1 });
    cache.set('playlists:nested', { items: [], total: 2 });
    invalidatePlaylistsCache();
    assert.equal(cache.size, 1);
    assert.equal(cache.get('albums:root'), album);
    cache.clear();
    cache.set('playlists:new-server', { items: [], total: 3 });
    invalidatePlaylistsCache();
    assert.equal(cache.size, 0);
});

test('capability updates remain visible to consumers loaded before or after a change', () => {
    assert.equal(exports.supportsPlaylistWrite, false);
    exports.setPlaylistWriteCapability(true);
    assert.equal(exports.supportsPlaylistWrite, true);
    exports.setPlaylistWriteCapability(false);
    assert.equal(exports.supportsPlaylistWrite, false);
});
