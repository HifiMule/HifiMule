import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import test from "node:test";

const root = resolve(import.meta.dirname, "../..");
const read = (path) => readFileSync(resolve(root, path), "utf8");

test("all installer configurations include mediacheck as an external console binary", () => {
  const config = JSON.parse(read("hifimule-ui/src-tauri/tauri.conf.json"));
  assert.deepEqual(config.bundle.externalBin, ["sidecars/hifimule-daemon", "sidecars/mediacheck"]);
  assert.match(read("hifimule-ui/src-tauri/nsis/installer.nsi"), /Copy external binaries\s+\{\{#each binaries\}\}/);
  assert.match(read("hifimule-ui/src-tauri/wix/main.wxs"), /\{\{#each binaries as \|bin\|/);
});

test("installed runtime command-line verifiers execute mediacheck help with packaged libraries", () => {
  for (const platform of ["macos", "linux", "windows"]) {
    const source = read(`scripts/${platform}-audio-runtime.mjs`);
    assert.match(source, /installed\.mediacheck, \["--help"\]/);
    assert.match(source, /help\.includes\("Usage: mediacheck"\)/);
  }
  assert.match(read("scripts/macos-audio-runtime.mjs"), /delete env\.DYLD_LIBRARY_PATH/);
  assert.match(read("scripts/linux-audio-runtime.mjs"), /delete env\.LD_LIBRARY_PATH/);
  assert.match(read("scripts/windows-audio-runtime.mjs"), /console subsystem/);
});

test("Windows CI audits actual SMB sources from built and installed executables", () => {
  const smoke = "scripts/smoke-tests/check-mediacheck-windows-shares.ps1";
  const build = read(".github/workflows/build.yml");
  const release = read(".github/workflows/release.yml");
  assert.ok(build.includes(`./${smoke} -MediaCheck target/debug/mediacheck.exe`));
  assert.ok(release.includes(`./${smoke} -MediaCheck $MediaCheck`));
  assert.match(build, /npm run build:daemon -- build -p hifimule-daemon --bin mediacheck/);
  assert.match(release, /Get-ChildItem \$Extracted -Filter mediacheck\.exe/);
  const script = read(smoke);
  for (const required of ["New-SmbShare", "\\\\localhost\\", "net.exe use $Drive $Unc /persistent:no", "& $Executable $Folder --log $Report", "Björk été", "healthy source.wav", "Get-FileHash", "Remove-SmbShare", "net.exe use $Drive /delete /y"]) {
    assert.ok(script.includes(required), `SMB smoke must contain ${required}`);
  }
  assert.match(script, /finally \{[\s\S]*Remove-SmbShare[\s\S]*Remove-Item/);
  assert.doesNotMatch(script, /^\s*(?:New-PSDrive|Set-Location)\b/m);
  // These assertions supplement the real SMB executions in both workflows.
  assert.match(script, /Checked files: 1/);
  assert.match(script, /Ignored files: 1/);
  assert.match(script, /Operational errors: 0/);
});
