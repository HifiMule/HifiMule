import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

const library = readFileSync(new URL('../src/library.ts', import.meta.url), 'utf8');
const card = readFileSync(new URL('../src/components/MediaCard.ts', import.meta.url), 'utf8');
const rpc = readFileSync(new URL('../src/rpc.ts', import.meta.url), 'utf8');
const albumPlay = readFileSync(new URL('../src/components/AlbumPlayButton.ts', import.meta.url), 'utf8');
const catalog = JSON.parse(readFileSync(new URL('../../hifimule-i18n/catalog.json', import.meta.url), 'utf8'));

test('Audiobookshelf albums retain ordered public credit presentation', () => {
    assert.match(library, /presentationCredits/);
    assert.match(library, /role === 'author'/);
    assert.match(library, /role === 'narrator'/);
});

test('Audiobookshelf books and parts use the existing playback routes without music actions', () => {
    assert.match(card, /\['Book', 'BookPart'\]/);
    assert.match(card, /\['MusicAlbum', 'Book'\]/);
    assert.match(card, /\['Audio', 'BookPart'\]/);
    assert.match(card, /if \(!isPart\)/);
    assert.match(library, /item\.type === 'Audio' \|\| item\.type === 'BookPart'/);
    assert.match(library, /item\.type === 'MusicAlbum' \|\| item\.type === 'Book'/);
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
