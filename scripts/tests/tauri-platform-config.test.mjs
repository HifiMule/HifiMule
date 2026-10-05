import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import test from "node:test";

const root = resolve(import.meta.dirname, "../..");
const base = JSON.parse(readFileSync(resolve(root, "hifimule-ui/src-tauri/tauri.conf.json")));
const windows = JSON.parse(readFileSync(resolve(root, "hifimule-ui/src-tauri/tauri.windows.conf.json")));
const macos = JSON.parse(readFileSync(resolve(root, "hifimule-ui/src-tauri/tauri.macos.conf.json")));
const linux = JSON.parse(readFileSync(resolve(root, "hifimule-ui/src-tauri/tauri.linux.conf.json")));

test("RPM retains private resources and uses Fedora runtime package names", () => {
  const effective = merge(base, linux);
  assert.deepEqual(effective.bundle.targets, ["deb", "appimage", "rpm"]);
  assert.deepEqual(effective.bundle.linux.rpm.depends, ["gtk3", "webkit2gtk4.1", "libsoup3", "libappindicator-gtk3", "libmtp", "openssl-libs", "libxdo"]);
  assert.deepEqual(effective.bundle.linux.rpm.recommends, ["gnome-shell-extension-appindicator"]);
  assert.ok(!effective.bundle.linux.rpm.depends.includes("gnome-shell-extension-appindicator"));
  assert.deepEqual(effective.bundle.externalBin, ["sidecars/hifimule-daemon"]);
  assert.equal(effective.bundle.resources["bundled-libs/*"], "bundled-libs/");
  assert.equal(effective.bundle.resources["../../hifimule-daemon/audio-runtime.json"], "audio-runtime.json");
  assert.equal(effective.bundle.resources["../../hifimule-daemon/THIRD_PARTY_AUDIO_NOTICES.md"], "THIRD_PARTY_AUDIO_NOTICES.md");
});

function merge(left, right) {
  if (!left || !right || Array.isArray(left) || Array.isArray(right) || typeof left !== "object" || typeof right !== "object") return right;
  const result = { ...left };
  for (const [key, value] of Object.entries(right)) result[key] = key in result ? merge(result[key], value) : value;
  return result;
}

test("installed Linux packages start only the daemon at desktop login", () => {
  const effective = merge(base, linux);
  const destination = "/etc/xdg/autostart/hifimule-daemon.desktop";
  const source = "linux/hifimule-daemon.desktop";
  assert.equal(effective.bundle.linux.rpm.files?.[destination], source);
  assert.equal(effective.bundle.linux.deb.files?.[destination], source);
  const desktop = readFileSync(resolve(root, "hifimule-ui/src-tauri", source), "utf8");
  const fields = Object.fromEntries(desktop.trim().split(/\r?\n/).slice(1).map((line) => {
    const index = line.indexOf("=");
    return [line.slice(0, index), line.slice(index + 1)];
  }));
  assert.equal(desktop.split(/\r?\n/)[0], "[Desktop Entry]");
  assert.equal(fields.Type, "Application");
  assert.equal(fields.Exec, "/usr/bin/hifimule-daemon");
  assert.equal(fields.TryExec, fields.Exec);
  assert.equal(fields.Terminal, "false");
  assert.notEqual(fields.Hidden, "true");
  assert.equal(fields.OnlyShowIn, undefined);
  assert.equal(fields.NotShowIn, undefined);
  assert.equal(fields.DBusActivatable, undefined);
  assert.ok(!Object.keys(effective.bundle.linux.appimage.files).some((path) => path.includes("autostart")));
});

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
  assert.match(hooks, /!macro NSIS_HOOK_PREINSTALL[\s\S]*?!insertmacro CheckIfAppIsRunning "\$INSTDIR\\hifimule-daemon\.exe" "\$\{PRODUCTNAME\}"[\s\S]*?!macroend/);
});

test("NSIS provides Restart Manager macros before hooks and checks installed executable paths", () => {
  const installer = readFileSync(resolve(root, "hifimule-ui/src-tauri/nsis/installer.nsi"), "utf8");
  const header = '!include "Win\\RestartManager.nsh"';
  const includeIndex = installer.indexOf(header);
  assert.notEqual(includeIndex, -1, "Tauri 2.12 running-app checks require RestartManager.nsh");
  assert.ok(includeIndex < installer.indexOf('!include "{{installer_hooks}}"'));
  const calls = [...installer.matchAll(/!insertmacro CheckIfAppIsRunning "([^"]+)"/g)];
  assert.equal(calls.length, 2, "install and uninstall both check the UI executable");
  for (const call of calls) {
    assert.equal(call[1], "$INSTDIR\\${MAINBINARYNAME}.exe");
    assert.ok(includeIndex < call.index);
  }
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
  assert.equal(effective.bundle.linux.deb.files?.["usr/lib"], undefined);
  assert.deepEqual(effective.bundle.resources["bundled-libs/*"], "bundled-libs/");
});


test("macOS platform overrides preserve certificate-free ad-hoc bundle signing", () => {
  const effective = merge(base, macos);
  assert.equal(effective.bundle.macOS.signingIdentity, "-");
  assert.equal(effective.bundle.macOS.hardenedRuntime, false);
});
