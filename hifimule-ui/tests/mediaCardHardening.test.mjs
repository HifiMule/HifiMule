import assert from 'node:assert/strict';
import test from 'node:test';
import vm from 'node:vm';
import { readFileSync } from 'node:fs';
import ts from 'typescript';

const compile = name => ts.transpileModule(readFileSync(new URL(`../src/components/${name}.ts`, import.meta.url), 'utf8'), {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
}).outputText;
const compiled = compile('MediaCard');
const compiledTrackPlay = compile('TrackPlayButton');
const compiledAlbumPlay = compile('AlbumPlayButton');
const compiledPlaybackAction = compile('playbackAction');
const tick = () => new Promise(resolve => setImmediate(resolve));

function harness() {
    let document;
    class Element {
        constructor(tag = 'div') {
            this.tag = tag; this.children = []; this.listeners = {}; this.attributes = {};
            this.dataset = {}; this.style = {}; this.isConnected = true; this.open = false;
            this.classList = { add() {}, remove() {}, contains: () => false, toggle() {} };
            this.offsetWidth = 100; this.offsetHeight = 40;
        }
        set innerHTML(value) {
            this.html = value; this.children = []; this.nodes = {};
            for (const match of value.matchAll(/<(sl-[\w-]+|button|div|hm-card-artwork)\b([^>]*)>/g)) {
                const node = new Element(match[1]), attrs = match[2];
                for (const attr of attrs.matchAll(/([\w-]+)="([^"]*)"/g)) if (attr[1] !== 'style') node[attr[1]] = attr[2];
                if (node.id) this.nodes['#' + node.id] = node;
                if (node.class) this.nodes['.' + node.class] = node;
                if (node.tag === 'sl-icon-button') node.shadowRoot = { querySelector: () => node.internal ??= new Element('button') };
                this.appendChild(node);
            }
        }
        get innerHTML() { return this.html ?? this.textContent ?? ''; }
        querySelector(selector) { return this.nodes?.[selector] ?? null; }
        querySelectorAll(selector) { return this.children.filter(node => node.tag === selector); }
        appendChild(node) { node.parent = this; this.children.push(node); return node; }
        setAttribute(name, value) { this.attributes[name] = value; }
        removeAttribute(name) { delete this.attributes[name]; }
        addEventListener(type, callback) { (this.listeners[type] ??= []).push(callback); }
        removeEventListener(type, callback) { this.listeners[type] = (this.listeners[type] ?? []).filter(fn => fn !== callback); }
        async emit(type, event = {}) {
            const promises = (this.listeners[type] ?? []).map(fn => fn({ target: this, stopPropagation() {}, preventDefault() {}, ...event }));
            await Promise.all(promises);
        }
        focus() { document.activeElement = this; }
        contains(node) { return node === this || this.children.some(child => child.contains(node)); }
        remove() { this.isConnected = false; if (this.parent) this.parent.children = this.parent.children.filter(node => node !== this); }
        show() { this.open = true; }
        hide() { this.open = false; }
        setCustomValidity() {}
        reportValidity() {}
    }
    document = new Element(); document.body = new Element('body');
    document.createElement = tag => {
        const element = new Element(tag);
        if (tag === 'sl-dialog') {
            element.panel = new Element();
            element.closeButton = new Element('sl-icon-button');
            element.shadowRoot = { querySelector: () => element.closeButton };
        }
        return element;
    };
    const requests = [], toasts = [], items = new Map(), exports = {};
    const rpcCall = (method, params) => new Promise((resolve, reject) => requests.push({ method, params, resolve, reject }));
    const action = {}, trackPlay = {}, albumPlay = {};
    vm.runInNewContext(compiledPlaybackAction, {
        exports: action, document, require: () => ({ showToast: message => toasts.push(message) }),
    });
    for (const [source, exports] of [[compiledTrackPlay, trackPlay], [compiledAlbumPlay, albumPlay]]) {
        vm.runInNewContext(source, {
            exports, document,
            require: name => name === './playbackAction' ? action
                : name === '../rpc' ? {
                    playbackPlayTrack: (serverId, trackId) => rpcCall('playback.playTrack', { serverId, trackId }),
                    playbackPlayAlbum: (serverId, albumId) => rpcCall('playback.playAlbum', { serverId, albumId }),
                } : name === '../i18n' ? { t: (key, params) => `${key}: ${params.title}` } : {},
        });
    }
    const basketStore = { has: id => items.has(id), add: item => items.set(item.id, item), remove: id => items.delete(id),
        admitPhysicalTargetMutation: () => true, getActiveServerId: () => 'server' };
    vm.runInNewContext(compiled, {
        exports, document, console: { error() {}, warn() {} },
        customElements: { whenDefined: async () => {} }, requestAnimationFrame: fn => fn(),
        window: Object.assign(new Element(), { innerWidth: 900, innerHeight: 640 }),
        require: name => name === '../rpc' ? { rpcCall, fetchBrowseAlbum: id => rpcCall('book', { id }) }
            : name === '../state/basket' ? { basketStore }
            : name === '../i18n' ? { t: (key, params) => params?.message ?? key }
            : name === '../toast' ? { showToast: message => toasts.push(message) }
            : name === '../state/libraryCache' ? { invalidatePlaylistsCache() {} }
            : name === './RadioPlayButton' ? { createRadioPlayButton: () => null }
            : name === './AlbumPlayButton' ? albumPlay
            : name === './TrackPlayButton' ? trackPlay
            : name === '../state/mediaSyncSelection' ? { bookBasketItem: id => ({ id }) } : {},
    });
    return { MediaCard: exports.MediaCard, Element, document, requests, toasts, items };
}

for (const [type, id, label, method, idKey] of [
    ['Book', 'book-1', 'play_book', 'playback.playAlbum', 'albumId'],
    ['BookPart', 'part-2', 'play_part', 'playback.playTrack', 'trackId'],
]) {
    test(`${type} cards play their source through the shared button without music-only actions`, async () => {
        const h = harness();
        const card = h.MediaCard.create({ id, name: 'Chapitre é — 夜', type, serverId: 'books-server' }, 'items', false, () => {});
        const actions = card.querySelector('.card-content').children;
        assert.equal(actions.length, 1, 'Books and parts have play, without preview or queue actions');
        const play = actions[0];
        assert.equal(play.label, `library.books.${label}: Chapitre é — 夜`);
        const playing = play.emit('click');
        assert.equal(h.requests.length, 1);
        assert.equal(h.requests[0].method, method);
        assert.deepEqual({ ...h.requests[0].params }, { serverId: 'books-server', [idKey]: id });
        h.requests[0].resolve();
        await playing;
        assert.equal(play.disabled, false);
    });
}

test('production card refresh updates the accessible action in both directions', () => {
    const h = harness();
    const card = h.MediaCard.create({ id: 'album', name: 'Album', type: 'MusicAlbum' }, 'items', false, () => {});
    const root = { querySelectorAll: () => [card] }, button = card.querySelector('.basket-toggle-btn');
    h.items.set('album', {}); h.MediaCard.refreshSelection(root);
    assert.equal(button.name, 'dash-circle-fill'); assert.equal(button.label, 'tracks.view.remove_from_basket — Album');
    h.items.clear(); h.MediaCard.refreshSelection(root);
    assert.equal(button.name, 'plus-circle-fill'); assert.equal(button.label, 'tracks.view.add_to_basket — Album');
});

for (const type of ['Book', 'MusicAlbum']) {
    test(`${type} additions reject repeated activation and recover after failure`, async () => {
        const h = harness();
        const card = h.MediaCard.create({ id: 'album', name: 'Album', type, serverId: 'server' }, 'items', false, () => {});
        const button = card.querySelector('.basket-toggle-btn');
        const adding = button.emit('click');
        for (let i = 0; i < 10; i++) await button.emit('click');
        assert.equal(h.requests.length, type === 'Book' ? 1 : 2);
        assert.equal(button.attributes['aria-busy'], 'true');
        assert.equal(button.internal.attributes['aria-disabled'], 'true');
        assert.equal(card.querySelector('.card-image').children.filter(n => n.className === 'nav-loading-overlay').length, 1);
        h.requests.forEach(request => request.reject('offline')); await adding;
        assert.equal(h.toasts.at(-1), 'offline'); assert.equal(button.attributes['aria-busy'], undefined);
        assert.equal(card.querySelector('.card-image').children.filter(n => n.className === 'nav-loading-overlay').length, 0);
        const retry = button.emit('click');
        h.requests.slice(type === 'Book' ? 1 : 2).forEach(request => request.resolve(type === 'Book' ? { tracks: [] } : []));
        await retry; assert.equal(h.items.has('album'), true);
    });
}

for (const key of ['Escape', 'Tab']) {
    test(`context menu ${key} restores its invoking control without swallowing Tab`, async () => {
        const h = harness(), trigger = new h.Element('button'); trigger.focus();
        h.MediaCard.showItemContextMenu(5, 5, 'album', 'Album', trigger);
        assert.notEqual(h.document.activeElement, trigger);
        let prevented = false;
        await h.document.emit('keydown', { key, preventDefault() { prevented = true; } });
        assert.equal(h.document.activeElement, trigger); assert.equal(h.document.body.children.length, 0);
        if (key === 'Tab') assert.equal(prevented, false);
    });
}

for (const shortcut of [{ key: 'F10', shiftKey: true }, { key: 'ContextMenu' }]) {
    test(`playlist menu opens with ${shortcut.key} and returns focus on Escape`, async () => {
        const h = harness(), trigger = new h.Element('button');
        trigger.getBoundingClientRect = () => ({ left: 80, bottom: 120 });
        h.MediaCard.bindPlaylistMenuKeyboard(trigger, 'album', 'Album');
        trigger.focus();
        await trigger.emit('keydown', { key: 'F10' });
        assert.equal(h.document.body.children.length, 0);
        let prevented = false;
        await trigger.emit('keydown', { ...shortcut, preventDefault() { prevented = true; } });
        assert.equal(prevented, true);
        const menu = h.document.body.children[0];
        assert.equal(menu.attributes.role, 'menu');
        assert.equal(menu.style.left, '80px');
        assert.equal(menu.style.top, '120px');
        assert.equal(h.document.activeElement.attributes.role, 'menuitem');
        await h.document.emit('keydown', { key: 'Escape' });
        assert.equal(h.document.activeElement, trigger);
        assert.equal(h.document.body.children.length, 0);
    });
}

test('create playlist guards submit and every dismissal, then preserves input on failure', async () => {
    const h = harness(); h.MediaCard.openCreatePlaylistDialog(['track'], ''); await tick();
    const dialog = h.document.body.children[0], input = dialog.querySelector('#ctx-playlist-name');
    const create = dialog.querySelector('#ctx-playlist-create'), cancel = dialog.querySelector('#ctx-playlist-cancel');
    input.value = '夜の音楽 🎵 ' + 'Long name '.repeat(20);
    const writing = create.emit('click'); await create.emit('click'); await tick();
    assert.equal(h.requests.length, 1); assert.equal(cancel.disabled, true); assert.equal(input.disabled, true);
    assert.equal(dialog.closeButton.disabled, true); assert.equal(h.document.activeElement, dialog.panel);
    await cancel.emit('click'); assert.equal(dialog.open, true);
    let prevented = false;
    await dialog.emit('sl-request-close', { preventDefault() { prevented = true; } }); assert.equal(prevented, true);
    h.requests[0].reject('offline'); await writing;
    assert.equal(dialog.open, true); assert.equal(cancel.disabled, false); assert.equal(input.disabled, false);
    assert.equal(dialog.closeButton.disabled, false);
    assert.match(input.value, /夜の音楽/); assert.equal(dialog.querySelector('#ctx-playlist-error').textContent, 'offline');
    assert.equal(h.document.activeElement, create);
    const retry = create.emit('click'); await tick(); h.requests[1].resolve({}); await retry; assert.equal(dialog.open, false);
});

test('playlist picker serializes writes across choices and unlocks all actions after failure', async () => {
    const h = harness(); h.MediaCard.openAddToPlaylistDialog(['track'], 'Track'); await tick();
    h.requests[0].resolve({ playlists: [{ id: 'one', name: 'One' }, { id: 'two', name: 'Two' }] }); await tick();
    const dialog = h.document.body.children[0], list = dialog.querySelector('#ctx-track-playlist-list');
    const [newButton, one, two] = list.querySelectorAll('sl-button'), cancel = dialog.querySelector('#ctx-track-cancel');
    const writing = one.emit('click'); await two.emit('click'); await newButton.emit('click'); await cancel.emit('click');
    assert.equal(h.requests.length, 2); assert.equal(dialog.open, true);
    assert.equal(two.disabled, true); assert.equal(cancel.disabled, true);
    let prevented = false; await dialog.emit('sl-request-close', { preventDefault() { prevented = true; } }); assert.equal(prevented, true);
    h.requests[1].reject('offline'); await writing;
    assert.equal(dialog.querySelector('#ctx-track-error').textContent, 'offline'); assert.equal(h.document.activeElement, one);
    assert.equal(two.disabled, false); assert.equal(cancel.disabled, false); assert.equal(newButton.disabled, false);
    const retry = two.emit('click'); h.requests[2].resolve({}); await retry; assert.equal(dialog.open, false);
});
