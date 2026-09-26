import assert from 'node:assert/strict';
import test from 'node:test';
import { defaultLegacyPipeline, normalizePipeline, serializePipeline } from '../src/state/autoFill.ts';

test('podcast retention settings survive the manifest pipeline round trip', () => {
    const pipeline = defaultLegacyPipeline(2_000_000);
    pipeline.podcastRetention = { recentCount: 4, unplayedOnly: true, mode: 'selectedShows', showIds: ['show-a', 'show-b'] };
    const stored = serializePipeline(pipeline);
    assert.deepEqual(stored.podcastRetention, { recentCount: 4, unplayedOnly: true, mode: 'selectedShows', showIds: ['show-a', 'show-b'] });
    assert.deepEqual(normalizePipeline(stored).podcastRetention, stored.podcastRetention);
    assert.equal(stored.budget.maxBytes, 2_000_000);
});

test('legacy pipelines inherit recent episodes and clamp invalid retention count', () => {
    assert.deepEqual(normalizePipeline({}).podcastRetention, { recentCount: 10, unplayedOnly: false, mode: 'latest', showIds: [] });
    const pipeline = defaultLegacyPipeline();
    pipeline.podcastRetention.recentCount = 200;
    assert.equal(serializePipeline(pipeline).podcastRetention.recentCount, 100);
});
