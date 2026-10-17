import assert from 'node:assert/strict';
import test from 'node:test';
import vm from 'node:vm';
import { readFileSync } from 'node:fs';
import ts from 'typescript';

const compiled = ts.transpileModule(readFileSync(new URL('../src/components/PlaybackControls.ts', import.meta.url), 'utf8'), {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
}).outputText;

function harness() {
    const timers = new Map(), exports = {}, document = { hidden: false };
    let nextId = 0, renders = 0, fresh = true;
    vm.runInNewContext(compiled, {
        exports, require: () => ({}), document,
        setInterval: callback => { const id = ++nextId; timers.set(id, callback); return id; },
        clearInterval: id => timers.delete(id),
    });
    const controls = Object.create(exports.PlaybackControls.prototype);
    Object.assign(controls, {
        disposed: false, container: { isConnected: true },
        snapshot: { current: { occurrenceId: 'track' }, playback: { status: 'active' } },
        fresh: () => fresh,
        renderTimeline: () => { renders++; controls.scheduleRepaint(); },
        destroy: () => { controls.disposed = true; controls.scheduleRepaint(); },
    });
    return { controls, document, timers, setFresh: value => { fresh = value; },
        tick: () => { for (const callback of [...timers.values()]) callback(); },
        renders: () => renders };
}

test('paused and idle sessions perform no periodic repaints; resume starts one timer', () => {
    const h = harness();
    h.controls.snapshot.playback.status = 'paused';
    h.controls.scheduleRepaint();
    for (let i = 0; i < 10; i++) h.tick();
    assert.equal(h.renders(), 0);
    h.controls.snapshot.playback.status = 'active';
    h.controls.scheduleRepaint(); h.controls.scheduleRepaint();
    assert.equal(h.timers.size, 1);
    h.tick(); assert.equal(h.renders(), 1);
    h.controls.snapshot.current = null;
    h.controls.scheduleRepaint();
    assert.equal(h.timers.size, 0);
});

test('hidden, stale, and pending-seek states stop interpolation and can resume', () => {
    const h = harness();
    for (const [stop, resume] of [
        [() => { h.document.hidden = true; }, () => { h.document.hidden = false; }],
        [() => h.setFresh(false), () => h.setFresh(true)],
        [() => { h.controls.snapshot.playback.pendingSeek = {}; }, () => { h.controls.snapshot.playback.pendingSeek = null; }],
    ]) {
        h.controls.scheduleRepaint(); assert.equal(h.timers.size, 1);
        stop(); h.controls.scheduleRepaint(); assert.equal(h.timers.size, 0);
        resume(); h.controls.scheduleRepaint(); assert.equal(h.timers.size, 1);
    }
    h.controls.container.isConnected = false;
    h.tick();
    assert.equal(h.controls.disposed, true);
    assert.equal(h.timers.size, 0);
});
