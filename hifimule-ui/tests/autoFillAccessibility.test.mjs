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
    }).outputText, { exports, require: name => name === '../html' ? html : require(name), clearTimeout });
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
