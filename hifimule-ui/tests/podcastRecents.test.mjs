import assert from 'node:assert/strict';
import test from 'node:test';
import { acceptRecentEpisodePage } from '../src/podcastRecents.ts';

test('recent episode pages preserve order and deduplicate while advancing the server offset', () => {
    const first = acceptRecentEpisodePage(1, 1, true, [], 0, 2, {
        episodes: [{ id: 'new' }, { id: 'older' }], total: 4, sourceCount: 2,
    });
    assert.deepEqual(first, {
        episodes: [{ id: 'new' }, { id: 'older' }], nextOffset: 2, total: 4,
    });
    const second = acceptRecentEpisodePage(2, 2, true, first.episodes, first.nextOffset, 2, {
        episodes: [{ id: 'older' }, { id: 'oldest' }], total: 4, sourceCount: 2,
    });
    assert.deepEqual(second.episodes.map(episode => episode.id), ['new', 'older', 'oldest']);
    assert.equal(second.nextOffset, 4);
});

test('stale and inactive tab responses cannot change recent episodes', () => {
    const page = { episodes: [{ id: 'unwanted' }], total: 1, sourceCount: 1 };
    assert.equal(acceptRecentEpisodePage(1, 2, true, [], 0, 50, page), null);
    assert.equal(acceptRecentEpisodePage(2, 2, false, [], 0, 50, page), null);
});

test('duplicate episode identities within a server page render once', () => {
    const result = acceptRecentEpisodePage(1, 1, true, [], 0, 50, {
        episodes: [{ id: 'same' }, { id: 'same' }], total: 2, sourceCount: 2,
    });
    assert.deepEqual(result.episodes, [{ id: 'same' }]);
});

test('an empty server page ends pagination even if its total remains high', () => {
    const result = acceptRecentEpisodePage(1, 1, true, [{ id: 'only' }], 50, 50, {
        episodes: [], total: 100, sourceCount: 0,
    });
    assert.equal(result.total, 1);
});

test('a page containing only removed shows still allows later pages', () => {
    const result = acceptRecentEpisodePage(1, 1, true, [], 0, 50, {
        episodes: [], total: 100, sourceCount: 50,
    });
    assert.equal(result.nextOffset, 50);
    assert.equal(result.total, 100);
});
