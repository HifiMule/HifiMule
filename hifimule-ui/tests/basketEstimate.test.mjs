import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import ts from 'typescript';

const source = readFileSync(new URL('../src/state/basketEstimate.ts', import.meta.url), 'utf8')
    .replace(/^import .*;\r?\n/gm, '').replace(/export /g, '');
const script = ts.transpile(source, { target: ts.ScriptTarget.ES2022 });
const { BasketEstimateCache, basketEstimateKey } = new Function(`${script}\nreturn { BasketEstimateCache, basketEstimateKey };`)();
const selection = [{ id: 'playlist', serverId: 'a', type: 'Playlist', childCount: 91, sizeTicks: 244699607951, sizeBytes: 600000000 }];
const device = { device_id: 'usb', transcoding_profile_id: 'mp3-192', last_sync_at: 1 };
const estimate = { targetDeviceId: 'usb', selectedBytes: 571680976, trackCount: 91, items: [] };
const deferred = () => { let resolve; const promise = new Promise(done => { resolve = done; }); return { promise, resolve }; };

test('polling and metadata repair do not re-expand containers; meaningful inputs invalidate', () => {
    const key = basketEstimateKey(device, 'a', selection);
    assert.equal(basketEstimateKey({ ...device, syncedByteTotals: { selectedBytes: 99 }, freeBytes: 3 }, 'a', [
        { ...selection[0], childCount: 92, sizeTicks: 228850000000, sizeBytes: 1 },
        { id: '__auto_fill_slot__:a', type: 'AutoFillSlot', sizeBytes: 999 },
    ]), key);
    for (const changed of [
        { ...device, device_id: 'other' }, { ...device, transcoding_profile_id: 'flac' },
        { ...device, plannedProfileRevision: 'edited-profile' },
        { ...device, last_sync_at: 2 }, { ...device, transcoding_profile_dirty: true },
        { ...device, reconcileBeforeSync: true },
    ]) assert.notEqual(basketEstimateKey(changed, 'a', selection), key);
    assert.notEqual(basketEstimateKey(device, 'a', selection, true), key);
    assert.notEqual(basketEstimateKey(device, 'b', selection), key);
    assert.notEqual(basketEstimateKey(device, 'a', [{ ...selection[0], id: 'other' }]), key);
    assert.notEqual(basketEstimateKey(device, 'a', [{ ...selection[0], serverId: 'b' }]), key);
});

test('one selection shares pending work and retains its authoritative result across polls', async () => {
    const cache = new BasketEstimateCache();
    const pending = deferred();
    let calls = 0;
    const fetch = () => { calls++; return pending.promise; };
    const one = cache.load('same', fetch);
    const two = cache.load('same', fetch);
    await Promise.resolve();
    assert.equal(calls, 1);
    pending.resolve(estimate);
    assert.equal(await one, estimate);
    assert.equal(await two, estimate);
    assert.equal(await cache.load('same', fetch), estimate);
    assert.equal(calls, 1);
});

test('late basket/profile replies cannot overwrite a newer selection or its pending job', async () => {
    const cache = new BasketEstimateCache();
    const old = deferred(), current = deferred();
    const oldJob = cache.load('mp3-playlist', () => old.promise);
    const currentJob = cache.load('flac-album', () => current.promise);
    old.resolve(estimate);
    assert.equal(await oldJob, undefined);
    assert.equal(cache.get('mp3-playlist'), undefined);
    const replacement = { ...estimate, selectedBytes: 123 };
    current.resolve(replacement);
    assert.equal(await currentJob, replacement);
    assert.equal(cache.get('flac-album'), replacement);
});

test('disconnect/reconnect with the same key rejects the old connection reply', async () => {
    const cache = new BasketEstimateCache();
    const old = deferred(), current = deferred();
    const oldJob = cache.load('same', () => old.promise);
    cache.clear();
    const currentJob = cache.load('same', () => current.promise);
    old.resolve(estimate);
    assert.equal(await oldJob, undefined);
    const replacement = { ...estimate, selectedBytes: 321 };
    current.resolve(replacement);
    assert.equal(await currentJob, replacement);
    assert.equal(cache.get('same'), replacement);
});

test('failed estimates do not hammer providers on the two-second daemon poll', async () => {
    const cache = new BasketEstimateCache();
    let calls = 0;
    const fetch = async () => { calls++; throw new Error('offline'); };
    await assert.rejects(cache.load('same', fetch), /offline/);
    assert.equal(await cache.load('same', fetch), undefined);
    assert.equal(calls, 1);
    await assert.rejects(cache.load('changed', fetch), /offline/);
    assert.equal(calls, 2);
});

test('metadata repair preserves dirty state and does not notify/save while repairing persisted duration', () => {
    const source = readFileSync(new URL('../src/state/basket.ts', import.meta.url), 'utf8')
        .replace(/^import .*;\r?\n/gm, '').replace(/export /g, '')
        .replace('const basketStore = new BasketStore();', '');
    const script = ts.transpile(source, { target: ts.ScriptTarget.ES2022 });
    const { BasketStore } = new Function(`${script}\nreturn { BasketStore };`)();
    const store = Object.create(BasketStore.prototype);
    store.items = new Map([['playlist', { ...selection[0] }]]);
    store.activeServerId = 'a';
    store._dirty = false;
    store.notify = store.saveToLocalStorage = () => assert.fail('observational repair must not notify/save');
    store.applyEstimatedMetadata([{ ...selection[0], sizeTicks: 228850000000, sizeBytes: 594981058, childCount: 91 }]);
    assert.equal(store.items.get('playlist').sizeTicks, 228850000000);
    assert.equal(store._dirty, false);
    store.applyEstimatedMetadata([{ ...selection[0], serverId: 'b', sizeTicks: 1 }]);
    assert.equal(store.items.get('playlist').sizeTicks, 228850000000);
});

function pendingSync(prepared = { planId: 'plan' }) {
    const pending = deferred();
    const calls = [];
    let items = [{ ...selection[0] }, { id: '__auto_fill_slot__:a', serverId: 'a', type: 'AutoFillSlot', sizeBytes: 30 }];
    const basket = {
        getItems: () => items,
        getManualItemIdsForServer: () => items.filter(item => item.type !== 'AutoFillSlot').map(item => item.id),
        applyEstimatedMetadata() {},
    };
    const capacityScript = ts.transpile(readFileSync(new URL('../src/state/syncCapacity.ts', import.meta.url), 'utf8').replace(/export /g, ''), { target: ts.ScriptTarget.ES2022 });
    const { plannedSelectionBytes, plannedSelectionSize, selectionSize, syncCapacity } = new Function(`${capacityScript}\nreturn { plannedSelectionBytes, plannedSelectionSize, selectionSize, syncCapacity };`)();
    const sidebarScript = ts.transpile(readFileSync(new URL('../src/components/BasketSidebar.ts', import.meta.url), 'utf8').replace(/^import .*;\r?\n/gm, '').replace(/export class /g, 'class '), { target: ts.ScriptTarget.ES2022 });
    const rpc = async (method, params) => {
        calls.push({ method, params });
        if (method === 'basket.estimate') return pending.promise;
        if (method === 'sync_prepare') return prepared;
        if (method === 'sync_execute') return { operationId: 'op' };
        if (method === 'sync_plan_discard') return { discarded: true };
        throw new Error(`Unexpected RPC ${method}`);
    };
    const { BasketSidebar } = new Function('basketEstimateKey', 'rpcCall', 'basketStore', 'isAutoFillSlotId', 'plannedSelectionSize', 'selectionSize', 'syncCapacity', 't', `${sidebarScript}\nreturn { BasketSidebar };`)(basketEstimateKey, rpc, basket, id => id.startsWith('__auto_fill_slot__'), plannedSelectionSize, selectionSize, syncCapacity, key => key);
    const sidebar = Object.create(BasketSidebar.prototype);
    Object.assign(sidebar, {
        estimateCache: new BasketEstimateCache(), currentDevice: { ...device }, currentServerId: 'a',
        selectedDevicePath: '/usb', storageInfo: { totalBytes: 1000, freeBytes: 1000, usedBytes: 0 },
        autoFillPipelines: new Map([['a', { budget: {} }]]), render() {}, syncSlotsFromPipelines() {},
        attachToRunningSync: async () => false, startPolling() {}, stopPolling() {},
        showError(message) { assert.fail(message); },
    });
    return { sidebar, calls, pending, changeItems(next) { items = next; } };
}

test('Sync waits for authoritative selection bytes and does not retain a stale slot ceiling', async () => {
    const { sidebar, calls, pending } = pendingSync();
    const sync = sidebar.handleStartSync();
    await new Promise(resolve => setImmediate(resolve));
    assert.deepEqual(calls.map(call => call.method), ['basket.estimate']);
    pending.resolve({ ...estimate, selectedBytes: 200 });
    await sync;
    assert.deepEqual(calls.map(call => call.method), ['basket.estimate', 'sync_prepare', 'sync_execute']);
    assert.equal(calls[1].params.autoFill[0].maxBytes, 800);
});

test('basket/profile changes while Sync awaits estimation cannot send the old UI reservation', async () => {
    const { sidebar, calls, pending, changeItems } = pendingSync();
    const sync = sidebar.handleStartSync();
    await new Promise(resolve => setImmediate(resolve));
    sidebar.currentDevice.transcoding_profile_id = 'flac';
    changeItems([{ ...selection[0], id: 'new-selection' }, { id: '__auto_fill_slot__:a', serverId: 'a', type: 'AutoFillSlot', sizeBytes: 30 }]);
    pending.resolve({ ...estimate, selectedBytes: 999 });
    await sync;
    const prepare = calls.find(call => call.method === 'sync_prepare').params;
    assert.equal(prepare.basketItems[0].id, 'new-selection');
    assert.equal(prepare.autoFill[0].maxBytes, undefined, 'daemon derives capacity after stale estimate');
    assert.equal(sidebar.estimateCache.get(sidebar.capacityRevision()), undefined);
});


test('explicit Sync revalidates cached server membership without re-expanding every poll', async () => {
    const { sidebar, calls, pending } = pendingSync();
    await sidebar.estimateCache.load(sidebar.capacityRevision(), async () => ({ ...estimate, selectedBytes: 999 }));
    const sync = sidebar.handleStartSync();
    await new Promise(resolve => setImmediate(resolve));
    assert.deepEqual(calls.map(call => call.method), ['basket.estimate']);
    pending.resolve({ ...estimate, selectedBytes: 200 });
    await sync;
    assert.equal(calls.find(call => call.method === 'sync_prepare').params.autoFill[0].maxBytes, 800);
});


test('expected payload never increases the Auto-Fill ceiling reserved by the daemon', async () => {
    const { sidebar, calls, pending } = pendingSync();
    const sync = sidebar.handleStartSync();
    await new Promise(resolve => setImmediate(resolve));
    pending.resolve({ ...estimate, expectedBytes: 190, reserveBytes: 10, selectedBytes: 200 });
    await sync;
    assert.equal(sidebar.plannedManualSize().expectedBytes, 190);
    assert.equal(calls.find(call => call.method === 'sync_prepare').params.autoFill[0].maxBytes, 800);
});

test('declined capacity confirmation retains its prepared pair and revisions invalidate the whole pair', async () => {
    const { sidebar, calls, pending } = pendingSync({ planId: 'prepared', selectedBytes: 700, selectedExpectedBytes: 650, selectedReserveBytes: 50, totalBytes: 700, availableBytes: 680 });
    sidebar.confirmEstimatedCapacity = async () => false;
    const sync = sidebar.handleStartSync();
    await new Promise(resolve => setImmediate(resolve));
    pending.resolve({ ...estimate, selectedBytes: 200, expectedBytes: 190, reserveBytes: 10 });
    await sync;
    assert.equal(sidebar.isSyncing, false);
    assert.deepEqual(sidebar.plannedManualSize(), { selectedBytes: 700, expectedBytes: 650, reserveBytes: 50 });
    assert.equal(sidebar.selectedCapacity().autofillCapacityBytes, 300);
    assert.deepEqual(calls.map(call => call.method), ['basket.estimate', 'sync_prepare', 'sync_plan_discard']);
    for (const mutate of [() => { sidebar.forceSyncMode = true; }, () => { sidebar.currentDevice.plannedProfileRevision = 2; }, () => { sidebar.selectedDevicePath = '/other'; }]) {
        const previous = sidebar.capacityRevision();
        mutate();
        assert.notEqual(sidebar.capacityRevision(), previous);
        assert.notEqual(sidebar.plannedManualSize().selectedBytes, 700);
    }
});

test('legacy authoritative totals remain unsplit through preparation', async () => {
    const { sidebar, pending } = pendingSync({ planId: 'legacy', selectedBytes: 700, totalBytes: 700, availableBytes: 680 });
    sidebar.confirmEstimatedCapacity = async () => false;
    const sync = sidebar.handleStartSync();
    await new Promise(resolve => setImmediate(resolve));
    pending.resolve({ ...estimate, selectedBytes: 200 });
    await sync;
    assert.deepEqual(sidebar.plannedManualSize(), { selectedBytes: 700 });
});
