import assert from 'node:assert/strict';
import test from 'node:test';
import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, renameSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { validateRuntimeLock, ensurePrebuiltAudioRuntime, usesPrebuiltAudioRuntime } from '../prebuilt-audio-runtime.mjs';

const recipe = JSON.parse(readFileSync(new URL('../../dependencies/ffmpeg-runtime/manifest.json', import.meta.url), 'utf8'));
const app = JSON.parse(readFileSync(new URL('../../hifimule-daemon/audio-runtime.json', import.meta.url), 'utf8'));
const commit = 'a'.repeat(40), hash = value => createHash('sha256').update(value).digest('hex');
const release = `ffmpeg-${recipe.ffmpegRelease}-r${recipe.recipeRevision}`;
const makeLock = () => ({ schemaVersion: 1, repository: 'HifiMule/ffmpeg-runtime', producerCommit: commit, buildIdentity: hash(JSON.stringify(recipe)), release,
  targets: Object.fromEntries(Object.keys(recipe.targets).map(target => [target, { asset: `${release}-${target}.tar.gz`, sha256: hash('fixture archive') }])) });

test('lock binds all artifacts to producer commit/config while ignoring app-only settings', () => {
  assert.equal(validateRuntimeLock(makeLock(), { ...app, bufferPolicy: {} }, recipe, commit).release, release);
  assert.throws(() => validateRuntimeLock(makeLock(), app, recipe, 'b'.repeat(40)), /submodule/);
  assert.throws(() => validateRuntimeLock(makeLock(), { ...app, configureFlags: [] }, recipe, commit), /configuration mismatch/);
  const incomplete = makeLock(); delete incomplete.targets['x86_64-pc-windows-msvc'];
  assert.throws(() => validateRuntimeLock(incomplete, app, recipe, commit), /every shipping target/);
  const bad = makeLock(); Object.values(bad.targets)[0].sha256 = 'invalid';
  assert.throws(() => validateRuntimeLock(bad, app, recipe, commit), /Invalid pinned/);
  assert.equal(usesPrebuiltAudioRuntime({ HIFIMULE_FFMPEG_PREBUILT: '1' }), true);
  assert.equal(usesPrebuiltAudioRuntime({ HIFIMULE_FFMPEG_SOURCE_BUILD: '1', HIFIMULE_FFMPEG_PREBUILT: '1' }), false);
});

function fixture(t, { corrupt = false, interrupted = false, relocationFailure = false } = {}) {
  const cacheRoot = mkdtempSync(join(tmpdir(), 'prebuilt-consumer-')); t.after(() => rmSync(cacheRoot, { recursive: true, force: true }));
  const target = 'aarch64-apple-darwin', lock = makeLock();
  const calls = [];
  const execute = (command, args) => {
    calls.push({ command, args });
    if (command.endsWith('curl') || command.endsWith('curl.exe')) { writeFileSync(args[args.indexOf('--output') + 1], corrupt ? 'bad' : 'fixture archive'); return; }
    if (args[0].endsWith('install-runtime.mjs')) {
      const staging = args[3]; mkdirSync(staging, { recursive: true });
      if (interrupted) throw new Error('Interrupted extraction');
      writeFileSync(join(staging, '.ffmpeg-runtime.json'), JSON.stringify({ target, release, producerCommit: commit, buildIdentity: lock.buildIdentity })); return;
    }
    assert.ok(args[0].endsWith('relocate-runtime.mjs'));
    assert.equal(args[2], args[4], 'relocate at the destination so otool headings cannot contain the old staging path');
    assert.ok(existsSync(args[2]));
    if (relocationFailure) throw new Error('Relocation failed');
  };
  const verify = directory => assert.ok(!directory.includes('.staging-'), 'macOS verifier rejects foreign staging load paths');
  return { cacheRoot, target, lock, calls, execute, verify };
}

test('prebuilt install validates before atomic publish and reuses matching prefix', t => {
  const f = fixture(t);
  const prefix = ensurePrebuiltAudioRuntime(f.target, f.verify, f);
  assert.ok(existsSync(prefix));
  assert.equal(ensurePrebuiltAudioRuntime(f.target, f.verify, f), prefix);
  assert.equal(f.calls.filter(call => /curl(?:\.exe)?$/.test(call.command)).length, 1);
  assert.deepEqual(readdirSync(f.cacheRoot), [`${release}-${f.target}`]);
});

for (const failure of ['corrupt', 'interrupted', 'relocationFailure']) test(`prebuilt ${failure} fails without source compilation or partial prefix`, t => {
  const f = fixture(t, { [failure]: true });
  assert.throws(() => ensurePrebuiltAudioRuntime(f.target, f.verify, f), /checksum mismatch|Interrupted extraction|Relocation failed/);
  assert.deepEqual(readdirSync(f.cacheRoot), []);
  assert.ok(f.calls.every(({ command }) => !['make', 'cmake', './configure'].includes(command)));
});

test('unsupported targets fail before attempting a download', t => {
  const f = fixture(t);
  assert.throws(() => ensurePrebuiltAudioRuntime('unsupported', f.verify, f), /No prebuilt FFmpeg/);
  assert.equal(f.calls.length, 0);
});

test('an interrupted published prefix is recovered under the installation lock', t => {
  const f = fixture(t);
  const prefix = join(f.cacheRoot, `${release}-${f.target}`);
  mkdirSync(prefix);
  writeFileSync(join(prefix, '.ffmpeg-runtime.json'), 'unfinished');
  assert.equal(ensurePrebuiltAudioRuntime(f.target, f.verify, f), prefix);
  assert.ok(existsSync(join(prefix, '.hifimule-install-complete')));
});

test('publication retries transient Windows file locks', t => {
  const f = fixture(t);
  let attempts = 0;
  const delays = [];
  f.renameSync = (source, destination) => {
    if (++attempts < 3) throw Object.assign(new Error('Scanner holds DLL'), { code: 'EPERM' });
    renameSync(source, destination);
  };
  f.sleep = delay => delays.push(delay);
  assert.ok(existsSync(ensurePrebuiltAudioRuntime(f.target, f.verify, f)));
  assert.equal(attempts, 3);
  assert.deepEqual(delays, [250, 250]);
});
