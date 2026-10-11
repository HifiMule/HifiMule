#!/usr/bin/env node
// Read-only, dependency-free inspector. Node 24+ provides Zstd and lossless
// JSON numeric tokens via reviver context.source / JSON.rawJSON.
import { openSync, readSync, closeSync } from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { zstdDecompressSync, constants } from 'node:zlib';

const MAX_BYTES = 512 * 1024 * 1024;
export function readBoundedFile(path, limit = MAX_BYTES) {
  requireValue(Number.isSafeInteger(limit) && limit >= 0 && limit <= MAX_BYTES, 'Invalid manifest input limit');
  const descriptor = openSync(path, 'r');
  const chunks = [];
  let size = 0;
  try {
    for (;;) {
      const chunk = Buffer.allocUnsafe(Math.min(64 * 1024, limit + 1 - size));
      const count = readSync(descriptor, chunk, 0, chunk.length, null);
      if (count === 0) return Buffer.concat(chunks, size);
      size += count;
      requireValue(size <= limit, `Manifest input exceeds ${limit} byte limit`);
      chunks.push(chunk.subarray(0, count));
    }
  } finally { closeSync(descriptor); }
}
const MAGIC = Buffer.from([0x28, 0xb5, 0x2f, 0xfd]);
const types = {
  isAutoFill: 'boolean', mediaRole: 'role', providerItemId: 'string', name: 'string',
  album: '?string', artist: '?string', localPath: 'string', sizeBytes: 'u64', syncedAt: 'string',
  originalName: '?string', etag: '?string', providerAlbumId: '?string', providerContentType: '?string',
  providerSuffix: '?string', originalBitrate: '?u32', originalContainer: '?string',
  trackNumber: '?u32', serverId: '?string',
};
function requireValue(condition, message) { if (!condition) throw new Error(message); }
function object(value) { return value !== null && typeof value === 'object' && !Array.isArray(value) && !JSON.isRawJSON(value); }
function unsigned(value, max) {
  if (!JSON.isRawJSON(value) || !/^(0|[1-9][0-9]*)$/.test(value.rawJSON)) return false;
  return BigInt(value.rawJSON) <= max;
}
function valid(value, type) {
  if (type.startsWith('?')) return value === null || valid(value, type.slice(1));
  if (type === 'u64') return unsigned(value, 18446744073709551615n);
  if (type === 'u32') return unsigned(value, 4294967295n);
  if (type === 'i64') return JSON.isRawJSON(value) && /^-?(0|[1-9][0-9]*)$/.test(value.rawJSON) && BigInt(value.rawJSON) >= -9223372036854775808n && BigInt(value.rawJSON) <= 9223372036854775807n;
  if (type === 'float') return JSON.isRawJSON(value) && Number.isFinite(Number(value.rawJSON));
  if (type === 'strings') return Array.isArray(value) && value.every(item => typeof item === 'string');
  if (type === 'array') return Array.isArray(value);
  if (type === 'object') return object(value);
  if (type === 'role') return ['music', 'audiobook', 'podcast'].includes(value);
  return typeof value === type;
}
function exactKeys(value, names, label) {
  requireValue(object(value) && Object.keys(value).length === names.length && names.every(name => Object.hasOwn(value, name)), `Invalid ${label} fields`);
}
function checkFrame(bytes) {
  requireValue(bytes.length >= 5 && bytes.subarray(0, 4).equals(MAGIC), 'Invalid Zstd manifest magic');
  const descriptor = bytes[4];
  requireValue((descriptor & 4) !== 0, 'Manifest frame checksum is required');
  requireValue((descriptor & 0x18) === 0, 'Invalid Zstd frame descriptor');
  const single = (descriptor & 0x20) !== 0;
  const sizeFlag = descriptor >> 6;
  let offset = 5 + (single ? 0 : 1) + [0, 1, 2, 4][descriptor & 3] + (sizeFlag === 0 && single ? 1 : [0, 2, 4, 8][sizeFlag]);
  let last = false;
  while (!last) {
    requireValue(offset + 3 <= bytes.length, 'Truncated Zstd block');
    const block = bytes.readUIntLE(offset, 3); offset += 3;
    last = (block & 1) !== 0;
    const type = (block >> 1) & 3;
    requireValue(type !== 3, 'Invalid Zstd block type');
    offset += type === 1 ? 1 : block >> 3;
    requireValue(offset <= bytes.length, 'Truncated Zstd block');
  }
  requireValue(offset + 4 === bytes.length, 'Manifest must contain exactly one complete Zstd frame');
}
function checkFields(value, fields, required = [], label = 'metadata') {
  requireValue(object(value), `Invalid ${label}`);
  for (const [name, type] of Object.entries(fields)) {
    requireValue((!Object.hasOwn(value, name) && !required.includes(name)) || valid(value[name], type), `Invalid ${label} field ${name}`);
  }
}
function validatePipeline(pipeline) {
  const stages = {
    podcastRetention: { recentCount: 'u64', unplayedOnly: 'boolean', mode: 'string', showIds: 'strings' },
    filter: { includeTags: 'strings', excludeTags: 'strings', includeGenres: 'strings', excludeGenres: 'strings' },
    memory: { cooldownWeeks: '?u32', playedExclusion: 'boolean', stableCorePct: '?float', repeatTolerance: '?float' },
    budget: { maxBytes: '?u64', targetDurationSecs: '?u64', headroomBytes: '?u64', encodingFromGoals: 'boolean' },
    quality: { bestVersion: 'boolean', versionPreference: 'array' },
    rarity: { enabled: 'boolean', legendaryWeight: 'float', rareWeight: 'float', commonWeight: 'float', rareMaxPlays: 'u32' },
    pity: { enabled: 'boolean', thresholdSyncs: 'u32', guaranteedRatio: 'float', discoveryMaxPlays: 'u32' },
    context: { enabled: 'boolean', rules: 'array' },
    promotion: { spotlight: 'boolean', spotlightShare: '?float', albumTrackRatio: '?float', promoteAlbumMinFavorites: '?u32', coherence: 'boolean' },
  };
  checkFields(pipeline, { enabled: 'boolean', sources: 'array', fallback: 'array', unit: 'string', ordering: 'strings', ...Object.fromEntries(Object.keys(stages).map(name => [name, 'object'])) }, [], 'auto-fill pipeline');
  for (const [name, fields] of Object.entries(stages)) {
    if (pipeline[name] !== undefined) checkFields(pipeline[name], fields, [], `auto-fill ${name}`);
  }
  if (pipeline.unit !== undefined) requireValue(['track', 'album', 'artist'].includes(pipeline.unit), 'Invalid auto-fill unit');
  if (pipeline.podcastRetention?.mode !== undefined) requireValue(['latest', 'selectedShows'].includes(pipeline.podcastRetention.mode), 'Invalid podcast retention mode');
  if (pipeline.ordering !== undefined) requireValue(pipeline.ordering.every(key => ['favorite', 'playCount', 'dateCreated', 'random', 'quality', 'excavation', 'rediscovery', 'rarity'].includes(key)), 'Invalid auto-fill ordering');
  for (const source of [...(pipeline.sources ?? []), ...(pipeline.fallback ?? [])]) {
    checkFields(source, { kind: 'string', ref: '?string', share: '?float' }, ['kind'], 'auto-fill source');
    requireValue(['library', 'favorites', 'history', 'playlist'].includes(source.kind), 'Invalid auto-fill source kind');
  }
}
function validateManifest(manifest) {
  requireValue(object(manifest) && typeof manifest.device_id === 'string' && typeof manifest.version === 'string', 'Invalid manifest metadata');
  for (const [canonical, alias] of [['audiobookPath', 'audiobook_path'], ['podcastPath', 'podcast_path'], ['playlistPath', 'playlist_path']]) {
    requireValue(!(Object.hasOwn(manifest, canonical) && Object.hasOwn(manifest, alias)), `Duplicate manifest field ${canonical}`);
  }
  checkFields(manifest, {
    name: '?string', icon: '?string', managed_paths: 'strings',
    audiobookPath: '?string', audiobook_path: '?string', podcastPath: '?string', podcast_path: '?string', playlistPath: '?string', playlist_path: '?string',
    dirty: 'boolean', pending_item_ids: 'strings', basket_items: 'array',
    auto_sync_on_connect: 'boolean', last_sync_at: '?i64', auto_fill: 'object',
    transcoding_profile_id: '?string', last_synced_transcoding_profile_id: '?string', transcoding_profile_dirty: 'boolean',
    playlists: 'array', storage_id: '?string', folder_ids: 'object',
  });
  for (const item of manifest.basket_items ?? []) {
    checkFields(item, { id: 'string', name: 'string', type: 'string', serverId: '?string', artist: '?string', childCount: 'u32', sizeTicks: 'i64', sizeBytes: 'u64' }, ['id', 'name', 'type', 'childCount', 'sizeTicks', 'sizeBytes'], 'basket item');
  }
  for (const playlist of manifest.playlists ?? []) {
    checkFields(playlist, { jellyfinId: 'string', filename: 'string', trackCount: 'u32', trackIds: 'strings', lastModified: 'string' }, ['jellyfinId', 'filename', 'trackCount', 'trackIds', 'lastModified'], 'playlist');
  }
  for (const id of Object.values(manifest.folder_ids ?? {})) requireValue(valid(id, 'u32'), 'Invalid folder ID');
  if (manifest.auto_fill !== undefined) {
    const config = manifest.auto_fill;
    if (Object.values(config).every(object)) {
      for (const pipeline of Object.values(config)) validatePipeline(pipeline);
    } else {
      checkFields(config, { enabled: 'boolean', maxBytes: '?u64' }, [], 'legacy auto-fill');
    }
  }
  if (manifest.synced_items !== undefined) {
    requireValue(Array.isArray(manifest.synced_items), 'Invalid synced_items');
    for (const item of manifest.synced_items) {
      requireValue(object(item), 'Invalid synced item');
      for (const [name, type] of Object.entries(types)) {
        const required = ['providerItemId', 'name', 'localPath', 'sizeBytes', 'syncedAt'].includes(name);
        requireValue((!required && item[name] === undefined) || valid(item[name], type), `Invalid track field ${name}`);
      }
    }
  }
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
try {
  requireValue(process.argv.length === 3, 'Usage: node scripts/inspect-manifest.mjs <manifest.json.zst|manifest.json>');
  requireValue(typeof zstdDecompressSync === 'function' && typeof JSON.rawJSON === 'function', 'Node.js 24 or newer is required');
  const path = process.argv[2];
  let bytes = readBoundedFile(path);
  const compressed = bytes.subarray(0, 4).equals(MAGIC) || path.endsWith('.zst');
  if (compressed) {
    checkFrame(bytes);
    bytes = zstdDecompressSync(bytes, { maxOutputLength: MAX_BYTES, params: { [constants.ZSTD_d_windowLogMax]: 29 } });
  }
  const text = new TextDecoder('utf-8', { fatal: true }).decode(bytes);
  let manifest = JSON.parse(text, (key, value, context) => typeof value === 'number' ? JSON.rawJSON(context.source) : value);
  if (compressed) {
    exactKeys(manifest, ['format', 'storageVersion', 'manifest', 'syncedItems'], 'storage wrapper');
    requireValue(manifest.format === 'hifimule-column-json' && JSON.isRawJSON(manifest.storageVersion) && manifest.storageVersion.rawJSON === '1', 'Unsupported manifest storage format/version');
    const rows = manifest.syncedItems;
    exactKeys(rows, ['count', 'columns'], 'track rows');
    requireValue(unsigned(rows.count, BigInt(MAX_BYTES)), 'Invalid track count');
    const count = Number(rows.count.rawJSON);
    exactKeys(rows.columns, Object.keys(types), 'track columns');
    for (const [name, type] of Object.entries(types)) {
      const column = rows.columns[name];
      requireValue(Array.isArray(column) && column.length === count, `Manifest column ${name} count mismatch`);
      requireValue(column.every(value => valid(value, type)), `Invalid column type ${name}`);
    }
    const metadata = manifest.manifest;
    requireValue(object(metadata) && (metadata.synced_items === undefined || (Array.isArray(metadata.synced_items) && metadata.synced_items.length === 0)), 'Manifest metadata cannot contain track records');
    metadata.synced_items = Array.from({ length: count }, (_, index) => Object.fromEntries(Object.keys(types).map(name => [name, rows.columns[name][index]])));
    manifest = metadata;
  }
  validateManifest(manifest);
  process.stdout.write(`${JSON.stringify(manifest, null, 2)}\n`);
} catch (error) {
  process.stderr.write(`Manifest inspection failed: ${error.message}\n`);
  process.exitCode = 1;
}
}
