import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import ts from 'typescript';

const sidebarSource = readFileSync(new URL('../src/components/BasketSidebar.ts', import.meta.url), 'utf8');
const helperScript = ts.transpile(readFileSync(new URL('../src/state/syncCapacity.ts', import.meta.url), 'utf8').replace(/export /g, ''), { target: ts.ScriptTarget.ES2022 });
const { syncCapacity } = new Function(`${helperScript}\nreturn { syncCapacity };`)();
const basketSource = readFileSync(new URL('../src/state/basket.ts', import.meta.url), 'utf8')
    .replace(/^import .*;\r?\n/gm, '').replace(/export /g, '')
    .replace('const basketStore = new BasketStore();', '');
const basketScript = ts.transpile(basketSource, { target: ts.ScriptTarget.ES2022 });
const { BasketStore } = new Function(`${basketScript}\nreturn { BasketStore };`)();
// Exercise the capacity renderer without mounting its browser event handlers.
const script = ts.transpile(sidebarSource.replace(/^import .*;\r?\n/gm, '').replace(/export class /g, 'class '), { target: ts.ScriptTarget.ES2022 });
const { renderCapacityBar } = new Function('t', 'syncCapacity', `${script}\nreturn { renderCapacityBar };`)((key, params) => `${key}:${params?.size ?? ''}`, syncCapacity);

test('selected content can reclaim existing managed autofill without double counting selected files', () => {
    const html = renderCapacityBar({ totalBytes: 20 * 2 ** 30, freeBytes: 2 ** 30, usedBytes: 19 * 2 ** 30 }, 4 * 2 ** 30, [
        { sizeBytes: 2 * 2 ** 30, isAutoFill: false },
        { sizeBytes: 7 * 2 ** 30, isAutoFill: true },
    ]);
    assert.match(html, /basket.capacity.remaining:6.0 GB/);
    assert.doesNotMatch(html, /capacity-zone-red/);
});

test('capacity distinguishes manual and autofill bytes and only credits managed files', () => {
    const result = syncCapacity({ totalBytes: 20, freeBytes: 1, usedBytes: 19 }, 4, [{ sizeBytes: 2 }, { sizeBytes: 7, isAutoFill: true }]);
    assert.equal(result.syncedSelectedBytes, 2);
    assert.equal(result.syncedAutofillBytes, 7);
    assert.equal(result.availableBytes, 10);
    assert.equal(result.autofillCapacityBytes, 6);
    assert.equal(result.usedFraction, 0.5);
    assert.equal(result.selectedFraction, 0.2);
    assert.equal(result.freeFraction, 0.3);
});

test('already selected tracks are credited once and true overflow is disabled', () => {
    const storage = { totalBytes: 20, freeBytes: 1, usedBytes: 19 };
    assert.equal(syncCapacity(storage, 4, [{ sizeBytes: 4 }]).remainingBytes, 1);
    const overflow = syncCapacity(storage, 6, [{ sizeBytes: 4 }]);
    assert.equal(overflow.zone, 'red');
    assert.equal(overflow.overBytes, 1);
    assert.equal(overflow.autofillCapacityBytes, 0);
});

test('unavailable storage stays unavailable and bar segments stay bounded', () => {
    assert.equal(syncCapacity(null, 4, [{ sizeBytes: 10 }]), null);
    const overflow = syncCapacity({ totalBytes: 20, freeBytes: 1, usedBytes: 19 }, 99, [{ sizeBytes: 4 }]);
    assert.equal(overflow.usedFraction, 0.75);
    assert.equal(overflow.selectedFraction, 0.25);
    assert.equal(overflow.freeFraction, 0);
});

test('MTP free-space-only storage still credits manifest-managed files', () => {
    const result = syncCapacity({ totalBytes: 1, freeBytes: 1, usedBytes: 0 }, 4, [{ sizeBytes: 2 }, { sizeBytes: 7, isAutoFill: true }]);
    assert.equal(result.availableBytes, 10);
    assert.equal(result.remainingBytes, 6);
    assert.notEqual(result.zone, 'red');
});

test('multiple autofill ceilings and other-server selections share manual admission and slot readouts', () => {
    const basket = Object.create(BasketStore.prototype);
    basket.items = new Map([
        ['manual-a', { id: 'manual-a', serverId: 'a', sizeBytes: 2 }],
        ['manual-b', { id: 'manual-b', serverId: 'b', sizeBytes: 2 }],
        ['slot-a', { id: '__auto_fill_slot__:a', sizeBytes: 100 }],
        ['slot-b', { id: '__auto_fill_slot__:b', sizeBytes: 100 }],
    ]);
    assert.equal(basket.getManualSizeBytes(), 4);
    const { BasketSidebar } = new Function('t', 'syncCapacity', 'basketStore', `${script}\nreturn { BasketSidebar };`)(() => '', syncCapacity, basket);
    const sidebar = Object.create(BasketSidebar.prototype);
    sidebar.storageInfo = { totalBytes: 20, freeBytes: 1, usedBytes: 19 };
    sidebar.currentDevice = { synced_items: [{ sizeBytes: 2 }, { sizeBytes: 7, isAutoFill: true }] };
    assert.notEqual(sidebar.selectedCapacity().zone, 'red');
    assert.equal(sidebar.slotSizeBytes({ budget: { maxBytes: 100 } }), 6);
    assert.equal(sidebar.slotSizeBytes({ budget: { maxBytes: 3 } }), 3);
});
