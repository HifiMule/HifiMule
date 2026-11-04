// Runtime build recipes live in the pinned ffmpeg-runtime submodule.
import { readFileSync } from 'node:fs';
export const encodingDependencyReceipt = JSON.parse(readFileSync(new URL('../hifimule-daemon/audio-runtime.json', import.meta.url), 'utf8')).encodingDependencies;
