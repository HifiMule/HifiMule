import {numeric} from './numericTestHelpers.mjs';
import { readFileSync } from 'node:fs';
import ts from 'typescript';
import { escapeAttr } from '../src/html.ts';
const source=readFileSync(new URL('../src/deviceProfiles.ts',import.meta.url),'utf8');
const script=ts.transpileModule(source,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText;
const t=(key,vars={})=>key+Object.values(vars).join(' ');
export function loadProfiles(extra={}) {
 const exports={};new Function('require','exports',script)(name=>name==='virtual:hifimule-locale/en'?{default:JSON.parse(readFileSync(new URL('../../hifimule-i18n/catalog.json',import.meta.url),'utf8')).en}:name==='./i18n'?{t}:name==='./html'?{escapeAttr}:name==='./numericValidation'?numeric:extra[name]??{},exports);return exports;
}
