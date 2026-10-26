import assert from 'node:assert/strict';
import test from 'node:test';
import vm from 'node:vm';
import { readFileSync } from 'node:fs';
import ts from 'typescript';

const catalog = JSON.parse(readFileSync(new URL('../../hifimule-i18n/catalog.json', import.meta.url), 'utf8'));
const compiled = ts.transpileModule(readFileSync(new URL('../src/i18n.ts', import.meta.url), 'utf8'), {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2020, esModuleInterop: false },
}).outputText;

function harness(language = 'en', fail = new Set()) {
    const requested = [], warnings = [], exports = {};
    const storage = new Map();
    const document = { documentElement: { lang: '' } };
    vm.runInNewContext(compiled, {
        exports, document, navigator: { language },
        localStorage: { getItem: key => storage.get(key), setItem: (key, value) => storage.set(key, value) },
        console: { warn: (...args) => warnings.push(args) },
        require: id => {
            const locale = id.split('/').at(-1);
            requested.push(locale);
            if (fail.has(locale)) throw new Error('Missing locale chunk');
            return { default: catalog[locale] };
        },
    });
    return { ...exports, requested, warnings, document, storage };
}

for (const language of ['en', 'fr', 'es', 'de']) {
    test(`${language} startup loads only English and the selected locale`, async () => {
        const h = harness(language + '-ZZ');
        await h.initializeI18n();
        assert.deepEqual(h.requested, language === 'en' ? ['en'] : ['en', language]);
        assert.equal(h.document.documentElement.lang, language);
        const name = '夜の音楽 🎵 <live> "quoted"';
        assert.equal(h.t('playlist.curation.remove_track_named', { name }),
            catalog[language]['playlist.curation.remove_track_named'].replace('{name}', name));
        assert.equal(h.t('missing.key'), 'missing.key');
    });
}

test('locale requests are deduplicated and the last selection wins', async () => {
    const h = harness();
    await Promise.all([h.setLanguage('fr'), h.setLanguage('fr'), h.setLanguage('de')]);
    assert.deepEqual(h.requested, ['en', 'fr', 'de']);
    assert.equal(h.currentLanguage(), 'de');
    await h.setLanguage('fr');
    assert.equal(h.currentLanguage(), 'fr');
    assert.deepEqual(h.requested, ['en', 'fr', 'de']);
});

test('startup chunk failure retains readable English and allows a later retry', async () => {
    const failures = new Set(['de']);
    const h = harness('de', failures);
    await h.initializeI18n();
    assert.equal(h.document.documentElement.lang, 'en');
    assert.equal(h.t('ui.library.title'), catalog.en['ui.library.title']);
    assert.equal(h.warnings.length, 1);
    failures.clear();
    await h.initializeI18n();
    assert.equal(h.document.documentElement.lang, 'de');
    assert.equal(h.t('ui.library.title'), catalog.de['ui.library.title']);
});

test('failed explicit language change preserves the current selection', async () => {
    const h = harness('en', new Set(['es']));
    await h.setLanguage('fr');
    await assert.rejects(h.setLanguage('es'), /Missing locale chunk/);
    assert.equal(h.currentLanguage(), 'fr');
    assert.equal(h.document.documentElement.lang, 'fr');
    await h.setLanguage('unsupported');
    assert.equal(h.currentLanguage(), 'en');
});
