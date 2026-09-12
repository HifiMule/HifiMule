import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import ts from 'typescript';
const source = await readFile(new URL('../src/lifecycleDeadline.ts', import.meta.url), 'utf8');
const js = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ES2022 } }).outputText;
const { withDeadline } = await import(`data:text/javascript;base64,${Buffer.from(js).toString('base64')}`);

test('late native hydration cannot route after timeout', async () => {
    let finish;
    let routes = 0;
    const native = new Promise(resolve => { finish = resolve; });
    const hydration = withDeadline(native, 10, 'STATE_LOAD_TIMEOUT').then(() => { routes++; });
    await assert.rejects(hydration, /STATE_LOAD_TIMEOUT/);
    finish({ servers: [] });
    await new Promise(resolve => setTimeout(resolve, 10));
    assert.equal(routes, 0);
});

test('an early provider error is preserved and timely hydration routes once', async () => {
    const error = new Error('provider unavailable');
    await assert.rejects(withDeadline(Promise.reject(error), 1000, 'STATE_LOAD_TIMEOUT'), e => e === error);
    const value = { servers: ['configured'] };
    assert.equal(await withDeadline(Promise.resolve(value), 1000, 'STATE_LOAD_TIMEOUT'), value);
});
