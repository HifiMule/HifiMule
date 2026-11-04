import { execFileSync } from 'node:child_process';
import { createHash, randomUUID } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, renameSync, rmSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { acquireRuntimeLock } from './runtime-lock.mjs';

const root = resolve(import.meta.dirname, '..');
const lockPath = join(root, 'hifimule-daemon/audio-runtime-lock.json');
const producer = join(root, 'dependencies/ffmpeg-runtime');
const digest = path => createHash('sha256').update(readFileSync(path)).digest('hex');
export function usesPrebuiltAudioRuntime(env = process.env) {
  return env.HIFIMULE_FFMPEG_SOURCE_BUILD !== '1' && (env.HIFIMULE_FFMPEG_PREBUILT === '1' || existsSync(lockPath));
}

export function validateRuntimeLock(lock, app, recipe, producerCommit) {
  const identity = createHash('sha256').update(JSON.stringify(recipe)).digest('hex');
  if (lock.schemaVersion !== 1 || lock.repository !== 'HifiMule/ffmpeg-runtime' || !/^ffmpeg-[0-9.]+-r[1-9][0-9]*$/.test(lock.release) || lock.producerCommit !== producerCommit || !/^[a-f0-9]{40}$/.test(producerCommit) || lock.buildIdentity !== identity) throw new Error('FFmpeg runtime lock does not match the pinned submodule');
  for (const key of Object.keys(recipe).filter(key => !['schemaVersion', 'recipeRevision', 'targets'].includes(key))) {
    if (JSON.stringify(app[key]) !== JSON.stringify(recipe[key])) throw new Error(`FFmpeg producer/app configuration mismatch: ${key}`);
  }
  if (lock.release !== `ffmpeg-${recipe.ffmpegRelease}-r${recipe.recipeRevision}` || JSON.stringify(Object.keys(lock.targets ?? {}).sort()) !== JSON.stringify(Object.keys(recipe.targets).sort())) throw new Error('FFmpeg lock must cover every shipping target');
  for (const [target, asset] of Object.entries(lock.targets)) {
    if (asset.asset !== `${lock.release}-${target}.tar.gz` || !/^[a-f0-9]{64}$/.test(asset.sha256)) throw new Error(`Invalid pinned runtime asset ${target}`);
  }
  return lock;
}

export function loadRuntimeLock() {
  if (!existsSync(join(producer, 'manifest.json'))) throw new Error('Initialize the FFmpeg submodule: rtk git submodule update --init --recursive');
  if (!existsSync(lockPath)) throw new Error('No validated FFmpeg release is pinned yet; publish/validate all four assets, then run scripts/pin-audio-runtime.mjs');
  const lock = JSON.parse(readFileSync(lockPath, 'utf8'));
  const recipe = JSON.parse(readFileSync(join(producer, 'manifest.json'), 'utf8'));
  const app = JSON.parse(readFileSync(join(root, 'hifimule-daemon/audio-runtime.json'), 'utf8'));
  const commit = execFileSync('git', ['rev-parse', 'HEAD'], { cwd: producer, encoding: 'utf8' }).trim();
  return validateRuntimeLock(lock, app, recipe, commit);
}

export function ensurePrebuiltAudioRuntime(target, verify, options = {}) {
  const lock = options.lock ?? loadRuntimeLock();
  const asset = lock.targets[target];
  if (!asset) throw new Error(`No prebuilt FFmpeg for ${target}; supported: ${Object.keys(lock.targets).join(', ')}. Use the producer source-build command for other targets.`);
  const cache = resolve(options.cacheRoot ?? join(root, 'target/audio-runtime/prebuilt'));
  const prefix = join(cache, `${lock.release}-${target}`);
  const complete = join(prefix, '.hifimule-install-complete');
  const run = options.execute ?? execFileSync;
  const check = directory => {
    const metadata = JSON.parse(readFileSync(join(directory, '.ffmpeg-runtime.json'), 'utf8'));
    if (metadata.target !== target || metadata.release !== lock.release || metadata.producerCommit !== lock.producerCommit || metadata.buildIdentity !== lock.buildIdentity) throw new Error('Cached FFmpeg identity mismatch');
    verify(directory, target);
    return directory;
  };
  const unlock = acquireRuntimeLock(`${prefix}.lock`);
  const staging = `${prefix}.install-${randomUUID()}`;
  const archive = join(cache, `${asset.asset}.download-${randomUUID()}`);
  try {
    if (existsSync(prefix)) {
      if (!existsSync(complete)) rmSync(prefix, { recursive: true, force: true });
      else {
        try { return check(prefix); }
        catch (error) { throw new Error(`Cached FFmpeg verification failed. Remove ${prefix} and retry.`, { cause: error }); }
      }
    }
    mkdirSync(cache, { recursive: true });
    const url = `https://github.com/${lock.repository}/releases/download/${lock.release}/${asset.asset}`;
    run(process.platform === 'win32' ? 'curl.exe' : 'curl', ['--fail', '--location', '--retry', '5', '--retry-all-errors', '--connect-timeout', '30', '--max-time', '600', '--output', archive, url], { stdio: 'inherit' });
    if (digest(archive) !== asset.sha256) throw new Error(`FFmpeg archive checksum mismatch for ${target}`);
    run(process.execPath, [join(producer, 'scripts/install-runtime.mjs'), target, archive, staging, lock.producerCommit], { stdio: 'inherit' });
    check(staging);
    const publish = options.renameSync ?? renameSync;
    const sleep = options.sleep ?? (delay => Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, delay));
    for (let attempt = 0; ; attempt++) {
      try { publish(staging, prefix); break; }
      catch (error) {
        if (!['EPERM', 'EACCES', 'EBUSY'].includes(error.code) || attempt >= 20) throw error;
        sleep(250);
      }
    }
    try {
      // Keep the lock through final relocation and validation. otool headings
      // then name the destination rather than the old staging directory.
      run(process.execPath, [join(producer, 'scripts/relocate-runtime.mjs'), target, prefix, staging, prefix], { stdio: 'inherit' });
      check(prefix);
      writeFileSync(complete, `${lock.producerCommit}\n`);
      return prefix;
    } catch (error) { rmSync(prefix, { recursive: true, force: true }); throw error; }
  } finally {
    rmSync(staging, { recursive: true, force: true });
    rmSync(archive, { force: true });
    unlock();
  }
}
