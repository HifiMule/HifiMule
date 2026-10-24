import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import { pathToFileURL } from 'node:url';

test('codec download recovers from an interrupted transfer and only caches verified bytes', { timeout: 20000 }, async t => {
  const fixture = mkdtempSync(join(tmpdir(), 'hifimule-download-'));
  t.after(() => rmSync(fixture, { recursive: true, force: true }));
  const payload = Buffer.from('complete source archive fixture');
  let requests = 0;
  const server = createServer((_request, response) => {
    requests += 1;
    response.writeHead(200, { 'Content-Length': payload.length, Connection: 'close' });
    // A clean close with a short body produces curl error 18, which --retry
    // alone does not retry (like the TLS error 35 seen in Windows CI).
    response.end(requests === 1 ? payload.subarray(0, 5) : payload);
  });
  t.after(() => new Promise(resolve => server.close(resolve)));
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  for (const name of ['scripts', 'hifimule-daemon', 'cache', 'work']) mkdirSync(join(fixture, name));
  const modulePath = join(fixture, 'scripts/encoding-dependencies.mjs');
  copyFileSync(new URL('../encoding-dependencies.mjs', import.meta.url), modulePath);
  const manifestPath = join(fixture, 'hifimule-daemon/audio-runtime.json');
  const dependency = {
    name: 'opus', version: 'fixture', archive: 'opus.tar.gz', directory: 'opus',
    sha256: createHash('sha256').update(payload).digest('hex'),
    url: `http://127.0.0.1:${server.address().port}/opus.tar.gz`,
  };
  writeFileSync(manifestPath, JSON.stringify({ encodingDependencies: [dependency] }));
  // Run the synchronous builder in a child so this test can serve curl requests.
  const program = `
    import { buildEncodingDependencies } from ${JSON.stringify(pathToFileURL(modulePath).href)};
    import { execFileSync } from 'node:child_process';
    import { mkdirSync, writeFileSync } from 'node:fs';
    import { join } from 'node:path';
    const root = process.argv[1];
    buildEncodingDependencies({
      cache: join(root, 'cache'), work: join(root, 'work'), prefix: join(root, 'prefix'),
      windows: true, target: 'x86_64-pc-windows-msvc',
      execute(command, args, options) {
        if (command === 'curl.exe') return execFileSync(process.platform === 'win32' ? command : 'curl', args, options);
        if (command.endsWith('tar.exe')) {
          mkdirSync(join(root, 'work/opus'), { recursive: true });
          writeFileSync(join(root, 'work/opus/COPYING'), 'fixture license');
        }
      },
    });
  `;
  async function build() {
    const child = spawn(process.execPath, ['--input-type=module', '-e', program, fixture]);
    t.after(() => { if (child.exitCode === null) child.kill(); });
    let output = '';
    child.stdout.on('data', data => { output += data; });
    child.stderr.on('data', data => { output += data; });
    const code = await new Promise((resolve, reject) => {
      child.on('error', reject);
      child.on('close', resolve);
    });
    return { code, output };
  }
  const recovered = await build();
  assert.equal(recovered.code, 0, recovered.output);
  assert.equal(requests, 2);
  assert.deepEqual(readFileSync(join(fixture, 'cache/opus.tar.gz')), payload);
  const cached = await build();
  assert.equal(cached.code, 0, cached.output);
  assert.equal(requests, 2, 'verified cache avoids another network request');

  rmSync(join(fixture, 'cache/opus.tar.gz'));
  dependency.sha256 = '0'.repeat(64);
  writeFileSync(manifestPath, JSON.stringify({ encodingDependencies: [dependency] }));
  const rejected = await build();
  assert.notEqual(rejected.code, 0);
  assert.match(rejected.output, /source checksum mismatch/);
  assert.deepEqual(readdirSync(join(fixture, 'cache')), [], 'unverified and temporary files must not survive');
});
