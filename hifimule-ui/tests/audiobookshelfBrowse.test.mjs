import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

const library = readFileSync(new URL('../src/library.ts', import.meta.url), 'utf8');
const card = readFileSync(new URL('../src/components/MediaCard.ts', import.meta.url), 'utf8');
const rpc = readFileSync(new URL('../src/rpc.ts', import.meta.url), 'utf8');
const albumPlay = readFileSync(new URL('../src/components/AlbumPlayButton.ts', import.meta.url), 'utf8');
const playbackControls = readFileSync(new URL('../src/components/PlaybackControls.ts', import.meta.url), 'utf8');
const basketSidebar = readFileSync(new URL('../src/components/BasketSidebar.ts', import.meta.url), 'utf8');
const catalog = JSON.parse(readFileSync(new URL('../../hifimule-i18n/catalog.json', import.meta.url), 'utf8'));

test('Audiobookshelf albums retain ordered public credit presentation', () => {
    assert.match(library, /presentationCredits/);
    assert.match(library, /role === 'author'/);
    assert.match(library, /role === 'narrator'/);
});

test('Audiobookshelf books and parts use the existing playback routes without music actions', () => {
    assert.match(card, /const showSelection = isBrowseItem \|\| mode === 'items'/);
    assert.match(card, /basketStore\.add\(bookBasketItem/);
    assert.match(card, /\['MusicAlbum', 'Book'\]/);
    assert.match(card, /\['Audio', 'BookPart'\]/);
    assert.match(card, /if \(!isPart\)/);
    assert.match(library, /item\.type === 'Audio' \|\| item\.type === 'BookPart'/);
    assert.match(library, /item\.type === 'MusicAlbum' \|\| item\.type === 'Book'/);
    assert.match(library, /resolved === 'Book' \|\| resolved === 'BookPart'/);
    assert.match(albumPlay, /playbackPlayAlbum\(source\.serverId, source\.albumId\)/);
    assert.match(card, /playbackPlayTrack\(playbackSource\.serverId, playbackSource\.trackId\)/);
    for (const locale of Object.values(catalog)) {
        assert.match(locale['library.books.play_book'], /\{title\}/);
        assert.match(locale['library.books.play_part'], /\{title\}/);
    }
});

test('browse search response remains tracks-compatible while admitting books', () => {
    assert.match(rpc, /tracks: BrowseTrack\[\]; albums\?: BrowseAlbum\[\]/);
    assert.match(rpc, /possiblyTruncated/);
});

test('book progress recovery guidance stays scoped and localized', () => {
    assert.match(rpc, /continuityStatus\?: 'refresh' \| 'relink' \| null/);
    assert.match(playbackControls, /snapshot\.mode === 'main' && snapshot\.current && snapshot\.continuityStatus/);
    assert.match(playbackControls, /playback\.book_progress\.\$\{snapshot\.continuityStatus\}/);
    for (const locale of Object.values(catalog)) {
        assert.ok(locale['playback.book_progress.refresh']);
        assert.ok(locale['playback.book_progress.relink']);
    }
});

test('source groupings use read-only browse presentation and compatibility stays unknown until planning', () => {
    assert.match(library, /function mapPlaylists\(/);
    assert.match(library, /serverId: p\.serverId/);
    assert.match(rpc, /export interface BrowsePlaylist \{\s+id: string;\s+serverId\?: string;/);
    assert.match(library, /abs-series-/);
    assert.match(library, /series_read_only/);
    assert.match(library, /collection_read_only/);
    assert.match(library, /compatibility_unknown/);
    assert.match(library, /state\.browseMode === 'playlists' && _supportsPlaylistWrite/);
    assert.match(basketSidebar, /item\.mediaRole === 'audiobook'/);
    assert.match(basketSidebar, /verified-direct-format/);
    assert.match(basketSidebar, /incompatible-direct-format/);
    for (const locale of Object.values(catalog)) {
        for (const key of [
            'library.books.series_read_only', 'library.books.collection_read_only',
            'library.books.compatibility_unknown', 'library.books.compatibility_direct',
            'library.books.compatibility_transcoded', 'library.books.compatibility_blocked',
            'basket.sync.compatibility_title', 'basket.sync.reason.incompatible_direct',
        ]) assert.ok(locale[key], key);
    }
});
