#!/usr/bin/env node
import { readFileSync, writeFileSync } from 'node:fs';
import { resolve, join } from 'node:path';
import { execFileSync } from 'node:child_process';
import { validateRuntimeLock } from './prebuilt-audio-runtime.mjs';

const root = resolve(import.meta.dirname, '..');
if (!process.argv[2]) throw new Error('Usage: node scripts/pin-audio-runtime.mjs <downloaded-release-manifest.json>');
const lock = JSON.parse(readFileSync(resolve(process.argv[2]), 'utf8'));
const producer = join(root, 'dependencies/ffmpeg-runtime');
validateRuntimeLock(lock,
  JSON.parse(readFileSync(join(root, 'hifimule-daemon/audio-runtime.json'), 'utf8')),
  JSON.parse(readFileSync(join(producer, 'manifest.json'), 'utf8')),
  execFileSync('git', ['rev-parse', 'HEAD'], { cwd: producer, encoding: 'utf8' }).trim());
writeFileSync(join(root, 'hifimule-daemon/audio-runtime-lock.json'), JSON.stringify(lock, null, 2) + '\n');
console.log(`Pinned ${lock.release}: ordinary builds now consume verified prebuilt archives`);
