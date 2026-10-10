import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import ts from 'typescript';

const source = readFileSync(new URL('../src/components/BasketSidebar.ts', import.meta.url), 'utf8');
const script = ts.transpile(source.replace(/^import .*;\r?\n/gm, '').replace(/export class /g, 'class '), { target: ts.ScriptTarget.ES2022 });

test('starting a sync exchanges a prepared token without sending track metadata back to the daemon', async () => {
    const calls = [];
    const rpcCall = async (method, params) => {
        calls.push({ method, params });
        if (method === 'sync_execute') return { operationId: 'operation-1' };
        return { planId: 'plan-1', blockedCount: 0, destructiveCleanupCount: 0, destructiveCleanupThreshold: 100, changeReasons: [] };
    };
    const { BasketSidebar } = new Function('rpcCall', 'basketStore', 'isAutoFillSlotId', 't', `${script}\nreturn { BasketSidebar };`)(rpcCall, { getItems: () => [] }, () => false, key => key);
    const sidebar = Object.create(BasketSidebar.prototype);
    Object.assign(sidebar, { isSyncing: false, render() {}, attachToRunningSync: async () => false, itemIdsWithIncrementalChanges: async ids => ids, startPolling() {}, showError(message) { throw new Error(message); } });
    await sidebar.handleStartSync();
    assert.equal(calls[0].method, 'sync_prepare');
    assert.equal(calls[1].method, 'sync_execute');
    assert.equal(calls[1].params.planId, 'plan-1');
    assert.equal('delta' in calls[1].params, false);
    assert.equal(sidebar.currentOperationId, 'operation-1');
});

function sidebarWith(calls, summary, overrides = {}) {
    const rpcCall = async (method, params) => {
        calls.push({ method, params });
        if (method === 'sync_prepare') return summary;
        if (method === 'sync_plan_discard') return { discarded: true };
        if (method === 'sync_execute') throw new Error('Prepared sync plan expired; prepare sync again');
        throw new Error(`Unexpected RPC ${method}`);
    };
    const { BasketSidebar } = new Function('rpcCall', 'basketStore', 'isAutoFillSlotId', 't', `${script}\nreturn { BasketSidebar };`)(rpcCall, { getItems: () => [] }, () => false, key => key);
    const sidebar = Object.create(BasketSidebar.prototype);
    Object.assign(sidebar, { isSyncing: false, render() {}, attachToRunningSync: async () => false, itemIdsWithIncrementalChanges: async ids => ids, stopPolling() {}, startPolling() {}, showError(message) { this.errorMessage = message; }, ...overrides });
    return sidebar;
}

test('declining blocked content discards the owned prepared plan without starting a transfer', async () => {
    const calls = [];
    const sidebar = sidebarWith(calls, { planId: 'blocked-plan', blockedCount: 1000 }, {
        confirmBlockedPlan: async (id, count) => { assert.equal(id, 'blocked-plan'); assert.equal(count, 1000); return false; },
    });
    await sidebar.handleStartSync();
    assert.deepEqual(calls.map(c => c.method), ['sync_prepare', 'sync_plan_discard']);
    assert.deepEqual(calls[1].params, { planId: 'blocked-plan' });
    assert.equal(sidebar.isSyncing, false);
});

test('declining destructive cleanup discards the plan and preserves the exact confirmation count', async () => {
    const calls = [];
    const sidebar = sidebarWith(calls, { planId: 'delete-plan', blockedCount: 0, destructiveCleanupCount: 40001, destructiveCleanupThreshold: 100 }, {
        confirmDestructiveCleanup: async count => { assert.equal(count, 40001); return false; },
    });
    await sidebar.handleStartSync();
    assert.deepEqual(calls.map(c => c.method), ['sync_prepare', 'sync_plan_discard']);
    assert.equal(sidebar.isSyncing, false);
});

test('an expired execute token resets the busy state and discards its remaining ownership', async () => {
    const calls = [];
    const sidebar = sidebarWith(calls, { planId: 'expired-plan', blockedCount: 0, destructiveCleanupCount: 0 });
    await sidebar.handleStartSync();
    assert.deepEqual(calls.map(c => c.method), ['sync_prepare', 'sync_execute', 'sync_plan_discard']);
    assert.equal(calls[2].params.planId, 'expired-plan');
    assert.equal(sidebar.isSyncing, false);
    assert.equal(sidebar.currentOperationId, null);
    assert.ok(sidebar.errorMessage);
});

test('force mode is bound at preparation and sent with the same single-use token', async () => {
    const calls = [];
    const sidebar = sidebarWith(calls, { planId: 'force-plan', blockedCount: 0 }, { forceSyncMode: true });
    await sidebar.handleStartSync();
    assert.equal(calls[0].params.force, true);
    assert.equal(calls[1].params.force, true);
    assert.equal(calls[1].params.planId, 'force-plan');
});

test('a rejected page request after Continue or Cancel cannot repaint the closed confirmation', async () => {
    for (const action of ['blocked-continue', 'blocked-cancel']) {
        let rejectDetails;
        const detail = new Promise((_, reject) => { rejectDetails = reject; });
        const nodes = new Map();
        const listeners = new Map();
        const node = id => {
            if (!nodes.has(id)) nodes.set(id, { disabled: false, addEventListener(event, fn) { this[event] = fn; }, innerHTML: '', textContent: '' });
            return nodes.get(id);
        };
        const dialog = { querySelector: selector => node(selector.slice(1)), addEventListener(event, fn) { listeners.set(event, fn); }, hide() { listeners.get('sl-after-hide')(); }, remove() { this.removed = true; }, show() {} };
        const document = { createElement: () => dialog, body: { appendChild() {} } };
        const { BasketSidebar } = new Function('rpcCall', 'basketStore', 'isAutoFillSlotId', 't', 'document', 'customElements', `${script}\nreturn { BasketSidebar };`)(() => detail, { getItems: () => [] }, () => false, key => key, document, { whenDefined: async () => {} });
        const sidebar = Object.create(BasketSidebar.prototype);
        Object.assign(sidebar, { escapeHtml: value => value, showError(message) { this.errorMessage = message; } });
        const result = sidebar.confirmBlockedMedia([{ name: 'First' }], { planId: 'owned', total: 501, nextOffset: 500 });
        node('blocked-next').click();
        node(action).click();
        assert.equal(await result, action === 'blocked-continue');
        rejectDetails(new Error('Late expiry'));
        await new Promise(resolve => setImmediate(resolve));
        assert.equal(sidebar.errorMessage, undefined);
        assert.equal(dialog.removed, true);
    }
});
