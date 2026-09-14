import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

const controls = readFileSync(
  new URL('../../hifimule-ui/src/components/PlaybackControls.ts', import.meta.url),
  'utf8',
);
const styles = readFileSync(
  new URL('../../hifimule-ui/src/styles.css', import.meta.url),
  'utf8',
);

test('playback status is an atomic live status region', () => {
  assert.match(controls, /status\.setAttribute\('role', 'status'\)/);
  assert.match(controls, /status\.setAttribute\('aria-live', 'polite'\)/);
  assert.match(controls, /status\.setAttribute\('aria-atomic', 'true'\)/);
});

test('transport buttons expose localized accessible names and preserve focus', () => {
  assert.match(controls, /button\.setAttribute\('aria-label', t\(`playback\.\$\{action\}`\)\)/);
  assert.match(controls, /dataset\.playbackAction = action/);
  assert.match(controls, /\[data-playback-action="\$\{focused\}"\]/);
  assert.match(styles, /\.playback-controls sl-button:focus-visible/);
});
