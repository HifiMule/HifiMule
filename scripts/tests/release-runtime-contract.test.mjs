import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import test from "node:test";

const root = resolve(import.meta.dirname, "../..");
const read = (path) => readFileSync(resolve(root, path), "utf8");
const manifest = JSON.parse(read("hifimule-daemon/audio-runtime.json"));
const appleSigningVariables = ["APPLE_CERTIFICATE", "APPLE_CERTIFICATE_PASSWORD", "APPLE_SIGNING_IDENTITY", "APPLE_ID", "APPLE_PASSWORD", "APPLE_TEAM_ID"];

function namedWorkflowStep(workflow, name) {
  workflow = workflow.replace(/\r\n?/g, "\n");
  const marker = `      - name: ${name}\n`;
  const start = workflow.indexOf(marker);
  assert.notEqual(start, -1, `workflow step must exist: ${name}`);
  const next = workflow.indexOf("\n      - name:", start + marker.length);
  return workflow.slice(start, next === -1 ? workflow.length : next);
}

test("named workflow steps are extracted with LF, CRLF, and CR line endings", () => {
  const fixture = [
    "jobs:",
    "  release:",
    "    steps:",
    "      - name: First step",
    "        run: echo first",
    "      - name: Second step",
    "        run: echo second",
  ].join("\n");
  const expected = "      - name: First step\n        run: echo first";
  for (const newline of ["\n", "\r\n", "\r"]) {
    assert.equal(namedWorkflowStep(fixture.replaceAll("\n", newline), "First step"), expected);
  }
});

test("controlled runtime is the signed FFmpeg 9.0.2 source on shipping targets only", () => {
  assert.equal(manifest.ffmpegRelease, "9.0.2");
  assert.equal(manifest.sourceSha256, "8c3850283eb25fa026482078a04051e0be17347b09ef81a0849bec15a96e002e");
  assert.deepEqual(manifest.abiVersions, {
    avcodec: "63.1.102",
    avformat: "63.1.102",
    avutil: "61.1.102",
    swresample: "7.1.102",
  });
  assert.deepEqual(Object.keys(manifest.verification).sort(), ["linuxX64", "macosArm64", "macosX64", "windowsX64"]);
  assert.ok(Object.values(manifest.verification).every((value) => value === "candidate-required"));
});

test("audio notices match the locked bindings and official source offer", () => {
  const notices = read("hifimule-daemon/THIRD_PARTY_AUDIO_NOTICES.md");
  for (const phrase of ["FFmpeg 9.0.2", "CPAL 0.18.2", "ffmpeg-next` 9.0.0", manifest.sourceSha256]) {
    assert.ok(notices.includes(phrase), `notice must include ${phrase}`);
  }
  assert.doesNotMatch(notices, /CPAL 0\.16\.0|Homebrew-derived FFmpeg/);
});

test("macOS builds provision controlled source and every platform verifies installed bundles", () => {
  const release = read(".github/workflows/release.yml");
  const build = read(".github/workflows/build.yml");
  assert.doesNotMatch(release, /brew install[^\n]*ffmpeg/);
  assert.doesNotMatch(build, /brew install[^\n]*ffmpeg/);
  for (const workflow of [release, build]) assert.match(workflow, /macos-audio-runtime\.mjs env/);
  assert.match(release, /windows-audio-runtime\.mjs verify-bundle/);
  assert.match(release, /macos-audio-runtime\.mjs verify-bundle/);
  assert.match(read("scripts/prepare-sidecar.mjs"), /ensureMacosAudioRuntime/);
  assert.match(read("scripts/build-daemon.mjs"), /ensureMacosAudioRuntime/);
});

test("shipping workflow makes platform signing optional but strict when configured", () => {
  const config = JSON.parse(read("hifimule-ui/src-tauri/tauri.conf.json"));
  assert.equal(config.bundle.macOS.signingIdentity, "-");
  assert.equal(config.bundle.macOS.hardenedRuntime, false);
  const release = read(".github/workflows/release.yml");
  assert.match(release, /Configure optional Windows Authenticode signing/);
  assert.match(release, /Windows Authenticode credentials are not configured; building unsigned MSI and NSIS artifacts/);
  assert.match(release, /Authenticode verification will be skipped/);
  assert.match(release, /Windows Authenticode credentials are partially configured/);
  assert.match(release, /Set-Content[^\n]+tauri\.windows-signing\.conf\.json[^\n]+-Value '\{\}'/);
  assert.match(release, /steps\.windows_signing\.outputs\.enabled == 'true'/);
  assert.match(release, /Verify Windows Authenticode signatures/);
  assert.match(release, /Configure optional macOS Developer ID signing and notarization/);
  assert.match(release, /Apple signing and notarization credentials are not configured; building ad-hoc signed app and DMG artifacts/);
  assert.match(release, /Developer ID and notarization verification will be skipped/);
  assert.match(release, /Apple signing and notarization credentials are partially configured; missing:/);
  assert.match(release, /steps\.macos_signing\.outputs\.enabled == 'true'/);
  assert.match(release, /Verify macOS Developer ID and notarization/);
  assert.match(release, /'enabled=false' >> \$env:GITHUB_OUTPUT/);
  assert.match(release, /'enabled=true' >> \$env:GITHUB_OUTPUT/);
  assert.match(release, /echo 'enabled=false' >> "\$GITHUB_OUTPUT"/);
  assert.match(release, /echo 'enabled=true' >> "\$GITHUB_OUTPUT"/);
  assert.match(release, /value="\$\{!name:-\}"/);
  assert.match(release, /value\/\/\[\[:space:\]\]\//);
});

test("Linux packages use host GUI libraries and allow the lifecycle startup budget", () => {
  const config = JSON.parse(read("hifimule-ui/src-tauri/tauri.conf.json"));
  assert.deepEqual(config.bundle.linux.deb.depends, [
    "libmtp9",
    "libayatana-appindicator3-1 | libappindicator3-1",
    "libssl3 | libssl3t64",
    "libxdo3",
  ]);
  const runtime = read("scripts/linux-audio-runtime.mjs");
  assert.match(runtime, /The daemon's GUI and tray dependencies must resolve from the host/);
  assert.doesNotMatch(runtime, /for \(const name of inspectElf\(sidecar, target\)\.needed\)/);
  assert.match(read("hifimule-lifecycle/src/lib.rs"), /STARTUP_DEADLINE: Duration = Duration::from_secs\(30\)/);
  assert.match(read("hifimule-ui/src/main.ts"), /performance\.now\(\) \+ 30_000/);
});

test("ad-hoc macOS release paths do not export Apple signing credentials to Tauri", () => {
  const release = read(".github/workflows/release.yml");
  const unsignedCandidate = namedWorkflowStep(release, "Build ad-hoc signed macOS immutable candidate");
  const unsignedTag = namedWorkflowStep(release, "Build ad-hoc signed macOS draft release");
  const signedCandidate = namedWorkflowStep(release, "Build signed macOS immutable candidate");
  const signedTag = namedWorkflowStep(release, "Build signed macOS draft release");

  assert.match(unsignedCandidate, /steps\.macos_signing\.outputs\.enabled == 'false'/);
  assert.match(unsignedCandidate, new RegExp(`unset ${appleSigningVariables.join(" ")}`));
  assert.match(unsignedCandidate, /pnpm exec tauri build/);
  assert.match(unsignedTag, /steps\.macos_signing\.outputs\.enabled == 'false'/);
  assert.match(unsignedTag, /uses: tauri-apps\/tauri-action@/);
  for (const variable of appleSigningVariables) {
    const environmentEntry = new RegExp(`^\\s+${variable}:`, "m");
    const allEnvironmentEntries = new RegExp(`^\\s+${variable}:`, "gm");
    assert.doesNotMatch(unsignedCandidate, environmentEntry);
    assert.doesNotMatch(unsignedTag, environmentEntry);
    assert.match(signedCandidate, environmentEntry);
    assert.match(signedTag, environmentEntry);
    assert.equal(
      [...release.matchAll(allEnvironmentEntries)].length,
      3,
      `${variable} must appear only in signing preflight and the two signed macOS build steps`,
    );
  }
  assert.match(signedCandidate, /steps\.macos_signing\.outputs\.enabled == 'true'/);
  assert.match(signedTag, /steps\.macos_signing\.outputs\.enabled == 'true'/);
  for (const step of [signedCandidate, signedTag]) {
    assert.match(step, /--config '\{"bundle":\{"macOS":\{"hardenedRuntime":true\}\}\}'/);
  }
  for (const step of [unsignedCandidate, unsignedTag]) assert.doesNotMatch(step, /hardenedRuntime/);
});


for (const [lineEnding, newline] of [["LF", "\n"], ["CRLF", "\r\n"], ["CR", "\r"]]) {
  test(`every macOS release verifies bundle and nested signature integrity before conditional trust checks (${lineEnding})`, () => {
    const workflow = read(".github/workflows/release.yml").replace(/\r\n?/g, "\n").replaceAll("\n", newline);
    // Compare offsets in the same normalized text used by namedWorkflowStep.
    const release = workflow.replace(/\r\n?/g, "\n");
    const integrity = namedWorkflowStep(release, "Verify macOS bundle signature integrity");
    const trust = namedWorkflowStep(release, "Verify macOS Developer ID and notarization");
    assert.match(integrity, /^        if: startsWith\(matrix.platform, 'macos'\)$/m);
    assert.match(integrity, /set -euo pipefail/);
    assert.match(integrity, /codesign --verify --deep --strict --verbose=2 "\$APP_DIR"/);
    assert.match(integrity, /grep -q 'Sealed Resources'/);
    assert.match(integrity, /Bundle signature is missing sealed resources[\s\S]*exit 1/);
    assert.match(integrity, /codesign --verify --strict --verbose=2 "\$APP_DIR\/Contents\/MacOS\/hifimule-daemon"/);
    assert.match(integrity, /for library in "\$APP_DIR"\/Contents\/Resources\/bundled-libs\/\*\.dylib/);
    assert.match(integrity, /codesign --verify --strict --verbose=2 "\$library"/);
    assert.doesNotMatch(integrity, /macos_signing|spctl|stapler|Authority=Developer ID/);
    assert.match(trust, /^        if: startsWith\(matrix.platform, 'macos'\) && steps\.macos_signing\.outputs\.enabled == 'true'$/m);
    assert.match(trust, /Authority=Developer ID Application:/);
    assert.match(trust, /spctl --assess --type execute/);
    assert.match(trust, /xcrun stapler validate "\$DMG"/);
    assert.ok(release.indexOf(integrity) < release.indexOf(trust));
  });
}
