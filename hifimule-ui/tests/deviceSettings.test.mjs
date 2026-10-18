import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import ts from 'typescript';
const source = readFileSync(new URL('../src/components/BasketSidebar.ts', import.meta.url), 'utf8');
const script = ts.transpile(source.replace(/^import .*;\r?\n/gm, '').replace(/export class /g, 'class '), { target: ts.ScriptTarget.ES2022 });
class Element {
    children = new Map(); listeners = new Map(); style = {}; checked = false; value = ''; loading = false;
    set innerHTML(html) {
        this.html = html;
        for (const tag of html.matchAll(/<[^>]+id="([^"]+)"[^>]*>/g)) {
            const child = new Element();
            child.checked = /\schecked(?:\s|>)/.test(tag[0]);
            child.value = tag[0].match(/value="([^"]*)"/)?.[1] ?? '';
            this.children.set('#' + tag[1], child);
        }
    }
    querySelector(selector) { return this.children.get(selector); }
    querySelectorAll() { return []; }
    addEventListener(event, fn) { this.listeners.set(event, fn); }
    click() { return this.listeners.get('click')?.(); }
    show() {} hide() {} remove() {}
}
async function setup(selected, current = {}) {
    const calls = []; const dialog = new Element();
    const rpc = async (method, params) => { calls.push({ method, params }); return method === 'device_profiles.list' ? [] : {}; };
    const { BasketSidebar } = new Function('rpcCall', 'basketStore', 't', 'document', 'customElements', `${script}\nreturn { BasketSidebar };`)(rpc, { getItems: () => [] }, key => key, { createElement: () => dialog, body: { appendChild() {} } }, { whenDefined: async () => {} });
    const sidebar = Object.create(BasketSidebar.prototype);
    Object.assign(sidebar, { currentDevice: current, selectedDeviceSummary: () => ({ deviceId: 'device', ...selected }), escapeHtml: value => value, iconLabel: value => value, refreshAndRender: async () => {} });
    await sidebar.openDeviceSettings();
    return { sidebar, dialog, calls };
}
test('device settings load the selected device value and default missing values off', async () => {
    for (const value of [undefined, false, true]) {
        const { dialog } = await setup({ reconcileBeforeSync: value });
        assert.equal(dialog.querySelector('#device-settings-reconcile').checked, value === true);
        assert.match(dialog.html, /basket.device.reconcile_hint/);
    }
    const { dialog } = await setup({ reconcileBeforeSync: false }, { reconcileBeforeSync: true });
    assert.equal(dialog.querySelector('#device-settings-reconcile').checked, false);
});
test('settings save sends both enabled and disabled values explicitly', async () => {
    for (const value of [true, false]) {
        const { dialog, calls } = await setup({ reconcileBeforeSync: !value });
        dialog.querySelector('#device-settings-reconcile').checked = value;
        await dialog.querySelector('#device-settings-save').click();
        const save = calls.find(call => call.method === 'device.update_manifest');
        assert.equal(save.params.reconcileBeforeSync, value);
        assert.equal(save.params.deviceId, 'device');
    }
});
test('changing reconciliation invalidates prepared capacity', async () => {
    const { sidebar } = await setup({}, { device_id: 'device', reconcileBeforeSync: false });
    const before = sidebar.capacityRevision();
    sidebar.currentDevice.reconcileBeforeSync = true;
    assert.notEqual(sidebar.capacityRevision(), before);
});
test('every supported language explains external file management and scan cost', () => {
    const catalog = JSON.parse(readFileSync(new URL('../../hifimule-i18n/catalog.json', import.meta.url), 'utf8'));
    for (const language of ['en', 'fr', 'es', 'de']) {
        assert.ok(catalog[language]['basket.device.reconcile_before_sync'].includes('HifiMule'));
        assert.ok(catalog[language]['basket.device.reconcile_hint']);
    }
});
