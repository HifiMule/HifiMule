#!/usr/bin/env node
import { execFileSync, spawnSync } from "node:child_process";
import { createHash, randomUUID } from "node:crypto";
import { appendFileSync, copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, renameSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { basename, join, resolve, win32 } from "node:path";
import { fileURLToPath } from "node:url";
import { audioRuntimeVerification } from "./verify-audio-runtime.mjs";

const root = resolve(import.meta.dirname, "..");
const manifest = JSON.parse(readFileSync(join(root, "hifimule-daemon/audio-runtime.json"), "utf8"));
const targets = {
  "aarch64-pc-windows-msvc": { machine: 0xaa64, libdir: "arm64" },
  "x86_64-pc-windows-msvc": { machine: 0x8664, libdir: "amd64" },
};

export function prependWindowsPath(env, directory) {
  const result = { ...env };
  const pathKeys = Object.keys(result).filter((key) => key.toUpperCase() === "PATH");
  // Spreading process.env loses Windows' case-insensitive property lookup.
  // Emit one key so Node cannot choose an alias that omits Cargo or the DLLs.
  const paths = pathKeys.map((key) => result[key]).filter(Boolean);
  for (const key of pathKeys) delete result[key];
  result.PATH = [directory, ...new Set(paths)].join(";");
  return result;
}

function fail(message) {
  throw new Error(`${message}\n\nWindows playback automatically builds the signed official FFmpeg ` +
    `${manifest.ffmpegRelease} source. Set FFMPEG_DIR only to override it with a ` +
    `previously validated prefix containing include/, lib/*.lib, bin/*-<ABI>.dll, and .hifimule-audio-runtime.json.`);
}

function macro(header, name) {
  return Number(header.match(new RegExp(`^\\s*#\\s*define\\s+${name}\\s+(\\d+)`, "m"))?.[1]);
}

function peMachine(path) {
  const bytes = readFileSync(path);
  if (bytes.length < 64 || bytes.toString("ascii", 0, 2) !== "MZ") fail(`Not a PE DLL: ${path}`);
  const offset = bytes.readUInt32LE(0x3c);
  if (offset + 6 > bytes.length || bytes.toString("ascii", offset, offset + 4) !== "PE\0\0") fail(`Invalid PE header: ${path}`);
  return bytes.readUInt16LE(offset + 4);
}

function expectedReceipt(target) {
  return {
    schemaVersion: 1,
    targetTriple: target,
    ffmpegRelease: manifest.ffmpegRelease,
    sourceUrl: manifest.sourceUrl,
    signatureUrl: manifest.signatureUrl,
    signingKey: manifest.signingKey,
    sourceSha256: manifest.sourceSha256,
    configureFlags: [...manifest.configureFlags, ...manifest.windowsConfigureFlags],
    abiVersions: manifest.abiVersions,
  };
}

export function writeWindowsAudioRuntimeReceipt(prefix, target) {
  if (!targets[target]) fail(`Unsupported Windows playback target: ${target}`);
  mkdirSync(prefix, { recursive: true });
  const path = join(prefix, ".hifimule-audio-runtime.json");
  const receipt = expectedReceipt(target);
  writeFileSync(path, `${JSON.stringify(receipt, null, 2)}\n`);
  return path;
}

export function verifyWindowsAudioRuntime(prefix, target) {
  const targetInfo = targets[target];
  if (!targetInfo) fail(`Unsupported Windows playback target: ${target}`);
  prefix = resolve(prefix);
  const receiptPath = join(prefix, ".hifimule-audio-runtime.json");
  if (!existsSync(receiptPath)) fail(`Controlled FFmpeg receipt is missing: ${receiptPath}`);
  let receipt;
  try { receipt = JSON.parse(readFileSync(receiptPath, "utf8")); } catch { fail(`Controlled FFmpeg receipt is invalid: ${receiptPath}`); }
  if (JSON.stringify(receipt) !== JSON.stringify(expectedReceipt(target))) fail(`Controlled FFmpeg receipt is stale or for another target: ${receiptPath}`);

  const libDir = existsSync(join(prefix, "lib", targetInfo.libdir)) ? join(prefix, "lib", targetInfo.libdir) : join(prefix, "lib");
  const binDir = join(prefix, "bin");
  const dlls = [];
  for (const [library, version] of Object.entries(receipt.abiVersions)) {
    const upper = library.toUpperCase();
    const headerPath = join(prefix, "include", `lib${library}`, "version.h");
    if (!existsSync(headerPath)) fail(`FFmpeg version header is missing: ${headerPath}`);
    const majorHeaderPath = join(prefix, "include", `lib${library}`, "version_major.h");
    const header = [headerPath, majorHeaderPath].filter(existsSync).map((path) => readFileSync(path, "utf8")).join("\n");
    const parts = version.split(".").map(Number);
    for (const [suffix, expected] of [["MAJOR", parts[0]], ["MINOR", parts[1]], ["MICRO", parts[2]]]) {
      if (macro(header, `LIB${upper}_VERSION_${suffix}`) !== expected) fail(`FFmpeg ${library} header version does not match ${version}`);
    }
    const importLibrary = join(libDir, `${library}.lib`);
    if (!existsSync(importLibrary)) fail(`MSVC import library is missing: ${importLibrary}`);
    const dllName = `${library}-${parts[0]}.dll`;
    const dll = join(binDir, dllName);
    if (!existsSync(dll)) fail(`FFmpeg runtime DLL is missing: ${dll}`);
    if (peMachine(dll) !== targetInfo.machine) fail(`FFmpeg DLL architecture does not match ${target}: ${dll}`);
  }
  for (const name of readdirSync(binDir)) {
    if (!name.toLowerCase().endsWith(".dll")) continue;
    const dll = join(binDir, name);
    if (peMachine(dll) !== targetInfo.machine) fail(`Runtime DLL architecture does not match ${target}: ${dll}`);
    dlls.push(dll);
  }
  if (!dlls.length) fail(`No runtime DLLs found under ${binDir}`);
  return { prefix, dlls };
}

function sha(path) { return createHash("sha256").update(readFileSync(path)).digest("hex"); }

function executable(candidates, env = process.env) {
  for (const candidate of candidates) {
    if (candidate.includes("\\") && existsSync(candidate)) return candidate;
    if (!candidate.includes("\\")) {
      const located = spawnSync("where.exe", [candidate], { encoding: "utf8", env });
      if (located.status === 0) return located.stdout.split(/\r?\n/).find(Boolean)?.trim() ?? candidate;
    }
  }
  return undefined;
}

export function windowsBuildEnvironment(target, env = process.env, options = {}) {
  const hasExecutable = options.hasExecutable ?? ((name, selectedEnv) => Boolean(executable([name], selectedEnv)));
  if (hasExecutable("lib.exe", env) && hasExecutable("cl.exe", env)) return { ...env };
  const execute = options.execFileSync ?? execFileSync;
  const vswhere = options.vswhere ?? "C:\\Program Files (x86)\\Microsoft Visual Studio\\Installer\\vswhere.exe";
  if (!existsSync(vswhere) && !options.vswhere) fail("Visual Studio locator is missing; install Visual Studio Build Tools with Desktop development with C++");
  const installation = String(execute(vswhere, ["-latest", "-products", "*", "-requires", "Microsoft.VisualStudio.Component.VC.Tools.x86.x64", "-property", "installationPath"], { encoding: "utf8", env })).trim();
  if (!installation) fail("Visual Studio C++ Build Tools are missing; install Desktop development with C++");
  const vcvars = win32.join(installation, "VC", "Auxiliary", "Build", "vcvarsall.bat");
  if (!existsSync(vcvars) && !options.vswhere) fail(`Visual Studio developer environment script is missing: ${vcvars}`);
  const architecture = target.startsWith("aarch64") ? "amd64_arm64" : "amd64";
  const output = String(execute("cmd.exe", ["/d", "/s", "/c", `""${vcvars}" ${architecture} >nul && set"`], { encoding: "utf8", env, windowsVerbatimArguments: true }));
  const result = { ...env };
  for (const line of output.split(/\r?\n/)) {
    const separator = line.indexOf("=");
    if (separator <= 0) continue;
    const key = line.slice(0, separator);
    for (const existing of Object.keys(result)) if (existing.toUpperCase() === key.toUpperCase()) delete result[existing];
    result[key.toUpperCase() === "PATH" ? "PATH" : key] = line.slice(separator + 1);
  }
  if (!hasExecutable("lib.exe", result) || !hasExecutable("cl.exe", result)) fail("Visual Studio developer environment did not expose cl.exe and lib.exe");
  return result;
}

function shellQuote(value) { return `'${String(value).replaceAll("'", `'\\''`)}'`; }

export function toMsysPath(value) {
  const normalized = String(value).replaceAll("\\", "/");
  const drive = normalized.match(/^([A-Za-z]):\/(.*)$/);
  return drive ? `/${drive[1].toLowerCase()}/${drive[2]}` : normalized;
}

export function pathForGpg(gpg, value, env = process.env) {
  const command = String(gpg);
  const effective = command.includes("\\") || command.includes("/")
    ? command
    : win32.join(String(env.PATH ?? "").split(";").find(Boolean) ?? "", command);
  const normalizedExecutable = effective.replaceAll("\\", "/").toLowerCase();
  return normalizedExecutable.includes("/msys") && normalizedExecutable.endsWith("/usr/bin/gpg.exe") ? toMsysPath(value) : value;
}

function isMsys2Gpg(gpg) {
  const normalized = String(gpg).replaceAll("\\", "/").toLowerCase();
  return normalized.includes("/msys") && normalized.endsWith("/usr/bin/gpg.exe");
}

export function gpgInvocation(gpg, args) {
  if (!isMsys2Gpg(gpg)) return { command: gpg, args };
  const bash = win32.join(win32.dirname(gpg), "bash.exe");
  return { command: bash, args: ["--noprofile", "--norc", "-c", `exec ${[toMsysPath(gpg), ...args].map(shellQuote).join(" ")}`] };
}

export function createGpgHome(temporaryRoot = tmpdir()) {
  return mkdtempSync(join(temporaryRoot, "hm-gpg-"));
}

export function findMsys2Bash(env = process.env, pathExists = existsSync) {
  const roots = [env.MSYS2_ROOT, "C:\\msys64", "C:\\tools\\msys64"].filter(Boolean);
  return roots.map((rootPath) => win32.join(rootPath, "usr", "bin", "bash.exe")).find(pathExists);
}

export function validateMsys2Toolchain(bash, pathExists = existsSync) {
  const bin = win32.dirname(bash);
  for (const tool of ["make", "sed", "grep", "awk"]) {
    if (!pathExists(win32.join(bin, `${tool}.exe`))) fail(`MSYS2 ${tool} is missing; install it with: pacman -S --needed make sed grep gawk`);
  }
  return bin;
}

export function verifyOfficialSignature(archive, signature, key, options = {}) {
  const execute = options.execFileSync ?? execFileSync;
  const baseEnv = options.env ?? process.env;
  const gpg = options.gpg ?? executable(["C:\\msys64\\usr\\bin\\gpg.exe", "gpg.exe"], baseEnv);
  if (!gpg) fail("GnuPG is missing; install MSYS2 gnupg before building the official FFmpeg source");
  const home = createGpgHome(options.tempRoot);
  try {
    const gpgHome = pathForGpg(gpg, home, baseEnv);
    const runGpg = (args, executionOptions) => {
      const invocation = gpgInvocation(gpg, args);
      return execute(invocation.command, invocation.args, { ...executionOptions, env: baseEnv });
    };
    runGpg(["--batch", "--homedir", gpgHome, "--import", pathForGpg(gpg, key, baseEnv)], { stdio: "inherit" });
    const fingerprints = runGpg(["--batch", "--homedir", gpgHome, "--with-colons", "--fingerprint"], { encoding: "utf8" });
    if (!fingerprints.toUpperCase().includes(manifest.signingKey)) fail(`Official FFmpeg signing key fingerprint mismatch; expected ${manifest.signingKey}`);
    const status = runGpg(["--batch", "--homedir", gpgHome, "--status-fd", "1", "--verify", pathForGpg(gpg, signature, baseEnv), pathForGpg(gpg, archive, baseEnv)], { encoding: "utf8", stdio: ["ignore", "pipe", "inherit"] });
    const valid = status.match(/^\[GNUPG:\] VALIDSIG ([0-9A-F]+) /m)?.[1]?.toUpperCase();
    if (valid !== manifest.signingKey) fail(`Official FFmpeg signature was not made by ${manifest.signingKey}`);
  } finally { rmSync(home, { recursive: true, force: true }); }
}

function buildOfficialSource(archive, staging, target, options = {}) {
  const execute = options.execFileSync ?? execFileSync;
  const baseEnv = options.env ?? process.env;
  const bash = options.bash ?? findMsys2Bash(baseEnv);
  if (!bash) fail("MSYS2 bash is missing; install MSYS2 with make, diffutils and pkgconf (Windows/WSL bash is not compatible)");
  if (!options.bash) validateMsys2Toolchain(bash);
  const librarian = options.librarian ?? executable(["lib.exe"], baseEnv);
  if (!librarian) fail("MSVC lib.exe is missing; run the build from a Visual Studio Native Tools environment");
  const sourceRoot = `${staging}.source`;
  rmSync(sourceRoot, { recursive: true, force: true });
  mkdirSync(sourceRoot, { recursive: true });
  try {
    execute("tar.exe", ["-xf", archive, "-C", sourceRoot], { stdio: "inherit" });
    const source = join(sourceRoot, `ffmpeg-${manifest.ffmpegRelease}`);
    if (!existsSync(join(source, "configure"))) fail(`Official FFmpeg archive did not contain ffmpeg-${manifest.ffmpegRelease}/configure`);
    mkdirSync(staging, { recursive: true });
    const arch = target.startsWith("aarch64") ? "aarch64" : "x86_64";
    const command = [
      `cd ${shellQuote(toMsysPath(source))}`,
      "export VSLANG=1033",
      `./configure --toolchain=msvc --arch=${arch} --prefix=${shellQuote(toMsysPath(staging))} ${[...manifest.configureFlags, ...manifest.windowsConfigureFlags].map(shellQuote).join(" ")}`,
      `make -j${Math.max(1, Number(process.env.NUMBER_OF_PROCESSORS) || 2)}`,
      "make install",
    ].join(" && ");
    execute(bash, ["--noprofile", "--norc", "-c", command], { stdio: "inherit", env: { ...baseEnv, VSLANG: "1033" } });
    const machine = target.startsWith("aarch64") ? "ARM64" : "X64";
    for (const library of manifest.requiredLibraries) {
      const definition = readdirSync(join(staging, "lib")).find((name) => name.toLowerCase().startsWith(library) && name.toLowerCase().endsWith(".def"));
      if (!definition) fail(`FFmpeg install did not produce a module definition for ${library}`);
      execute(librarian, [`/def:${join(staging, "lib", definition)}`, `/out:${join(staging, "lib", `${library}.lib`)}`, `/machine:${machine}`], { stdio: "inherit", env: { ...baseEnv, VSLANG: "1033" } });
    }
  } finally { rmSync(sourceRoot, { recursive: true, force: true }); }
}

function lock(path) {
  const deadline = Date.now() + 30 * 60_000;
  while (true) {
    try { mkdirSync(path); writeFileSync(join(path, "owner.json"), JSON.stringify({ pid: process.pid, at: new Date().toISOString() })); return () => rmSync(path, { recursive: true, force: true }); }
    catch (error) {
      if (error.code !== "EEXIST") throw error;
      if (Date.now() - statSync(path).mtimeMs > 60 * 60_000) { rmSync(path, { recursive: true, force: true }); continue; }
      if (Date.now() >= deadline) fail(`Timed out waiting for Windows audio runtime lock: ${path}`);
      Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 500);
    }
  }
}

export function ensureWindowsAudioRuntime(target, options = {}) {
  if (!targets[target]) fail(`Unsupported Windows playback target: ${target}`);
  const env = options.env ?? process.env;
  if (env.FFMPEG_DIR) return verifyWindowsAudioRuntime(env.FFMPEG_DIR, target).prefix;
  const platform = options.platform ?? process.platform;
  if (platform !== "win32") fail("Automatic Windows FFmpeg provisioning can only run on Windows");
  const cacheRoot = resolve(options.cacheRoot ?? join(root, "target/audio-runtime"));
  const prefix = join(cacheRoot, `ffmpeg-${manifest.ffmpegRelease}-official-${target}`);
  const archive = join(cacheRoot, "sources", `ffmpeg-${manifest.ffmpegRelease}.tar.xz`);
  const signature = `${archive}.asc`;
  const key = join(cacheRoot, "sources", "ffmpeg-devel.asc");
  const calculateSha = options.sha ?? sha;
  const download = options.download ?? ((url, path) => execFileSync("curl.exe", ["--fail", "--location", "--retry", "3", "--output", path, url], { stdio: "inherit" }));
  let runtimeOptions = options;
  const verifySignature = options.verifySignature ?? ((archivePath, signaturePath, keyPath) => verifyOfficialSignature(archivePath, signaturePath, keyPath, runtimeOptions));
  const buildSource = options.buildSource ?? ((archivePath, staging, selectedTarget) => buildOfficialSource(archivePath, staging, selectedTarget, runtimeOptions));
  mkdirSync(join(cacheRoot, "sources"), { recursive: true });
  const unlock = lock(`${prefix}.lock`);
  try {
    if (existsSync(prefix)) { try { return verifyWindowsAudioRuntime(prefix, target).prefix; } catch { rmSync(prefix, { recursive: true, force: true }); } }
    const buildEnv = options.buildSource ? env : windowsBuildEnvironment(target, env, options);
    runtimeOptions = { ...options, env: buildEnv };
    if (!options.download && !executable(["curl.exe"], buildEnv)) fail("Required Windows official-source build tool is missing: curl.exe");
    if (!options.verifySignature && !executable(["C:\\msys64\\usr\\bin\\gpg.exe", "gpg.exe"], buildEnv)) fail("Required Windows official-source build tool is missing: gpg.exe");
    if (!options.buildSource) {
      const msysBash = findMsys2Bash(buildEnv);
      if (!msysBash) fail("MSYS2 bash is missing; install MSYS2 with make, diffutils and pkgconf (Windows/WSL bash is not compatible)");
      const msysBin = validateMsys2Toolchain(msysBash);
      runtimeOptions = { ...options, env: prependWindowsPath(buildEnv, msysBin), bash: msysBash };
      for (const [tool, candidates] of [
        ["tar.exe", ["tar.exe"]], ["lib.exe", ["lib.exe"]],
      ]) if (!executable(candidates, buildEnv)) fail(`Required Windows official-source build tool is missing: ${tool}`);
    }
    if (!existsSync(archive) || calculateSha(archive) !== manifest.sourceSha256) {
      rmSync(archive, { force: true });
      const temporaryArchive = `${archive}.${process.pid}.${randomUUID()}.tmp`;
      try {
        download(manifest.sourceUrl, temporaryArchive);
        if (calculateSha(temporaryArchive) !== manifest.sourceSha256) fail(`Windows FFmpeg archive checksum mismatch: ffmpeg-${manifest.ffmpegRelease}.tar.xz`);
        renameSync(temporaryArchive, archive);
      } finally { rmSync(temporaryArchive, { force: true }); }
    }
    if (!existsSync(signature)) download(manifest.signatureUrl, signature);
    if (!existsSync(key)) download(manifest.signingKeyUrl, key);
    verifySignature(archive, signature, key);
    const staging = `${prefix}.staging-${process.pid}-${randomUUID()}`;
    try {
      buildSource(archive, staging, target);
      writeFileSync(join(staging, ".hifimule-audio-runtime.json"), `${JSON.stringify(expectedReceipt(target), null, 2)}\n`);
      verifyWindowsAudioRuntime(staging, target);
      renameSync(staging, prefix);
    } finally { rmSync(staging, { recursive: true, force: true }); }
    return verifyWindowsAudioRuntime(prefix, target).prefix;
  } finally { unlock(); }
}

export function stageWindowsAudioRuntime(prefix, target) {
  const verified = verifyWindowsAudioRuntime(prefix, target);
  const out = join(root, "hifimule-ui/src-tauri/bundled-libs");
  mkdirSync(out, { recursive: true });
  for (const name of readdirSync(out)) if (name.toLowerCase().endsWith(".dll")) rmSync(join(out, name), { force: true });
  for (const dll of verified.dlls) copyFileSync(dll, join(out, basename(dll)));
  console.log(`Staged controlled Windows FFmpeg DLLs: ${windowsRuntimeDllNames(verified.dlls).join(", ")}`);
  return verified;
}

export function windowsRuntimeDllNames(dlls) {
  return dlls.map((dll) => basename(dll));
}

if (process.argv[1] && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url))) {
  const [mode, target, output] = process.argv.slice(2);
  if (mode === "receipt") {
    if (!process.env.FFMPEG_DIR) fail("FFMPEG_DIR is not set for receipt generation");
    console.log(writeWindowsAudioRuntimeReceipt(process.env.FFMPEG_DIR, target));
  } else if (["ensure", "verify", "env"].includes(mode)) {
    const prefix = ensureWindowsAudioRuntime(target);
    if (mode === "env") {
      if (!output) fail("The `env` command requires an output environment file path");
      appendFileSync(output, `FFMPEG_DIR=${prefix}\n`);
      appendFileSync(output, `PATH=${prependWindowsPath(process.env, join(prefix, "bin")).PATH}\n`);
      appendFileSync(output, `${audioRuntimeVerification.environmentVariable}=${audioRuntimeVerification.value}\n`);
      console.log(prefix);
    } else console.log(JSON.stringify(mode === "verify" ? stageWindowsAudioRuntime(prefix, target) : { prefix }));
  } else fail("Expected `ensure`, `env`, `verify`, or `receipt` command");
}
