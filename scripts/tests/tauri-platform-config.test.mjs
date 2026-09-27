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

test("Windows installers register the daemon at login on a fresh install", () => {
  const hooks = readFileSync(resolve(root, "hifimule-ui/src-tauri/nsis/hooks.nsh"), "utf8");
  const wix = readFileSync(resolve(root, "hifimule-ui/src-tauri/wix/startup-fragment.wxs"), "utf8");
  assert.match(hooks, /!macro NSIS_HOOK_POSTINSTALL\s+WriteRegStr HKCU "Software\\Microsoft\\Windows\\CurrentVersion\\Run"/);
  assert.doesNotMatch(hooks, /ReadRegStr \$0 HKCU "Software\\Microsoft\\Windows\\CurrentVersion\\Run"/);
  assert.match(wix, /<RegistryValue[\s\S]*?Name="HifiMule"[\s\S]*?Value="\[INSTALLDIR\]hifimule-daemon\.exe"/);
  assert.doesNotMatch(wix, /HIFIMULE_STARTUP_ENABLED/);
});

test("NSIS same-version uninstall ends the installer flow", () => {
  const installer = readFileSync(resolve(root, "hifimule-ui/src-tauri/nsis/installer.nsi"), "utf8");
  assert.equal(base.bundle.windows.nsis.template, "nsis/installer.nsi");
  assert.match(installer, /User chose to uninstall\s+StrCpy \$ReinstallPageCheck 2\s+Goto reinst_uninstall/);
  assert.match(installer, /\$\{If\} \$R0 = 0\s+\$\{AndIf\} \$ReinstallPageCheck = 2\s+Quit/);
});

test("both Windows uninstallers wait for daemon shutdown", () => {
  const hooks = readFileSync(resolve(root, "hifimule-ui/src-tauri/nsis/hooks.nsh"), "utf8");
  const wix = readFileSync(resolve(root, "hifimule-ui/src-tauri/wix/startup-fragment.wxs"), "utf8");
  assert.match(hooks, /NSIS_HOOK_PREUNINSTALL[\s\S]*?hifimule-daemon\.exe" --quit[\s\S]*?StrCmp \$0 0/);
  assert.match(wix, /Id="StopHifiMuleDaemon"[\s\S]*?--quit[\s\S]*?Return="check"/);
  assert.match(wix, /Action="StopHifiMuleDaemon" Before="InstallValidate">Installed</);
});

test("MSI omits the shortcut property write that can raise Warning 1946", () => {
  const wixTemplate = readFileSync(resolve(root, "hifimule-ui/src-tauri/wix/main.wxs"), "utf8");
  assert.equal(base.bundle.windows.wix.template, "wix/main.wxs");
  assert.match(wixTemplate, /Shortcut Id="ApplicationStartMenuShortcut"/);
  assert.doesNotMatch(wixTemplate, /<ShortcutProperty Key="System\.AppUserModel\.ID"/);
  const fragment = readFileSync(resolve(root, "hifimule-ui/src-tauri/wix/startup-fragment.wxs"), "utf8");
  assert.match(fragment, /Id="RegisterHifiMuleShortcutIdentity"[\s\S]*?--set-shortcut-appid[\s\S]*?Return="check"/);
  assert.match(fragment, /Action="RegisterHifiMuleShortcutIdentity" After="CreateShortcuts">NOT REMOVE</);
});

test("AppImage explicitly places the staged private closure in the loader directory", () => {
  const effective = merge(base, linux);
  // AppImage custom files map destination to source, unlike resource mappings.
  // Copy the whole closure, including libraries excluded by linuxdeploy discovery.
  assert.deepEqual(effective.bundle.linux.appimage.files, { "usr/lib": "bundled-libs" });
  assert.equal(effective.bundle.linux.deb.files, undefined);
  assert.deepEqual(effective.bundle.resources["bundled-libs/*"], "bundled-libs/");
});


test("macOS platform overrides preserve certificate-free ad-hoc bundle signing", () => {
  const effective = merge(base, macos);
  assert.equal(effective.bundle.macOS.signingIdentity, "-");
  assert.equal(effective.bundle.macOS.hardenedRuntime, false);
});
