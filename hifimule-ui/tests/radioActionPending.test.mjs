import assert from 'node:assert/strict';
import test from 'node:test';
import vm from 'node:vm';
import { readFileSync } from 'node:fs';
import ts from 'typescript';

function setup() {
    class Element {
        attributes = {}; listeners = {}; children = [];
        appendChild(child) { this.children.push(child); }
        setAttribute(key, value) { this.attributes[key] = value; }
        removeAttribute(key) { delete this.attributes[key]; }
        addEventListener(type, fn) { this.listeners[type] = fn; }
    }
    const registry = new Map(), requests = [], messages = [], exports = {};
    const source = ts.transpileModule(readFileSync(new URL('../src/components/RadioPlayButton.ts', import.meta.url), 'utf8'), {
        compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
    }).outputText;
    vm.runInNewContext(source, { exports, HTMLElement: Element,
        customElements: { get: name => registry.get(name), define: (name, ctor) => registry.set(name, ctor) },
        document: { createElement: name => new (registry.get(name) || Element)() },
        require: name => name === '../rpc' ? new Proxy({}, {get: (_, method) => (...args) => new Promise((resolve, reject) => requests.push({method, args, resolve, reject}))})
            : name === '../i18n' ? {t: (key, values) => values ? key + ':' + values.title : key}
            : { showToast: (message, variant) => messages.push({message, variant}) },
    });
    const make = name => {
        const tooltip = exports.createRadioPlayButton({type:'MusicArtist', id:name, name, serverId:'server'});
        const action = tooltip.children[0], button = action.children[0];
        action.connectedCallback();
        return {action, button, click: () => button.listeners.click({stopPropagation(){}})};
    };
    return {make, requests, messages};
}

for (const failingStage of [null, 'read', 'save', 'start']) {
    test(`radio shared pending state, lifecycle and recovery: ${failingStage || 'success'}`, async () => {
        const {make, requests, messages} = setup();
        const first = make('夜の音楽 🎵'), peer = make('Other');
        const pending = first.click();
        assert.equal(peer.button.attributes['aria-disabled'], 'true');
        assert.equal(first.button.disabled, undefined, 'focusable during pending operation');
        assert.equal(first.button.name, 'hourglass-split');
        assert.equal(first.button.attributes['aria-busy'], 'true');
        const added = make('Mounted while pending');
        assert.equal(added.button.attributes['aria-disabled'], 'true');
        peer.action.disconnectedCallback();
        await Promise.all(Array.from({length:10}, () => added.click()));
        assert.equal(requests.length, 1);
        for (const [i, stage] of ['read','save','start'].entries()) {
            if (stage === failingStage) { requests[i].reject(new Error('Offline')); break; }
            requests[i].resolve(i === 0 ? {sources:[], ordering:['random']} : {});
            await new Promise(resolve => setImmediate(resolve));
        }
        await pending;
        for (const control of [first, added]) {
            assert.equal(control.button.attributes['aria-disabled'], 'false');
            assert.equal(control.button.attributes['aria-busy'], undefined);
            assert.equal(control.button.name, 'broadcast');
        }
        peer.action.connectedCallback();
        assert.equal(peer.button.attributes['aria-disabled'], 'false', 'remount synchronizes current state');
        assert.match(first.button.label, /夜の音楽 🎵/);
        assert.equal(messages[0].message, 'playback.selection.starting');
        assert.equal(messages.at(-1).variant, failingStage ? 'danger' : 'success');
        const count = requests.length, retry = peer.click();
        assert.equal(requests.length, count + 1);
        requests.at(-1).reject(new Error('Offline')); await retry;
    });
}
