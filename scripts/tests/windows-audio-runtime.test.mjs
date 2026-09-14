import assert from "node:assert/strict";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import test from "node:test";
import { createGpgHome, ensureWindowsAudioRuntime, findMsys2Bash, gpgInvocation, pathForGpg, prependWindowsPath, toMsysPath, validateMsys2Toolchain, verifyWindowsAudioRuntime, windowsBuildEnvironment, windowsRuntimeDllNames } from "../windows-audio-runtime.mjs";

test("Windows PATH normalization retains entries from competing case aliases", () => {
  const source = { Path: "C:\\Rust\\bin", PATH: "C:\\Windows", path: "C:\\Rust\\bin", KEEP: "yes" };
  assert.deepEqual(prependWindowsPath(source, "C:\\ffmpeg\\bin"), {
    PATH: "C:\\ffmpeg\\bin;C:\\Rust\\bin;C:\\Windows", KEEP: "yes",
  });
  assert.equal(source.Path, "C:\\Rust\\bin");
  assert.deepEqual(prependWindowsPath({}, "C:\\ffmpeg\\bin"), { PATH: "C:\\ffmpeg\\bin" });
});

test("loads the Visual Studio developer environment when lib.exe is not on PATH", () => {
  const calls = [];
  const execute = (command, args) => {
    calls.push({ command, args });
    if (command.endsWith("vswhere.exe")) return "C:\\Program Files\\Microsoft Visual Studio\\2022\\BuildTools\r\n";
    if (command === "cmd.exe") return "Path=C:\\VS\\VC\\bin;C:\\Windows\r\nLIB=C:\\VS\\VC\\lib\r\nINCLUDE=C:\\VS\\VC\\include\r\n";
    throw new Error(`Unexpected command: ${command}`);
  };
  const env = windowsBuildEnvironment("x86_64-pc-windows-msvc", { Path: "C:\\Windows" }, {
    execFileSync: execute,
    vswhere: "C:\\Program Files (x86)\\Microsoft Visual Studio\\Installer\\vswhere.exe",
    hasExecutable: (_name, selectedEnv) => selectedEnv.PATH?.startsWith("C:\\VS\\") ?? false,
  });
  assert.equal(env.PATH, "C:\\VS\\VC\\bin;C:\\Windows");
  assert.equal(env.LIB, "C:\\VS\\VC\\lib");
  assert.equal(env.INCLUDE, "C:\\VS\\VC\\include");
  assert.match(calls.find((call) => call.command === "cmd.exe").args.at(-1), /vcvarsall\.bat.*amd64/i);
});

test("converts Windows drive paths for MSYS bash", () => {
  assert.equal(toMsysPath("C:\\Workspaces\\HifiMule\\target\\audio-runtime"), "/c/Workspaces/HifiMule/target/audio-runtime");
  assert.equal(toMsysPath("c:/Users/Alex/source"), "/c/Users/Alex/source");
});

test("selects MSYS2 bash without falling back to WSL bash.exe", () => {
  const existing = new Set(["C:\\Windows\\System32\\bash.exe", "D:\\tools\\msys64\\usr\\bin\\bash.exe"]);
  assert.equal(findMsys2Bash({ MSYS2_ROOT: "D:\\tools\\msys64" }, (path) => existing.has(path)), "D:\\tools\\msys64\\usr\\bin\\bash.exe");
  assert.equal(findMsys2Bash({}, (path) => existing.has(path)), undefined);
});

test("prefers the action-installed MSYS2 over a preinstalled runner copy", () => {
  const selectedBin = "D:\\a\\_temp\\msys64\\usr\\bin";
  const existing = new Set([
    "C:\\msys64\\usr\\bin\\bash.exe",
    ...["bash", "make", "sed", "grep", "awk"].map((tool) => `${selectedBin}\\${tool}.exe`),
  ]);
  const pathExists = (path) => existing.has(path);
  const bash = findMsys2Bash({ MSYS2_ROOT: "D:/a/_temp/msys64" }, pathExists);
  assert.equal(bash, `${selectedBin}\\bash.exe`);
  assert.equal(validateMsys2Toolchain(bash, pathExists), selectedBin);
});

test("rejects an MSYS2 toolchain without sed before FFmpeg configure", () => {
  const bash = "C:\\Tools\\msys64\\usr\\bin\\bash.exe";
  const existing = new Set([bash, "C:\\Tools\\msys64\\usr\\bin\\make.exe"]);
  assert.throws(() => validateMsys2Toolchain(bash, (path) => existing.has(path)), /MSYS2 sed is missing.*pacman.*sed/s);
});

test("passes POSIX paths to MSYS2 GPG and native paths to other GPG builds", () => {
  const source = "C:\\Workspaces\\HifiMule\\source.asc";
  assert.equal(pathForGpg("C:\\Tools\\msys64\\usr\\bin\\gpg.exe", source), "/c/Workspaces/HifiMule/source.asc");
  assert.equal(pathForGpg("gpg.exe", source, { PATH: "C:\\Tools\\msys64\\usr\\bin;C:\\Windows" }), "/c/Workspaces/HifiMule/source.asc");
  assert.equal(pathForGpg("C:\\Program Files\\GnuPG\\bin\\gpg.exe", source), source);
});

test("invokes MSYS2 GPG through Bash so gpg-agent can start", () => {
  const invocation = gpgInvocation("C:\\Tools\\msys64\\usr\\bin\\gpg.exe", ["--batch", "--verify", "/c/source.asc"]);
  assert.equal(invocation.command, "C:\\Tools\\msys64\\usr\\bin\\bash.exe");
  assert.deepEqual(invocation.args.slice(0, 3), ["--noprofile", "--norc", "-c"]);
  assert.match(invocation.args[3], /gpg\.exe.*--verify.*\/c\/source\.asc/);
});

test("creates the isolated GPG home under a short temporary root", () => {
  const temporaryRoot = mkdtempSync(join(tmpdir(), "hm-gpg-root-"));
  const home = createGpgHome(temporaryRoot);
  try {
    assert.equal(home.startsWith(temporaryRoot), true);
    assert.equal(existsSync(home), true);
    assert.ok(home.length < 100, `GPG home is too long for an MSYS2 agent socket: ${home}`);
  } finally { rmSync(temporaryRoot, { recursive: true, force: true }); }
});

const root = resolve(import.meta.dirname, "../..");
const manifest = JSON.parse(readFileSync(join(root, "hifimule-daemon/audio-runtime.json"), "utf8"));

test("manifest selects only the signed official Windows source", () => {
  assert.equal(manifest.sourceUrl, `https://ffmpeg.org/releases/ffmpeg-${manifest.ffmpegRelease}.tar.xz`);
  assert.equal(manifest.signatureUrl, `${manifest.sourceUrl}.asc`);
  assert.equal(manifest.signingKey, "FCF986EA15E6E293A5644F10B4322F04D67658D8");
  assert.equal("windowsDistribution" in manifest, false);
  assert.ok(manifest.windowsConfigureFlags.includes("--disable-x86asm"), "Windows source builds must not require an external NASM toolchain");
});

function fakeRuntime(target = "aarch64-pc-windows-msvc") {
  const prefix = mkdtempSync(join(tmpdir(), "hifimule-windows-ffmpeg-"));
  const arch = target.startsWith("aarch64") ? "arm64" : "amd64";
  const machine = target.startsWith("aarch64") ? 0xaa64 : 0x8664;
  mkdirSync(join(prefix, "lib", arch), { recursive: true });
  mkdirSync(join(prefix, "bin"));
  const receipt = {
    schemaVersion: 1, targetTriple: target, ffmpegRelease: manifest.ffmpegRelease,
    sourceUrl: manifest.sourceUrl, signatureUrl: manifest.signatureUrl, signingKey: manifest.signingKey,
    sourceSha256: manifest.sourceSha256, configureFlags: [...manifest.configureFlags, ...manifest.windowsConfigureFlags], abiVersions: manifest.abiVersions,
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

test("rejects a runtime built for the wrong Windows architecture", () => {
  const prefix = fakeRuntime("x86_64-pc-windows-msvc");
  const receipt = JSON.parse(readFileSync(join(prefix, ".hifimule-audio-runtime.json")));
  receipt.targetTriple = "aarch64-pc-windows-msvc";
  writeFileSync(join(prefix, ".hifimule-audio-runtime.json"), JSON.stringify(receipt));
  assert.throws(() => verifyWindowsAudioRuntime(prefix, "aarch64-pc-windows-msvc"), /import library is missing|architecture/);
});

test("missing runtime fails with exact setup guidance", () => {
  assert.throws(() => verifyWindowsAudioRuntime(join(tmpdir(), "missing-hifimule-ffmpeg"), "aarch64-pc-windows-msvc"), /automatically builds.*FFMPEG_DIR/s);
});

test("formats staged DLL names without passing map indexes as basename suffixes", () => {
  assert.deepEqual(
    windowsRuntimeDllNames([join("runtime", "bin", "avcodec-63.dll"), join("runtime", "bin", "avutil-61.dll")]),
    ["avcodec-63.dll", "avutil-61.dll"],
  );
});

for (const target of ["aarch64-pc-windows-msvc", "x86_64-pc-windows-msvc"]) {
  test(`builds the signed official FFmpeg source for ${target}`, () => {
    const cacheRoot = mkdtempSync(join(tmpdir(), "hifimule-windows-cache-"));
    let downloads = 0;
    let signatures = 0;
    let builds = 0;
    const options = {
      platform: "win32",
      env: {},
      cacheRoot,
      sha: () => manifest.sourceSha256,
      download: (url, path) => {
        downloads += 1;
        assert.ok(url.startsWith("https://ffmpeg.org/"));
        writeFileSync(path, "archive fixture");
      },
      verifySignature: (archive, signature) => {
        signatures += 1;
        assert.ok(archive.endsWith("ffmpeg-9.0.1.tar.xz"));
        assert.ok(signature.endsWith("ffmpeg-9.0.1.tar.xz.asc"));
      },
      buildSource: (_archive, staging, selectedTarget) => {
        builds += 1;
        assert.equal(selectedTarget, target);
        populateOfficialRuntime(staging, target);
      },
    };
    const prefix = ensureWindowsAudioRuntime(target, options);
    assert.equal(verifyWindowsAudioRuntime(prefix, target).dlls.length, 4);
    const receipt = JSON.parse(readFileSync(join(prefix, ".hifimule-audio-runtime.json"), "utf8"));
    assert.equal(receipt.sourceSha256, manifest.sourceSha256);
    assert.equal(receipt.sourceUrl, manifest.sourceUrl);
    assert.equal(receipt.signatureUrl, manifest.signatureUrl);
    assert.equal(receipt.signingKey, manifest.signingKey);
    assert.equal(ensureWindowsAudioRuntime(target, options), prefix);
    assert.equal(downloads, 3, "source, detached signature and official signing key should be downloaded once");
    assert.equal(signatures, 1);
    assert.equal(builds, 1);
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
      verifySignature: () => assert.fail("bad archive must not be signature-verified"),
      buildSource: () => assert.fail("bad archive must not be built"),
    }),
    /checksum mismatch/,
  );
});
