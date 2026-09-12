import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import ts from 'typescript';

const source = await readFile(new URL('../src/shutdownStatus.ts', import.meta.url), 'utf8');
const js = ts.transpileModule(source, {
    compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ES2022 },
}).outputText;
const { shutdownMessageKey, ShutdownPollGate } = await import(
    `data:text/javascript;base64,${Buffer.from(js).toString('base64')}`
);

test('deadline state takes precedence over ordinary progress', () => {
    assert.equal(shutdownMessageKey({ deadlineExceeded: true }, null), 'lifecycle.shutdown_delayed');
    assert.equal(shutdownMessageKey({ deadlineExceeded: false }, 'SHUTDOWN_TIMEOUT'), 'lifecycle.shutdown_delayed');
    assert.equal(shutdownMessageKey({ deadlineExceeded: false }, null), 'lifecycle.shutdown_progress');
    assert.equal(
        shutdownMessageKey({ deadlineExceeded: false }, 'QUIT_PERSISTENCE_FAILED'),
        'lifecycle.quit_persistence_failed',
    );
});

test('shutdown polling permits only one request at a time', async () => {
    let release;
    let calls = 0;
    const gate = new ShutdownPollGate(async () => {
        calls += 1;
        await new Promise(resolve => { release = resolve; });
    });
    const first = gate.poll();
    await gate.poll();
    assert.equal(calls, 1);
    release();
    await first;
    const second = gate.poll();
    assert.equal(calls, 2);
    release();
    await second;
});
