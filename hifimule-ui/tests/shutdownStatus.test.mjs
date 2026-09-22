import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import ts from 'typescript';
import { runInNewContext } from 'node:vm';

const source = await readFile(new URL('../src/shutdownStatus.ts', import.meta.url), 'utf8');
const js = ts.transpileModule(source, {
    compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ES2022 },
}).outputText;
const { shutdownMessageKey, ShutdownPollGate, ShutdownPoller, canRetryQuit, canRetryCheckpoint } = await import(
    `data:text/javascript;base64,${Buffer.from(js).toString('base64')}`
);

test('deadline state takes precedence over ordinary progress', () => {
    assert.equal(shutdownMessageKey({ deadlineExceeded: true }, null), 'lifecycle.shutdown_delayed');
    assert.equal(shutdownMessageKey({ deadlineExceeded: false }, 'SHUTDOWN_TIMEOUT'), 'lifecycle.shutdown_delayed');
    assert.equal(shutdownMessageKey({ deadlineExceeded: false }, null), 'lifecycle.shutdown_progress');
    assert.equal(
        shutdownMessageKey({ deadlineExceeded: false }, 'QUIT_PERSISTENCE_FAILED'),
        'lifecycle.quit_persistence_failed',
    );
});

test('shutdown polling permits only one request at a time', async () => {
    let release;
    let calls = 0;
    const gate = new ShutdownPollGate(async () => {
        calls += 1;
        await new Promise(resolve => { release = resolve; });
    });
    const first = gate.poll();
    await gate.poll();
    assert.equal(calls, 1);
    release();
    await first;
    const second = gate.poll();
    assert.equal(calls, 2);
    release();
    await second;
});

function clock() {
    let now = 0, id = 0;
    const timers = new Map();
    return { timers, now: () => now,
        schedule: (fn, delay) => { timers.set(++id, { fn, at: now + delay }); return id; },
        cancel: id => timers.delete(id),
        async advance(ms) { now += ms; for (const [id, timer] of [...timers]) {
            if (timer.at <= now) { timers.delete(id); timer.fn(); }
        } await Promise.resolve(); await Promise.resolve(); },
    };
}

test('default timers retain the browser Window receiver', () => {
    const commonJs = ts.transpileModule(source, {
        compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
    }).outputText;
    const context = { exports: {}, assert };
    runInNewContext(`
        let scheduled = 0;
        let cancelled = 0;
        globalThis.performance = { now: () => 0 };
        globalThis.setTimeout = function (callback, delay) {
            assert.equal(this, globalThis, 'setTimeout requires the Window receiver');
            assert.equal(delay, 1000);
            scheduled++;
            return 42;
        };
        globalThis.clearTimeout = function (id) {
            assert.equal(this, globalThis, 'clearTimeout requires the Window receiver');
            assert.equal(id, 42);
            cancelled++;
        };
        ${commonJs}
        const poller = new exports.ShutdownPoller(async () => {});
        poller.refresh();
        assert.equal(scheduled, 1);
        poller.dispose();
        assert.equal(cancelled, 1);
    `, context);
});

test('refresh spam retains one timer and at most one request each second', async () => {
    const time = clock(); let calls = 0;
    const poller = new ShutdownPoller(async () => { calls++; }, time.now, time.schedule, time.cancel);
    for (let i = 0; i < 30; i++) poller.refresh();
    assert.equal(time.timers.size, 1);
    await time.advance(999); assert.equal(calls, 0);
    await time.advance(1); assert.equal(calls, 1);
    for (let i = 0; i < 30; i++) poller.refresh();
    await time.advance(999); assert.equal(calls, 1);
    await time.advance(1); assert.equal(calls, 2);
    assert.equal(time.timers.size, 1);
    poller.dispose(); assert.equal(time.timers.size, 0);
});

test('disposing during a blocked request prevents rescheduling after completion', async () => {
    const time = clock(); let release; let calls = 0;
    const poller = new ShutdownPoller(async () => { calls++; await new Promise(r => { release = r; }); }, time.now, time.schedule, time.cancel);
    await time.advance(1_000);
    poller.refresh(); assert.equal(time.timers.size, 0);
    poller.dispose(); release(); await Promise.resolve(); await Promise.resolve();
    poller.refresh(); await time.advance(5_000);
    assert.equal(calls, 1); assert.equal(time.timers.size, 0);
});

test('Retry Quit and continue are available only after completed fence failure', () => {
    assert.equal(canRetryQuit({ shutdown: { phase: 'fenceFailed' }, errorCode: 'QUIT_PERSISTENCE_FAILED' }), true);
    for (const phase of ['fencing', 'cancelling', 'draining', 'waiting', 'finalizing']) {
        assert.equal(canRetryQuit({ shutdown: { phase }, errorCode: 'QUIT_PERSISTENCE_FAILED' }), false);
    }
    assert.equal(canRetryQuit({ shutdown: { phase: 'fenceFailed' } }), false);
});


test('rendered failed fence retries the same daemon and Continue only reloads the UI', async () => {
    const main = await readFile(new URL('../src/main.ts', import.meta.url), 'utf8');
    const renderSource = main.slice(main.indexOf('function renderShutdownStatus('), main.indexOf('async function showMainWindow'));
    const renderJs = ts.transpileModule(renderSource, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText;
    const elements = new Map();
    const element = id => {
        if (!elements.has(id)) elements.set(id, { hidden: false, disabled: false, textContent: '', handlers: {},
            addEventListener(event, fn) { this.handlers[event] = fn; }, focus() {} });
        return elements.get(id);
    };
    const document = { body: { dataset: {}, innerHTML: '' }, getElementById: element };
    let reloads = 0; const window = { addEventListener() {}, location: { reload() { reloads++; } } };
    const stored = new Map(); const sessionStorage = { setItem: (key, value) => stored.set(key, value) };
    let disposed = false;
    let latestPoller;
    class Poller { constructor(request) { this.request = request; latestPoller = this; } refresh() {} dispose() { disposed = true; } }
    const render = new Function('document', 'window', 'sessionStorage', 't', 'ShutdownPoller', 'shutdownMessageKey', 'canRetryQuit', 'canRetryCheckpoint', 'disposePlaybackControls',
        'let activeBasketSidebar = null; ' + renderJs + '; return renderShutdownStatus;')(
        document, window, sessionStorage, key => key, Poller, shutdownMessageKey, canRetryQuit, canRetryCheckpoint, () => {});
    const calls = [];
    const health = { status: 'ok', errorCode: 'QUIT_PERSISTENCE_FAILED', shutdown: {
        shutdownId: 'failed-1', phase: 'fenceFailed', deadlineExceeded: false, activeOperationCount: 0, pendingMutationCount: 0 } };
    render(async method => { calls.push(method); return { data: { accepted: true } }; }, health);
    assert.equal(element('shutdown-retry').hidden, false);
    assert.equal(element('shutdown-continue').hidden, false);
    await element('shutdown-retry').handlers.click();
    assert.deepEqual(calls, ['daemon.retryQuit']);
    assert.equal(element('shutdown-retry').hidden, true);
    render(async () => {}, health);
    element('shutdown-continue').handlers.click();
    assert.equal(stored.get('dismissedQuit'), 'failed-1');
    assert.equal(reloads, 1); assert.equal(disposed, true);

    const checkpoint = { status: 'stopping', instanceId: 'owner-1', errorCode: 'PLAYBACK_CHECKPOINT_FAILED', shutdown: {
        shutdownId: 'quit-1', phase: 'waiting', sessionCheckpoint: 'failed', deadlineExceeded: true, activeOperationCount: 0, pendingMutationCount: 0 } };
    const retries = [];
    render(async (method, params) => {
        if (method === 'daemon.health') return { data: { ...checkpoint, instanceId: 'replacement-owner' } };
        retries.push([method, params]); return { data: { accepted: true } };
    }, checkpoint);
    assert.equal(element('shutdown-retry').hidden, false);
    assert.equal(element('shutdown-retry').textContent, 'lifecycle.retry_saving_session');
    assert.equal(element('shutdown-status').textContent, 'lifecycle.playback_checkpoint_failed');
    assert.equal(element('shutdown-continue').hidden, true);
    assert.match(document.body.innerHTML, /role="status" aria-live="polite"/);
    await latestPoller.request(); // A stale replacement response must not rebind the action.
    await element('shutdown-retry').handlers.click();
    assert.deepEqual(retries, [['playback.retryCheckpoint', { schemaVersion: 1, instanceId: 'owner-1', shutdownId: 'quit-1' }]]);
    assert.equal(element('shutdown-retry').hidden, true);
    assert.equal(element('shutdown-continue').hidden, true);
    element('shutdown-continue').handlers.click();
    assert.equal(reloads, 1, 'committed shutdown must not allow Continue');
});

test('checkpoint failure takes precedence over timeouts and only completed failure enables retry', () => {
    assert.equal(shutdownMessageKey({ deadlineExceeded: true, sessionCheckpoint: 'failed' }, 'SHUTDOWN_TIMEOUT'), 'lifecycle.playback_checkpoint_failed');
    assert.equal(shutdownMessageKey({ deadlineExceeded: true, sessionCheckpoint: 'pending' }, 'SHUTDOWN_TIMEOUT'), 'lifecycle.playback_checkpoint_pending');
    for (const sessionCheckpoint of ['notRequired', 'pending', 'succeeded']) {
        assert.equal(canRetryCheckpoint({ status: 'stopping', instanceId: 'owner', shutdown: { shutdownId: 'quit', sessionCheckpoint } }), false);
    }
    assert.equal(canRetryCheckpoint({ status: 'stopping', instanceId: 'owner', shutdown: { shutdownId: 'quit', sessionCheckpoint: 'failed' } }), true);
    assert.equal(canRetryCheckpoint({ status: 'ok', instanceId: 'owner', shutdown: { shutdownId: 'quit', sessionCheckpoint: 'failed' } }), false);
});
