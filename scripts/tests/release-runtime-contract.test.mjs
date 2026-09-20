import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import test from "node:test";

const root = resolve(import.meta.dirname, "../..");
const read = (path) => readFileSync(resolve(root, path), "utf8");
const manifest = JSON.parse(read("hifimule-daemon/audio-runtime.json"));
const appleSigningVariables = ["APPLE_CERTIFICATE", "APPLE_CERTIFICATE_PASSWORD", "APPLE_SIGNING_IDENTITY", "APPLE_ID", "APPLE_PASSWORD", "APPLE_TEAM_ID"];

function namedWorkflowStep(workflow, name) {
  const marker = `      - name: ${name}\n`;
  const start = workflow.indexOf(marker);
  assert.notEqual(start, -1, `workflow step must exist: ${name}`);
  const next = workflow.indexOf("\n      - name:", start + marker.length);
  return workflow.slice(start, next === -1 ? workflow.length : next);
}

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
  assert.equal(config.bundle.macOS.signingIdentity, null);
  assert.equal(config.bundle.macOS.hardenedRuntime, true);
  const release = read(".github/workflows/release.yml");
  assert.match(release, /Configure optional Windows Authenticode signing/);
  assert.match(release, /Windows Authenticode credentials are not configured; building unsigned MSI and NSIS artifacts/);
  assert.match(release, /Authenticode verification will be skipped/);
  assert.match(release, /Windows Authenticode credentials are partially configured/);
  assert.match(release, /Set-Content[^\n]+tauri\.windows-signing\.conf\.json[^\n]+-Value '\{\}'/);
  assert.match(release, /steps\.windows_signing\.outputs\.enabled == 'true'/);
  assert.match(release, /Verify Windows Authenticode signatures/);
  assert.match(release, /Configure optional macOS Developer ID signing and notarization/);
  assert.match(release, /Apple signing and notarization credentials are not configured; building unsigned app and DMG artifacts/);
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
  assert.doesNotMatch(release, /validly ad-hoc signed/);
});

test("unsigned macOS release paths do not export Apple signing credentials to Tauri", () => {
  const release = read(".github/workflows/release.yml");
  const unsignedCandidate = namedWorkflowStep(release, "Build unsigned macOS immutable candidate");
  const unsignedTag = namedWorkflowStep(release, "Build unsigned macOS draft release");
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
});
