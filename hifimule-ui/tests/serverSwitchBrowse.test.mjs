import assert from 'node:assert/strict';
import { test } from 'node:test';
import { build } from 'esbuild';

const bundle = await build({
    entryPoints: [new URL('../src/rpc.ts', import.meta.url).pathname],
    bundle: true,
    platform: 'node',
    format: 'esm',
    write: false,
    plugins: [{
        name: 'test-rpc-dependencies',
        setup(build) {
            build.onResolve({ filter: /^@tauri-apps\/api\/core$/ }, () => ({ path: 'tauri', namespace: 'test' }));
            build.onResolve({ filter: /^\.\/i18n$/ }, () => ({ path: 'i18n', namespace: 'test' }));
            build.onLoad({ filter: /.*/, namespace: 'test' }, ({ path }) => ({
                contents: path === 'tauri'
                    ? 'export const invoke = (...args) => globalThis.testInvoke(...args);'
                    : 'export const t = key => key;',
                loader: 'js',
            }));
        },
    }],
});
const rpc = await import(`data:text/javascript;base64,${Buffer.from(bundle.outputFiles[0].contents).toString('base64')}`);

function deferred() {
    let resolve;
    let reject;
    const promise = new Promise((accept, fail) => { resolve = accept; reject = fail; });
    return { promise, resolve, reject };
}

test('old browse success cannot overwrite the newly selected server', async () => {
    const oldBrowse = deferred();
    globalThis.testInvoke = (_command, { method }) => method === 'browse.listArtists'
        ? oldBrowse.promise
        : Promise.resolve({ ok: true });

    const pending = rpc.rpcCall('browse.listArtists');
    await rpc.serverSelect('new-server');
    oldBrowse.resolve({ artists: [{ name: 'old server' }] });

    await assert.rejects(pending, rpc.StaleBrowseResponse);
    globalThis.testInvoke = () => Promise.resolve({ artists: [{ name: 'new server' }] });
    assert.deepEqual(await rpc.rpcCall('browse.listArtists'), { artists: [{ name: 'new server' }] });
});

test('old browse errors do not trigger reauthentication for the new server', async () => {
    const oldBrowse = deferred();
    let reauthEvents = 0;
    globalThis.window = { dispatchEvent: () => { reauthEvents++; } };
    globalThis.testInvoke = (_command, { method }) => method === 'browse.listArtists'
        ? oldBrowse.promise
        : Promise.resolve({ ok: true });

    const pending = rpc.rpcCall('browse.listArtists');
    await rpc.serverSelect('another-server');
    oldBrowse.reject({ code: rpc.ERR_UNAUTHORIZED, message: 'old credential expired' });

    await assert.rejects(pending, rpc.StaleBrowseResponse);
    assert.equal(reauthEvents, 0);
});

test('failed server selection leaves the current browse requests valid', async () => {
    const browse = deferred();
    globalThis.testInvoke = (_command, { method }) => method === 'server.select'
        ? Promise.reject({ code: -4, message: 'server unavailable' })
        : browse.promise;

    const pending = rpc.rpcCall('browse.listArtists');
    await assert.rejects(rpc.serverSelect('unavailable'));
    browse.resolve({ artists: [{ name: 'current server' }] });

    assert.deepEqual(await pending, { artists: [{ name: 'current server' }] });
});
