import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import test from "node:test";
import { ensureWindowsAudioRuntime, verifyWindowsAudioRuntime, windowsRuntimeDllNames } from "../windows-audio-runtime.mjs";

const root = resolve(import.meta.dirname, "../..");
const manifest = JSON.parse(readFileSync(join(root, "hifimule-daemon/audio-runtime.json"), "utf8"));

function fakeRuntime(target = "aarch64-pc-windows-msvc") {
  const prefix = mkdtempSync(join(tmpdir(), "hifimule-windows-ffmpeg-"));
  const arch = target.startsWith("aarch64") ? "arm64" : "amd64";
  const machine = target.startsWith("aarch64") ? 0xaa64 : 0x8664;
  mkdirSync(join(prefix, "lib", arch), { recursive: true });
  mkdirSync(join(prefix, "bin"));
  const receipt = { schemaVersion: 1, targetTriple: target, ffmpegRelease: manifest.ffmpegRelease, sourceSha256: manifest.sourceSha256, configureFlags: manifest.configureFlags, abiVersions: manifest.abiVersions };
  writeFileSync(join(prefix, ".hifimule-audio-runtime.json"), JSON.stringify(receipt));
  for (const [library, version] of Object.entries(manifest.abiVersions)) {
    const [major, minor, micro] = version.split(".");
    const include = join(prefix, "include", `lib${library}`); mkdirSync(include, { recursive: true });
    const upper = library.toUpperCase();
    writeFileSync(join(include, "version.h"), `#define LIB${upper}_VERSION_MINOR ${minor}\n#define LIB${upper}_VERSION_MICRO ${micro}\n`);
    writeFileSync(join(include, "version_major.h"), `#define LIB${upper}_VERSION_MAJOR ${major}\n`);
    writeFileSync(join(prefix, "lib", arch, `${library}.lib`), "fixture");
    const pe = Buffer.alloc(128); pe.write("MZ"); pe.writeUInt32LE(64, 0x3c); pe.write("PE\0\0", 64); pe.writeUInt16LE(machine, 68);
    writeFileSync(join(prefix, "bin", `${library}-${major}.dll`), pe);
  }
  return prefix;
}

function populateDistributionRuntime(prefix, target) {
  const machine = target.startsWith("aarch64") ? 0xaa64 : 0x8664;
  mkdirSync(join(prefix, "lib"), { recursive: true });
  mkdirSync(join(prefix, "bin"), { recursive: true });
  for (const [library, version] of Object.entries(manifest.windowsDistribution.abiVersions)) {
    const [major, minor, micro] = version.split(".");
    const include = join(prefix, "include", `lib${library}`); mkdirSync(include, { recursive: true });
    const upper = library.toUpperCase();
    writeFileSync(join(include, "version.h"), `#define LIB${upper}_VERSION_MAJOR ${major}\n#define LIB${upper}_VERSION_MINOR ${minor}\n#define LIB${upper}_VERSION_MICRO ${micro}\n`);
    writeFileSync(join(prefix, "lib", `${library}.lib`), "fixture");
    const pe = Buffer.alloc(128); pe.write("MZ"); pe.writeUInt32LE(64, 0x3c); pe.write("PE\0\0", 64); pe.writeUInt16LE(machine, 68);
    writeFileSync(join(prefix, "bin", `${library}-${major}.dll`), pe);
  }
}

test("accepts an exact controlled ARM64 FFmpeg development runtime", () => {
  const prefix = fakeRuntime();
  assert.equal(verifyWindowsAudioRuntime(prefix, "aarch64-pc-windows-msvc").dlls.length, 4);
});

test("rejects a runtime built for the wrong Windows architecture", () => {
  const prefix = fakeRuntime("x86_64-pc-windows-msvc");
  const receipt = JSON.parse(readFileSync(join(prefix, ".hifimule-audio-runtime.json")));
  receipt.targetTriple = "aarch64-pc-windows-msvc";
  writeFileSync(join(prefix, ".hifimule-audio-runtime.json"), JSON.stringify(receipt));
  assert.throws(() => verifyWindowsAudioRuntime(prefix, "aarch64-pc-windows-msvc"), /import library is missing|architecture/);
});

test("missing runtime fails with exact setup guidance", () => {
  assert.throws(() => verifyWindowsAudioRuntime(join(tmpdir(), "missing-hifimule-ffmpeg"), "aarch64-pc-windows-msvc"), /automatically provisions.*FFMPEG_DIR/s);
});

test("formats staged DLL names without passing map indexes as basename suffixes", () => {
  assert.deepEqual(
    windowsRuntimeDllNames([join("runtime", "bin", "avcodec-63.dll"), join("runtime", "bin", "avutil-61.dll")]),
    ["avcodec-63.dll", "avutil-61.dll"],
  );
});

for (const target of ["aarch64-pc-windows-msvc", "x86_64-pc-windows-msvc"]) {
  test(`auto-provisions the hash-pinned BtbN SDK for ${target}`, () => {
    const cacheRoot = mkdtempSync(join(tmpdir(), "hifimule-windows-cache-"));
    const selected = manifest.windowsDistribution.artifacts[target];
    let downloads = 0;
    const options = {
      platform: "win32",
      env: {},
      cacheRoot,
      sha: () => selected.sha256,
      download: (url, path) => {
        downloads += 1;
        assert.ok(url.endsWith(`/${selected.archive}`));
        assert.match(url, new RegExp(`/${manifest.windowsDistribution.releaseTag}/`));
        writeFileSync(path, "archive fixture");
      },
      extract: (_archive, staging) => populateDistributionRuntime(staging, target),
    };
    const prefix = ensureWindowsAudioRuntime(target, options);
    assert.equal(verifyWindowsAudioRuntime(prefix, target).dlls.length, 4);
    const receipt = JSON.parse(readFileSync(join(prefix, ".hifimule-audio-runtime.json"), "utf8"));
    assert.equal(receipt.archiveSha256, selected.sha256);
    assert.equal(receipt.provider, "BtbN/FFmpeg-Builds");
    assert.equal(ensureWindowsAudioRuntime(target, options), prefix);
    assert.equal(downloads, 1, "validated cache should be reused");
  });
}

test("FFMPEG_DIR remains a validated explicit override", () => {
  const prefix = fakeRuntime("x86_64-pc-windows-msvc");
  assert.equal(ensureWindowsAudioRuntime("x86_64-pc-windows-msvc", { env: { FFMPEG_DIR: prefix } }), prefix);
});

test("rejects a downloaded Windows SDK with the wrong hash", () => {
  const cacheRoot = mkdtempSync(join(tmpdir(), "hifimule-windows-bad-hash-"));
  assert.throws(
    () => ensureWindowsAudioRuntime("aarch64-pc-windows-msvc", {
      platform: "win32",
      env: {},
      cacheRoot,
      sha: () => "0".repeat(64),
      download: (_url, path) => writeFileSync(path, "bad archive"),
      extract: () => assert.fail("bad archive must not be extracted"),
    }),
    /checksum mismatch/,
  );
});
