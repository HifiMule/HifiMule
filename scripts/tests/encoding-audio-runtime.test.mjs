import assert from 'node:assert/strict';
import test from 'node:test';
import { readFileSync } from 'node:fs';
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
