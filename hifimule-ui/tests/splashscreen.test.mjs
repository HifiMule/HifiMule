import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import vm from 'node:vm';
import ts from 'typescript';

const source = await readFile(new URL('../src/splashscreen.ts', import.meta.url), 'utf8');
const js = ts.transpileModule(source.replace("import { t, initializeI18n } from './i18n';", '')
    .replaceAll("import('@tauri-apps/api/core')", 'Promise.resolve({ invoke: globalThis.invoke })'), {
    compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.None },
}).outputText;

function fixture(status) {
    const events = new Map(), timers = new Map(), calls = [], elements = new Map();
    let reloads = 0;
    for (const id of ['status-text', 'container', 'retry-btn', 'close-btn']) {
        elements.set(id, { textContent: '', classes: new Set(), handlers: new Map(), focused: false,
            classList: { add(value) { elements.get(id).classes.add(value); } },
            addEventListener(event, handler) { this.handlers.set(event, handler); },
            focus() { this.focused = true; },
        });
    }
    vm.runInNewContext(js, {
        t: key => key, initializeI18n: async () => {}, console,
        document: { getElementById: id => elements.get(id) },
        window: { addEventListener: (event, handler) => events.set(event, handler), location: { reload: () => reloads++ } },
        invoke: async command => { calls.push(command); if (command === 'get_sidecar_status') return status; },
        setTimeout: (handler, delay) => { timers.set(1, { handler, delay }); return 1; },
        clearTimeout: id => timers.delete(id),
    });
    return { events, timers, calls, elements, reloads: () => reloads };
}

test('splash polls readiness without hydrating the application and stops on pagehide', async () => {
    const f = fixture({ state: 'ready' });
    await f.events.get('DOMContentLoaded')();
    await new Promise(resolve => setImmediate(resolve));
    assert.equal(f.elements.get('status-text').textContent, 'ui.splash.daemon_ready');
    assert.deepEqual(f.calls, ['get_sidecar_status']);
    assert.equal(f.timers.get(1).delay, 250);
    f.events.get('pagehide')();
    assert.equal(f.timers.size, 0);
});

test('failure exposes retry and close; retry restarts daemon before reloading windows', async () => {
    const f = fixture({ state: 'failed', errorCode: 'SPAWN_FAILED' });
    await f.events.get('DOMContentLoaded')();
    await new Promise(resolve => setImmediate(resolve));
    assert.equal(f.elements.get('status-text').textContent, 'SPAWN_FAILED');
    assert.ok(f.elements.get('container').classes.has('error'));
    assert.ok(f.elements.get('retry-btn').focused);
    assert.equal(f.timers.size, 0);
    await f.elements.get('retry-btn').handlers.get('click')();
    assert.deepEqual(f.calls, ['get_sidecar_status', 'retry_daemon_startup', 'reload_main_window']);
    assert.equal(f.reloads(), 1);
    f.elements.get('close-btn').handlers.get('click')();
    await new Promise(resolve => setImmediate(resolve));
    assert.equal(f.calls.at(-1), 'close_ui');
});
