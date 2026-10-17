import assert from 'node:assert/strict';
import test from 'node:test';
import vm from 'node:vm';
import { readFileSync } from 'node:fs';
import ts from 'typescript';

const deferred = () => {
    let resolve, reject;
    const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
    return { promise, resolve, reject };
};
function harness() {
    class Element {
        listeners = {}; elements = {}; style = {}; dataset = {}; value = ''; innerHTML = '';
        hidden = false; isConnected = true; loading = false; disabled = false; focused = false;
        addEventListener(type, handler) { (this.listeners[type] ??= []).push(handler); }
        removeEventListener(type, handler) { this.listeners[type] = (this.listeners[type] ?? []).filter(h => h !== handler); }
        async fire(type) {
            let prevented = false;
            await Promise.all((this.listeners[type] ?? []).map(h => h({ target: this, preventDefault() { prevented = true; } })));
            return prevented;
        }
        querySelector(selector) { return this.elements[selector] ?? null; }
        querySelectorAll() { return []; }
        focus() { this.focused = true; }
    }
    const root = new Element(), form = new Element(), picker = new Element();
    const url = new Element(), provider = new Element(), indicator = new Element(), button = new Element(), error = new Element();
    const back = new Element(), commit = new Element(), group = new Element();
    Object.assign(root.elements, { '#login-form': form, '#audiobookshelf-picker': picker, '#login-error': error, '#server-type-indicator': indicator });
    Object.assign(form.elements, { 'sl-input[name="url"]': url, 'sl-select[name="serverType"]': provider, 'sl-button': button });
    Object.assign(picker.elements, { '#audiobookshelf-picker-back': back, '#audiobookshelf-picker-commit': commit, 'sl-radio-group': group, '#audiobookshelf-picker-error': error });
    provider.value = 'auto'; url.value = 'https://example.test';
    const calls = [], timers = new Map(); let timerId = 0, successes = 0;
    let rpc = async () => ({}), cancel = async () => {}, commitRpc = async () => {};
    const exports = {};
    const source = readFileSync(new URL('../src/login.ts', import.meta.url), 'utf8') + '\nexport const probe = { bindLoginForm, renderAudiobookshelfPicker, loginFormHtml };';
    vm.runInNewContext(ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText, {
        exports, console: { log() {}, error() {} },
        FormData: class { get(key) { return { url: url.value, username: 'listener', password: 'secret' }[key] ?? null; } },
        customElements: { whenDefined: async () => {} },
        setTimeout: fn => { timers.set(++timerId, fn); return timerId; }, clearTimeout: id => timers.delete(id),
        require: name => name === './rpc' ? {
            rpcCall: (method, params) => { calls.push({ method, params }); return rpc(method, params); },
            audiobookshelfCancelSetup: id => { calls.push({ method: 'cancel', id }); return cancel(); },
            audiobookshelfCommit: params => { calls.push({ method: 'commit', params }); return commitRpc(); },
        } : name === './i18n' ? { t: key => key }
        : name === './serverIdentity' ? { SERVER_ICON_OPTIONS: [], defaultServerIcon: () => 'server', serverTypeLabel: x => x }
        : { isLoginProviderChoice: () => true, shouldUseAudiobookshelfDiscovery: () => false, validLibraryChoices: x => x, audiobookshelfRoleLabelKey: () => 'books' },
    });
    const success = () => { successes++; };
    return { ...exports.probe, root, form, picker, url, provider, indicator, button, error, back, commit, group, calls,
        bind() { exports.probe.bindLoginForm(root, 'add', {}, success); },
        render(libraries = []) { exports.probe.renderAudiobookshelfPicker(root, { setupId: 'setup', libraries }, {}, success); },
        runTimer() { const [id, fn] = [...timers].at(-1); timers.delete(id); return fn(); },
        setRpc(fn) { rpc = fn; }, setCancel(fn) { cancel = fn; }, setCommit(fn) { commitRpc = fn; },
        get successes() { return successes; },
    };
}

test('stale failed probe cannot erase a newer provider badge', async () => {
    const h = harness(); h.bind(); const old = deferred();
    h.setRpc(() => old.promise); await h.url.fire('sl-input'); const pending = h.runTimer();
    h.url.value = 'https://new.test'; h.setRpc(async () => ({ serverType: 'jellyfin' }));
    await h.url.fire('sl-input'); await h.runTimer();
    old.reject(new Error('offline')); await pending;
    assert.match(h.indicator.innerHTML, /jellyfin/);
});

test('repeated submits connect once and block dismiss until the request settles', async () => {
    const h = harness(); h.provider.value = 'jellyfin'; h.bind(); const request = deferred();
    h.setRpc(() => request.promise); const pending = h.form.fire('submit');
    await h.form.fire('submit'); assert.equal(h.calls.length, 1);
    assert.equal(await h.root.fire('sl-request-close'), true);
    request.resolve({}); await pending;
    assert.equal(h.successes, 1); assert.equal(h.button.loading, false);
    assert.equal(await h.root.fire('sl-request-close'), false);
});

test('failed connection preserves input, focuses error, and permits retry', async () => {
    const h = harness(); h.provider.value = 'jellyfin'; h.bind();
    h.setRpc(async () => { throw { message: 'Cannot reach server' }; }); await h.form.fire('submit');
    assert.equal(h.error.textContent, 'Cannot reach server'); assert.equal(h.error.focused, true);
    assert.equal(h.url.value, 'https://example.test');
    h.setRpc(async () => ({})); await h.form.fire('submit'); assert.equal(h.successes, 1);
});

test('closed view ignores queued and in-flight probe results', async () => {
    const h = harness(); h.bind(); const pending = deferred(); h.setRpc(() => pending.promise);
    await h.url.fire('sl-input'); const probe = h.runTimer(); await h.root.fire('sl-hide');
    pending.resolve({ serverType: 'jellyfin' }); await probe; assert.equal(h.indicator.innerHTML, '');
    await h.url.fire('sl-input'); h.root.isConnected = false; await h.runTimer();
    assert.equal(h.calls.length, 1);
});

test('empty library discovery keeps a Back action and cancels once', async () => {
    const h = harness(); h.render();
    assert.match(h.picker.innerHTML, /audiobookshelf-picker-back/);
    assert.doesNotMatch(h.picker.innerHTML, /id="audiobookshelf-picker-commit"/);
    const pending = deferred(); h.setCancel(() => pending.promise);
    const goingBack = h.back.fire('click'); await h.back.fire('click');
    assert.equal(h.calls.length, 1); pending.resolve(); await goingBack;
    assert.equal(h.form.hidden, false); assert.equal(h.picker.hidden, true);
    assert.equal(h.url.focused, true); assert.equal(h.picker.dataset.setupId, undefined);
});

test('commit cannot race a second commit or Back', async () => {
    const h = harness(); h.render([{ choiceId: 'books', name: 'Books', role: 'audiobook' }]); h.group.value = 'books';
    const pending = deferred(); h.setCommit(() => pending.promise);
    const saving = h.commit.fire('click'); await h.commit.fire('click'); await h.back.fire('click');
    assert.equal(h.calls.length, 1); assert.equal(await h.root.fire('sl-request-close'), true);
    pending.resolve(); await saving; assert.equal(h.successes, 1);
    assert.equal(await h.root.fire('sl-request-close'), false);
});

test('prefilled URL preserves literal entities and cannot create markup', () => {
    const h = harness(); const html = h.loginFormHtml({ prefillUrl: 'https://host/?q=&quot;<tag>' });
    assert.match(html, /value="https:\/\/host\/\?q=&amp;quot;&lt;tag&gt;" readonly/);
    assert.match(html, /id="login-error"[^>]*role="alert"/);
});


test('null rejection still restores a usable form and announces the fallback', async () => {
    const h = harness(); h.provider.value = 'jellyfin'; h.bind();
    h.setRpc(async () => { throw null; }); await h.form.fire('submit');
    assert.equal(h.error.textContent, 'login.authentication_failed');
    assert.equal(h.error.focused, true); assert.equal(h.button.loading, false);
});
