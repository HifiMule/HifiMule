import { execFileSync } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { resolve, join } from 'node:path';

const producer = resolve(import.meta.dirname, '../dependencies/ffmpeg-runtime');
export function buildSourceAudioRuntime(target, { env = process.env } = {}) {
  if (!existsSync(join(producer, 'scripts/build-runtime.mjs'))) throw new Error('Initialize the FFmpeg submodule: rtk git submodule update --init --recursive');
  const work = mkdtempSync(join(tmpdir(), 'hifimule-source-prefix-'));
  const output = join(work, 'prefix.json');
  try {
    execFileSync(process.execPath, [join(producer, 'scripts/build-runtime.mjs'), target, output], { env, stdio: 'inherit' });
    return JSON.parse(readFileSync(output, 'utf8')).prefix;
  } finally { rmSync(work, { recursive: true, force: true }); }
}
