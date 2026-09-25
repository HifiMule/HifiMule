import assert from 'node:assert/strict';
import test from 'node:test';
import { bookBasketItem, podcastEpisodeBasketItem, podcastShowBasketItem } from '../src/state/mediaSyncSelection.ts';

test('whole book selection preserves its server and sums ordered part sizes', () => {
    const selected = bookBasketItem('book', 'Book', 'books-server', 'Author', [
        { id: 'part-1', duration: 60, sizeBytes: 100 },
        { id: 'part-2', duration: 120, sizeBytes: 200 },
    ]);
    assert.deepEqual({ id: selected.id, type: selected.type, serverId: selected.serverId, count: selected.childCount,
        bytes: selected.sizeBytes, ticks: selected.sizeTicks },
    { id: 'book', type: 'Book', serverId: 'books-server', count: 2, bytes: 300, ticks: 1_800_000_000 });
});

test('book parts without byte metadata get a nonzero duration estimate', () => {
    const selected = bookBasketItem('book', 'Book', 'books-server', undefined, [
        { id: 'part', duration: 120, sizeBytes: null },
    ]);
    assert.equal(selected.sizeBytes, 1_920_000);
});

test('podcast selections carry typed show and episode identities with conservative size estimates', () => {
    const episodes = [
        { id: 'episode-1', title: 'First', durationSeconds: 60 },
        { id: 'episode-2', title: 'Second', durationSeconds: null },
    ];
    const show = podcastShowBasketItem('show', 'Talks', 'podcast-server', episodes);
    const episode = podcastEpisodeBasketItem(episodes[1], 'podcast-server');
    assert.equal(show.type, 'PodcastShow');
    assert.equal(show.serverId, 'podcast-server');
    assert.equal(show.childCount, 2);
    assert.equal(show.sizeBytes, (60 + 3600) * 16_000);
    assert.equal(episode.type, 'PodcastEpisode');
    assert.equal(episode.id, 'episode-2');
    assert.equal(episode.sizeBytes, 3600 * 16_000);
});

test('zero-duration podcast metadata uses the unknown-duration size estimate', () => {
    const episode = { id: 'episode-zero', title: 'Unknown length', durationSeconds: 0 };
    assert.equal(podcastEpisodeBasketItem(episode, 'podcast-server').sizeBytes, 3_600 * 16_000);
    assert.equal(podcastShowBasketItem('show', 'Talks', 'podcast-server', [episode]).sizeBytes, 3_600 * 16_000);
});
