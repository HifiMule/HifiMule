#!/usr/bin/env node
import { execFileSync, spawnSync } from "node:child_process";
import { createHash, randomUUID } from "node:crypto";
import { appendFileSync, copyFileSync, existsSync, mkdirSync, readFileSync, readdirSync, renameSync, rmSync, statSync, writeFileSync } from "node:fs";
import { basename, join, resolve } from "node:path";
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
  throw new Error(`${message}\n\nWindows playback automatically provisions the pinned ${manifest.windowsDistribution.provider} ` +
    `${manifest.windowsDistribution.revision} ${manifest.windowsDistribution.variant} SDK. Set FFMPEG_DIR only to override it with a ` +
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
  return { schemaVersion: 1, targetTriple: target, ffmpegRelease: manifest.ffmpegRelease, sourceSha256: manifest.sourceSha256, configureFlags: manifest.configureFlags, abiVersions: manifest.abiVersions };
}

function artifact(target) {
  const value = manifest.windowsDistribution.artifacts[target];
  if (!targets[target] || !value) fail(`Unsupported Windows playback target: ${target}`);
  return value;
}

function expectedDistributionReceipt(target) {
  const value = artifact(target);
  return {
    schemaVersion: 1,
    targetTriple: target,
    ffmpegRelease: manifest.ffmpegRelease,
    provider: manifest.windowsDistribution.provider,
    releaseTag: manifest.windowsDistribution.releaseTag,
    revision: manifest.windowsDistribution.revision,
    variant: manifest.windowsDistribution.variant,
    archive: value.archive,
    archiveUrl: value.url,
    archiveSha256: value.sha256,
    abiVersions: manifest.windowsDistribution.abiVersions,
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
  const accepted = [expectedReceipt(target), expectedDistributionReceipt(target)].some((value) => JSON.stringify(receipt) === JSON.stringify(value));
  if (!accepted) fail(`Controlled FFmpeg receipt is stale or for another target: ${receiptPath}`);

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
  const targetArtifact = artifact(target);
  const env = options.env ?? process.env;
  if (env.FFMPEG_DIR) return verifyWindowsAudioRuntime(env.FFMPEG_DIR, target).prefix;
  const platform = options.platform ?? process.platform;
  if (platform !== "win32") fail("Automatic Windows FFmpeg provisioning can only run on Windows");
  const cacheRoot = resolve(options.cacheRoot ?? join(root, "target/audio-runtime"));
  const prefix = join(cacheRoot, `btbn-${manifest.windowsDistribution.revision}-${target}`);
  const archive = join(cacheRoot, "sources", targetArtifact.archive);
  const calculateSha = options.sha ?? sha;
  const download = options.download ?? ((url, path) => execFileSync("curl.exe", ["--fail", "--location", "--retry", "3", "--output", path, url], { stdio: "inherit" }));
  const extract = options.extract ?? ((path, staging) => execFileSync("tar.exe", ["-xf", path, "-C", staging, "--strip-components=1"], { stdio: "inherit" }));
  for (const tool of ["curl.exe", "tar.exe"]) {
    if (!options.download && !options.extract && spawnSync(tool, ["--version"], { stdio: "ignore" }).status !== 0) fail(`Required Windows archive tool is missing: ${tool}`);
  }
  mkdirSync(join(cacheRoot, "sources"), { recursive: true });
  const unlock = lock(`${prefix}.lock`);
  try {
    if (existsSync(prefix)) { try { return verifyWindowsAudioRuntime(prefix, target).prefix; } catch { rmSync(prefix, { recursive: true, force: true }); } }
    if (!existsSync(archive) || calculateSha(archive) !== targetArtifact.sha256) {
      rmSync(archive, { force: true });
      const temporaryArchive = `${archive}.${process.pid}.${randomUUID()}.tmp`;
      try {
        download(targetArtifact.url, temporaryArchive);
        if (calculateSha(temporaryArchive) !== targetArtifact.sha256) fail(`Windows FFmpeg archive checksum mismatch: ${targetArtifact.archive}`);
        renameSync(temporaryArchive, archive);
      } finally { rmSync(temporaryArchive, { force: true }); }
    }
    const staging = `${prefix}.staging-${process.pid}-${randomUUID()}`;
    try {
      mkdirSync(staging, { recursive: true });
      extract(archive, staging);
      writeFileSync(join(staging, ".hifimule-audio-runtime.json"), `${JSON.stringify(expectedDistributionReceipt(target), null, 2)}\n`);
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
