import { buildSourceAudioRuntime } from "./source-audio-runtime.mjs";

import { ensurePrebuiltAudioRuntime, usesPrebuiltAudioRuntime } from "./prebuilt-audio-runtime.mjs";
import { encodingDependencyReceipt } from "./encoding-dependencies.mjs";
import { execFileSync } from "node:child_process";

import { appendFileSync, copyFileSync, existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";

import { basename, dirname, join, resolve } from "node:path";
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
  throw new Error(`${message}\n\nUse the pinned FFmpeg runtime, or set FFMPEG_DIR to a previously validated prefix containing include/, lib/*.lib, bin/*-<ABI>.dll, and .hifimule-audio-runtime.json.`);
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
    encodingDependencies: encodingDependencyReceipt,
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
  if (!existsSync(join(binDir, "ffmpeg.exe"))) fail(`Controlled FFmpeg reference executable is missing: ${join(binDir, "ffmpeg.exe")}`);
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

export function ensureWindowsAudioRuntime(target, options = {}) {
  if (!targets[target]) fail(`Unsupported Windows playback target: ${target}`);
  const env = options.env ?? process.env;
  if (env.FFMPEG_DIR) return verifyWindowsAudioRuntime(env.FFMPEG_DIR, target).prefix;
  if (usesPrebuiltAudioRuntime(env)) return ensurePrebuiltAudioRuntime(target, verifyWindowsAudioRuntime);
  const prefix = buildSourceAudioRuntime(target, { env });
  return verifyWindowsAudioRuntime(prefix, target).prefix;
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

export function stageWindowsAudioRuntimeForCargo(prefix, target, output) {
  const verified = verifyWindowsAudioRuntime(prefix, target);
  mkdirSync(output, { recursive: true });
  const staged = verified.dlls.map((dll) => {
    const destination = join(output, basename(dll));
    copyFileSync(dll, destination);
    return destination;
  });
  return staged;
}

export function windowsRuntimeDllNames(dlls) {
  return dlls.map((dll) => basename(dll));
}

function walkFiles(directory) {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = join(directory, entry.name);
    return entry.isDirectory() ? walkFiles(path) : entry.isFile() ? [path] : [];
  });
}

export function verifyInstalledWindowsBundle(bundleRoot, target) {
  const targetInfo = targets[target];
  if (!targetInfo) fail(`Unsupported Windows playback target: ${target}`);
  const files = walkFiles(resolve(bundleRoot));
  const sidecar = files.find((path) => basename(path).toLowerCase() === "hifimule-daemon.exe");
  if (!sidecar) fail(`Installed Windows bundle is missing hifimule-daemon.exe under ${bundleRoot}`);
  const mediacheck = files.find((path) => basename(path).toLowerCase() === "mediacheck.exe");
  if (!mediacheck || dirname(mediacheck) !== dirname(sidecar)) fail(`Installed Windows bundle is missing mediacheck.exe beside the daemon under ${bundleRoot}`);
  if (peMachine(mediacheck) !== targetInfo.machine) fail(`Installed Windows mediacheck architecture does not match ${target}`);
  const cliBytes = readFileSync(mediacheck);
  const subsystemOffset = cliBytes.readUInt32LE(0x3c) + 24 + 68;
  if (cliBytes.length < subsystemOffset + 2 || cliBytes.readUInt16LE(subsystemOffset) !== 3) fail("Installed mediacheck.exe must use the Windows console subsystem");
  if (peMachine(sidecar) !== targetInfo.machine) fail(`Installed Windows daemon architecture does not match ${target}`);
  for (const required of ["audio-runtime.json", "THIRD_PARTY_AUDIO_NOTICES.md"]) {
    if (!files.some((path) => basename(path) === required)) fail(`Installed Windows bundle is missing ${required}`);
  }
  const dlls = files.filter((path) => path.toLowerCase().endsWith(".dll"));
  for (const [library, version] of Object.entries(manifest.abiVersions)) {
    const expected = `${library}-${version.split(".")[0]}.dll`.toLowerCase();
    const matches = dlls.filter((path) => basename(path).toLowerCase() === expected);
    if (matches.length !== 1) fail(`Installed Windows bundle must contain exactly one ${expected}`);
    if (dirname(matches[0]) !== dirname(sidecar)) fail(`Installed Windows DLL is not beside the audio executables: ${matches[0]}`);
    if (peMachine(matches[0]) !== targetInfo.machine) fail(`Installed Windows DLL architecture does not match ${target}: ${matches[0]}`);
    if (resolve(matches[0]).toLowerCase().startsWith(resolve(process.env.SystemRoot ?? "C:\\Windows").toLowerCase())) fail(`System DLL cannot satisfy controlled runtime: ${matches[0]}`);
  }
  return { sidecar, mediacheck, dllCount: dlls.length };
}

if (process.argv[1] && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url))) {
  const [mode, target, output] = process.argv.slice(2);
  if (mode === "receipt") {
    if (!process.env.FFMPEG_DIR) fail("FFMPEG_DIR is not set for receipt generation");
    console.log(writeWindowsAudioRuntimeReceipt(process.env.FFMPEG_DIR, target));
  } else if (mode === "verify-bundle") {
    if (!output) fail("The `verify-bundle` command requires an extracted bundle root");
    const installed = verifyInstalledWindowsBundle(output, target);
    const env = { ...process.env };
    for (const key of Object.keys(env)) if (key.toUpperCase() === "PATH") delete env[key];
    env.PATH = join(process.env.SystemRoot ?? "C:\\Windows", "System32");
    const help = execFileSync(installed.mediacheck, ["--help"], { encoding: "utf8", env });
    if (!help.includes("Usage: mediacheck")) fail("Installed mediacheck console help was not displayed");
    console.log(JSON.stringify(installed));
  } else if (["ensure", "verify", "env"].includes(mode)) {
    const prefix = ensureWindowsAudioRuntime(target);
    if (mode === "env") {
      if (!output) fail("The `env` command requires an output environment file path");
      appendFileSync(output, `FFMPEG_DIR=${prefix}\n`);
      appendFileSync(output, `PATH=${prependWindowsPath(process.env, join(prefix, "bin")).PATH}\n`);
      appendFileSync(output, `${audioRuntimeVerification.environmentVariable}=${audioRuntimeVerification.value}\n`);
      console.log(prefix);
    } else console.log(JSON.stringify(mode === "verify" ? stageWindowsAudioRuntime(prefix, target) : { prefix }));
  } else fail("Expected `ensure`, `env`, `verify`, `verify-bundle`, or `receipt` command");
}
