import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import ts from 'typescript';

// Transpile the production class so tests exercise the real polling and click handlers.
const source = readFileSync(new URL('../src/components/BasketSidebar.ts', import.meta.url), 'utf8')
    .replace(/^import .*;\r?\n/gm, '').replace(/export class /g, 'class ');
const script = ts.transpile(source, { target: ts.ScriptTarget.ES2022 });
const running = { status: 'running', filesCompleted: 1, filesTotal: 10, errors: [] };
const deferred = () => {
    let resolve, reject;
    const promise = new Promise((done, fail) => { resolve = done; reject = fail; });
    return { promise, resolve, reject };
};
const settle = async () => { for (let i = 0; i < 8; i++) await Promise.resolve(); };

function harness() {
    let now = 0, wallClockOffset = 0, nextTimer = 1, response = () => { throw new Error('offline'); };
    const timers = new Map(), calls = [], progress = [], terminals = [], announcements = [], errors = [];
    const document = { body: {}, activeElement: null };
    let retry = null;
    const container = {
        querySelector: selector => selector === '#sync-status-retry' ? retry : null,
        contains: () => false,
        removeEventListener() {},
        set innerHTML(value) {
            assert.match(value, /sync-status-retry/);
            const attributes = new Map(), listeners = new Map();
            retry = {
                textContent: 'basket.sync.check_status',
                setAttribute: (key, value) => attributes.set(key, value),
                getAttribute: key => attributes.get(key) ?? null,
                removeAttribute: key => attributes.delete(key),
                addEventListener: (event, callback) => listeners.set(event, callback),
                click: () => listeners.get('click')(),
            };
        },
    };
    const clearInterval = id => timers.delete(id);
    const window = { setInterval(callback, delay) {
        const id = nextTimer++;
        timers.set(id, { callback, delay, next: now + delay });
        return id;
    } };
    class ClockDate extends Date { static now() { return 1700000000000 + now + wallClockOffset; } }
    const performance = { now: () => now };
    const rpcCall = (method, params) => {
        assert.equal(method, 'sync_get_operation_status');
        calls.push({ at: now, operationId: params.operationId });
        return response();
    };
    const { BasketSidebar } = new Function('window', 'clearInterval', 'Date', 'performance', 'rpcCall', 't', 'console', 'basketStore', 'document',
        `${script}\nreturn { BasketSidebar };`)(window, clearInterval, ClockDate, performance, rpcCall, key => key,
        { error: (...args) => errors.push(args) }, { removeEventListener() {} }, document);
    // Existing sidebar fixtures also bypass field initializers; keep this regression covered.
    const sidebar = Object.create(BasketSidebar.prototype);
    Object.assign(sidebar, {
        container, currentOperationId: 'operation-1', currentOperation: running, isSyncing: true,
        announceSync: message => announcements.push(message),
        render() { this.renderSyncUnavailable(); },
        renderSyncProgress() { progress.push(this.currentOperation); retry = null; },
        async handleSyncComplete() { terminals.push('complete'); },
        handleSyncFailed(op) { terminals.push(op.status); },
        handleSyncCancelled() { terminals.push('cancelled'); },
    });
    return {
        sidebar, calls, progress, terminals, announcements, errors, timers,
        get retry() { return retry; },
        get wallClock() { return ClockDate.now(); },
        jumpWallClock(milliseconds) { wallClockOffset += milliseconds; },
        respond(callback) { response = callback; },
        async advance(milliseconds) {
            const until = now + milliseconds;
            while (true) {
                const due = [...timers.values()].filter(timer => timer.next <= until).sort((a, b) => a.next - b.next)[0];
                if (!due) break;
                now = due.next;
                due.next += due.delay;
                void due.callback();
                await settle();
            }
            now = until;
            await settle();
        },
    };
}

async function unavailable(h) {
    h.sidebar.startPolling();
    await h.advance(1500);
    assert.equal(h.sidebar.syncMonitoringUnavailable, true);
    assert.equal(h.calls.length, 3);
    assert.equal(h.timers.size, 1);
}

test('three errors preserve the tracked sync and recover automatically at the same 500 ms cadence', async () => {
    const h = harness();
    await unavailable(h);
    const retry = h.retry;
    assert.equal(h.sidebar.isSyncing, true);
    assert.equal(h.sidebar.currentOperationId, 'operation-1');
    assert.equal(h.sidebar.currentOperation, running);
    assert.equal(h.announcements.at(-1), 'basket.sync.status_unavailable_help');
    h.respond(() => running);
    await h.advance(1999);
    assert.equal(h.calls.length, 3);
    assert.equal(h.retry, retry);
    await h.advance(1);
    assert.equal(h.sidebar.syncMonitoringUnavailable, false);
    assert.equal(h.progress.length, 1);
    assert.equal(h.announcements.at(-1), 'basket.sync.status_restored');
    await h.advance(500);
    assert.deepEqual(h.calls.map(call => call.at), [500, 1000, 1500, 3500, 4000]);
    assert.ok(h.calls.every(call => call.operationId === 'operation-1'));
});

test('unavailable checks back off at 2/4/8/16/30 seconds, stay capped, and success resets failures', async () => {
    const h = harness();
    h.sidebar.startPolling();
    await h.advance(91500);
    assert.deepEqual(h.calls.map(call => call.at), [500, 1000, 1500, 3500, 7500, 15500, 31500, 61500, 91500]);
    const retry = h.retry;
    h.respond(() => running);
    await h.advance(30000);
    assert.equal(h.calls.at(-1).at, 121500);
    h.respond(() => { throw new Error('offline again'); });
    await h.advance(1500);
    assert.equal(h.sidebar.syncMonitoringUnavailable, true);
    assert.notEqual(h.retry, retry);
    assert.deepEqual(h.calls.slice(-3).map(call => call.at), [122000, 122500, 123000]);
    await h.advance(2000);
    assert.equal(h.calls.at(-1).at, 125000);
});

test('a backward wall-clock adjustment cannot postpone automatic recovery', async () => {
    const h = harness();
    await unavailable(h);
    const before = h.wallClock;
    h.jumpWallClock(-24 * 60 * 60 * 1000);
    assert.ok(h.wallClock < before);
    h.respond(() => running);
    await h.advance(1999);
    assert.equal(h.calls.length, 3);
    await h.advance(1);
    assert.equal(h.calls.at(-1).at, 3500);
    assert.equal(h.sidebar.syncMonitoringUnavailable, false);
    await h.advance(500);
    assert.equal(h.calls.at(-1).at, 4000);
});

test('manual checking bypasses backoff in the existing session, prevents overlap, and re-enables on failure', async () => {
    const h = harness();
    await unavailable(h);
    await h.advance(250);
    const retry = h.retry, revision = h.sidebar.syncPollRevision, timer = h.sidebar.pollingInterval;
    const pending = deferred();
    h.respond(() => pending.promise);
    retry.click();
    assert.equal(h.calls.at(-1).at, 1750, 'manual check starts immediately');
    assert.equal(retry.getAttribute('aria-busy'), 'true');
    assert.equal(retry.getAttribute('aria-disabled'), 'true');
    retry.click();
    await h.advance(30000);
    assert.equal(h.calls.length, 4, 'ticks and repeat clicks share the pending check');
    assert.equal(h.sidebar.syncPollRevision, revision);
    assert.equal(h.sidebar.pollingInterval, timer);
    pending.reject(new Error('still offline'));
    await settle();
    assert.equal(h.retry, retry, 'failed checks keep the focused button mounted');
    assert.equal(retry.getAttribute('aria-busy'), null);
    assert.equal(retry.getAttribute('aria-disabled'), null);
    assert.equal(retry.textContent, 'basket.sync.check_status');
    h.respond(() => running);
    retry.click();
    await settle();
    assert.equal(h.calls.length, 5);
    assert.equal(h.sidebar.syncMonitoringUnavailable, false);
});

test('a manual action during an automatic request shares its response without another request', async () => {
    const h = harness();
    await unavailable(h);
    const pending = deferred();
    h.respond(() => pending.promise);
    await h.advance(2000);
    const retry = h.retry;
    retry.click();
    assert.equal(h.calls.length, 4);
    assert.equal(retry.getAttribute('aria-busy'), 'true');
    await h.advance(3000);
    assert.equal(h.calls.length, 4);
    pending.reject(new Error('offline'));
    await settle();
    assert.equal(retry.getAttribute('aria-busy'), null);
    h.respond(() => running);
    await h.advance(4000);
    assert.equal(h.sidebar.syncMonitoringUnavailable, false);
    assert.equal(h.calls.length, 5);
});

for (const action of ['stop', 'destroy', 'change operation']) {
    for (const outcome of ['resolve', 'reject']) {
        test(`${action} suppresses late ${outcome} and future requests from the old session`, async () => {
            const h = harness(), pending = deferred();
            h.respond(() => pending.promise);
            h.sidebar.startPolling();
            await h.advance(500);
            const oldCheck = h.sidebar.requestSyncStatusCheck;
            if (action === 'stop') h.sidebar.stopPolling();
            if (action === 'destroy') h.sidebar.destroy();
            if (action === 'change operation') h.sidebar.currentOperationId = 'operation-2';
            if (outcome === 'resolve') pending.resolve({ ...running, status: 'failed' });
            else pending.reject(new Error('late failure'));
            await settle();
            await oldCheck();
            await h.advance(60000);
            assert.equal(h.calls.length, 1);
            assert.equal(h.progress.length, 0);
            assert.equal(h.terminals.length, 0);
            assert.equal(h.errors.length, 0);
            assert.equal(h.sidebar.currentOperation, running);
            assert.equal(h.sidebar.requestSyncStatusCheck, null);
            assert.equal(h.timers.size, 0);
        });
    }
}

test('a late old response cannot affect a replacement operation session', async () => {
    const h = harness(), old = deferred(), replacement = deferred();
    h.respond(() => old.promise);
    h.sidebar.startPolling();
    await h.advance(500);
    const oldCheck = h.sidebar.requestSyncStatusCheck;
    h.sidebar.currentOperationId = 'operation-2';
    h.respond(() => replacement.promise);
    h.sidebar.startPolling();
    await h.advance(500);
    const currentCheck = h.sidebar.requestSyncStatusCheck;
    old.reject(new Error('stale'));
    await settle();
    await oldCheck();
    assert.equal(h.sidebar.requestSyncStatusCheck, currentCheck);
    assert.equal(h.timers.size, 1);
    assert.equal(h.errors.length, 0);
    replacement.resolve({ ...running, filesCompleted: 2 });
    await settle();
    assert.equal(h.progress.length, 1);
    assert.equal(h.sidebar.currentOperation.filesCompleted, 2);
    assert.deepEqual(h.calls.map(call => call.operationId), ['operation-1', 'operation-2']);
});

for (const status of ['complete', 'failed', 'cancelled']) {
    test(`automatic recovery reconciles ${status} once and stops polling`, async () => {
        const h = harness();
        await unavailable(h);
        const staleCheck = h.sidebar.requestSyncStatusCheck;
        h.respond(() => ({ ...running, status }));
        await h.advance(2000);
        await staleCheck();
        await h.advance(60000);
        assert.deepEqual(h.terminals, [status]);
        assert.equal(h.calls.length, 4);
        assert.equal(h.sidebar.requestSyncStatusCheck, null);
        assert.equal(h.timers.size, 0);
        assert.equal(h.sidebar.syncMonitoringUnavailable, false);
    });
}

for (const action of ['stop', 'destroy', 'change operation']) {
    test(`${action} clears scheduled unavailable retries`, async () => {
        const h = harness();
        await unavailable(h);
        if (action === 'stop') h.sidebar.stopPolling();
        if (action === 'destroy') h.sidebar.destroy();
        if (action === 'change operation') h.sidebar.currentOperationId = 'operation-2';
        h.respond(() => running);
        await h.advance(60000);
        h.retry.click();
        await settle();
        assert.equal(h.calls.length, 3);
        assert.equal(h.retry.getAttribute('aria-busy'), null);
        assert.equal(h.sidebar.requestSyncStatusCheck, null);
        assert.equal(h.timers.size, 0);
    });
}
