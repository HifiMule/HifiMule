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
    const storage = { totalBytes: 20 * 2 ** 30, freeBytes: 2 ** 30, usedBytes: 19 * 2 ** 30 };
    const items = [
        { sizeBytes: 2 * 2 ** 30, isAutoFill: false },
        { sizeBytes: 7 * 2 ** 30, isAutoFill: true },
    ];
    const html = renderCapacityBar(storage, 4 * 2 ** 30, items);
    assert.equal(syncCapacity(storage, 4 * 2 ** 30, items).remainingBytes, 6 * 2 ** 30);
    assert.match(html, /basket.capacity.available:0 MB/);
    assert.match(html, /basket.capacity.autofill_used:7.0 GB/);
    assert.doesNotMatch(html, /capacity-zone-red/);
});

function segmentGeometry(html, name) {
    const match = html.match(new RegExp(`class="capacity-segment capacity-${name}" style="--seg-x: ([\\d.e+-]+); --seg-w: ([\\d.e+-]+);`));
    assert.ok(match, `${name} segment is present`);
    return { x: Number(match[1]), width: Number(match[2]) };
}

test('compact capacity separates recorded autofill from the reservation and available space', () => {
    const html = renderCapacityBar({ totalBytes: 20 * 2 ** 30, freeBytes: 9 * 2 ** 30, usedBytes: 11 * 2 ** 30 }, 4 * 2 ** 30, [
        { sizeBytes: 2 * 2 ** 30 },
        { sizeBytes: 3 * 2 ** 30, isAutoFill: true, serverId: 'a' },
        { sizeBytes: 4 * 2 ** 30, isAutoFill: true, serverId: 'b' },
    ]);
    assert.match(html, /basket.capacity.available:7.0 GB/);
    assert.match(html, /basket.capacity.autofill_used:7.0 GB/);
    assert.doesNotMatch(html, /basket.capacity.remaining:/);
    assert.deepEqual(segmentGeometry(html, 'used'), { x: 0, width: 0.1 });
    assert.deepEqual(segmentGeometry(html, 'projected'), { x: 0.1, width: 0.2 });
    const autofill = segmentGeometry(html, 'autofill');
    assert.ok(Math.abs(autofill.x - 0.3) < 1e-12);
    assert.equal(autofill.width, 0.35);
    const free = segmentGeometry(html, 'free');
    assert.ok(Math.abs(free.x - 0.65) < 1e-12);
    assert.ok(Math.abs(free.width - 0.35) < 1e-12);
    assert.match(html, /role="img"[^>]*aria-label="[^"]*basket.capacity.selection:4.0 GB[^"]*basket.capacity.autofill_used:7.0 GB[^"]*basket.capacity.available:7.0 GB/);
});

test('displacing autofill clips its segment while preserving the full recorded usage', () => {
    const html = renderCapacityBar({ totalBytes: 20 * 2 ** 30, freeBytes: 2 ** 30, usedBytes: 19 * 2 ** 30 }, 4 * 2 ** 30,
        { selectedBytes: 2 * 2 ** 30, autofillBytes: 7 * 2 ** 30 });
    assert.match(html, /basket.capacity.available:0 MB/);
    assert.match(html, /basket.capacity.autofill_used:7.0 GB/);
    const autofill = segmentGeometry(html, 'autofill');
    assert.equal(autofill.x, 0.7);
    assert.ok(Math.abs(autofill.width - 0.3) < 1e-12);
    assert.ok(Math.abs(autofill.x + autofill.width - 1) < 1e-12);
    assert.equal(segmentGeometry(html, 'free').width, 0);
});

test('zero recorded autofill retains the remaining-space fallback', () => {
    const html = renderCapacityBar({ totalBytes: 20 * 2 ** 30, freeBytes: 10 * 2 ** 30, usedBytes: 10 * 2 ** 30 }, 4 * 2 ** 30,
        { selectedBytes: 2 * 2 ** 30, autofillBytes: 0 });
    assert.match(html, /basket.capacity.remaining:8.0 GB/);
    assert.doesNotMatch(html, /basket.capacity.available:|basket.capacity.autofill_used:|capacity-autofill/);
});

test('capacity warnings remain primary when recorded autofill is present', () => {
    const storage = { totalBytes: 20 * 2 ** 30, freeBytes: 2 ** 30, usedBytes: 19 * 2 ** 30 };
    const totals = { selectedBytes: 2 * 2 ** 30, autofillBytes: 7 * 2 ** 30 };
    for (const [selected, zone, message, color] of [
        [9, 'amber', 'basket.capacity.tight_fit:1.0 GB', 'var(--amber-warn)'],
        [11, 'red', 'basket.capacity.exceeds:1.0 GB', 'var(--sl-color-danger-500)'],
    ]) {
        const html = renderCapacityBar(storage, selected * 2 ** 30, totals);
        assert.ok(html.includes(`capacity-zone-${zone}`));
        assert.ok(html.includes(message));
        assert.ok(html.includes(`background: ${color}`));
        assert.match(html, /basket.capacity.autofill_used:7.0 GB/);
        assert.doesNotMatch(html, /basket.capacity.available:/);
        const autofill = segmentGeometry(html, 'autofill');
        assert.ok(autofill.x >= 0 && autofill.width >= 0 && autofill.x + autofill.width <= 1);
    }
});

test('unavailable storage does not imply recorded autofill geometry', () => {
    const totals = { selectedBytes: 2 * 2 ** 30, autofillBytes: 7 * 2 ** 30 };
    const html = renderCapacityBar(null, 4 * 2 ** 30, totals);
    assert.match(html, /capacity-no-device/);
    assert.doesNotMatch(html, /capacity-autofill|basket.capacity.autofill_used:/);
    assert.equal(renderCapacityBar({ totalBytes: 0, freeBytes: 0, usedBytes: 0 }, 4 * 2 ** 30, totals), '');
});

test('MTP estimates keep geometry bounded without losing the recorded autofill label', () => {
    const html = renderCapacityBar({ totalBytes: 1, freeBytes: 1, usedBytes: 0 }, 4 * 2 ** 30,
        { selectedBytes: 2 * 2 ** 30, autofillBytes: 7 * 2 ** 30 });
    assert.match(html, /basket.capacity.available:0 MB/);
    assert.match(html, /basket.capacity.autofill_used:7.0 GB/);
    assert.deepEqual(segmentGeometry(html, 'autofill'), { x: 1, width: 0 });
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

test('compact daemon byte totals preserve capacity and rendering without library records', () => {
    const storage={ totalBytes:20,freeBytes:1,usedBytes:19 };
    const records=[{sizeBytes:2},{sizeBytes:7,isAutoFill:true}];
    const totals={selectedBytes:2,autofillBytes:7};
    assert.deepEqual(syncCapacity(storage,4,totals),syncCapacity(storage,4,records));
    assert.equal(renderCapacityBar(storage,4,totals),renderCapacityBar(storage,4,records));
    const basket={getManualSizeBytes:()=>4};
    const {BasketSidebar}=new Function('t','syncCapacity','basketStore',`${script}\nreturn {BasketSidebar};`)(()=>'',syncCapacity,basket);
    const sidebar=Object.create(BasketSidebar.prototype);
    sidebar.storageInfo=storage;sidebar.currentDevice={syncedItemCount:40000,syncedByteTotals:totals};
    assert.deepEqual(sidebar.selectedCapacity(),syncCapacity(storage,4,records));
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
