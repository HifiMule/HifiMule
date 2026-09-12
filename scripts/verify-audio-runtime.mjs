#!/usr/bin/env node
import { readFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(import.meta.dirname, '..');
const manifest = JSON.parse(readFileSync(resolve(root, 'hifimule-daemon/audio-runtime.json'), 'utf8'));
if (manifest.ffmpegRelease !== '9.0.1' || !/^[a-f0-9]{64}$/.test(manifest.sourceSha256)) {
  throw new Error('Audio runtime manifest is incomplete');
}

export const audioRuntimeVerification = Object.freeze({
  environmentVariable: 'HIFIMULE_FFMPEG_RUNTIME_VERIFIED',
  value: 'ffmpeg-9-runtime-verified-v2',
});

export function withoutAudioRuntimeVerification(env = process.env) {
  const clean = { ...env };
  delete clean[audioRuntimeVerification.environmentVariable];
  return clean;
}

export function withAudioRuntimeVerification(env) {
  return {
    ...withoutAudioRuntimeVerification(env),
    [audioRuntimeVerification.environmentVariable]: audioRuntimeVerification.value,
  };
}

function versionParts(version) {
  if (!/^\d+(?:\.\d+){2}$/.test(version)) return undefined;
  return version.split('.').map(Number);
}

function compareVersions(left, right) {
  for (let index = 0; index < Math.max(left.length, right.length); index += 1) {
    const difference = (left[index] ?? 0) - (right[index] ?? 0);
    if (difference !== 0) return Math.sign(difference);
  }
  return 0;
}

export function verifyAudioRuntime(options = {}) {
  const execute = options.execFileSync ?? execFileSync;
  const prefix = options.prefix ? resolve(options.prefix) : undefined;
  const baseEnv = withoutAudioRuntimeVerification(options.env ?? process.env);
  const pkgConfigEnv = prefix
    ? {
        ...baseEnv,
        PKG_CONFIG_PATH: [resolve(prefix, 'lib/pkgconfig'), baseEnv.PKG_CONFIG_PATH]
          .filter(Boolean)
          .join(':'),
      }
    : baseEnv;
  const versions = {};

  for (const [library, expectedVersion] of Object.entries(manifest.abiVersions)) {
    const packageName = `lib${library}`;
    let version;
    try {
      version = execute('pkg-config', ['--modversion', packageName], {
        encoding: 'utf8',
        env: pkgConfigEnv,
      }).trim();
    } catch (error) {
      const location = prefix ? ` under controlled prefix ${prefix}` : '';
      const expectation = prefix
        ? `exact ABI ${expectedVersion}`
        : `ABI ${expectedVersion} or newer within major ${expectedVersion.split('.')[0]}`;
      throw new Error(
        `Controlled FFmpeg ${manifest.ffmpegRelease} dependency ${packageName}${location} could not be verified. ` +
        `Expected ${expectation}. Ensure pkg-config can read the validated runtime metadata.`,
        { cause: error },
      );
    }
    const expectedParts = versionParts(expectedVersion);
    const foundParts = versionParts(version);
    const exactPrefixMatch = prefix && version === expectedVersion;
    const compatibleHostMatch = !prefix && foundParts && expectedParts &&
      foundParts[0] === expectedParts[0] && compareVersions(foundParts, expectedParts) >= 0;
    if (!exactPrefixMatch && !compatibleHostMatch) {
      const expectation = prefix
        ? `exact ABI ${expectedVersion}`
        : `ABI ${expectedVersion} or newer within major ${expectedParts[0]}`;
      throw new Error(`${packageName} ABI mismatch: expected ${expectation}, found ${version}`);
    }
    versions[packageName] = version;
    if (prefix) {
      const libdir = resolve(execute('pkg-config', ['--variable=libdir', packageName], {
        encoding: 'utf8',
        env: pkgConfigEnv,
      }).trim());
      if (libdir !== prefix && !libdir.startsWith(`${prefix}/`)) {
        throw new Error(`${packageName} resolved outside controlled prefix ${prefix}: ${libdir}`);
      }
    }
  }
  return { ffmpegRelease: manifest.ffmpegRelease, prefix, versions };
}

if (process.argv[1] && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url))) {
  const prefixArg = process.argv.indexOf('--prefix');
  if (prefixArg >= 0 && !process.argv[prefixArg + 1]) {
    throw new Error('The --prefix option requires a controlled FFmpeg prefix path');
  }
  const result = verifyAudioRuntime({ prefix: prefixArg >= 0 ? process.argv[prefixArg + 1] : undefined });
  const details = Object.entries(result.versions).map(([name, version]) => `${name}=${version}`).join(', ');
  console.log(`Verified controlled audio ABI for FFmpeg ${result.ffmpegRelease}: ${details}`);
}
