import assert from 'node:assert/strict';
import test from 'node:test';
import vm from 'node:vm';
import { readFileSync } from 'node:fs';
import ts from 'typescript';
function harness(withObserver = true) {
    const exports = {}, observers = [], requests = [];
    class Observer {
        constructor(callback) { this.callback = callback; observers.push(this); }
        observe() {}
        unobserve() {}
        disconnect() { this.disconnected = true; }
        show(target) { this.callback([{target, isIntersecting: true}]); }
    }
    vm.runInNewContext(ts.transpileModule(readFileSync(new URL('../src/components/basketArtwork.ts', import.meta.url), 'utf8'), {
        compilerOptions: {module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022},
    }).outputText, {exports, IntersectionObserver: withObserver ? Observer : undefined});
    const loader = new exports.BasketArtwork(id => new Promise((resolve, reject) => requests.push({id, resolve, reject})));
    const row = id => ({dataset: {imageId: id}, isConnected: true, style: {}});
    const mount = (rows, server = 'a') => loader.mount({querySelectorAll: () => rows}, server);
    return {loader, observers, requests, row, mount};
}
const flush = async () => { for (let i = 0; i < 4; i++) await Promise.resolve(); };
test('40 basket rows request only visible artwork and reuse pending/resolved results on redraw', async () => {
    const h = harness(); const rows = Array.from({length:40}, (_,i) => h.row(String(i)));
    h.mount(rows); assert.equal(h.requests.length,0);
    h.observers[0].show(rows[0]); assert.equal(h.requests.length,1);
    const replacement = h.row('0'); h.mount([replacement]); h.observers[1].show(replacement);
    assert.equal(h.requests.length,1); h.requests[0].resolve('cover'); await flush();
    assert.equal(rows[0].style.backgroundImage,undefined);
    assert.equal(replacement.style.backgroundImage,"url('cover')");
    h.mount([replacement]); h.observers[2].show(replacement); await flush();
    assert.equal(h.requests.length,1);
});
test('provider switches isolate equal IDs, and destroy ignores late visibility and responses', async () => {
    const h = harness(), a = h.row('same'), b = h.row('same');
    h.mount([a]); h.observers[0].show(a); h.mount([b],'b');
    h.observers[0].show(a); h.observers[1].show(b); assert.equal(h.requests.length,2);
    h.requests[0].resolve('old'); await flush(); assert.equal(a.style.backgroundImage,undefined);
    h.loader.destroy(); h.requests[1].resolve('new'); h.observers[1].show(b); await flush();
    assert.equal(b.style.backgroundImage,undefined); assert.equal(h.requests.length,2);
    assert.equal(h.observers[1].disconnected,true);
});
test('failed artwork retries on redraw; detached fallback rows do not request images', async () => {
    const h = harness(false), row = h.row('1'), detached = h.row('2'); detached.isConnected=false;
    h.mount([row,detached]); assert.equal(h.requests.length,1);
    h.requests[0].reject(new Error('offline')); await flush(); h.mount([row]);
    assert.equal(h.requests.length,2); h.requests[1].resolve('ok'); await flush();
    assert.equal(row.style.backgroundImage,"url('ok')");
});
test('cache remains bounded across large selections', async () => {
    const h = harness(false);
    h.mount(Array.from({length:129}, (_,i) => h.row(String(i))));
    for (const request of h.requests) request.resolve('cover'); await flush();
    h.mount([h.row('0')]); assert.equal(h.requests.length,130);
});
