import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import test from 'node:test';

const root = resolve(import.meta.dirname, '../..');
const previewScript = join(root, 'scripts/preview-browse-mode.mjs');
const generated = [
  'hifimule-ui/src/.browse-current.ts',
  'hifimule-ui/src/.browse-before.ts',
  'hifimule-ui/.browse-before.css',
  'hifimule-ui/.browse-preview.html',
];

function runSetupFailure(t, { failOnCall = 0, failOnWrite = 0, conflict = false } = {}) {
  const scratch = mkdtempSync(join(tmpdir(), 'hifimule-browse-preview-'));
  t.after(() => rmSync(scratch, { recursive: true, force: true }));
  mkdirSync(join(scratch, 'hifimule-ui/src'), { recursive: true });
  writeFileSync(join(scratch, 'hifimule-ui/src/library.ts'), '// current library fixture');
  if (conflict) writeFileSync(join(scratch, generated[1]), '// pre-existing fixture');

  const preload = join(scratch, 'preload.mjs');
  writeFileSync(preload, `
    import childProcess from 'node:child_process';
    import fs from 'node:fs';
    import { syncBuiltinESMExports } from 'node:module';
    let call = 0;
    let writeCall = 0;
    const realWriteFileSync = fs.writeFileSync;
    childProcess.execFileSync = () => {
      call += 1;
      if (call === Number(process.env.PREVIEW_FAIL_ON_CALL)) {
        throw new Error(process.env.PREVIEW_FAILURE_MESSAGE);
      }
      return '// baseline fixture ' + call;
    };
    fs.writeFileSync = (...args) => {
      writeCall += 1;
      const result = realWriteFileSync(...args);
      if (writeCall === Number(process.env.PREVIEW_FAIL_ON_WRITE)) {
        throw new Error(process.env.PREVIEW_FAILURE_MESSAGE);
      }
      return result;
    };
    syncBuiltinESMExports();
  `);

  const failureMessage = `preview setup failure ${failOnCall || `write-${failOnWrite}`}`;
  const result = spawnSync(process.execPath, [
    '--import', pathToFileURL(preload).href, previewScript,
  ], {
    cwd: scratch,
    encoding: 'utf8',
    env: {
      ...process.env,
      PREVIEW_FAIL_ON_CALL: String(failOnCall),
      PREVIEW_FAIL_ON_WRITE: String(failOnWrite),
      PREVIEW_FAILURE_MESSAGE: failureMessage,
    },
  });
  return { scratch, result, failureMessage };
}

test('early preview setup failure removes files created by that run and keeps the root error', (t) => {
  const { scratch, result, failureMessage } = runSetupFailure(t, { failOnCall: 1 });
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, new RegExp(failureMessage));
  for (const file of generated) assert.equal(existsSync(join(scratch, file)), false, file);
});

test('later preview setup failure removes every partially generated file and keeps the root error', (t) => {
  const { scratch, result, failureMessage } = runSetupFailure(t, { failOnCall: 2 });
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, new RegExp(failureMessage));
  for (const file of generated) assert.equal(existsSync(join(scratch, file)), false, file);
});

test('a write failure after file creation removes the partial file and keeps the root error', (t) => {
  const { scratch, result, failureMessage } = runSetupFailure(t, { failOnWrite: 1 });
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, new RegExp(failureMessage));
  for (const file of generated) assert.equal(existsSync(join(scratch, file)), false, file);
});

test('a pre-existing conflicting preview file survives EEXIST cleanup', (t) => {
  const { scratch, result } = runSetupFailure(t, { conflict: true });
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /EEXIST/);
  assert.equal(readFileSync(join(scratch, generated[1]), 'utf8'), '// pre-existing fixture');
  assert.equal(existsSync(join(scratch, generated[0])), false);
  assert.equal(existsSync(join(scratch, generated[2])), false);
  assert.equal(existsSync(join(scratch, generated[3])), false);
});
