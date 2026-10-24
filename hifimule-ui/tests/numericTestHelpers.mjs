import {readFileSync} from 'node:fs';
import ts from 'typescript';
const exports = {};
new Function('require', 'exports', ts.transpileModule(readFileSync(new URL('../src/numericValidation.ts', import.meta.url), 'utf8'), {
    compilerOptions: {module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022}
}).outputText)(() => ({t: (key, vars = {}) => key + Object.values(vars).join(' ')}), exports);
export const numeric = exports;
