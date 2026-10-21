import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import vm from 'node:vm';
import ts from '../../hifimule-ui/node_modules/typescript/lib/typescript.js';

function load(name, imports = {}) {
  const source = ts.transpileModule(readFileSync(new URL(`../../hifimule-ui/src/${name}.ts`, import.meta.url), 'utf8'), {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
  }).outputText;
  const exports = {};
  vm.runInNewContext(source, { exports, require: name => imports[name] });
  return exports;
}

test('text and attribute escaping preserve Unicode and neutralize HTML delimiters', () => {
  const { escapeHtml, escapeAttr } = load('html');
  const value = `<img src="x" onerror='alert(1)'> & Björk`;
  assert.equal(escapeHtml(value), `&lt;img src="x" onerror='alert(1)'&gt; &amp; Björk`);
  assert.equal(escapeAttr(value), '&lt;img src=&quot;x&quot; onerror=&#39;alert(1)&#39;&gt; &amp; Björk');
  assert.equal(escapeAttr('&quot;'), '&amp;quot;');
  assert.equal(escapeHtml(''), '');
});

test('device icon labels retain fallback and translate using the current locale', () => {
  let locale = 'en';
  const { deviceIconLabel } = load('deviceIcon', { './i18n': { t: key => `${locale}:${key}` } });
  for (const [icon, key] of Object.entries({
    'usb-drive': 'usb_drive', 'phone-fill': 'phone', watch: 'watch',
    'sd-card': 'sd_card', headphones: 'headphones', 'music-note-list': 'music_player',
  })) assert.equal(deviceIconLabel(icon), `en:basket.icon.${key}`);
  locale = 'fr';
  assert.equal(deviceIconLabel('usb-drive'), 'fr:basket.icon.usb_drive');
  assert.equal(deviceIconLabel('custom-icon'), 'custom-icon');
});
