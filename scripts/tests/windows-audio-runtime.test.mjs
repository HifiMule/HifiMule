import assert from "node:assert/strict";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { basename, join, resolve } from "node:path";
import test from "node:test";
import { buildOfficialSource, createGpgHome, ensureWindowsAudioRuntime, findMsys2Bash, gpgInvocation, pathForGpg, prependWindowsPath, runWindowsFfmpegBuild, stageWindowsAudioRuntimeForCargo, toMsysPath, validateMsys2Toolchain, verifyInstalledWindowsBundle, verifyWindowsAudioRuntime, windowsBuildEnvironment, windowsRuntimeDllNames } from "../windows-audio-runtime.mjs";

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

for (const extractionFails of [false, true]) {
  test(`Windows source workspace avoids staging path length and cleans up after ${extractionFails ? 'extraction failure' : 'invalid source'}`, t => {
    const parent = mkdtempSync(join(tmpdir(), "hm-staging-"));
    t.after(() => rmSync(parent, { recursive: true, force: true }));
    const staging = join(parent, "ffmpeg-9.0.2-official-x86_64-pc-windows-msvc.staging-3252-d88e00dd-b3e7-4d5e-b047-ff9e41976286");
    const extractionError = new Error("fixture extraction failure");
    let sourceRoot;
    assert.throws(() => buildOfficialSource("fixture.tar.xz", staging, "x86_64-pc-windows-msvc", {
      bash: "fixture-bash", librarian: "fixture-lib", env: {},
      execFileSync(_command, args) {
        sourceRoot = args[args.indexOf("-C") + 1];
        assert.equal(existsSync(sourceRoot), true);
        const compilerDependency = join(sourceRoot, "libvorbis-1.3.7/build-hifimule/CMakeFiles/CMakeScratch/TryCompile-axk2wi/CMakeFiles/cmTC_c9011.dir/compiler_depend.make");
        assert.ok(compilerDependency.length < 250, `CMake path exceeds native tool limit: ${compilerDependency}`);
        assert.equal(sourceRoot.startsWith(staging), false);
        if (extractionFails) throw extractionError;
      },
    }), extractionFails ? error => error === extractionError : /Official FFmpeg archive did not contain/);
    assert.equal(existsSync(sourceRoot), false, "failed builds must remove their source workspace");
  });
}

const root = resolve(import.meta.dirname, "../..");
const manifest = JSON.parse(readFileSync(join(root, "hifimule-daemon/audio-runtime.json"), "utf8"));

for (const target of ["x86_64-pc-windows-msvc", "aarch64-pc-windows-msvc"]) {
  test(`FFmpeg receives native linker search flags for ${target}`, () => {
    let invocation;
    runWindowsFfmpegBuild({
      source: "C:\\Temp\\ffmpeg", staging: "D:\\AudioRuntime", target, bash: "fixture-bash", env: {},
      execute(command, args, options) { invocation = { command, args, options }; },
    });
    const command = invocation.args.at(-1);
    // configure adds extra ldflags before detecting MSVC. GNU -L reaches
    // link.exe unchanged and its LNK4044 warning fails the compiler check.
    assert.ok(command.includes("--extra-ldflags='-libpath:D:/AudioRuntime/lib'"));
    assert.equal(command.includes("--extra-ldflags='-L"), false);
    // Codec archives use CMake's MultiThreadedDLL runtime (/MD). FFmpeg's
    // probes and both language compilers must use that same runtime.
    assert.ok(command.includes("'--extra-cflags=-MD'"));
    assert.ok(command.includes("'--extra-cxxflags=-MD'"));
    assert.equal(invocation.options.env.VSLANG, "1033");
  });
}

test("failed FFmpeg configure retains compiler diagnostics without its environment dump", t => {
  const fixture = mkdtempSync(join(tmpdir(), "hm-config-log-"));
  t.after(() => rmSync(fixture, { recursive: true, force: true }));
  const source = join(fixture, "source");
  const logRoot = join(fixture, "logs");
  mkdirSync(join(source, "ffbuild"), { recursive: true });
  const diagnostic = 'test_ld cc\ntest_cc\nBEGIN ./test.c\n1 int main(void){ return 0; }\nEND ./test.c\ncl.exe -c -Fo./test.o ./test.c\nLINK : fatal error LNK1181: cannot open input file kernel32.lib\nC compiler test failed.\n';
  writeFileSync(join(source, "ffbuild/config.log"), `CI_SECRET=fixture-private-value\n${diagnostic}`);
  const messages = [];
  t.mock.method(console, "error", message => messages.push(message));
  const failure = new Error("configure exited with status 1");
  assert.throws(() => runWindowsFfmpegBuild({
    source, staging: join(fixture, "staging"), target: "x86_64-pc-windows-msvc", bash: "fixture-bash", env: {}, logRoot,
    execute() { throw failure; },
  }), error => error === failure);
  rmSync(source, { recursive: true, force: true });
  const saved = readFileSync(join(logRoot, "ffmpeg-x86_64-pc-windows-msvc-configure.log"), "utf8");
  assert.equal(saved, diagnostic);
  assert.ok(messages.join('\n').includes('LNK1181'));
  assert.equal(messages.join('\n').includes('fixture-private-value'), false);
});

test("FFmpeg failure without a configure log retains the original process error", t => {
  const source = mkdtempSync(join(tmpdir(), "hm-no-config-log-"));
  t.after(() => rmSync(source, { recursive: true, force: true }));
  const failure = new Error("Bash could not start");
  assert.throws(() => runWindowsFfmpegBuild({
    source, staging: source, target: "x86_64-pc-windows-msvc", bash: "fixture-bash", env: {}, logRoot: join(source, "logs"),
    execute() { throw failure; },
  }), error => error === failure);
});

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
        assert.ok(archive.endsWith(`ffmpeg-${manifest.ffmpegRelease}.tar.xz`));
        assert.ok(signature.endsWith(`ffmpeg-${manifest.ffmpegRelease}.tar.xz.asc`));
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
  const pe = () => { const bytes = Buffer.alloc(128); bytes.write("MZ"); bytes.writeUInt32LE(64, 0x3c); bytes.write("PE\0\0", 64); bytes.writeUInt16LE(machine, 68); return bytes; };
  writeFileSync(join(bundle, "hifimule-daemon.exe"), pe());
  writeFileSync(join(bundle, "audio-runtime.json"), JSON.stringify(manifest));
  writeFileSync(join(bundle, "THIRD_PARTY_AUDIO_NOTICES.md"), "notices");
  for (const [library, version] of Object.entries(manifest.abiVersions)) writeFileSync(join(bundle, `${library}-${version.split(".")[0]}.dll`), pe());
  assert.equal(verifyInstalledWindowsBundle(bundle, "x86_64-pc-windows-msvc").dllCount, 4);
  rmSync(join(bundle, "avcodec-63.dll"));
  assert.throws(() => verifyInstalledWindowsBundle(bundle, "x86_64-pc-windows-msvc"), /exactly one avcodec-63.dll/);
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
