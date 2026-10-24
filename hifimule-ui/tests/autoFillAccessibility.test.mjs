import {numeric} from './numericTestHelpers.mjs';
import * as html from '../src/html.ts';
import assert from 'node:assert/strict';
import test from 'node:test';
import vm from 'node:vm';
import { readFileSync } from 'node:fs';
import ts from 'typescript';

function load(path, require = () => ({})) {
    const exports = {};
    vm.runInNewContext(ts.transpileModule(readFileSync(new URL(path, import.meta.url), 'utf8'), {
        compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
    }).outputText, { exports, require: name => name === '../html' ? html : name === '../numericValidation' ? numeric : require(name), clearTimeout });
    return exports;
}

for (const [selector, target] of [
    ['#af-enabled', { checked: false }],
    ['#af-unit', { value: 'album' }],
    ['#af-played-exclusion', { checked: true }],
]) {
    test(`${selector} clears displayed results and discards an old in-flight preview`, async () => {
        const state = load('../src/state/autoFill.ts');
        let resolvePreview;
        const { AutoFillPanel } = load('../src/components/AutoFillPanel.ts', name => {
            if (name === '../state/autoFill') return state;
            if (name === '../rpc') return { previewAutoFill: () => new Promise(resolve => { resolvePreview = resolve; }) };
            return { t: key => key };
        });
        const panel = new AutoFillPanel({ pipeline: null, modes: [], playlists: [], formatSize: String });
        const handlers = {};
        const control = { addEventListener: (name, fn) => { handlers[name] = fn; } };
        const preview = { innerHTML: '' }, button = { loading: false };
        panel.dialog = {
            querySelector: key => key === selector ? control : key === '#af-preview' ? preview : key === '#af-preview-btn' ? button : null,
            querySelectorAll: () => [],
        };
        button.addEventListener = () => {};
        panel.buildPipeline = () => panel.pipeline;
        panel.bindEvents();
        panel.previewResult = { count: 10, bytes: 1000 };
        panel.updatePreviewUi();
        handlers['sl-change']({ target });
        assert.equal(preview.innerHTML, '');
        panel.pipeline.enabled = true;
        const pending = panel.runPreview();
        assert.equal(panel.previewInFlight, true);
        handlers['sl-change']({ target });
        resolvePreview([{ sizeBytes: 1000 }]);
        await pending;
        assert.equal(panel.previewResult, null);
        assert.equal(preview.innerHTML, '');
        assert.equal(button.loading, false);
        assert.equal(panel.previewInFlight, false);
    });
}

test('percentage edits preserve controls, update the pipeline, and invalidate old previews', () => {
    const state = load('../src/state/autoFill.ts');
    const { AutoFillPanel } = load('../src/components/AutoFillPanel.ts', name =>
        name === '../state/autoFill' ? state : { t: key => key });
    const panel = new AutoFillPanel({ pipeline: null, modes: [], playlists: [] });
    const selectors = ['#af-stable-core', '#af-repeat-tolerance', '#af-pity-ratio', '#af-spotlight-share', '#af-album-track-ratio'];
    const controls = new Map(selectors.map(selector => {
        const readout = { textContent: '' }, handlers = {};
        return [selector, { value: 0, handlers, readout,
            parentElement: { querySelector: () => readout },
            addEventListener: (name, fn) => { handlers[name] = fn; } }];
    }));
    panel.dialog = { querySelector: selector => controls.get(selector), querySelectorAll: () => [] };
    panel.captureInputs = () => {};
    panel.renderBody = () => assert.fail('slider change must not replace focused controls');
    panel.bindEvents();
    for (const slider of controls.values()) {
        panel.previewResult = { count: 1, bytes: 1 };
        const generation = panel.previewGeneration;
        slider.value = 41; slider.handlers['sl-change']({ target: slider });
        slider.value = 42; slider.handlers['sl-change']({ target: slider });
        assert.equal(slider.readout.textContent, '42%');
        assert.equal(panel.previewResult, null);
        assert.ok(panel.previewGeneration > generation);
    }
    assert.equal(panel.pipeline.memory.stableCorePct, .42);
    assert.equal(panel.pipeline.memory.repeatTolerance, .42);
    assert.equal(panel.pipeline.pity.guaranteedRatio, .42);
    assert.equal(panel.pipeline.promotion.spotlightShare, .42);
    assert.equal(panel.pipeline.promotion.albumTrackRatio, .42);
});

function numericPanel(pipeline={}) {
 const state=load('../src/state/autoFill.ts');let saves=0,previews=0;
 const {AutoFillPanel}=load('../src/components/AutoFillPanel.ts',name=>name==='../state/autoFill'?state:name==='../rpc'?{previewAutoFill:async()=>{previews++;return [];}}:{t:key=>key});
 const panel=new AutoFillPanel({pipeline,modes:[],playlists:[],formatSize:String,onSave:()=>saves++});
 panel.advancedOpen=true;let controls=[];
 panel.dialog={hide(){},querySelector:key=>controls.find(control=>'#'+control.id===key)??null,querySelectorAll:selector=>selector==='[data-numeric]'?controls:selector.startsWith('.af-ctx-')?controls.filter(c=>c.classList.contains(selector.slice(1))):[]};
 const control=(id,value,classes=[],index=0)=>({id,value,dataset:{index:String(index)},classList:{contains:key=>classes.includes(key),*[Symbol.iterator](){yield* classes;}},hasAttribute:attr=>attr==='data-numeric',handlers:{},addEventListener(event,fn){(this.handlers[event]??=[]).push(fn);},focus(){this.focused=true;}});
 return {panel,control,setControls:values=>{controls=values;},counts:()=>({saves,previews})};
}
test('invalid numeric edits block Save and Preview before serialization, then corrected arbitrary duration saves',async()=>{
 const {panel,control,setControls,counts}=numericPanel();const budget=control('af-budget-gb','1e999');setControls([budget]);
 panel.handleSave();panel.onPreviewClick();await panel.runPreview();assert.deepEqual(counts(),{saves:0,previews:0});assert.equal(budget.focused,true);assert.equal(panel.budgetGbInput,'1e999');
 budget.value='0.5';const duration=control('af-duration-hours','0.1234567');setControls([budget,duration]);panel.handleSave();assert.equal(counts().saves,1);assert.equal(panel.pipeline.budget.targetDurationSecs,444);
});
test('raw rarity and context drafts survive replacement, row removal and disabled stages without clamping',()=>{
 const rule=()=>({window:{timeOfDay:{startHour:6,endHour:11}},sourceRefs:[],includeTags:[],excludeTags:[],includeGenres:[],excludeGenres:[]});
 const {panel,control,setControls}=numericPanel({rarity:{enabled:true,rareMaxPlays:5},context:{enabled:true,rules:[rule(),rule()]}});
 const rarity=control('af-rarity-rare-max','5'),hour=control('','6',['af-ctx-start-hour'],1);setControls([rarity,hour]);panel.restoreNumericDrafts();panel.bindEvents();
 rarity.value=' 2.5 ';for(const fn of rarity.handlers['sl-input'])fn({target:rarity});hour.value='99';for(const fn of hour.handlers['sl-input'])fn({target:hour});
 assert.equal(panel.pipeline.rarity.rareMaxPlays,5);assert.equal(panel.pipeline.context.rules[1].window.timeOfDay.startHour,6);
 panel.captureInputs();panel.pipeline.context.rules.splice(0,1);
 const rerarity=control('af-rarity-rare-max','5'),rehour=control('','6',['af-ctx-start-hour'],0);setControls([rerarity,rehour]);panel.restoreNumericDrafts();
 assert.equal(rerarity.value,' 2.5 ');assert.equal(rehour.value,'99');assert.equal(panel.validateNumericInputs(),false);
 panel.pipeline.rarity.enabled=false;panel.pipeline.context.enabled=false;setControls([]);assert.equal(panel.validateNumericInputs(),true);
});
test('unchanged bytes and seconds retain exact stored integers through roundtrip',()=>{
 const {panel,setControls}=numericPanel({budget:{maxBytes:9876543210123,targetDurationSecs:123457,headroomBytes:1234567890123}});setControls([]);
 const out=panel.buildPipeline();assert.equal(out.budget.maxBytes,9876543210123);assert.equal(out.budget.targetDurationSecs,123457);assert.equal(out.budget.headroomBytes,1234567890123);
});
test('restored context hour drafts update the effective window after changing its kind',()=>{
 const {panel,control,setControls}=numericPanel({context:{enabled:true,rules:[{window:{timeOfDay:{startHour:8,endHour:17}},sourceRefs:[],includeTags:[],excludeTags:[],includeGenres:[],excludeGenres:[]}]}});
 const hour=control('','8',['af-ctx-start-hour']);setControls([hour]);panel.captureInputs();
 panel.pipeline.context.rules[0].window={timeOfDay:{startHour:6,endHour:11}};
 const restored=control('','6',['af-ctx-start-hour']);setControls([restored]);panel.restoreNumericDrafts();panel.bindEvents();
 assert.equal(restored.value,'8');assert.equal(panel.buildPipeline().context.rules[0].window.timeOfDay.startHour,8);
});
test('podcast checkbox drafts survive retention mode rerenders without parsing invalid episode counts',()=>{
 const {panel,control}=numericPanel();panel.opts.modes=['podcasts'];panel.pipeline.podcastRetention.mode='selectedShows';
 const mode=control('af-podcast-mode','latest'),unplayed={checked:true},show={value:'chosen',checked:true},count=control('af-podcast-recent','bad');
 const fields=new Map([['#af-podcast-mode',mode],['#af-podcast-unplayed',unplayed],['#af-podcast-recent',count]]);
 unplayed.addEventListener=()=>{};show.addEventListener=()=>{};
 panel.dialog.querySelector=key=>fields.get(key)??null;
 panel.dialog.querySelectorAll=key=>key==='[data-numeric]'?[count]:key==='.af-podcast-show'?[show]:[];
 panel.renderBody=()=>{};panel.bindEvents();mode.handlers['sl-change'][0]();
 assert.equal(panel.pipeline.podcastRetention.unplayedOnly,true);assert.deepEqual(Array.from(panel.pipeline.podcastRetention.showIds),['chosen']);assert.equal(panel.pipeline.podcastRetention.mode,'latest');
 assert.equal(panel.pipeline.podcastRetention.recentCount,10);assert.equal(panel.numericDrafts.get('af-podcast-recent'),'bad');
});
