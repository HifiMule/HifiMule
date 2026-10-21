import { escapeAttr } from '../src/html.ts';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import vm from 'node:vm';
import ts from 'typescript';

class Element {
    children = new Map(); listeners = new Map(); checked = false; value = ''; hidden = false;
    set innerHTML(html) {
        this.html = html;
        this.children.clear();
        for (const tag of html.matchAll(/<[^>]+id="([^"]+)"[^>]*>/g)) {
            const child = new Element();
            child.checked = /\schecked(?:\s|>)/.test(tag[0]);
            child.hidden = /\shidden(?:\s|>)/.test(tag[0]);
            child.value = tag[0].match(/value="([^"]*)"/)?.[1] ?? '';
            this.children.set('#' + tag[1], child);
        }
    }
    querySelector(selector) { return this.children.get(selector); }
    querySelectorAll() { return []; }
    addEventListener(event, fn) { this.listeners.set(event, fn); }
    emit(event) { return this.listeners.get(event)?.(); }
    cloneNode() { const copy = new Element(); copy.replaceWith = () => {}; return copy; }
    replaceWith() {} hide() {}
}
function setup() {
    const calls = [], body = new Element(), confirm = new Element();
    const dialog = new Element();
    dialog.querySelector = selector => selector === '.init-device-body' ? body
        : selector === '#init-device-confirm-btn' ? confirm : body.querySelector(selector);
    const source = readFileSync(new URL('../src/components/InitDeviceModal.ts', import.meta.url), 'utf8');
    const script = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText;
    const ctx = { exports: {}, require: name => name === '../html' ? { escapeAttr }
        : name === '../rpc' ? { rpcCall: async (method, params) => { calls.push({ method, params }); return {}; } }
        : name === '../i18n' ? { t: key => key }
        : name === '../deviceIcon' ? { deviceIconLabel: icon => icon } : {} };
    vm.runInNewContext(script, ctx);
    const modal = new ctx.exports.InitDeviceModal({});
    Object.assign(modal, { dialog, pendingId: 'pending-target', observedDestinationRevision: '7' });
    modal.renderContent(body, 'listener', []);
    return { modal, body, calls };
}
test('creation shows localized reconciliation and artwork choices with every option off', () => {
    const { body } = setup();
    for (const id of ['reconcile', 'artwork', ...['windows', 'macos', 'kde', 'gnome'].map(kind => `artwork-${kind}`)]) {
        assert.equal(body.querySelector(`#init-device-${id}`).checked, false);
    }
    assert.equal(body.querySelector('#init-device-artwork-additions').hidden, true);
    for (const key of ['reconcile_before_sync', 'reconcile_hint', 'artwork', 'artwork_hint', ...['windows', 'macos', 'kde', 'gnome'].flatMap(kind => [`artwork_${kind}`, `artwork_${kind}_hint`])]) {
        assert.ok(body.html.includes(`basket.device.${key}`));
        const catalog = JSON.parse(readFileSync(new URL('../../hifimule-i18n/catalog.json', import.meta.url), 'utf8'));
        for (const language of ['en', 'fr', 'es', 'de']) assert.ok(catalog[language][`basket.device.${key}`]);
    }
});
test('untouched creation explicitly saves all preferences off', async () => {
    const { modal, calls } = setup();
    await modal.handleConfirm('listener', 'usb-drive', { value: 'Player' });
    const params = JSON.parse(JSON.stringify(calls[0].params));
    assert.equal(params.reconcileBeforeSync, false);
    assert.deepEqual(params.folderArtwork, { enabled: false, windows: false, macos: false, kde: false, gnome: false });
});
test('thumbnail hide/show preserves independent native icon selections', () => {
    const { body } = setup();
    const master = body.querySelector('#init-device-artwork');
    const additions = body.querySelector('#init-device-artwork-additions');
    body.querySelector('#init-device-artwork-windows').checked = true;
    for (const enabled of [true, false, true]) {
        master.checked = enabled; master.emit('sl-change');
        assert.equal(additions.hidden, !enabled);
        assert.equal(body.querySelector('#init-device-artwork-windows').checked, true);
        assert.equal(body.querySelector('#init-device-artwork-macos').checked, false);
    }
});
test('creation captures enabled and disabled preferences before submitting in the single request', async () => {
    for (const enabled of [true, false]) {
        const { modal, body, calls } = setup();
        body.querySelector('#init-device-reconcile').checked = enabled;
        body.querySelector('#init-device-artwork').checked = enabled;
        body.querySelector('#init-device-artwork-windows').checked = true;
        body.querySelector('#init-device-artwork-kde').checked = true;
        await modal.handleConfirm('listener', 'usb-drive', { value: 'Player' });
        assert.equal(body.querySelector('#init-device-reconcile'), undefined, 'submission replaces the form');
        assert.equal(calls.length, 1);
        assert.equal(calls[0].method, 'device_initialize');
        const params = JSON.parse(JSON.stringify(calls[0].params));
        assert.equal(params.reconcileBeforeSync, enabled);
        assert.deepEqual(params.folderArtwork, { enabled, windows: true, macos: false, kde: true, gnome: false });
        assert.equal(params.pendingId, 'pending-target');
        assert.equal(params.observedDestinationRevision, '7');
    }
});
