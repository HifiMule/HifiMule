import {numeric} from './numericTestHelpers.mjs';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import ts from 'typescript';
import { escapeAttr } from '../src/html.ts';
import { loadProfiles } from './deviceProfileTestHelpers.mjs';
const t=(key,vars={})=>key+Object.values(vars).join(' ');
const profiles=loadProfiles();
test('new profile uses MP3 fallback, incompatible conversion and no limits',()=>{
 const p=profiles.newProfile(), draft=profiles.toDraft(p);
 assert.equal(draft.mode,'compatible');assert.equal(draft.output,'mp3');assert.equal(draft.bitrate,'256');assert.deepEqual(draft.formats,['mp3']);assert.ok(Object.values(draft.limits).every(value=>value===''));
});
test('media folder defaults roundtrip, trim and clear without changing audio capabilities',()=>{
 const profile=profiles.newProfile(),draft=profiles.toDraft(profile);
 assert.equal(draft.audiobooks,'');assert.equal(draft.podcasts,'');
 draft.audiobooks=' Books/Authors ';draft.podcasts=' Shows ';
 const saved=profiles.fromDraft(profile,draft);assert.equal(saved.defaultAudiobookFolder,'Books/Authors');assert.equal(saved.defaultPodcastFolder,'Shows');
 assert.deepEqual(saved.deviceProfile,profile.deviceProfile);assert.equal(profiles.toDraft(saved).audiobooks,'Books/Authors');assert.equal(profiles.toDraft(saved).podcasts,'Shows');
 draft.audiobooks=' ';draft.podcasts='';const cleared=profiles.fromDraft(saved,draft);assert.equal(cleared.defaultAudiobookFolder,null);assert.equal(cleared.defaultPodcastFolder,null);
 for(const field of ['audiobooks','podcasts'])for(const bad of ['../Books','/Shows','C:/Books','Shows\\Episodes','Books/./Author','Books\0Bad']){
  const invalid=profiles.toDraft(profile);invalid[field]=bad;assert.ok(profiles.validateDraft(invalid),`${field}: ${bad}`);
 }
});
test('media folder inputs render stored values, inherit hints and system locks and capture changes',()=>{
 const {DeviceProfileManager}=loadManager(async()=>{});const manager=new DeviceProfileManager();const elements=new Map();
 manager.dialog={querySelector:selector=>{if(!elements.has(selector))elements.set(selector,{addEventListener(){}});return elements.get(selector);},querySelectorAll:()=>[]};manager.renderList=()=>{};
 const profile={...profiles.newProfile(),defaultAudiobookFolder:'Books',defaultPodcastFolder:'Shows'};
 manager.setProfile(profile,false);let html=elements.get('.profile-manager-editor').innerHTML;
 assert.match(html,/id="profile-audiobooks"[^>]*value="Books"[^>]*placeholder="basket.device.media_folder_inherit"/);
 assert.match(html,/id="profile-podcasts"[^>]*value="Shows"/);
 elements.set('#profile-audiobooks',{value:'Edited/Books'});elements.set('#profile-podcasts',{value:'Edited/Shows'});manager.capture();
 assert.equal(manager.draft.audiobooks,'Edited/Books');assert.equal(manager.draft.podcasts,'Edited/Shows');
 manager.setProfile({...profile,ownership:'system'},false);html=elements.get('.profile-manager-editor').innerHTML;
 assert.match(html,/id="profile-audiobooks"[^>]*disabled/);assert.match(html,/id="profile-podcasts"[^>]*disabled/);
});
test('metadata edits preserve complete audio payload, aliases, unknown fields and lossless bitrate',()=>{
 const bundled=JSON.parse(readFileSync(new URL('../../hifimule-daemon/assets/device-profiles.json',import.meta.url),'utf8'));
 for(const profile of bundled.profiles){
  const before=structuredClone(profile.deviceProfile);const draft=profiles.toDraft(profile);draft.name='My name';draft.description='My description';draft.music='New/Music';
  const result=profiles.fromDraft(profile,draft);assert.deepEqual(result.deviceProfile,before,profile.id);assert.equal(result.name,'My name');
  assert.equal(new Set(draft.formats).size,draft.formats.length);
 }
});
test('capability mapping distinguishes AAC ALAC and M4B, supports always/original and unlimited',()=>{
 const p=profiles.newProfile(),draft=profiles.toDraft(p);draft.formats=['aac','alac','m4b'];draft.limits.MaxAudioChannels='2';draft.bitrate='192';
 let mapped=profiles.fromDraft(p,draft);assert.deepEqual(mapped.deviceProfile.DirectPlayProfiles.map(p=>[p.Container,p.AudioCodec]),[['mp4','aac'],['m4a','alac'],['m4b','aac']]);assert.equal(mapped.deviceProfile.MusicStreamingTranscodingBitrate,192000);assert.equal(mapped.deviceProfile.MaxAudioChannels,2);assert.equal(mapped.deviceProfile.MaxAudioSampleRate,undefined);
 draft.mode='always';assert.deepEqual(profiles.fromDraft(p,draft).deviceProfile.DirectPlayProfiles,[]);
 draft.mode='originals';assert.equal(profiles.fromDraft(p,draft).deviceProfile,null);
});
test('unsupported conditions stay visible and preserved',()=>{
 const p=profiles.newProfile();p.deviceProfile.CodecProfiles=[{Conditions:[{Property:'AudioProfile',Value:'x'}]}];const draft=profiles.toDraft(p);draft.name='Renamed';draft.output='flac';
 assert.deepEqual(profiles.unsupportedRules(p),['CodecProfiles']);assert.deepEqual(profiles.fromDraft(p,draft).deviceProfile,p.deviceProfile);
});
test('validation rejects empty names, traversal and nonpositive limits',()=>{
 const draft=profiles.toDraft(profiles.newProfile());draft.name=' ';assert.ok(profiles.validateDraft(draft));draft.name='Player';draft.music='../Music';assert.ok(profiles.validateDraft(draft));draft.music='Music';draft.limits.MaxAudioChannels='0';assert.ok(profiles.validateDraft(draft));draft.limits.MaxAudioChannels='';assert.equal(profiles.validateDraft(draft),undefined);
});
test('dropdown actions restore real selection synchronously, preserve parent values and select saved clones',async()=>{
 let open,change,listener;const list=[{id:'system:test',name:'Test',ownership:'system'}];const parent={name:'Unfinished',folder:'Custom',flags:true};
 const module=loadProfiles({'./components/DeviceProfileManager':{openDeviceProfiles:async options=>{open=options;}},'./rpc':{rpcCall:async()=>[...list,{id:'custom:copy',name:'Copy'}]}});
 const select={value:'system:test',innerHTML:'',addEventListener:(_event,fn)=>{listener=fn;}};
 module.bindProfileActions(select,'system:test',list,(_list,profile)=>{change=profile;});select.value=module.PROFILE_EDIT;
 let stopped=false;listener({target:select,stopImmediatePropagation(){stopped=true;}});
 assert.equal(select.value,'system:test');assert.ok(stopped);await new Promise(resolve=>setImmediate(resolve));assert.equal(open.selectedId,'system:test');assert.equal(open.create,false);
 await open.onSaved({id:'custom:copy',name:'Copy'});assert.equal(select.value,'custom:copy');assert.equal(change.id,'custom:copy');assert.deepEqual(parent,{name:'Unfinished',folder:'Custom',flags:true});
 select.value=module.PROFILE_NEW;listener({target:select,stopImmediatePropagation(){}});assert.equal(select.value,'custom:copy');await new Promise(resolve=>setImmediate(resolve));assert.equal(open.create,true);
});

function loadManager(rpcCall, document){
 const exports={};const code=ts.transpileModule(readFileSync(new URL('../src/components/DeviceProfileManager.ts',import.meta.url),'utf8'),{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText;
 new Function('require','exports','document',code)(name=>name==='../rpc'?{rpcCall}:name==='../i18n'?{t}:name==='../html'?{escapeAttr}:name==='../deviceProfiles'?profiles:name==='../numericValidation'?numeric:{},exports,document);return exports;
}
test('systems cannot be saved; clones receive independent custom drafts',async()=>{
 let calls=0;const {DeviceProfileManager}=loadManager(async()=>{calls++;});const manager=new DeviceProfileManager();manager.renderList=()=>{};manager.renderEditor=()=>{};manager.focus=()=>{};
 const system={...profiles.newProfile(),id:'system:test',ownership:'system',revision:3};manager.setProfile(system,false);await manager.save();assert.equal(calls,0);
 await manager.clone();assert.equal(manager.profile.id,'');assert.equal(manager.profile.ownership,'custom');assert.equal(manager.cloneSource,'system:test');assert.equal(system.ownership,'system');assert.notEqual(manager.clean,JSON.stringify(manager.draft));
});
test('save conflicts keep draft and revision for retry',async()=>{
 const {DeviceProfileManager}=loadManager(async()=>{throw new Error('Profile changed elsewhere');});const manager=new DeviceProfileManager();manager.renderList=()=>{};manager.renderEditor=()=>{};manager.capture=()=>{};let error;
 manager.error=value=>{error=value.message;};const button={loading:false};manager.dialog={querySelector:()=>button,querySelectorAll:()=>[]};manager.setProfile({...profiles.newProfile(),id:'custom:test',revision:2},false);manager.draft.name='Unsaved';
 await manager.save();assert.equal(manager.draft.name,'Unsaved');assert.equal(manager.profile.revision,2);assert.match(error,/changed elsewhere/);assert.equal(button.loading,false);
});

test('reordered custom rules preserve their original byte structure on metadata edits',()=>{
 const p=profiles.newProfile();p.deviceProfile.DirectPlayProfiles=[{Container:'flac',AudioCodec:'flac',Type:'Audio'},{Container:'mp3',AudioCodec:'mp3',Type:'Audio'}];
 const draft=profiles.toDraft(p);draft.name='Renamed';assert.deepEqual(draft.formats,['mp3','flac']);assert.deepEqual(profiles.fromDraft(p,draft).deviceProfile,p.deviceProfile);
});

test('unsupported system and custom clones save through the clone boundary without changing capabilities',async()=>{
 for(const ownership of ['system','custom']){
  let request,saved;const {DeviceProfileManager}=loadManager(async(method,params)=>{if(method==='device_profiles.get')return saved;request={method,params};saved={...params.profile,id:'custom:saved',ownership:'custom',revision:1};return saved;});
  const manager=new DeviceProfileManager();manager.renderList=()=>{};manager.renderEditor=()=>{};manager.focus=()=>{};manager.capture=()=>{};manager.reload=async()=>{};
  manager.dialog={querySelector:()=>({}),querySelectorAll:()=>[]};const source={...profiles.newProfile(),id:ownership+':legacy',ownership};source.deviceProfile.CodecProfiles=[{Conditions:[{Property:'AudioProfile',Value:'x'}]}];
  manager.setProfile(source,false);await manager.clone();manager.draft.description='Kept description';await manager.save();
  assert.equal(request.method,'device_profiles.clone');assert.equal(request.params.sourceId,source.id);assert.equal(request.params.profile.description,'Kept description');assert.deepEqual(request.params.profile.deviceProfile,source.deviceProfile);
 }
});
test('dirty-switch discard dialog preserves draft on Keep editing and confirms discard explicitly',async()=>{
 let confirm;const fakeDocument={createElement:()=>{const callbacks={};confirm={buttons:{},addEventListener:(event,fn)=>{callbacks[event]=fn;},querySelector:id=>confirm.buttons[id]??=( {addEventListener:(_event,fn)=>{confirm.buttons[id].click=fn;}} ),show(){},hide(){callbacks['sl-after-hide']({target:confirm});},remove(){}};return confirm;},body:{appendChild(){}}};
 const {DeviceProfileManager}=loadManager(async()=>{},fakeDocument);const manager=new DeviceProfileManager();manager.renderList=()=>{};manager.renderEditor=()=>{};manager.setProfile(profiles.newProfile(),true);manager.draft.name='Unsaved draft';
 let answer=manager.permitDiscard();confirm.buttons['#profile-keep'].click();assert.equal(await answer,false);assert.equal(manager.draft.name,'Unsaved draft');
 answer=manager.permitDiscard();confirm.buttons['#profile-discard'].click();assert.equal(await answer,true);
});

test('unknown future audio rules remain visible and are preserved during edits',()=>{
 const p=profiles.newProfile();p.deviceProfile.FutureAudioRule={value:42};const draft=profiles.toDraft(p);draft.name='Renamed';draft.output='flac';assert.ok(profiles.unsupportedRules(p).includes('FutureAudioRule'));assert.deepEqual(profiles.fromDraft(p,draft).deviceProfile,p.deviceProfile);
});

test('dynamic usage names and unsupported-property names are escaped in rendered messages',()=>{
 const {DeviceProfileManager}=loadManager(async()=>{});const manager=new DeviceProfileManager();
 const elements=new Map();manager.dialog={querySelector:selector=>{if(!elements.has(selector))elements.set(selector,{addEventListener(){}});return elements.get(selector);},querySelectorAll:()=>[]};manager.renderList=()=>{};
 const html='<img src=x onerror="alert(1)">';const profile=profiles.newProfile();profile.usage=[{id:'device',name:html}];profile.deviceProfile[html]=true;
 manager.setProfile(profile,false);const rendered=elements.get('.profile-manager-editor').innerHTML;
 assert.ok(rendered.includes(escapeAttr(html)));assert.equal(rendered.includes(html),false);
 assert.ok(rendered.match(/profile-mode[^>]*disabled/));
});
test('mixed supported and unknown direct-play tokens lock audio edits without dropping raw rules',()=>{
 for(const row of [{Container:'mp3,aiff',AudioCodec:'mp3,pcm_s16le',Type:'Audio'},{Container:'mp3',AudioCodec:'mp3',Type:'Video'},{Container:'mp3',AudioCodec:'mp3',Unknown:true,Type:'Audio'}]){
  const profile=profiles.newProfile();profile.deviceProfile.DirectPlayProfiles=[row];const draft=profiles.toDraft(profile);draft.name='Safe metadata';draft.formats.push('flac');
  assert.ok(profiles.unsupportedRules(profile).includes('DirectPlayProfiles'));assert.deepEqual(profiles.fromDraft(profile,draft).deviceProfile,profile.deviceProfile);
 }
});
test('fractional kbps maps exactly to whole bps and preserves metadata-only legacy saves',()=>{
 const profile=profiles.newProfile();profile.deviceProfile.MaxStreamingBitrate=321999;profile.deviceProfile.MusicStreamingTranscodingBitrate=321999;
 const draft=profiles.toDraft(profile);assert.equal(draft.bitrate,'321.999');assert.equal(draft.limits.MaxStreamingBitrate,'321.999');assert.equal(profiles.validateDraft(draft),undefined);
 draft.name='Renamed';assert.deepEqual(profiles.fromDraft(profile,draft).deviceProfile,profile.deviceProfile);
 draft.bitrate='1.001';draft.limits.MaxStreamingBitrate='0.001';const edited=profiles.fromDraft(profile,draft);assert.equal(edited.deviceProfile.MusicStreamingTranscodingBitrate,1001);assert.equal(edited.deviceProfile.MaxStreamingBitrate,1);
 draft.bitrate='321.9999';assert.ok(profiles.validateDraft(draft));
});
test('in-flight selection disables old form, ignores edits and competing switches, restores draft after failure',async()=>{
 let reject,calls=0;const {DeviceProfileManager}=loadManager(async()=>{calls++;return new Promise((_resolve,fail)=>{reject=fail;});});const manager=new DeviceProfileManager();manager.renderList=()=>{};manager.renderEditor=()=>{};manager.error=()=>{};
 const input={disabled:false,value:'Original'},picker={value:'custom:old'};manager.dialog={querySelector:()=>picker,querySelectorAll:()=>[input]};manager.setProfile({...profiles.newProfile(),id:'custom:old'},false);
 const before=structuredClone(manager.draft);const pending=manager.select('custom:next');await Promise.resolve();assert.equal(input.disabled,true);
 input.value='Attempted edit';manager.capture();assert.deepEqual(manager.draft,before);
 await manager.select('custom:third');assert.equal(calls,1);reject(new Error('Offline'));await pending;
 assert.equal(input.disabled,false);assert.deepEqual(manager.draft,before);assert.equal(manager.profile.id,'custom:old');
});
test('successful saves refresh shared usage and keep successful identity when refresh fails',async()=>{
 for(const refreshFails of [false,true]){
  const methods=[];let saved;const {DeviceProfileManager}=loadManager(async(method,params)=>{methods.push(method);if(method==='device_profiles.get'){if(refreshFails)throw new Error('Offline');return {...saved,usage:[{id:'a',name:'A'},{id:'b',name:'B'}],usageIncomplete:true};}saved={...params.profile,id:'custom:saved',revision:(params.revision??0)+1};return saved;});
  const manager=new DeviceProfileManager();manager.renderList=()=>{};manager.renderEditor=()=>{};manager.reload=async()=>{};manager.capture=()=>{};let errorSaved;
  manager.error=(_error,success)=>{errorSaved=success;};manager.dialog={querySelector:()=>({}),querySelectorAll:()=>[]};manager.setProfile(profiles.newProfile(),true);await manager.save();
  assert.equal(manager.creating,false);assert.equal(manager.profile.id,'custom:saved');assert.equal(manager.profile.usageIncomplete,true);
  if(refreshFails){assert.equal(errorSaved,true);assert.deepEqual(manager.profile.usage,[]);}else assert.equal(manager.profile.usage.length,2);
  manager.draft.description='Second edit';await manager.save();assert.equal(methods.filter(method=>method==='device_profiles.create').length,1);assert.equal(methods.filter(method=>method==='device_profiles.update').length,1);
 }
});

test('save refresh keeps rendered controls disabled until detail refresh completes',async()=>{
 let resolveDetail,saved;const {DeviceProfileManager}=loadManager(async(method,params)=>{
  if(method==='device_profiles.get')return new Promise(resolve=>{resolveDetail=resolve;});
  saved={...params.profile,id:'custom:saved',revision:1};return saved;
 });
 const manager=new DeviceProfileManager();const elements=new Map();manager.dialog={querySelector:selector=>{if(!elements.has(selector))elements.set(selector,{addEventListener(){}});return elements.get(selector);},querySelectorAll:()=>[]};manager.renderList=()=>{};manager.reload=async()=>{};manager.capture=()=>{};
 manager.setProfile(profiles.newProfile(),true);const pending=manager.save();await Promise.resolve();
 assert.equal(manager.creating,false);assert.equal(manager.profile.id,'custom:saved');assert.equal(manager.busy,true);
 let html=elements.get('.profile-manager-editor').innerHTML;assert.match(html,/id="profile-name"[^>]*disabled/);assert.match(html,/id="profile-mode"[^>]*disabled/);assert.equal(elements.get('#profile-save').disabled,true);
 resolveDetail({...saved,usage:[],usageIncomplete:true});await pending;
 html=elements.get('.profile-manager-editor').innerHTML;assert.doesNotMatch(html,/id="profile-name"[^>]*disabled/);assert.equal(elements.get('#profile-save').disabled,false);assert.equal(manager.busy,false);
});
test('a completed profile selection re-enables the selected custom editor',async()=>{
 const selected={...profiles.newProfile(),id:'custom:next'};const {DeviceProfileManager}=loadManager(async()=>selected);const manager=new DeviceProfileManager();const elements=new Map();manager.dialog={querySelector:selector=>{if(!elements.has(selector))elements.set(selector,{addEventListener(){}});return elements.get(selector);},querySelectorAll:()=>[]};manager.renderList=()=>{};manager.setProfile({...profiles.newProfile(),id:'custom:old'},false);
 await manager.select(selected.id);assert.equal(manager.busy,false);assert.equal(manager.profile.id,selected.id);assert.equal(elements.get('#profile-save').disabled,false);assert.doesNotMatch(elements.get('.profile-manager-editor').innerHTML,/id="profile-name"[^>]*disabled/);
});

test('profile numeric boundaries and codec floors identify the field; missing metadata keeps save fallback',()=>{
 const base=profiles.newProfile(),draft=profiles.toDraft(base);draft.output='aac';
 const constraints={codecs:{aac:{sampleRateMin:7350,bitDepthMin:1},flac:{bitDepthMin:16}}};
 for(const [key,min,max] of [['MaxAudioChannels',1,32],['MaxAudioBitDepth',1,64],['MaxAudioSampleRate',7350,768000],['MaxStreamingBitrate',.001,100000]]){
  for(const value of [String(min),String(max),'']){draft.limits[key]=value;assert.equal(profiles.profileNumericErrors(draft,base,constraints)[key],undefined,`${key} ${value}`);}
  for(const value of ['0',String(max+1),'Infinity']){draft.limits[key]=value;assert.ok(profiles.profileNumericErrors(draft,base,constraints)[key],`${key} ${value}`);}draft.limits[key]='';
 }
 draft.limits.MaxAudioSampleRate='48';assert.match(profiles.profileNumericErrors(draft,base,constraints).MaxAudioSampleRate,/numeric.sample_rate/);
 assert.equal(profiles.profileNumericErrors(draft,base).MaxAudioSampleRate,undefined);
 draft.mode='originals';draft.bitrate='wrong';assert.deepEqual(profiles.profileNumericErrors(draft,base,constraints),{});
 draft.mode='compatible';const unsupported=structuredClone(base);unsupported.deviceProfile.FutureRule=true;assert.deepEqual(profiles.profileNumericErrors(draft,unsupported,constraints),{});
 draft.limits.MaxAudioSampleRate='';draft.bitrate=' 256.125 ';assert.equal(profiles.fromDraft(base,draft).deviceProfile.MusicStreamingTranscodingBitrate,256125);
 for(const key of profiles.limits)draft.limits[key]='  ';
 assert.equal(profiles.validateDraft(draft,base),undefined);
 const cleared=profiles.fromDraft(base,draft).deviceProfile;
 for(const key of profiles.limits)assert.equal(cleared[key],undefined);
});
test('profile invalid drafts never write and structured daemon errors remain field-specific',async()=>{
 let calls=0;const {DeviceProfileManager}=loadManager(async()=>{calls++;throw {data:{errorCode:'PROFILE_NUMERIC_CONSTRAINT',field:'MaxAudioSampleRate',min:7350,max:768000}};});
 const manager=new DeviceProfileManager();manager.renderList=()=>{};manager.renderEditor=()=>{};manager.capture=()=>{};
 const controls=new Map();manager.dialog={querySelector:key=>{if(!controls.has(key))controls.set(key,{});return controls.get(key);},querySelectorAll:()=>[]};
 manager.setProfile(profiles.newProfile(),true);manager.draft.limits.MaxAudioChannels='33';await manager.save();assert.equal(calls,0);assert.equal(manager.draft.limits.MaxAudioChannels,'33');
 manager.draft.limits.MaxAudioChannels='';manager.draft.limits.MaxAudioSampleRate='48';await manager.save();assert.equal(calls,1);assert.match(manager.serverErrors.MaxAudioSampleRate,/numeric.sample_rate/);assert.equal(manager.draft.limits.MaxAudioSampleRate,'48');
});

test('unchanged legacy numeric capabilities allow metadata edits but changed audio must validate',()=>{
 const base=profiles.newProfile();base.deviceProfile.MaxAudioChannels=33;const draft=profiles.toDraft(base);draft.name='Metadata edit';
 assert.equal(profiles.validateDraft(draft,base),undefined);assert.equal(profiles.fromDraft(base,draft).deviceProfile.MaxAudioChannels,33);
 draft.bitrate='192';assert.ok(profiles.profileNumericErrors(draft,base).MaxAudioChannels);
});
test('changing output clears a cached server error for the previous codec',()=>{
 const {DeviceProfileManager}=loadManager(async()=>{});const manager=new DeviceProfileManager();const elements=new Map();
 manager.dialog={querySelector:key=>{if(!elements.has(key))elements.set(key,{handlers:{},addEventListener(event,fn){this.handlers[event]=fn;}});return elements.get(key);},querySelectorAll:()=>[]};manager.renderList=()=>{};
 manager.setProfile(profiles.newProfile(),false);manager.capture=()=>{};manager.serverErrors={MaxAudioSampleRate:'Old codec error'};
 elements.get('.profile-editor-fields').handlers['sl-change']({target:{id:'profile-output'}});assert.deepEqual(manager.serverErrors,{});
});
