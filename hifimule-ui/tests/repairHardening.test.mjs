import assert from 'node:assert/strict';
import test from 'node:test';
import vm from 'node:vm';
import { readFileSync } from 'node:fs';
import ts from 'typescript';

function harness() {
    const pending = [], exports = {}, notices = [];
    vm.runInNewContext(ts.transpileModule(readFileSync(new URL('../src/components/RepairModal.ts', import.meta.url), 'utf8'), {
        compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
    }).outputText, {
        exports, console: { error() {} }, CSS: { escape: value => value },
        require: name => name === '../rpc' ? { rpcCall: (method) => new Promise((resolve, reject) => pending.push({ method, resolve, reject })) }
            : name === '../i18n' ? { t: (key, params) => params ? ({ key, ...params }) : key }
            : { showToast: value => notices.push(value) },
    });
    const modal = new exports.RepairModal({});
    const controls = [{ setAttribute() { this.disabled = true; }, removeAttribute() { this.disabled = false; } }];
    modal.dialog = { setAttribute(_, value) { this.busy = value; }, querySelectorAll: () => controls };
    modal.renderContent = () => { modal.rendered = modal.discrepancies; };
    modal.renderError = message => { modal.error = message; };
    return { modal, pending, controls, notices };
}

test('latest scan wins over an older success or failure', async () => {
    for (const fails of [true, false]) {
        const { modal, pending } = harness();
        const old = modal.scan(), current = modal.scan();
        const latest = { missing: [], orphaned: [] };
        pending[1].resolve(latest); await current;
        if (fails) pending[0].reject('stale failure'); else pending[0].resolve({ missing: ['stale'] });
        await old;
        assert.equal(modal.rendered, latest);
        assert.equal(modal.error, undefined);
    }
});

test('failed post-write scan removes stale data and offers scan recovery without repeating mutation', async () => {
    const { modal, pending, controls } = harness();
    modal.discrepancies = { missing: [{ jellyfinId: 'old' }], orphaned: [] };
    const prune = modal.handlePrune(['old']);
    await modal.handlePrune(['old']);
    await modal.handleClearDirty();
    assert.equal(pending.length, 1);
    assert.equal(controls[0].disabled, true);
    pending[0].resolve(); await Promise.resolve(); await Promise.resolve();
    assert.equal(pending[1].method, 'manifest_get_discrepancies');
    pending[1].reject('Device disconnected'); await prune;
    assert.equal(modal.discrepancies, null);
    assert.equal(modal.error, 'Device disconnected');
    assert.equal(controls[0].disabled, false);
    const retry = modal.scan();
    pending[2].resolve({ missing: [], orphaned: [] }); await retry;
    assert.equal(modal.rendered.missing.length, 0);
    assert.equal(pending.filter(p => p.method === 'manifest_prune').length, 1);
});

test('mutation failures unlock controls and preserve selection for retry', async () => {
    const { modal, pending, controls, notices } = harness();
    const data = { missing: [{ jellyfinId: 'old' }], orphaned: [] };
    modal.discrepancies = data;
    const relink = modal.handleRelink('old', 'new/path');
    pending[0].reject('Read-only device'); await relink;
    assert.equal(modal.discrepancies, data);
    assert.equal(controls[0].disabled, false);
    assert.equal(notices[0].message, 'Read-only device');
});

test('reopening cannot replace the dialog during a manifest write', async () => {
    const { modal, pending } = harness();
    modal.renderDialog = () => assert.fail('must not replace dialog');
    const prune = modal.handlePrune(['old']);
    await modal.open();
    pending[0].reject('offline'); await prune;
});


test('multi-item relink waits for the combobox before focusing and wires the chosen entry', async () => {
    const { modal } = harness();
    let ready, focused = false, change, written;
    modal.escapeHtml = value => value; // Fixed plain-text options; DOM escaping is outside this focus test.
    const select = {
        updateComplete: new Promise(resolve => { ready = resolve; }),
        isConnected: true, value: 'second',
        focus() { focused = true; },
        addEventListener(name, handler) { if (name === 'sl-change') change = handler; },
    };
    const actions = { innerHTML: '', querySelector: () => select };
    modal.dialog.querySelector = () => ({ querySelector: () => ({ querySelector: () => actions }) });
    modal.discrepancies = { missing: [{ jellyfinId: 'first', name: 'First' }, { jellyfinId: 'second', name: 'Second' }], orphaned: [] };
    modal.handleRelink = async (...args) => { written = args; };
    const prompt = modal.handleRelinkPrompt('Music/orphan.flac');
    assert.equal(focused, false);
    assert.equal(written, undefined);
    ready(true); await prompt;
    assert.equal(focused, true);
    await change();
    assert.deepEqual(written, ['second', 'Music/orphan.flac']);
});
