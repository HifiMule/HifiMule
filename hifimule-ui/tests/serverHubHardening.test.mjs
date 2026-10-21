import * as html from '../src/html.ts';
import assert from 'node:assert/strict';
import test from 'node:test';
import vm from 'node:vm';
import { readFileSync } from 'node:fs';
import ts from 'typescript';

function harness() {
    const listeners = {};
    const confirm = { loading: false, addEventListener: (_, fn) => { listeners.confirm = fn; } };
    const cancel = { disabled: false, addEventListener: (_, fn) => { listeners.cancel = fn; } };
    const dialog = {
        innerHTML: '', hidden: false,
        querySelector: selector => selector === '#server-remove-confirm' ? confirm : cancel,
        addEventListener(type, fn) { listeners[type] = fn; }, show() {}, hide() { this.hidden = true; },
    };
    const exports = {}, events = [], removed = [];
    let removeCalls = 0, resolveRemoval;
    const pending = new Promise(resolve => { resolveRemoval = resolve; });
    const source = readFileSync(new URL('../src/components/ServerHub.ts', import.meta.url), 'utf8');
    vm.runInNewContext(ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText, {
        exports, console: { error() {} },
        document: { createElement: () => dialog, body: { appendChild() {} } },
        customElements: { whenDefined: async () => {} },
        window: { dispatchEvent: e => events.push(e) }, CustomEvent: class { constructor(type, options) { this.type = type; this.detail = options.detail; } },
        require: name => name === '../html' ? html : name === '../rpc' ? { serverRemove: () => { removeCalls++; return pending; }, rpcCall: async () => { throw 'offline'; } }
            : name === '../state/basket' ? { basketStore: { removeItemsForServer: id => { removed.push(id); return 0; } } }
            : name === '../i18n' ? { t: (key, params) => params?.server ?? key }
            : name === '../serverIdentity' ? { formatServerIdentity: s => ({ label: s.name }) } : {},
    });
    const hub = Object.create(exports.ServerHub.prototype);
    hub.servers = [{ id: 'local', serverId: 'portable', name: '<img src=x onerror=alert(1)> & music' }];
    hub.selectedId = null; hub.refresh = async () => {}; hub.onServerChanged = () => {};
    return { hub, dialog, confirm, cancel, listeners, events, removed, resolveRemoval, get removeCalls() { return removeCalls; } };
}

test('removal treats a server name as text and rapid confirmation removes once', async () => {
    const h = harness(); h.hub.handleRemove('local');
    assert.match(h.dialog.innerHTML, /&lt;img src=x onerror=alert\(1\)&gt; &amp; music/);
    assert.doesNotMatch(h.dialog.innerHTML, /<img/);
    const removing = h.listeners.confirm(); await h.listeners.confirm();
    assert.equal(h.removeCalls, 1); assert.equal(h.confirm.loading, true);
    h.resolveRemoval(); await removing;
    assert.deepEqual(h.removed, ['portable', 'local']);
    assert.equal(h.confirm.loading, false); assert.equal(h.dialog.hidden, true);
});

test('logout failures are surfaced to the user', async () => {
    const h = harness(); await h.hub.handleLogout();
    assert.equal(h.events[0].type, 'toast');
    assert.equal(h.events[0].detail.type, 'error');
    assert.equal(h.events[0].detail.message, 'offline');
});


test('removal cannot be dismissed while pending and unlocks afterward', async () => {
    const h = harness(); h.hub.handleRemove('local');
    const removing = h.listeners.confirm();
    assert.equal(h.cancel.disabled, true);
    h.listeners.cancel();
    assert.equal(h.dialog.hidden, false);
    let prevented = false;
    h.listeners['sl-request-close']({ target: h.dialog, preventDefault() { prevented = true; } });
    assert.equal(prevented, true);
    h.resolveRemoval(); await removing;
    assert.equal(h.cancel.disabled, false);
});

function refreshHarness() {
    const pending = [], exports = {};
    const source = readFileSync(new URL('../src/components/ServerHub.ts', import.meta.url), 'utf8');
    vm.runInNewContext(ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText, {
        exports, console: { error() {} },
        require: name => name === '../html' ? html : name === '../rpc' ? { serverList: () => new Promise((resolve, reject) => pending.push({ resolve, reject })) }
            : name === '../state/basket' ? { basketStore: { reconcileServerIds() {} } } : {},
    });
    const hub = Object.create(exports.ServerHub.prototype);
    Object.assign(hub, { servers: [{ id: 'existing', selected: true }], selectedId: 'existing', refreshGeneration: 0, refreshFailed: false, render() {} });
    return { hub, pending };
}

test('failed refresh retains known servers and a successful retry clears failure', async () => {
    const { hub, pending } = refreshHarness();
    const first = hub.refresh(); pending[0].reject(new Error('offline')); await first;
    assert.equal(hub.servers[0].id, 'existing');
    assert.equal(hub.selectedId, 'existing');
    assert.equal(hub.refreshFailed, true);
    const retry = hub.refresh(); pending[1].resolve([]); await retry;
    assert.equal(hub.refreshFailed, false);
    assert.equal(hub.servers.length, 0);
    assert.equal(hub.selectedId, null);
});

for (const staleFails of [false, true]) {
    test(`a stale refresh ${staleFails ? 'failure' : 'success'} cannot replace newer state`, async () => {
        const { hub, pending } = refreshHarness();
        const old = hub.refresh(), latest = hub.refresh();
        pending[1].resolve([{ id: 'new', selected: true }]); await latest;
        if (staleFails) pending[0].reject(new Error('offline'));
        else pending[0].resolve([]);
        await old;
        assert.equal(hub.selectedId, 'new');
        assert.equal(hub.refreshFailed, false);
    });
}
