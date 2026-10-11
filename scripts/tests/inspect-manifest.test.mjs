import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, writeFileSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import { zstdCompressSync, constants } from 'node:zlib';
import { readBoundedFile } from '../inspect-manifest.mjs';

const script = fileURLToPath(new URL('../inspect-manifest.mjs', import.meta.url));
const raw = '{"device_id":"fixture","version":"1.0","synced_items":[{"providerItemId":"duplicate","name":"Été 🎵","localPath":"Music/song.mp3","sizeBytes":18446744073709551615,"syncedAt":"2026-10-11","isAutoFill":true,"mediaRole":"music","album":null,"artist":"","originalName":null,"etag":null,"providerAlbumId":null,"providerContentType":null,"providerSuffix":null,"originalBitrate":null,"originalContainer":null,"trackNumber":4294967295,"serverId":null}],"dirty":true,"pending_item_ids":["duplicate","duplicate"]}';
const columns = Object.fromEntries(Object.keys(JSON.parse(raw).synced_items[0]).map(name => [name, []]));
// Build the wrapper with raw numeric tokens, avoiding JS integer rounding.
const row = JSON.parse(raw, (key, value, context) => context?.source && typeof value === 'number' ? JSON.rawJSON(context.source) : value).synced_items[0];
for (const key of Object.keys(columns)) columns[key] = [row[key]];
const metadata = JSON.parse(raw); delete metadata.synced_items;
const wrapper = { format: 'hifimule-column-json', storageVersion: 1, manifest: metadata, syncedItems: { count: 1, columns } };
const compress = text => zstdCompressSync(Buffer.from(text), { params: { [constants.ZSTD_c_checksumFlag]: 1 } });
function inspect(bytes, suffix = '.json.zst') {
  const dir = mkdtempSync(join(tmpdir(), 'hifimule-inspect-'));
  const path = join(dir, `manifest${suffix}`);
  try {
    writeFileSync(path, bytes);
    const result = spawnSync(process.execPath, [script, path], { encoding: 'utf8' });
    assert.deepEqual(readFileSync(path), Buffer.from(bytes), 'inspection must leave source unchanged');
    return result;
  } finally { rmSync(dir, { recursive: true, force: true }); }
}
test('displays legacy JSON without losing u64 integer precision or duplicate recovery IDs', () => {
  const result = inspect(raw, '.json');
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /18446744073709551615/);
  assert.deepEqual(JSON.parse(result.stdout).pending_item_ids, ['duplicate', 'duplicate']);
});
test('reconstructs named columns and preserves exact u64 tokens', () => {
  const result = inspect(compress(JSON.stringify(wrapper)));
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /18446744073709551615/);
  assert.equal(JSON.parse(result.stdout).synced_items[0].name, 'Été 🎵');
});
test('rejects unsupported versions, inconsistent counts, wrong types and corrupt checksums', () => {
  const cases = [
    { ...wrapper, storageVersion: 2 },
    { ...wrapper, syncedItems: { ...wrapper.syncedItems, count: 2 } },
    { ...wrapper, syncedItems: { count: 1, columns: { ...columns, sizeBytes: ['wrong'] } } },
  ];
  for (const value of cases) {
    const result = inspect(compress(JSON.stringify(value)));
    assert.notEqual(result.status, 0); assert.equal(result.stdout, ''); assert.ok(result.stderr);
  }
  const corrupt = compress(JSON.stringify(wrapper)); corrupt[corrupt.length - 1] ^= 1;
  assert.notEqual(inspect(corrupt).status, 0);
});
test('rejects checksum-free, truncated, concatenated and non-UTF8 input', () => {
  const json = JSON.stringify(wrapper);
  const bytes = compress(json);
  const cases = [
    zstdCompressSync(Buffer.from(json)),
    bytes.subarray(0, bytes.length - 1),
    Buffer.concat([bytes, bytes]),
    Buffer.from('not a compressed manifest'),
  ];
  for (const value of cases) {
    const result = inspect(value);
    assert.notEqual(result.status, 0); assert.equal(result.stdout, '');
  }
  assert.notEqual(inspect(Buffer.from([0xff]), '.json').status, 0);
});
test('inspects checked-in Rust frames with identical lossless JSON to the Rust legacy fixture', () => {
  const fixture = fileURLToPath(new URL('./fixtures/manifest-rust.json', import.meta.url));
  const before = readFileSync(fixture);
  const legacy = inspect(before, '.json');
  const compressed = inspect(readFileSync(`${fixture}.zst`));
  assert.equal(legacy.status, 0, legacy.stderr); assert.equal(compressed.status, 0, compressed.stderr);
  const lossless = text => JSON.parse(text, (key, value, context) => typeof value === 'number' ? context.source : value);
  assert.deepEqual(lossless(compressed.stdout), lossless(legacy.stdout));
  assert.match(compressed.stdout, /18446744073709551615/);
  assert.match(compressed.stdout, /-9223372036854775808/);
  assert.deepEqual(readFileSync(fixture), before);
});

test('rejects malformed known metadata while preserving legacy defaults and numeric tokens', () => {
  const base = { device_id: 'fixture', version: '1.0' };
  assert.equal(inspect(JSON.stringify(base), '.json').status, 0);
  const bad = [
    { dirty: 'false' }, { pending_item_ids: [1] }, { managed_paths: null },
    { basket_items: [{ id: 'album' }] }, { auto_sync_on_connect: 'true' },
    { last_sync_at: '123' }, { transcoding_profile_id: false },
    { playlists: [{}] }, { folder_ids: { Music: -1 } },
    { auto_fill: null }, { auto_fill: { enabled: 'true', maxBytes: null } },
    { auto_fill: { server: { sources: false } } },
  ];
  for (const fields of bad) {
    const metadata = { ...base, ...fields };
    const legacy = inspect(JSON.stringify(metadata), '.json');
    const compressed = inspect(compress(JSON.stringify({ ...wrapper, manifest: metadata })));
    for (const result of [legacy, compressed]) {
      assert.notEqual(result.status, 0, JSON.stringify(fields)); assert.equal(result.stdout, '');
    }
  }
});
test('accepts empty metadata tracks like Rust and rejects nonempty metadata tracks', () => {
  const withEmpty = { ...wrapper, manifest: { ...metadata, synced_items: [] } };
  const result = inspect(compress(JSON.stringify(withEmpty)));
  assert.equal(result.status, 0, result.stderr);
  assert.equal(JSON.parse(result.stdout).synced_items.length, 1);
  const withTracks = { ...wrapper, manifest: { ...metadata, synced_items: [row] } };
  assert.notEqual(inspect(compress(JSON.stringify(withTracks))).status, 0);
});
test('preserves legacy auto-fill defaults when enabled is omitted', () => {
  const config = { maxBytes: JSON.rawJSON('18446744073709551615') };
  const base = { device_id: 'fixture', version: '1.0', auto_fill: config };
  for (const result of [inspect(JSON.stringify(base), '.json'), inspect(compress(JSON.stringify({ ...wrapper, manifest: base })))]) {
    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, /18446744073709551615/);
    assert.equal(JSON.parse(result.stdout).auto_fill.enabled, undefined);
  }
});
test('supports individual path aliases and rejects duplicate canonical and alias fields', () => {
  for (const [canonical, alias] of [['audiobookPath', 'audiobook_path'], ['podcastPath', 'podcast_path'], ['playlistPath', 'playlist_path']]) {
    const base = { device_id: 'fixture', version: '1.0' };
    for (const name of [canonical, alias]) {
      const metadata = { ...base, [name]: 'Media' };
      for (const result of [inspect(JSON.stringify(metadata), '.json'), inspect(compress(JSON.stringify({ ...wrapper, manifest: metadata })))]) {
        assert.equal(result.status, 0, result.stderr);
      }
    }
    const duplicate = { ...base, [canonical]: 'First', [alias]: 'Second' };
    for (const result of [inspect(JSON.stringify(duplicate), '.json'), inspect(compress(JSON.stringify({ ...wrapper, manifest: duplicate })))]) {
      assert.notEqual(result.status, 0, `duplicate ${canonical}/${alias} must fail like Rust`);
      assert.equal(result.stdout, '');
    }
  }
});
test('bounds descriptor reading at exact, over-limit and multiple-chunk sizes', () => {
  const dir = mkdtempSync(join(tmpdir(), 'hifimule-bounded-read-'));
  const path = join(dir, 'manifest.json');
  try {
    const bytes = Buffer.alloc(128 * 1024 + 3, 32);
    writeFileSync(path, bytes);
    assert.deepEqual(readBoundedFile(path, bytes.length), bytes);
    assert.throws(() => readBoundedFile(path, bytes.length - 1), /limit/);
    assert.throws(() => readBoundedFile(path, 0), /limit/);
    assert.deepEqual(readFileSync(path), bytes);
    writeFileSync(path, '');
    assert.equal(readBoundedFile(path, 0).length, 0);
  } finally { rmSync(dir, { recursive: true, force: true }); }
});
