import assert from 'node:assert/strict';
import test from 'node:test';
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { encodingDependencyReceipt } from '../encoding-dependencies.mjs';
const source = readFileSync(new URL('../encoding-dependencies.mjs',import.meta.url),'utf8');
test('encoding dependencies pin every archive hash and retain LGPL-compatible distribution',()=>{
  assert.deepEqual(encodingDependencyReceipt.map(d=>d.name),['lame','ogg','vorbis','opus']);
  for(const dependency of encodingDependencyReceipt) { assert.match(dependency.sha256,/^[0-9a-f]{64}$/);assert.match(dependency.url,/^https:/);assert.ok(['LGPL-2.0-or-later','BSD-3-Clause'].includes(dependency.license)); }
});
test('Windows provisions MSVC static libraries and explicit pkg-config metadata',()=>{
  assert.match(source,/-DBUILD_SHARED_LIBS=OFF/);assert.match(source,/-DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreadedDLL/);
  assert.match(source,/CMake is required/);assert.match(source,/add_library\(mp3lame STATIC/);assert.match(source,/'vorbisenc', 'vorbis', '-lvorbisenc'/);assert.match(source,/Requires.private:/);assert.match(source,/Cflags: -I/);
});
test('all platform receipts and builders include the pinned dependencies',()=>{
  for(const platform of ['macos','linux','windows']) {const script=readFileSync(new URL(`../${platform}-audio-runtime.mjs`,import.meta.url),'utf8');assert.match(script,/encodingDependencies: encodingDependencyReceipt/);assert.match(script,/buildEncodingDependencies\(/);}
});
test('installer notice includes the full dependency licenses',()=>{
 const notice=readFileSync(new URL('../../hifimule-daemon/THIRD_PARTY_AUDIO_NOTICES.md',import.meta.url),'utf8');
 for(const dependency of encodingDependencyReceipt) assert.ok(notice.includes(`${dependency.name} ${dependency.version} license`));
 assert.match(notice,/Redistribution and use/);assert.match(notice,/GNU LIBRARY GENERAL PUBLIC LICENSE/);
});

for (const windows of [false, true]) {
  test(`dependency extraction uses a local archive name (${windows ? 'Windows' : 'Unix'})`, async t => {
    const fixture = mkdtempSync(join(tmpdir(), 'hifimule-encoding-extract-'));
    t.after(() => rmSync(fixture, { recursive: true, force: true }));
    const scripts = join(fixture, 'scripts');
    const daemon = join(fixture, 'hifimule-daemon');
    const cache = join(fixture, 'source cache');
    const work = join(fixture, 'source work');
    const prefix = join(fixture, 'install prefix');
    const input = join(fixture, 'input');
    const dep = encodingDependencyReceipt[0];
    for (const directory of [scripts, daemon, cache, work, join(input, dep.directory)]) mkdirSync(directory, { recursive: true });
    writeFileSync(join(input, dep.directory, 'configMS.h'), '/* fixture config */');
    writeFileSync(join(input, dep.directory, 'COPYING'), 'fixture license');
    execFileSync('tar', ['-czf', dep.archive, '-C', input, dep.directory], { cwd: cache });
    const sha256 = createHash('sha256').update(readFileSync(join(cache, dep.archive))).digest('hex');
    writeFileSync(join(daemon, 'audio-runtime.json'), JSON.stringify({ encodingDependencies: [{ ...dep, sha256 }] }));
    copyFileSync(new URL('../encoding-dependencies.mjs', import.meta.url), join(scripts, 'encoding-dependencies.mjs'));
    const { buildEncodingDependencies } = await import(pathToFileURL(join(scripts, 'encoding-dependencies.mjs')).href);
    let extractions = 0;
    buildEncodingDependencies({
      cache, work, prefix, windows, target: windows ? 'x86_64-pc-windows-msvc' : 'x86_64-unknown-linux-gnu',
      execute(command, args, options) {
        if (command === 'tar' || command === 'tar.exe') {
          extractions += 1;
          const archive = args[args.indexOf('-xf') + 1];
          // GNU tar treats a drive colon in -f as remote; require a local basename.
          assert.equal(archive, dep.archive);
          assert.equal(resolve(options.cwd, archive), join(cache, dep.archive));
          assert.equal(resolve(options.cwd, args[args.indexOf('-C') + 1]), work);
          return execFileSync('tar', args, options);
        }
        assert.ok(command === 'cmake' || command === './configure' || command === 'make', `Unexpected command: ${command}`);
      },
    });
    assert.equal(extractions, 1);
    assert.equal(readFileSync(join(prefix, 'share/licenses/lame/COPYING'), 'utf8'), 'fixture license');
  });
}
