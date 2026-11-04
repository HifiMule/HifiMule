import assert from "node:assert/strict";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { basename, join, resolve } from "node:path";
import test from "node:test";
import { ensureWindowsAudioRuntime, prependWindowsPath, stageWindowsAudioRuntimeForCargo, verifyInstalledWindowsBundle, verifyWindowsAudioRuntime, windowsRuntimeDllNames } from "../windows-audio-runtime.mjs";

test("Windows PATH normalization retains entries from competing case aliases", () => {
  const source = { Path: "C:\\Rust\\bin", PATH: "C:\\Windows", path: "C:\\Rust\\bin", KEEP: "yes" };
  assert.deepEqual(prependWindowsPath(source, "C:\\ffmpeg\\bin"), {
    PATH: "C:\\ffmpeg\\bin;C:\\Rust\\bin;C:\\Windows", KEEP: "yes",
  });
  assert.equal(source.Path, "C:\\Rust\\bin");
  assert.deepEqual(prependWindowsPath({}, "C:\\ffmpeg\\bin"), { PATH: "C:\\ffmpeg\\bin" });
});

const root = resolve(import.meta.dirname, "../..");
const manifest = JSON.parse(readFileSync(join(root, "hifimule-daemon/audio-runtime.json"), "utf8"));

function fakeRuntime(target = "aarch64-pc-windows-msvc") {
  const prefix = mkdtempSync(join(tmpdir(), "hifimule-windows-ffmpeg-"));
  const arch = target.startsWith("aarch64") ? "arm64" : "amd64";
  const machine = target.startsWith("aarch64") ? 0xaa64 : 0x8664;
  mkdirSync(join(prefix, "lib", arch), { recursive: true });
  mkdirSync(join(prefix, "bin"));
  writeFileSync(join(prefix, "bin", "ffmpeg.exe"), "fixture");
  const receipt = {
    schemaVersion: 1, targetTriple: target, ffmpegRelease: manifest.ffmpegRelease,
    sourceUrl: manifest.sourceUrl, signatureUrl: manifest.signatureUrl, signingKey: manifest.signingKey,
    sourceSha256: manifest.sourceSha256, configureFlags: [...manifest.configureFlags, ...manifest.windowsConfigureFlags], abiVersions: manifest.abiVersions,
    encodingDependencies: manifest.encodingDependencies,
  };
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

function populateOfficialRuntime(prefix, target) {
  const machine = target.startsWith("aarch64") ? 0xaa64 : 0x8664;
  mkdirSync(join(prefix, "lib"), { recursive: true });
  mkdirSync(join(prefix, "bin"), { recursive: true });
  writeFileSync(join(prefix, "bin", "ffmpeg.exe"), "fixture");
  for (const [library, version] of Object.entries(manifest.abiVersions)) {
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

test("rejects a cached Windows runtime without the matching DLL CRT flags", t => {
  const prefix = fakeRuntime("x86_64-pc-windows-msvc");
  t.after(() => rmSync(prefix, { recursive: true, force: true }));
  const path = join(prefix, ".hifimule-audio-runtime.json");
  const receipt = JSON.parse(readFileSync(path, "utf8"));
  receipt.configureFlags = receipt.configureFlags.filter(flag => flag !== "--extra-cflags=-MD" && flag !== "--extra-cxxflags=-MD");
  writeFileSync(path, JSON.stringify(receipt));
  assert.throws(() => verifyWindowsAudioRuntime(prefix, "x86_64-pc-windows-msvc"), /receipt is stale/);
});

test("rejects a runtime built for the wrong Windows architecture", () => {
  const prefix = fakeRuntime("x86_64-pc-windows-msvc");
  const receipt = JSON.parse(readFileSync(join(prefix, ".hifimule-audio-runtime.json")));
  receipt.targetTriple = "aarch64-pc-windows-msvc";
  writeFileSync(join(prefix, ".hifimule-audio-runtime.json"), JSON.stringify(receipt));
  assert.throws(() => verifyWindowsAudioRuntime(prefix, "aarch64-pc-windows-msvc"), /import library is missing|architecture/);
});

test("missing runtime fails with exact setup guidance", () => {
  assert.throws(() => verifyWindowsAudioRuntime(join(tmpdir(), "missing-hifimule-ffmpeg"), "aarch64-pc-windows-msvc"), /pinned FFmpeg runtime.*FFMPEG_DIR/s);
});

test("formats staged DLL names without passing map indexes as basename suffixes", () => {
  assert.deepEqual(
    windowsRuntimeDllNames([join("runtime", "bin", "avcodec-63.dll"), join("runtime", "bin", "avutil-61.dll")]),
    ["avcodec-63.dll", "avutil-61.dll"],
  );
});

test("FFMPEG_DIR remains a validated explicit override", () => {
  const prefix = fakeRuntime("x86_64-pc-windows-msvc");
  assert.equal(ensureWindowsAudioRuntime("x86_64-pc-windows-msvc", { env: { FFMPEG_DIR: prefix } }), prefix);
});

test("stages the verified runtime beside Cargo test executables", (t) => {
  const prefix = fakeRuntime("x86_64-pc-windows-msvc");
  const output = mkdtempSync(join(tmpdir(), "hifimule-cargo-runtime-"));
  t.after(() => { rmSync(prefix, { recursive: true, force: true }); rmSync(output, { recursive: true, force: true }); });

  const staged = stageWindowsAudioRuntimeForCargo(prefix, "x86_64-pc-windows-msvc", output);

  assert.deepEqual(staged.map((path) => basename(path)).sort(), windowsRuntimeDllNames(verifyWindowsAudioRuntime(prefix, "x86_64-pc-windows-msvc").dlls).sort());
  for (const path of staged) assert.equal(readFileSync(path).toString("hex"), readFileSync(join(prefix, "bin", basename(path))).toString("hex"));
});

test("installed Windows verifier requires matching private DLLs and packaged notices", (t) => {
  const bundle = mkdtempSync(join(tmpdir(), "hifimule-windows-bundle-"));
  t.after(() => rmSync(bundle, { recursive: true, force: true }));
  const machine = 0x8664;
  const pe = () => { const bytes = Buffer.alloc(256); bytes.write("MZ"); bytes.writeUInt32LE(64, 0x3c); bytes.write("PE\0\0", 64); bytes.writeUInt16LE(machine, 68); bytes.writeUInt16LE(3, 156); return bytes; };
  writeFileSync(join(bundle, "hifimule-daemon.exe"), pe());
  writeFileSync(join(bundle, "mediacheck.exe"), pe());
  writeFileSync(join(bundle, "audio-runtime.json"), JSON.stringify(manifest));
  writeFileSync(join(bundle, "THIRD_PARTY_AUDIO_NOTICES.md"), "notices");
  for (const [library, version] of Object.entries(manifest.abiVersions)) writeFileSync(join(bundle, `${library}-${version.split(".")[0]}.dll`), pe());
  assert.equal(verifyInstalledWindowsBundle(bundle, "x86_64-pc-windows-msvc").dllCount, 4);
  const cli = join(bundle, "mediacheck.exe");
  const invalidCli = pe();
  invalidCli.writeUInt16LE(0xaa64, 68);
  writeFileSync(cli, invalidCli);
  assert.throws(() => verifyInstalledWindowsBundle(bundle, "x86_64-pc-windows-msvc"), /mediacheck architecture/);
  invalidCli.writeUInt16LE(machine, 68);
  invalidCli.writeUInt16LE(2, 156);
  writeFileSync(cli, invalidCli);
  assert.throws(() => verifyInstalledWindowsBundle(bundle, "x86_64-pc-windows-msvc"), /console subsystem/);
  rmSync(cli);
  assert.throws(() => verifyInstalledWindowsBundle(bundle, "x86_64-pc-windows-msvc"), /missing mediacheck.exe beside/);
  writeFileSync(cli, pe());
  rmSync(join(bundle, "avcodec-63.dll"));
  assert.throws(() => verifyInstalledWindowsBundle(bundle, "x86_64-pc-windows-msvc"), /exactly one avcodec-63.dll/);
});
