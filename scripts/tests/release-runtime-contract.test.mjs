import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import test from "node:test";

const root = resolve(import.meta.dirname, "../..");
const read = (path) => readFileSync(resolve(root, path), "utf8");
const manifest = JSON.parse(read("hifimule-daemon/audio-runtime.json"));

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

test("shipping workflow requires platform trust rather than ad-hoc signing", () => {
  const config = JSON.parse(read("hifimule-ui/src-tauri/tauri.conf.json"));
  assert.equal(config.bundle.macOS.signingIdentity, null);
  assert.equal(config.bundle.macOS.hardenedRuntime, true);
  const release = read(".github/workflows/release.yml");
  assert.match(release, /Require Windows Authenticode credentials/);
  assert.match(release, /Verify Windows Authenticode signatures/);
  assert.match(release, /Require macOS Developer ID and notarization credentials/);
  assert.match(release, /Verify macOS Developer ID and notarization/);
  assert.doesNotMatch(release, /validly ad-hoc signed/);
});
