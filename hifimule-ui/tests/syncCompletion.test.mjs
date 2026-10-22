import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import ts from 'typescript';

const source = readFileSync(new URL('../src/components/BasketSidebar.ts', import.meta.url), 'utf8')
    .replace(/^import .*;\r?\n/gm, '').replace(/export class /g, 'class ');
const script = ts.transpile(source, { target: ts.ScriptTarget.ES2022 });
const catalog = JSON.parse(readFileSync(new URL('../../hifimule-i18n/catalog.json', import.meta.url), 'utf8'));

function harness(locale = 'en') {
    const announcements = [], focused = [], listeners = new Map();
    const done = { addEventListener: (event, callback) => listeners.set(event, callback) };
    const list = { focus() { this.focused = true; } };
    const selectButton = { addEventListener: (_, callback) => listeners.set('select', callback) };
    const container = { innerHTML: '', contains: () => false, querySelector: selector => ({
        '#sync-done-btn': done, '#sync-select-messages': selectButton, '.sync-error-list': list,
    })[selector] ?? null };
    const selection = { removeAllRanges() { this.cleared = true; }, addRange(range) { this.range = range; } };
    const range = { selectNodeContents(node) { this.node = node; } };
    const basketStore = { getItems: () => [], resetDirty() {} };
    const t = (key, args = {}) => (catalog[locale][key] ?? key).replace(/\{(\w+)\}/g, (_, name) => args[name] ?? '');
    const escapeAttr = text => String(text).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;').replace(/'/g, '&#39;');
    const BasketSidebar = new Function('rpcCall', 'basketStore', 'document', 't', 'escapeAttr', 'console', 'window',
        `${script}\nreturn BasketSidebar;`)(async method => { assert.equal(method, 'device_get_storage_info'); return {}; },
        basketStore, { activeElement: null, createRange: () => range }, t, escapeAttr, { log() {} }, { getSelection: () => selection });
    const sidebar = Object.create(BasketSidebar.prototype);
    Object.assign(sidebar, { container, syncSnapshotIds: [], currentOperationId: 'op', isSyncing: true,
        announceSync: message => announcements.push(message), restoreSyncFocus: (_, selector) => focused.push(selector),
        refreshAndRender() {} });
    return { sidebar, container, announcements, focused, t, listeners, selection, list };
}

for (const locale of ['en', 'fr', 'de', 'es']) {
    test(`${locale}: completion preserves warnings, escapes details, announces and retains Done focus`, async () => {
        const h = harness(locale);
        const warning = '<img src=x onerror="bad()">' + 'long-track-name'.repeat(30) + ': damaged audio & possible defects';
        h.sidebar.currentOperation = { filesCompleted: 1, bytesTransferred: 1234, warnings: [warning] };
        await h.sidebar.handleSyncComplete();
        assert.equal(h.sidebar.currentOperation, null);
        assert.deepEqual(h.sidebar.completedWarnings, [warning]);
        assert.match(h.container.innerHTML, /exclamation-triangle-fill/);
        assert.match(h.container.innerHTML, /class="sync-error-list sync-warning-list"/);
        assert.ok(h.container.innerHTML.includes(h.t('basket.actions.select_all_messages')));
        assert.ok(h.container.innerHTML.includes(h.t('basket.sync.complete_with_warnings')));
        assert.match(h.container.innerHTML, /&lt;img src=x onerror=&quot;bad\(\)&quot;&gt;/);
        assert.ok(!h.container.innerHTML.includes('<img'));
        assert.ok(h.container.innerHTML.includes('long-track-name'.repeat(30) + ': damaged audio &amp; possible defects'));
        assert.ok(h.announcements.at(-1).includes(warning));
        assert.ok(h.announcements.at(-1).startsWith(h.t('basket.sync.complete_with_warnings')));
        assert.deepEqual(h.focused, ['#sync-done-btn']);
        h.listeners.get('select')();
        assert.equal(h.selection.range.node, h.list);
        assert.equal(h.selection.cleared, true);
        assert.equal(h.list.focused, true);
    });
}

test('a healthy completion clears earlier warnings and keeps normal success status', async () => {
    const h = harness();
    h.sidebar.completedWarnings = ['old warning'];
    h.sidebar.currentOperation = { filesCompleted: 1, bytesTransferred: 1234 };
    await h.sidebar.handleSyncComplete();
    assert.deepEqual(h.sidebar.completedWarnings, []);
    assert.match(h.container.innerHTML, /check-circle-fill/);
    assert.ok(!h.container.innerHTML.includes('exclamation-triangle-fill'));
    assert.ok(!h.container.innerHTML.includes('sync-error-list'));
    assert.ok(!h.container.innerHTML.includes('sync-select-messages'));
    assert.ok(h.announcements.at(-1).startsWith(h.t('basket.sync.complete')));
    assert.ok(!h.announcements.at(-1).includes('old warning'));
});

test('failed sync selects all error details, including fallback messages', () => {
    const h = harness();
    for (const errors of [['first error', '<second error>'], []]) {
        h.sidebar.renderSyncError(errors);
        assert.match(h.container.innerHTML, /sync-error-panel sync-details-panel/);
        assert.match(h.container.innerHTML, /name="textarea-t"/);
        assert.ok(h.container.innerHTML.includes(errors.length ? '&lt;second error&gt;' : h.t('basket.sync.failed_retry')));
        h.listeners.get('select')();
        assert.equal(h.selection.range.node, h.list);
    }
});

