import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import ts from 'typescript';

const helperSource = await readFile(new URL('../src/audiobookshelfSetup.ts', import.meta.url), 'utf8');
const helperJs = ts.transpileModule(helperSource, {
    compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ES2022 },
}).outputText;
const helpers = await import(`data:text/javascript;base64,${Buffer.from(helperJs).toString('base64')}`);

test('provider choices include explicit and automatic Audiobookshelf paths', () => {
    assert.equal(helpers.isLoginProviderChoice('auto'), true);
    assert.equal(helpers.isLoginProviderChoice('audiobookshelf'), true);
    assert.equal(helpers.isLoginProviderChoice('guessed-audiobookshelf'), false);
});

test('auto submit re-probes and routes Audiobookshelf into discovery', async () => {
    assert.equal(helpers.shouldUseAudiobookshelfDiscovery('auto', 'audiobookshelf'), true);
    assert.equal(helpers.shouldUseAudiobookshelfDiscovery('auto', 'jellyfin'), false);
    assert.equal(helpers.shouldUseAudiobookshelfDiscovery('audiobookshelf', null), true);
    const loginSource = await readFile(new URL('../src/login.ts', import.meta.url), 'utf8');
    assert.match(loginSource, /selectedProvider === 'auto'[\s\S]*rpcCall\('server\.probe', \{ url \}\)/);
    assert.match(loginSource, /shouldUseAudiobookshelfDiscovery\(selectedProvider, detectedProvider\)[\s\S]*audiobookshelfDiscover/);
    assert.match(loginSource, /serverType: selectedProvider/);
});

test('library choices accept only opaque safe role cards', () => {
    const choices = helpers.validLibraryChoices([
        { choiceId: 'opaque-a', name: 'Fiction', role: 'audiobook' },
        { choiceId: 'opaque-b', name: 'Talks', role: 'podcast' },
        { choiceId: 'bad', name: 'Folder', role: 'folder', path: '/private' },
    ]);
    assert.deepEqual(choices.map(choice => choice.role), ['audiobook', 'podcast']);
    assert.equal(helpers.audiobookshelfRoleLabelKey('audiobook'), 'login.audiobookshelf.role_books');
    assert.equal(helpers.audiobookshelfRoleLabelKey('podcast'), 'login.audiobookshelf.role_podcasts');
});

test('picker exposes one-library commit and no folder or collection controls', async () => {
    const loginSource = await readFile(new URL('../src/login.ts', import.meta.url), 'utf8');
    assert.match(loginSource, /sl-radio-group/);
    assert.match(loginSource, /audiobookshelfCommit/);
    assert.doesNotMatch(loginSource, /name="(?:folder|collection|series)"/i);
    assert.doesNotMatch(loginSource, /type="checkbox"[^>]*(?:library|folder|collection|series)/i);
});

test('RPC logging is method-only and scoped re-auth sends id plus password', async () => {
    const rpcSource = await readFile(new URL('../src/rpc.ts', import.meta.url), 'utf8');
    assert.match(rpcSource, /console\.log\(`RPC Call: \$\{method\}`\)/);
    assert.doesNotMatch(rpcSource, /console\.log\(`RPC Call: \$\{method\}`\s*,\s*params/);
    assert.match(rpcSource, /server\.reauthenticate', \{ id, password \}/);
});
