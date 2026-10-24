import { loadProfiles } from './deviceProfileTestHelpers.mjs';
import { escapeAttr } from '../src/html.ts';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import vm from 'node:vm';
import ts from 'typescript';

class Element {
    children = new Map(); listeners = new Map(); checked = false; value = ''; hidden = false;
    set innerHTML(html) {
        this.html = html;
        this.children.clear();
        for (const tag of html.matchAll(/<[^>]+id="([^"]+)"[^>]*>/g)) {
            const child = new Element();
            child.checked = /\schecked(?:\s|>)/.test(tag[0]);
            child.hidden = /\shidden(?:\s|>)/.test(tag[0]);
            child.value = tag[0].match(/value="([^"]*)"/)?.[1] ?? '';
            this.children.set('#' + tag[1], child);
        }
    }
    querySelector(selector) { return this.children.get(selector); }
    querySelectorAll() { return []; }
    addEventListener(event, fn) { this.listeners.set(event, fn); }
    emit(event) { return this.listeners.get(event)?.(); }
    cloneNode() { const copy = new Element(); copy.replaceWith = () => {}; return copy; }
    replaceWith() {} hide() {}
}
function setup(profiles = []) {
    let profileSaved;
    const calls = [], body = new Element(), confirm = new Element();
    const dialog = new Element();
    dialog.querySelector = selector => selector === '.init-device-body' ? body
        : selector === '#init-device-confirm-btn' ? confirm : body.querySelector(selector);
    const source = readFileSync(new URL('../src/components/InitDeviceModal.ts', import.meta.url), 'utf8');
    const script = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText;
    const ctx = { exports: {}, require: name => name === '../deviceProfiles' ? {...loadProfiles(),bindProfileActions:(_select,_initial,_profiles,callback)=>{profileSaved=callback;}} : name === '../html' ? { escapeAttr }
        : name === '../rpc' ? { rpcCall: async (method, params) => { calls.push({ method, params }); return {}; } }
        : name === '../i18n' ? { t: key => key }
        : name === '../deviceIcon' ? { deviceIconLabel: icon => icon } : {} };
    vm.runInNewContext(script, ctx);
    const modal = new ctx.exports.InitDeviceModal({});
    Object.assign(modal, { dialog, pendingId: 'pending-target', observedDestinationRevision: '7' });
    modal.renderContent(body, 'listener', profiles);
    return { modal, body, calls, profileSaved };
}
test('creation shows localized reconciliation and artwork choices with every option off', () => {
    const { body } = setup();
    for (const id of ['reconcile', 'artwork']) {
        assert.equal(body.querySelector(`#init-device-${id}`).checked, false);
    }
    for (const kind of ['windows', 'macos', 'kde', 'gnome']) assert.equal(body.querySelector(`#init-device-artwork-${kind}`), undefined);
    for (const key of ['reconcile_before_sync', 'reconcile_hint', 'artwork', 'artwork_hint']) {
        assert.ok(body.html.includes(`basket.device.${key}`));
        const catalog = JSON.parse(readFileSync(new URL('../../hifimule-i18n/catalog.json', import.meta.url), 'utf8'));
        for (const language of ['en', 'fr', 'es', 'de']) assert.ok(catalog[language][`basket.device.${key}`]);
    }
});
test('untouched creation explicitly saves all preferences off', async () => {
    const { modal, calls } = setup();
    await modal.handleConfirm('listener', 'usb-drive', { value: 'Player' });
    const params = JSON.parse(JSON.stringify(calls[0].params));
    assert.equal(params.reconcileBeforeSync, false);
    assert.deepEqual(params.folderArtwork, { enabled: false });
});
test('creation captures enabled and disabled preferences before submitting in the single request', async () => {
    for (const enabled of [true, false]) {
        const { modal, body, calls } = setup();
        body.querySelector('#init-device-reconcile').checked = enabled;
        body.querySelector('#init-device-artwork').checked = enabled;
        await modal.handleConfirm('listener', 'usb-drive', { value: 'Player' });
        assert.equal(body.querySelector('#init-device-reconcile'), undefined, 'submission replaces the form');
        assert.equal(calls.length, 1);
        assert.equal(calls[0].method, 'device_initialize');
        const params = JSON.parse(JSON.stringify(calls[0].params));
        assert.equal(params.reconcileBeforeSync, enabled);
        assert.deepEqual(params.folderArtwork, { enabled });
        assert.equal(params.pendingId, 'pending-target');
        assert.equal(params.observedDestinationRevision, '7');
    }
});

test('profile action sentinels never reach initialization writes',async()=>{
 const {modal,body,calls}=setup();body.querySelector('#init-transcoding-profile').value='__profile_new__';
 await modal.handleConfirm('listener','usb-drive',{value:'Player'});assert.equal(calls.length,0);
});

test('saved profile applies defaults only to individually untouched initialization folders',()=>{
 const fields=[['init-folder-input','defaultMusicFolder'],['init-playlist-folder-input','defaultPlaylistFolder'],['init-audiobook-folder-input','defaultAudiobookFolder'],['init-podcast-folder-input','defaultPodcastFolder']];
 for(const touched of ['none',...fields.map(([id])=>id)]){
  const {body,profileSaved}=setup();
  if(touched!=='none'){const input=body.querySelector('#'+touched);input.value='My/Folder';input.emit('sl-input');}
  profileSaved([],{id:'custom:saved',name:'Saved',...Object.fromEntries(fields.map(([,key])=>[key,'New/'+key]))});
  for(const [id,key] of fields)assert.equal(body.querySelector('#'+id).value,touched===id?'My/Folder':'New/'+key);
 }
});

test('initial profile media defaults reach initialization and missing defaults inherit music',async()=>{
 const profile={id:'passthrough',name:'Originals',defaultAudiobookFolder:'Books',defaultPodcastFolder:'Shows'};
 const {modal,body,calls}=setup([profile]);
 assert.equal(body.querySelector('#init-audiobook-folder-input').value,'Books');
 assert.equal(body.querySelector('#init-podcast-folder-input').value,'Shows');
 await modal.handleConfirm('listener','usb-drive',{value:'Player'});
 assert.equal(calls[0].params.audiobookFolderPath,'Books');assert.equal(calls[0].params.podcastFolderPath,'Shows');
 const legacy=setup([{id:'passthrough',name:'Originals'}]);
 assert.equal(legacy.body.querySelector('#init-audiobook-folder-input').value,'');
 legacy.profileSaved([],{id:'custom:new',defaultAudiobookFolder:'Books',defaultPodcastFolder:'Shows'});
 const select=legacy.body.querySelector('#init-transcoding-profile');select.value='passthrough';select.listeners.get('sl-change')({target:select});
 assert.equal(legacy.body.querySelector('#init-audiobook-folder-input').value,'');assert.equal(legacy.body.querySelector('#init-podcast-folder-input').value,'');
});


test('creation seeds artwork from initial, selected and newly saved profiles, then persists it',async()=>{
 const initial=setup([{id:'passthrough',name:'Originals',defaultFolderArtwork:true}]);
 assert.equal(initial.body.querySelector('#init-device-artwork').checked,true);
 await initial.modal.handleConfirm('listener','usb-drive',{value:'Player'});
 assert.equal(initial.calls[0].params.folderArtwork.enabled,true);
 const rockbox={id:'system:rockbox-mp3-320',name:'Rockbox',defaultFolderArtwork:true};
 const legacy={id:'passthrough',name:'Originals'};
 const {body,profileSaved,modal,calls}=setup([legacy,rockbox]),select=body.querySelector('#init-transcoding-profile'),artwork=body.querySelector('#init-device-artwork');
 for(const [id,expected] of [[rockbox.id,true],[legacy.id,false],[rockbox.id,true]]){
  select.value=id;select.listeners.get('sl-change')({target:select});assert.equal(artwork.checked,expected);
 }
 profileSaved([],{id:'custom:saved',defaultFolderArtwork:false});assert.equal(artwork.checked,false);
 profileSaved([],{id:'custom:saved',defaultFolderArtwork:true});assert.equal(artwork.checked,true);
 await modal.handleConfirm('listener','usb-drive',{value:'Player'});assert.equal(calls[0].params.folderArtwork.enabled,true);
});
test('manual artwork choices survive profile switches and saved-profile callbacks during creation',()=>{
 for(const enabled of [true,false]){
  const profile={id:'system:rockbox',name:'Rockbox',defaultFolderArtwork:!enabled};
  const {body,profileSaved}=setup([profile]),artwork=body.querySelector('#init-device-artwork');
  artwork.checked=enabled;artwork.emit('sl-change');
  const select=body.querySelector('#init-transcoding-profile');select.value=profile.id;select.listeners.get('sl-change')({target:select});assert.equal(artwork.checked,enabled);
  profileSaved([],{...profile,id:'custom:new'});assert.equal(artwork.checked,enabled);
 }
});
