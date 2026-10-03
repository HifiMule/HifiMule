import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import ts from 'typescript';
import { runInNewContext } from 'node:vm';

const source = await readFile(new URL('../src/components/TraySetupGuidance.ts', import.meta.url), 'utf8');
const js = ts.transpileModule(source.replace(/import[^;]+;/, ''), { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS } }).outputText;
function environment(storageFails = false) {
    class Element {
        constructor(tag) { this.tag = tag; this.children = []; this.hidden = false; this.handlers = {}; this.attrs = {}; this.parentElement = null; }
        get isConnected() { return this === body || !!this.parentElement?.isConnected; }
        append(...children) { for (const child of children) { child.parentElement = this; this.children.push(child); } }
        replaceChildren(...children) { for (const child of this.children) child.parentElement = null; this.children = []; this.append(...children); }
        remove() { if (this.parentElement) this.parentElement.children = this.parentElement.children.filter(child => child !== this); this.parentElement = null; }
        setAttribute(key, value) { this.attrs[key] = value; }
        addEventListener(event, fn) { this.handlers[event] = fn; }
    }
    const body = new Element('body'); const root = new Element('main'); body.append(root);
    const document = { body, createElement: tag => new Element(tag) };
    const store = new Map(); const storage = { getItem(key) { if (storageFails) throw Error('blocked'); return store.get(key); }, setItem(key, value) { if (storageFails) throw Error('blocked'); store.set(key,value); } };
    const context = { exports: {}, document, sessionStorage: storage, t: key => key };
    runInNewContext(js, context);
    return { body, root, storage, mount: context.exports.mountTraySetupGuidance };
}
const flush = async () => { for (let i = 0; i < 8; i++) await Promise.resolve(); };
const children = element => [element, ...element.children.flatMap(children)];
const button = (anchor, label) => children(anchor).find(el => el.tag === 'button' && el.textContent === 'traySetup.' + label);
const notice = env => env.body.children.find(el => el.tag === 'aside');

for (const state of ['available', 'unknown', 'not-applicable']) test(`${state} does not claim missing support`, async () => {
    const env = environment(); env.mount(async () => ({state, fedora: false}), async () => {}); await flush(); assert.equal(notice(env).hidden, true);
});
test('rejected initial probe does not claim missing support', async () => {
    const env = environment(); env.mount(async () => { throw Error('bus'); }, async () => {}); await flush(); assert.equal(notice(env).hidden, true);
});
for (const fedora of [true, false]) test(`missing host shows ${fedora ? 'Fedora commands' : 'generic instructions'} and survives onboarding`, async () => {
    const env = environment(); const urls = []; env.mount(async () => ({state: 'missing', fedora}), async url => { urls.push(url); }); await flush();
    const anchor = notice(env); assert.equal(anchor.hidden, false); assert.equal(anchor.parentElement, env.body);
    assert.ok(children(anchor).some(el => el.textContent === 'traySetup.' + (fedora ? 'fedora' : 'generic')));
    assert.equal(children(anchor).filter(el => el.tag === 'code').length, fedora ? 1 : 0);
    assert.ok(children(anchor).some(el => el.textContent === 'traySetup.activation'));
    assert.ok(children(anchor).some(el => el.attrs.role === 'status' && el.attrs['aria-live'] === 'polite'));
    env.root.replaceChildren(); assert.equal(anchor.isConnected, true);
    button(anchor, 'extensionPage').handlers.click(); await flush(); assert.deepEqual(urls, ['https://extensions.gnome.org/extension/615/appindicator-support/']);
});
test('recheck preserves notice on unknown, then removes it when available', async () => {
    const env = environment(); let state = 'missing'; let calls = 0;
    env.mount(async () => { calls++; return {state, fedora: true}; }, async () => {}); await flush();
    const anchor = notice(env); state = 'unknown'; button(anchor, 'checkAgain').handlers.click(); await flush();
    assert.equal(anchor.hidden, false); assert.ok(children(anchor).some(el => el.textContent === 'traySetup.unknown'));
    state = 'available'; button(anchor, 'checkAgain').handlers.click(); await flush(); assert.equal(anchor.hidden, true); assert.equal(calls, 3);
});
for (const failure of [false, true]) test(`session dismissal survives remount with storage ${failure ? 'failure' : 'available'}`, async () => {
    const env = environment(failure); let calls = 0; const probe = async () => { calls++; return {state:'missing', fedora:false}; };
    env.mount(probe, async () => {}); await flush(); button(notice(env),'dismiss').handlers.click();
    assert.equal(notice(env), undefined); env.mount(probe, async () => {}); await flush(); assert.equal(calls,1);
});
test('shutdown discards late initial detection and late recheck', async () => {
    for (const recheck of [false,true]) {
        const env = environment(); let resolve; let initial = true;
        env.mount(() => { if (recheck && initial) { initial = false; return Promise.resolve({state:'missing',fedora:true}); } return new Promise(done => {resolve=done;}); }, async () => {});
        await flush(); const anchor = notice(env);
        if (recheck) button(anchor,'checkAgain').handlers.click();
        env.body.replaceChildren(); resolve({state:'missing',fedora:true}); await flush();
        assert.equal(env.body.children.length,0); assert.equal(anchor.isConnected,false);
    }
});
test('recheck is serialized and dismissal discards pending result', async () => {
    const env = environment(); let calls = 0, resolve;
    env.mount(() => { calls++; return calls === 1 ? Promise.resolve({state:'missing',fedora:true}) : new Promise(done => {resolve=done;}); }, async () => {}); await flush();
    const anchor = notice(env); const check = button(anchor,'checkAgain'); check.handlers.click(); check.handlers.click(); assert.equal(calls,2); assert.equal(check.disabled,true);
    button(anchor,'dismiss').handlers.click(); resolve({state:'missing',fedora:true}); await flush(); assert.equal(notice(env),undefined);
});

test('blocked sessionStorage getter still permits guidance and memory dismissal', async () => {
    const env = environment(); const context = { exports: {}, document: {body: env.body, createElement: tag => new env.body.constructor(tag)}, t: key => key };
    Object.defineProperty(context, 'sessionStorage', { get() { throw Error('SecurityError'); } });
    runInNewContext(js, context);
    context.exports.mountTraySetupGuidance(async () => ({state:'missing',fedora:false}), async () => {}); await flush();
    assert.equal(notice(env).hidden, false); button(notice(env),'dismiss').handlers.click(); assert.equal(notice(env),undefined);
});

test('hydration acknowledgment precedes nonblocking guidance and translations are complete', async () => {
    const main = await readFile(new URL('../src/main.ts', import.meta.url), 'utf8');
    const init = main.slice(main.indexOf('async function init()'), main.indexOf('function observeShutdown'));
    assert.ok(init.indexOf("invoke<string | null>('report_ui_ready')") < init.indexOf('mountTraySetupGuidance('));
    assert.doesNotMatch(init, /await mountTraySetupGuidance/);
    const catalog = JSON.parse(await readFile(new URL('../../hifimule-i18n/catalog.json', import.meta.url), 'utf8'));
    const keys = Object.keys(catalog.en).filter(key => key.startsWith('traySetup.'));
    assert.equal(keys.length, 11);
    for (const language of ['en','fr','es','de']) for (const key of keys) assert.ok(catalog[language][key], `${language}: ${key}`);
});
