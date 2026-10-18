import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import vm from 'node:vm';
import ts from '../../hifimule-ui/node_modules/typescript/lib/typescript.js';

// Exercise the real sync-start method without constructing the sidebar's DOM.
function harness(items, { serverType = 'openSubsonic', force = false } = {}) {
  const path = new URL('../../hifimule-ui/src/components/BasketSidebar.ts', import.meta.url);
  const source = ts.createSourceFile('BasketSidebar.ts', readFileSync(path, 'utf8'), ts.ScriptTarget.Latest, true);
  const declaration = source.statements.find(node => ts.isClassDeclaration(node) && node.name?.text === 'BasketSidebar');
  const names = new Set(['handleStartSync', 'itemIdsWithIncrementalChanges', 'isSubsonicServer', 'syncTokenStorageKey']);
  const members = declaration.members.filter(node => names.has(node.name?.getText(source)));
  const compiled = ts.transpileModule(`class SyncStart { ${members.map(node => node.getText(source)).join('\n')} }; exports.SyncStart = SyncStart;`, {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
  }).outputText;
  const calls = [];
  const unrelatedChanges = Array.from({ length: 8394 }, (_, i) => ({ id: `unselected-${i}`, itemType: 'song', changeType: 'updated' }));
  const exports = {};
  vm.runInNewContext(compiled, {
    exports, console, t: key => key,
    localStorage: { getItem: () => 'old-sync-token' },
    isAutoFillSlotId: id => id.startsWith('__auto_fill_slot__'),
    basketStore: {
      getItems: () => items,
      getManualItemIdsForServer: serverId => items.filter(item => !item.id.startsWith('__auto_fill_slot__') && item.serverId === serverId).map(item => item.id),
    },
    rpcCall: async (method, params) => {
      calls.push({ method, params: JSON.parse(JSON.stringify(params)) });
      if (method === 'sync_detect_changes') return unrelatedChanges;
      if (method === 'sync_prepare') return { planId: 'fixture-plan', blockedCount: 1 };
      if (method === 'sync_plan_discard') return {};
      throw new Error(`Unexpected RPC: ${method}`);
    },
  });
  const sidebar = Object.assign(new exports.SyncStart(), {
    serverType, currentServerId: 'selected-server', lastHydratedDeviceId: 'device', forceSyncMode: force,
    autoFillPipelines: new Map(), render() {}, stopPolling() {}, selectedCapacity: () => undefined,
    attachToRunningSync: async () => false, confirmBlockedPlan: async () => false,
    showError: message => { throw new Error(message); },
  });
  return { sidebar, calls };
}

test('library changes never expand the manual sync selection or override server routing', async () => {
  for (const serverType of ['subsonic', 'openSubsonic', 'jellyfin']) {
    for (const force of [false, true]) {
      const items = [
        { id: 'album', type: 'MusicAlbum', serverId: 'server-a' },
        { id: 'song', type: 'Audio', serverId: 'server-b' },
        { id: 'playlist', type: 'Playlist' },
        { id: '__auto_fill_slot__:server-a', type: 'AutoFillSlot', serverId: 'server-a', sizeBytes: 1000 },
      ];
      const { sidebar, calls } = harness(items, { serverType, force });
      await sidebar.handleStartSync();
      const plan = calls.find(call => call.method === 'sync_prepare').params;
      assert.equal(plan.itemIds.length, 3);
      assert.deepEqual(plan.itemIds, [
        { id: 'album', serverId: 'server-a' },
        { id: 'song', serverId: 'server-b' },
        { id: 'playlist', serverId: 'selected-server' },
      ]);
      assert.deepEqual(plan.basketItems, items.slice(0, 3));
      assert.deepEqual(plan.autoFill, [{ serverId: 'server-a', maxBytes: 1000, enabled: true, excludeItemIds: ['album'] }]);
      assert.equal(plan.force, force);
      assert.deepEqual(calls.map(call => call.method), ['sync_prepare', 'sync_plan_discard']);
    }
  }
});

test('pure auto-fill leaves manual selection empty despite a previous sync token', async () => {
  const { sidebar, calls } = harness([
    { id: '__auto_fill_slot__:selected-server', type: 'AutoFillSlot', serverId: 'selected-server', sizeBytes: 1000 },
  ]);
  await sidebar.handleStartSync();
  const plan = calls.find(call => call.method === 'sync_prepare').params;
  assert.equal(plan.itemIds.length, 0);
  assert.deepEqual(plan.itemIds, []);
  assert.deepEqual(plan.basketItems, []);
  assert.equal(plan.autoFill.length, 1);
  assert.deepEqual(plan.autoFill[0].excludeItemIds, []);
  assert.equal(calls.some(call => call.method === 'sync_detect_changes'), false);
});
