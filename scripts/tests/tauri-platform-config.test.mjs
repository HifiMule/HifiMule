import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import test from "node:test";

const root = resolve(import.meta.dirname, "../..");
const base = JSON.parse(readFileSync(resolve(root, "hifimule-ui/src-tauri/tauri.conf.json")));
const windows = JSON.parse(readFileSync(resolve(root, "hifimule-ui/src-tauri/tauri.windows.conf.json")));
const macos = JSON.parse(readFileSync(resolve(root, "hifimule-ui/src-tauri/tauri.macos.conf.json")));
const linux = JSON.parse(readFileSync(resolve(root, "hifimule-ui/src-tauri/tauri.linux.conf.json")));

function merge(left, right) {
  if (!left || !right || Array.isArray(left) || Array.isArray(right) || typeof left !== "object" || typeof right !== "object") return right;
  const result = { ...left };
  for (const [key, value] of Object.entries(right)) result[key] = key in result ? merge(result[key], value) : value;
  return result;
}

test("effective native-library resource mappings are non-overlapping by platform", () => {
  assert.equal(base.bundle.resources["bundled-libs/*.dll"], undefined);
  assert.equal(base.bundle.resources["bundled-libs/*"], undefined);
  const native = (config) => Object.fromEntries(Object.entries(config.bundle.resources).filter(([key]) => key.startsWith("bundled-libs/")));
  assert.deepEqual(native(merge(base, windows)), { "bundled-libs/*.dll": "." });
  assert.deepEqual(native(merge(base, macos)), { "bundled-libs/*": "bundled-libs/" });
  assert.deepEqual(native(merge(base, linux)), { "bundled-libs/*": "bundled-libs/" });
});

test("NSIS stops the daemon before copying its runtime DLLs", () => {
  const hooks = readFileSync(resolve(root, "hifimule-ui/src-tauri/nsis/hooks.nsh"), "utf8");
  assert.match(hooks, /!macro NSIS_HOOK_PREINSTALL[\s\S]*?!insertmacro CheckIfAppIsRunning "hifimule-daemon\.exe" "\$\{PRODUCTNAME\}"[\s\S]*?!macroend/);
});
