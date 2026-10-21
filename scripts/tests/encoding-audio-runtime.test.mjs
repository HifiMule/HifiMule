import assert from 'node:assert/strict';
import test from 'node:test';
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { tmpdir } from 'node:os';
import { join, resolve, win32 } from 'node:path';
import { pathToFileURL } from 'node:url';
import { encodingDependencyReceipt, windowsTarExecutable } from '../encoding-dependencies.mjs';
const source = readFileSync(new URL('../encoding-dependencies.mjs',import.meta.url),'utf8');
const hostTar = process.platform === 'win32' ? windowsTarExecutable() : 'tar';
test('Windows tar selection ignores MSYS2 PATH order and respects the system directory', () => {
  for (const key of ['SystemRoot', 'SYSTEMROOT', 'systemroot']) {
    assert.equal(windowsTarExecutable({ [key]: 'E:\\Windows Root', PATH: 'D:\\msys64\\usr\\bin;C:\\Windows\\System32' }),
      'E:\\Windows Root\\System32\\tar.exe');
  }
  assert.equal(windowsTarExecutable({}), 'C:\\Windows\\System32\\tar.exe');
});
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
  test(`dependency extraction and package metadata match installed layouts (${windows ? 'Windows' : 'Unix'})`, async t => {
    const fixture = mkdtempSync(join(tmpdir(), 'hifimule-encoding-extract-'));
    t.after(() => rmSync(fixture, { recursive: true, force: true }));
    const scripts = join(fixture, 'scripts');
    const daemon = join(fixture, 'hifimule-daemon');
    const cache = join(fixture, 'source cache');
    const work = join(fixture, 'source work');
    const prefix = join(fixture, 'install prefix');
    const input = join(fixture, 'input');
    const dependencies = windows ? encodingDependencyReceipt : encodingDependencyReceipt.slice(0, 1);
    for (const directory of [scripts, daemon, cache, work]) mkdirSync(directory, { recursive: true });
    const fixtures = dependencies.map(dep => {
      mkdirSync(join(input, dep.directory), { recursive: true });
      writeFileSync(join(input, dep.directory, 'configMS.h'), '/* fixture config */');
      writeFileSync(join(input, dep.directory, 'COPYING'), 'fixture license');
      execFileSync(hostTar, [dep.archive.endsWith('.xz') ? '-cJf' : '-czf', dep.archive, '-C', input, dep.directory], { cwd: cache });
      const sha256 = createHash('sha256').update(readFileSync(join(cache, dep.archive))).digest('hex');
      return { ...dep, sha256 };
    });
    writeFileSync(join(daemon, 'audio-runtime.json'), JSON.stringify({ encodingDependencies: fixtures }));
    copyFileSync(new URL('../encoding-dependencies.mjs', import.meta.url), join(scripts, 'encoding-dependencies.mjs'));
    const { buildEncodingDependencies } = await import(pathToFileURL(join(scripts, 'encoding-dependencies.mjs')).href);
    let extractions = 0;
    buildEncodingDependencies({
      cache, work, prefix, windows, target: windows ? 'x86_64-pc-windows-msvc' : 'x86_64-unknown-linux-gnu',
      execute(command, args, options) {
        if (command === 'tar' || command.endsWith('tar.exe')) {
          extractions += 1;
          const systemRoot = Object.entries(process.env).find(([key]) => key.toUpperCase() === 'SYSTEMROOT')?.[1] ?? 'C:\\Windows';
          assert.equal(command, windows ? win32.join(systemRoot, 'System32', 'tar.exe') : 'tar',
            'Windows extraction must bypass MSYS2 tar on PATH');
          const archive = args[args.indexOf('-xf') + 1];
          // GNU tar treats a drive colon in -f as remote; require a local basename.
          assert.ok(dependencies.some(dep => dep.archive === archive));
          assert.equal(resolve(options.cwd, archive), join(cache, archive));
          assert.equal(resolve(options.cwd, args[args.indexOf('-C') + 1]), work);
          // Exercise the actual native executable on Windows; use the host tar elsewhere.
          return execFileSync(windows && process.platform === 'win32' ? command : hostTar, args, options);
        }
        assert.ok(command === 'cmake' || command === './configure' || command === 'make', `Unexpected command: ${command}`);
      },
    });
    assert.equal(extractions, dependencies.length);
    for (const dep of dependencies) {
      assert.equal(readFileSync(join(prefix, 'share/licenses', dep.name, 'COPYING'), 'utf8'), 'fixture license');
    }
    if (windows) {
      for (const name of ['lame', 'ogg', 'vorbis', 'vorbisenc', 'opus']) {
        const metadata = readFileSync(join(prefix, 'lib/pkgconfig', `${name}.pc`), 'utf8');
        const includeFlags = metadata.split('\n').find(line => line.startsWith('Cflags:'));
        // FFmpeg includes opus_multistream.h directly; the other APIs use
        // package-qualified headers such as vorbis/vorbisenc.h and lame/lame.h.
        assert.equal(includeFlags, name === 'opus' ? 'Cflags: -I${includedir}/opus' : 'Cflags: -I${includedir}');
      }
    }
  });
}
