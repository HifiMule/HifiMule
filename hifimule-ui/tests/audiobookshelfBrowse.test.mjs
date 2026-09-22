import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

const library = readFileSync(new URL('../src/library.ts', import.meta.url), 'utf8');
const card = readFileSync(new URL('../src/components/MediaCard.ts', import.meta.url), 'utf8');
const rpc = readFileSync(new URL('../src/rpc.ts', import.meta.url), 'utf8');

test('Audiobookshelf albums retain ordered public credit presentation', () => {
    assert.match(library, /presentationCredits/);
    assert.match(library, /role === 'author'/);
    assert.match(library, /role === 'narrator'/);
});

test('Audiobookshelf books and parts cannot acquire music actions', () => {
    assert.match(card, /'BookPart'/);
    assert.match(card, /\['Book', 'BookPart'\]/);
    assert.match(card, /type === 'MusicAlbum'/);
});

test('browse search response remains tracks-compatible while admitting books', () => {
    assert.match(rpc, /tracks: BrowseTrack\[\]; albums\?: BrowseAlbum\[\]/);
    assert.match(rpc, /possiblyTruncated/);
});
