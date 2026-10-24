import { loadProfiles } from './deviceProfileTestHelpers.mjs';
import { escapeAttr } from '../src/html.ts';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import ts from 'typescript';
const source = readFileSync(new URL('../src/components/BasketSidebar.ts', import.meta.url), 'utf8');
const script = ts.transpile(source.replace(/^import .*;\r?\n/gm, '').replace(/export class /g, 'class '), { target: ts.ScriptTarget.ES2022 });
const estimateScript = ts.transpile(readFileSync(new URL('../src/state/basketEstimate.ts', import.meta.url), 'utf8').replace(/^import .*;\r?\n/gm, '').replace(/export /g, ''), { target: ts.ScriptTarget.ES2022 });
const { basketEstimateKey } = new Function(`${estimateScript}\nreturn { basketEstimateKey };`)();
class Element {
    children = new Map(); listeners = new Map(); style = {}; checked = false; value = ''; loading = false;
    set innerHTML(html) {
        this.html = html;
        for (const tag of html.matchAll(/<[^>]+id="([^"]+)"[^>]*>/g)) {
            const child = new Element();
            child.checked = /\schecked(?:\s|>)/.test(tag[0]);
            child.value = tag[0].match(/value="([^"]*)"/)?.[1] ?? '';
            this.children.set('#' + tag[1], child);
        }
    }
    querySelector(selector) { return this.children.get(selector); }
    querySelectorAll() { return []; }
    addEventListener(event, fn) { this.listeners.set(event, fn); }
    click() { return this.listeners.get('click')?.(); }
    show() {} hide() {} remove() {}
}
async function setup(selected, current = {}) {
    let profileSaved;
    const calls = []; const dialog = new Element();
    const rpc = async (method, params) => { calls.push({ method, params }); return method === 'device_profiles.list' ? [] : {}; };
    const { BasketSidebar } = new Function('renderProfileOptions', 'bindProfileActions', 'isProfileAction', 'canonicalProfileId', 'displayProfile', 'escapeAttr', 'deviceIconLabel', 'basketEstimateKey', 'rpcCall', 'basketStore', 't', 'document', 'customElements', `${script}\nreturn { BasketSidebar };`)(loadProfiles().profileOptions, (_select,_initial,_profiles,callback)=>{profileSaved=callback;}, loadProfiles().isProfileAction, loadProfiles().canonicalProfileId, loadProfiles().displayProfile, escapeAttr, icon => icon, basketEstimateKey, rpc, { getItems: () => [] }, key => key, { createElement: () => dialog, body: { appendChild() {} } }, { whenDefined: async () => {} });
    const sidebar = Object.create(BasketSidebar.prototype);
    Object.assign(sidebar, { currentDevice: current, selectedDeviceSummary: () => ({ deviceId: 'device', ...selected }), refreshAndRender: async () => {} });
    await sidebar.openDeviceSettings();
    return { sidebar, dialog, calls, profileSaved };
}
test('device settings load the selected device value and default missing values off', async () => {
    for (const value of [undefined, false, true]) {
        const { dialog } = await setup({ reconcileBeforeSync: value });
        assert.equal(dialog.querySelector('#device-settings-reconcile').checked, value === true);
        assert.match(dialog.html, /basket.device.reconcile_hint/);
    }
    const { dialog } = await setup({ reconcileBeforeSync: false }, { reconcileBeforeSync: true });
    assert.equal(dialog.querySelector('#device-settings-reconcile').checked, false);
});
test('settings save sends both enabled and disabled values explicitly', async () => {
    for (const value of [true, false]) {
        const { dialog, calls } = await setup({ reconcileBeforeSync: !value });
        dialog.querySelector('#device-settings-reconcile').checked = value;
        await dialog.querySelector('#device-settings-save').click();
        const save = calls.find(call => call.method === 'device.update_manifest');
        assert.equal(save.params.reconcileBeforeSync, value);
        assert.equal(save.params.deviceId, 'device');
    }
});
test('changing reconciliation invalidates prepared capacity', async () => {
    const { sidebar } = await setup({}, { device_id: 'device', reconcileBeforeSync: false });
    const before = sidebar.capacityRevision();
    sidebar.currentDevice.reconcileBeforeSync = true;
    assert.notEqual(sidebar.capacityRevision(), before);
});
test('every supported language explains external file management and scan cost', () => {
    const catalog = JSON.parse(readFileSync(new URL('../../hifimule-i18n/catalog.json', import.meta.url), 'utf8'));
    for (const language of ['en', 'fr', 'es', 'de']) {
        assert.ok(catalog[language]['basket.device.reconcile_before_sync'].includes('HifiMule'));
        assert.ok(catalog[language]['basket.device.reconcile_hint']);
    }
});

test('folder artwork loads legacy values but saves only the device switch', async () => {
    const defaults = await setup({});
    assert.equal(defaults.dialog.querySelector('#device-settings-artwork').checked, false);
    for (const kind of ['windows','macos','kde','gnome']) assert.equal(defaults.dialog.querySelector(`#device-settings-artwork-${kind}`),undefined);
    const value = { enabled:true, windows:true, macos:false, kde:true, gnome:false };
    const { dialog,calls } = await setup({ folderArtwork:value });
    assert.equal(dialog.querySelector('#device-settings-artwork').checked,true);
    dialog.querySelector('#device-settings-artwork').checked=false;
    await dialog.querySelector('#device-settings-save').click();
    assert.deepEqual(calls.find(call=>call.method==='device.update_manifest').params.folderArtwork,{enabled:false});
});
test('all languages describe automatic integration during track updates', () => {
    const catalog=JSON.parse(readFileSync(new URL('../../hifimule-i18n/catalog.json',import.meta.url),'utf8'));
    for(const lang of ['en','fr','es','de']) for(const key of ['artwork','artwork_hint']) assert.ok(catalog[lang][`basket.device.${key}`]);
});

test('profile action sentinels never reach device settings writes',async()=>{
 const {dialog,calls}=await setup({});dialog.querySelector('#device-settings-transcoding-profile').value='__profile_edit__';
 await dialog.querySelector('#device-settings-save').click();assert.equal(calls.filter(c=>c.method==='device.update_manifest').length,0);
});

test('saved profile selection updates passthrough recovery and retains touched folder drafts',async()=>{
 const {dialog,profileSaved}=await setup({deviceClass:'msc',transcodingProfileId:'passthrough',managedPaths:['Music']},{transcoding_profile_dirty:true});
 const music=dialog.querySelector('#device-settings-music');music.value='Unfinished/Music';music.listeners.get('sl-input')();
 const confirmation=dialog.querySelector('#device-settings-profile-confirm');confirmation.checked=true;dialog.querySelector('#device-settings-profile-recover').disabled=false;
 profileSaved([],{id:'custom:saved',name:'Saved',description:'Custom',defaultMusicFolder:'Other',defaultPlaylistFolder:'New'});
 assert.equal(dialog.querySelector('#device-settings-profile-recovery').hidden,true);assert.equal(confirmation.checked,false);assert.equal(dialog.querySelector('#device-settings-profile-recover').disabled,true);assert.equal(music.value,'Unfinished/Music');
});

test('profile defaults fill each untouched device folder and persist audiobook and podcast paths',async()=>{
 const fields=[['music','defaultMusicFolder'],['playlist','defaultPlaylistFolder'],['audiobook','defaultAudiobookFolder'],['podcast','defaultPodcastFolder']];
 for(const touched of ['none',...fields.map(([id])=>id)]){
  const {dialog,profileSaved,calls}=await setup({});
  if(touched!=='none'){const input=dialog.querySelector('#device-settings-'+touched);input.value='My/Folder';input.listeners.get('sl-input')();}
  profileSaved([],{id:'custom:saved',...Object.fromEntries(fields.map(([id,key])=>[key,'New/'+id]))});
  for(const [id] of fields)assert.equal(dialog.querySelector('#device-settings-'+id).value,touched===id?'My/Folder':'New/'+id);
  await dialog.querySelector('#device-settings-save').click();
  const saved=calls.find(call=>call.method==='device.update_manifest').params;
  assert.equal(saved.audiobookFolderPath,touched==='audiobook'?'My/Folder':'New/audiobook');
  assert.equal(saved.podcastFolderPath,touched==='podcast'?'My/Folder':'New/podcast');
 }
});
test('profiles without media defaults retain configured device folders',async()=>{
 const {dialog,profileSaved}=await setup({audiobookPath:'Existing/Books',podcastPath:'Existing/Shows'});
 profileSaved([],{id:'custom:legacy',defaultMusicFolder:'Music'});
 assert.equal(dialog.querySelector('#device-settings-audiobook').value,'Existing/Books');
 assert.equal(dialog.querySelector('#device-settings-podcast').value,'Existing/Shows');
});


test('profile artwork defaults never overwrite an existing device preference',async()=>{
 for(const enabled of [true,false]){
  const {dialog,profileSaved,calls}=await setup({folderArtwork:{enabled}});
  profileSaved([],{id:'custom:saved',defaultFolderArtwork:!enabled});
  assert.equal(dialog.querySelector('#device-settings-artwork').checked,enabled);
  await dialog.querySelector('#device-settings-save').click();
  assert.deepEqual(calls.find(call=>call.method==='device.update_manifest').params.folderArtwork,{enabled});
 }
});
