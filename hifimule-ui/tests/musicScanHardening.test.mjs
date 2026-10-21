import * as html from '../src/html.ts';
import assert from 'node:assert/strict';
import test from 'node:test';
import vm from 'node:vm';
import { readFileSync } from 'node:fs';
import ts from 'typescript';

function harness() {
    const pending = [], timers = new Set(), elements = new Map(), exports = {};
    let focused, finished = 0;
    class Element {
        constructor(id) { this.id = id; this.isConnected = true; this.handlers = {}; this.textContent = ''; this.disabled = false; this.style = {}; }
        setAttribute(key, value) { this[key] = value; }
        addEventListener(name, handler) { this.handlers[name] = handler; }
        querySelector(selector) { return elements.get(selector.slice(1)) ?? null; }
        remove() { this.isConnected = false; }
        focus() { focused = this.id; }
        set innerHTML(html) {
            this.html = html;
            for (const id of this.children ?? []) elements.get(id).isConnected = false;
            this.children = [...html.matchAll(/id="([^"]+)"/g)].map(match => match[1]);
            for (const id of this.children) elements.set(id, new Element(id));
        }
        get innerHTML() { return this.html; }
        click() { return this.handlers.click?.(); }
    }
    vm.runInNewContext(ts.transpileModule(readFileSync(new URL('../src/components/InitDeviceModal.ts', import.meta.url), 'utf8'), {
        compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
    }).outputText, {
        exports, Date, Error,
        setInterval: fn => { timers.add(fn); return fn; }, clearInterval: fn => timers.delete(fn),
        require: name => name === '../html' ? html : name === '../rpc' ? { rpcCall: method => new Promise((resolve, reject) => pending.push({ method, resolve, reject })) }
            : name === '../i18n' ? { t: (key, params) => key + (params ? JSON.stringify(params) : '') }
            : name === '../lifecycleDeadline' ? { withDeadline: promise => promise }
            : { basketStore: { setDaemonContext() {}, hydrateFromDaemon() {} } },
    });
    const modal = new exports.InitDeviceModal({}, () => finished++);
    const dialog = new Element('dialog'); dialog.hide = () => { dialog.hidden = true; };
    modal.dialog = dialog;
    const body = new Element('body');
    modal.showMusicScan(body, 'scan', 'device');
    return { modal, dialog, body, pending, timers, elements, get focused() { return focused; }, get finished() { return finished; } };
}
const flush = async () => { for (let i = 0; i < 12; i++) await Promise.resolve(); };
const result = { imported: 3, unmatched: 2, ambiguous: 0, incompatible: 0 };

test('keeps announcement nodes, focuses transitions, and preserves completion against late polling', async () => {
    const h = harness(); await flush(); assert.equal(h.focused, 'scan-yes');
    const live = h.elements.get('scan-announcement');
    const run = h.elements.get('scan-yes').click(); await flush(); assert.equal(h.focused, 'scan-cancel');
    const poll = [...h.timers][0]();
    h.pending[0].resolve(result); await run; await flush();
    assert.equal(h.focused, 'scan-done'); assert.equal(h.elements.get('scan-announcement'), live);
    assert.match(live.textContent, /device.scan.result/);
    h.pending[1].reject('late offline'); await poll;
    assert.match(live.textContent, /device.scan.result/); assert.equal(h.timers.size, 0);
});

test('cancellation failure is visible and retry is single-flight', async () => {
    const h = harness(); const run = h.elements.get('scan-yes').click();
    const cancel = h.elements.get('scan-cancel'); cancel.click(); cancel.click();
    assert.equal(h.pending.length, 2); assert.equal(cancel.disabled, true);
    h.pending[1].reject('offline'); await flush();
    assert.match(h.elements.get('scan-error').textContent, /cancelFailed.*offline/);
    assert.equal(cancel.disabled, false); assert.equal(h.focused, 'scan-cancel');
    cancel.click(); h.pending[2].resolve({ cancelled: true }); await flush();
    h.pending[0].reject('Music scan cancelled'); await run;
    assert.equal(h.finished, 1); assert.equal(h.timers.size, 0);
});

test('late cancel failure cannot replace a completed scan', async () => {
    const h = harness(); const run = h.elements.get('scan-yes').click();
    h.elements.get('scan-cancel').click();
    h.pending[0].resolve(result); await run;
    h.pending[1].reject('expired'); await flush();
    assert.equal(h.elements.get('scan-error').textContent, ''); assert.equal(h.focused, 'scan-done');
});

test('repeated status failures show stale state, cleared by a successful poll', async () => {
    const h = harness(); const run = h.elements.get('scan-yes').click();
    const tick = [...h.timers][0];
    for (let i = 0; i < 3; i++) { const poll = tick(); h.pending.at(-1).reject('offline'); await poll; }
    assert.equal(h.elements.get('scan-stale').textContent, 'device.scan.stale');
    const poll = tick(); h.pending.at(-1).resolve({ scanned: 12, total: 20 }); await poll;
    assert.equal(h.elements.get('scan-stale').textContent, '');
    assert.match(h.elements.get('scan-progress').textContent, /12/);
    h.pending[0].resolve(result); await run;
});

test('skip failure retains dialog and permits retry without starting a scan', async () => {
    const h = harness(); const skip = h.elements.get('scan-no');
    const attempt = skip.click(); h.pending[0].reject('offline'); await attempt; await flush();
    assert.equal(h.finished, 0); assert.equal(skip.disabled, false); assert.equal(h.focused, 'scan-no');
    const retry = skip.click(); h.pending[1].resolve({ cancelled: true }); await retry;
    assert.equal(h.finished, 1); assert.equal(h.pending.length, 2);
});

test('scan failure announces error and returns keyboard focus to retry', async () => {
    const h = harness(); const run = h.elements.get('scan-yes').click();
    h.pending[0].reject('device disconnected'); await run; await flush();
    assert.match(h.elements.get('scan-error').textContent, /device.scan.failed.*disconnected/);
    assert.equal(h.focused, 'scan-yes'); assert.equal(h.timers.size, 0);
});

test('basket refresh can retry without repeating the import', async () => {
    const h = harness(); const run = h.elements.get('scan-yes').click(); h.pending[0].resolve(result); await run;
    const done = h.elements.get('scan-done'); const first = done.click(); h.pending[1].reject('offline'); await first;
    assert.match(h.elements.get('scan-error').textContent, /refreshFailed/); assert.equal(h.finished, 0);
    const retry = done.click(); h.pending[2].resolve({ targetDeviceId: 'other' }); await retry;
    assert.equal(h.finished, 1); assert.equal(h.pending.filter(p => p.method === 'device.music_scan_start').length, 1);
});

 test('an unreachable skipped scan can be dismissed without reporting cancellation', async () => {
    const h = harness(); const attempt = h.elements.get('scan-no').click();
    h.pending[0].reject('scan expired'); await attempt;
    assert.equal(h.elements.get('scan-dismiss').style.display, '');
    h.elements.get('scan-dismiss').click(); assert.equal(h.finished, 1);
});
