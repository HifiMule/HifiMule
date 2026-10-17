import assert from 'node:assert/strict';
import test from 'node:test';
import vm from 'node:vm';
import { readFileSync } from 'node:fs';
import ts from 'typescript';

function harness(withObserver = true) {
    const exports = {}, observers = [];
    class Observer {
        constructor(callback) { this.callback = callback; observers.push(this); }
        observe(target) { this.target = target; }
        disconnect() { this.disconnected = true; }
        intersect(visible = true) { this.callback([{ isIntersecting: visible }]); }
    }
    vm.runInNewContext(ts.transpileModule(readFileSync(new URL('../src/components/cardArtwork.ts', import.meta.url), 'utf8'), {
        compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
    }).outputText, {
        exports, HTMLElement: class { isConnected = true; }, queueMicrotask,
        IntersectionObserver: withObserver ? Observer : undefined,
        customElements: { get() {}, define() {} },
    });
    const card = new exports.CardArtwork();
    let requests = 0;
    card.load = async () => { requests++; };
    return { card, observers, requests: () => requests };
}

test('offscreen cards wait, then request artwork only once', () => {
    const { card, observers, requests } = harness();
    card.connectedCallback();
    assert.equal(requests(), 0);
    observers[0].intersect(false);
    assert.equal(requests(), 0);
    observers[0].intersect(); observers[0].intersect();
    assert.equal(requests(), 1);
    assert.equal(observers[0].disconnected, true);
    card.disconnectedCallback(); card.connectedCallback();
    assert.equal(requests(), 1);
});

test('removed cards release observers and ignore queued visibility callbacks', () => {
    const { card, observers, requests } = harness();
    card.connectedCallback();
    card.isConnected = false; card.disconnectedCallback();
    observers[0].intersect();
    assert.equal(requests(), 0);
    assert.equal(observers[0].disconnected, true);
    card.isConnected = true; card.connectedCallback();
    observers[1].intersect();
    assert.equal(requests(), 1);
});

test('older WebViews fall back to loading, but skip detached cards', async () => {
    const visible = harness(false), removed = harness(false);
    visible.card.connectedCallback(); removed.card.connectedCallback();
    removed.card.isConnected = false; removed.card.disconnectedCallback();
    await Promise.resolve();
    assert.equal(visible.requests(), 1);
    assert.equal(removed.requests(), 0);
});
