#!/usr/bin/env node
import { execFileSync, spawnSync } from "node:child_process";
import { createHash, randomUUID } from "node:crypto";
import { appendFileSync, copyFileSync, existsSync, lstatSync, mkdirSync, readFileSync, readdirSync, realpathSync, renameSync, rmSync, statSync, writeFileSync } from "node:fs";
import { availableParallelism } from "node:os";
import { basename, dirname, join, posix, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import {
  audioRuntimeVerification,
  verifyAudioRuntime,
  withoutAudioRuntimeVerification,
} from "./verify-audio-runtime.mjs";

const root = resolve(import.meta.dirname, "..");
const manifest = JSON.parse(readFileSync(join(root, "hifimule-daemon/audio-runtime.json"), "utf8"));
const receiptName = ".hifimule-audio-runtime.json";
const machines = { "aarch64-unknown-linux-gnu": "AArch64", "x86_64-unknown-linux-gnu": "Advanced Micro Devices X86-64" };
const baseline = new Set(["libc.so.6", "libm.so.6", "libpthread.so.0", "libdl.so.2", "librt.so.1", "libgcc_s.so.1", "libstdc++.so.6"]);
export const linuxBuildPackages = Object.freeze(["build-essential", "clang", "libclang-dev", "libc6-dev", "curl", "xz-utils", "pkg-config", "binutils", "patchelf", "libmtp-dev"]);
export function requiresHostAudioVerification(platform) { return platform !== "win32"; }

function run(command, args, options = {}) {
  return execFileSync(command, args, { cwd: options.cwd, encoding: "utf8", stdio: options.stdio ?? ["ignore", "pipe", "pipe"], env: { ...(options.env ?? process.env), LC_ALL: "C" } });
}
function expectedReceipt(target) {
  return { schemaVersion: 1, targetTriple: target, ffmpegRelease: manifest.ffmpegRelease, sourceSha256: manifest.sourceSha256, configureFlags: ["--disable-autodetect", ...manifest.configureFlags], abiVersions: manifest.abiVersions };
}
function pcEnv(prefix) {
  return { ...process.env, PKG_CONFIG_PATH: [join(prefix, "lib/pkgconfig"), process.env.PKG_CONFIG_PATH].filter(Boolean).join(":") };
}
function elf(path) {
  const header = run("readelf", ["-h", path]);
  const dynamic = run("readelf", ["-d", path]);
  return { machine: header.match(/^\s*Machine:\s*(.+)$/m)?.[1]?.trim(), soname: dynamic.match(/\(SONAME\).*\[([^\]]+)\]/)?.[1], needed: [...dynamic.matchAll(/\(NEEDED\).*\[([^\]]+)\]/g)].map((m) => m[1]) };
}
function assertElf(path, target, soname) {
  if (!lstatSync(path).isFile()) throw new Error(`Expected regular ELF file: ${path}`);
  const meta = elf(path);
  if (meta.machine !== machines[target]) throw new Error(`ELF architecture mismatch for ${path}: ${meta.machine || "unknown"}`);
  if (soname && meta.soname !== soname) throw new Error(`ELF SONAME mismatch for ${path}: ${meta.soname || "missing"}`);
  return meta;
}
export function validateLinuxRuntimeReceipt(prefix, target) {
  const path = join(prefix, receiptName);
  if (!existsSync(path)) throw new Error(`Controlled runtime receipt is missing: ${path}`);
  let found;
  try { found = JSON.parse(readFileSync(path, "utf8")); } catch { throw new Error(`Controlled runtime receipt is invalid: ${path}`); }
  if (JSON.stringify(found) !== JSON.stringify(expectedReceipt(target))) throw new Error(`Controlled runtime receipt is stale: ${path}`);
  return found;
}
export function verifyLinuxAudioPrefix(prefix, target, options = {}) {
  if (!machines[target]) throw new Error(`Unsupported Linux audio runtime target: ${target}`);
  prefix = realpathSync(prefix);
  if (!options.skipReceipt) validateLinuxRuntimeReceipt(prefix, target);
  const env = pcEnv(prefix);
  for (const [library, version] of Object.entries(manifest.abiVersions)) {
    const name = `lib${library}`;
    const actual = run("pkg-config", ["--modversion", name], { env }).trim();
    const libdir = realpathSync(run("pkg-config", ["--variable=libdir", name], { env }).trim());
    if (actual !== version) throw new Error(`${name} version mismatch in ${prefix}: ${actual}`);
    if (libdir !== prefix && !libdir.startsWith(`${prefix}/`)) throw new Error(`${name} resolved outside controlled prefix: ${libdir}`);
    assertElf(realpathSync(join(libdir, `${name}.so`)), target, `${name}.so.${version.split(".")[0]}`);
  }
  return prefix;
}
function containsLibclang(dir, list = readdirSync) {
  try { return list(dir).some((name) => /^libclang(?:-[0-9.]+)?\.so(?:\..*)?$/.test(name)); } catch { return false; }
}
export function preflightLinuxBuild(target, options = {}) {
  if (!machines[target]) throw new Error(`Unsupported Linux audio runtime target: ${target}`);
  const probeEnv = options.env ?? process.env;
  const commandExists = options.commandExists ?? ((tool) => spawnSync("sh", ["-c", "command -v \"$1\" >/dev/null", "sh", tool], { env: probeEnv }).status === 0);
  const spawn = options.spawn ?? spawnSync;
  const execute = options.run ?? run;
  const fileExists = options.exists ?? existsSync;
  const list = options.readdir ?? readdirSync;
  const packages = linuxBuildPackages.join(" ");
  const missing = ["cc", "make", "curl", "tar", "pkg-config", "readelf", "patchelf", "clang"].filter((tool) => !commandExists(tool));
  if (missing.length) throw new Error(`Missing Linux build tools: ${missing.join(", ")}\nInstall with: sudo apt-get install ${packages}`);
  if (spawn("pkg-config", ["--exists", "libmtp"], { env: probeEnv }).status !== 0) throw new Error(`libmtp development files are missing.\nInstall with: sudo apt-get install ${packages}`);
  if (!fileExists("/usr/include/limits.h")) throw new Error(`glibc development headers are missing (/usr/include/limits.h).\nInstall with: sudo apt-get install ${packages}`);
  const resourceDir = execute("clang", ["-print-resource-dir"], { env: probeEnv }).trim();
  if (!resourceDir || !fileExists(posix.join(resourceDir, "include/limits.h"))) {
    throw new Error(`Clang resource headers are missing (expected <resource-dir>/include/limits.h).\nInstall with: sudo apt-get install ${packages}`);
  }
  const headerProbe = spawn("clang", [`--target=${target}`, `-resource-dir=${resourceDir}`, "-fsyntax-only", "-x", "c", "-"], { input: "#include <limits.h>\n#include <stdint.h>\n", encoding: "utf8", env: probeEnv });
  if (headerProbe.status !== 0) {
    const detail = headerProbe.stderr?.toString().trim();
    throw new Error(`Clang cannot compile against the native ${target} libc headers${detail ? `: ${detail}` : "."}\nInstall with: sudo apt-get install ${packages}`);
  }
  const libclangDir = posix.resolve(resourceDir, "../..");
  if (!containsLibclang(libclangDir, list)) {
    throw new Error(`The libclang matching ${resourceDir} is missing from ${libclangDir}.\nInstall the matching Ubuntu toolchain with: sudo apt-get install ${packages}`);
  }
  return { resourceDir, libclangDir };
}
export function linuxBuildEnvironment(target, baseEnv = process.env, options = {}) {
  const { resourceDir, libclangDir } = preflightLinuxBuild(target, { ...options, env: baseEnv });
  const args = [`--target=${target}`, `-resource-dir=${resourceDir}`].map((arg) => JSON.stringify(arg));
  const exactKey = `BINDGEN_EXTRA_CLANG_ARGS_${target}`;
  const normalizedKey = `BINDGEN_EXTRA_CLANG_ARGS_${target.replaceAll("-", "_")}`;
  const result = { ...baseEnv, LIBCLANG_PATH: libclangDir };
  for (const key of [exactKey, normalizedKey]) {
    const existing = baseEnv[key] || baseEnv.BINDGEN_EXTRA_CLANG_ARGS || "";
    result[key] = [...args, existing].filter(Boolean).join(" ");
  }
  return result;
}
function sha(path) { return createHash("sha256").update(readFileSync(path)).digest("hex"); }
export function acquireLinuxAudioRuntimeLock(path) {
  mkdirSync(dirname(path), { recursive: true });
  const deadline = Date.now() + 30 * 60_000;
  while (true) {
    try { mkdirSync(path); writeFileSync(join(path, "owner.json"), JSON.stringify({ pid: process.pid, at: new Date().toISOString() })); return () => rmSync(path, { recursive: true, force: true }); }
    catch (error) {
      if (error.code !== "EEXIST") throw error;
      if (Date.now() - statSync(path).mtimeMs > 60 * 60_000) { rmSync(path, { recursive: true, force: true }); continue; }
      if (Date.now() >= deadline) throw new Error(`Timed out waiting for audio runtime lock: ${path}`);
      Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 500);
    }
  }
}
function download(archive) {
  const temp = `${archive}.${process.pid}.${randomUUID()}.tmp`;
  try { run("curl", ["--fail", "--location", "--retry", "3", "--output", temp, manifest.sourceUrl], { stdio: "inherit" }); if (sha(temp) !== manifest.sourceSha256) throw new Error("FFmpeg source checksum mismatch"); renameSync(temp, archive); }
  finally { rmSync(temp, { force: true }); }
}
function relocatePkgConfigFiles(staging, prefix) {
  const pcDir = join(staging, "lib/pkgconfig");
  for (const name of readdirSync(pcDir)) {
    if (!name.endsWith(".pc")) continue;
    const path = join(pcDir, name);
    const relocated = readFileSync(path, "utf8").split(staging).join(prefix);
    if (relocated.includes(staging)) throw new Error(`Failed to relocate pkg-config metadata: ${path}`);
    writeFileSync(path, relocated);
  }
}
export function ensureLinuxAudioRuntime(target) {
  if (process.platform !== "linux") throw new Error("The controlled Linux audio runtime can only be prepared on Linux");
  if (!machines[target]) throw new Error(`Unsupported Linux audio runtime target: ${target}`);
  preflightLinuxBuild(target);
  const override = process.env.HIFIMULE_FFMPEG_PREFIX;
  const prefix = resolve(override || join(root, "target/audio-runtime", `ffmpeg-${manifest.ffmpegRelease}-${target}`));
  if (override) return verifyLinuxAudioPrefix(prefix, target);
  const unlock = acquireLinuxAudioRuntimeLock(`${prefix}.lock`);
  try {
    if (existsSync(prefix)) { try { return verifyLinuxAudioPrefix(prefix, target); } catch { rmSync(prefix, { recursive: true, force: true }); } }
    const cache = join(root, "target/audio-runtime/sources");
    const archive = join(cache, `ffmpeg-${manifest.ffmpegRelease}.tar.xz`);
    const work = join(cache, `build-${target}-${process.pid}-${randomUUID()}`);
    const source = join(work, `ffmpeg-${manifest.ffmpegRelease}`);
    const staging = `${prefix}.staging-${process.pid}-${randomUUID()}`;
    mkdirSync(cache, { recursive: true });
    try {
      if (!existsSync(archive) || sha(archive) !== manifest.sourceSha256) { rmSync(archive, { force: true }); download(archive); }
      mkdirSync(work); run("tar", ["-xf", archive, "-C", work], { stdio: "inherit" }); mkdirSync(staging);
      run("./configure", [`--prefix=${staging}`, "--disable-autodetect", ...manifest.configureFlags], { cwd: source, stdio: "inherit" });
      run("make", [`-j${Math.min(8, Math.max(1, availableParallelism()))}`], { cwd: source, stdio: "inherit" });
      run("make", ["install"], { cwd: source, stdio: "inherit" });
      writeFileSync(join(staging, receiptName), `${JSON.stringify(expectedReceipt(target), null, 2)}\n`);
      verifyLinuxAudioPrefix(staging, target);
      relocatePkgConfigFiles(staging, prefix);
      renameSync(staging, prefix);
    } finally { rmSync(work, { recursive: true, force: true }); rmSync(staging, { recursive: true, force: true }); }
    return verifyLinuxAudioPrefix(prefix, target);
  } finally { unlock(); }
}
function resolveNeeded(source, name, dirs) {
  for (const dir of [dirname(source), ...dirs]) { const candidate = join(dir, name); if (existsSync(candidate)) return realpathSync(candidate); }
  const line = run("ldd", [source], { env: { ...process.env, LD_LIBRARY_PATH: dirs.join(":") } }).split("\n").find((l) => l.trim().startsWith(`${name} =>`));
  const path = line?.match(/=>\s+(\/\S+)/)?.[1];
  if (!path || !existsSync(path)) throw new Error(`Unable to resolve ${name}, required by ${source}`);
  return realpathSync(path);
}
function closure(roots, out, target, dirs) {
  const queue = [...roots], copied = new Map();
  while (queue.length) {
    const source = realpathSync(queue.shift()), meta = assertElf(source, target);
    if (!meta.soname) throw new Error(`ELF library has no SONAME: ${source}`);
    if (copied.has(meta.soname)) { if (sha(source) !== sha(copied.get(meta.soname))) throw new Error(`Conflicting SONAME ${meta.soname}`); continue; }
    copyFileSync(source, join(out, meta.soname)); copied.set(meta.soname, source);
    for (const name of meta.needed) if (!baseline.has(name) && !/^ld-linux/.test(name)) queue.push(resolveNeeded(source, name, dirs));
  }
  return copied;
}
function resolved(path, env) { const output = run("ldd", [path], { env }); if (/=>\s+not found/.test(output)) throw new Error(`Unresolved dependency for ${path}:\n${output}`); }
export function bundleLinuxAudioRuntime(prefix, sidecar, target) {
  prefix = verifyLinuxAudioPrefix(prefix, target);
  const env = pcEnv(prefix), out = join(root, "hifimule-ui/src-tauri/bundled-libs"); mkdirSync(out, { recursive: true });
  for (const name of readdirSync(out)) if (/\.so(?:\.|$)/.test(name)) rmSync(join(out, name), { force: true });
  const roots = [], dirs = [join(prefix, "lib")];
  for (const library of Object.keys(manifest.abiVersions)) { const dir = run("pkg-config", ["--variable=libdir", `lib${library}`], { env }).trim(); roots.push(realpathSync(join(dir, `lib${library}.so`))); }
  const mtpDir = run("pkg-config", ["--variable=libdir", "libmtp"], { env }).trim(); dirs.push(mtpDir); roots.push(realpathSync(join(mtpDir, "libmtp.so")));
  const copied = closure(roots, out, target, dirs);
  for (const name of copied.keys()) { const path = join(out, name); run("patchelf", ["--set-rpath", "$ORIGIN", path]); if (run("patchelf", ["--print-rpath", path]).trim() !== "$ORIGIN") throw new Error(`Invalid RUNPATH: ${path}`); }
  const sidecarPath = ["$ORIGIN/../bundled-libs", "$ORIGIN/bundled-libs", "$ORIGIN/../lib/HifiMule/bundled-libs", "$ORIGIN/../lib/hifimule/bundled-libs"].join(":");
  run("patchelf", ["--set-rpath", sidecarPath, sidecar]);
  const resolveEnv = { ...process.env, LD_LIBRARY_PATH: out }; for (const name of copied.keys()) resolved(join(out, name), resolveEnv); resolved(sidecar, process.env);
}
function walk(dir) { return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => entry.isDirectory() ? walk(join(dir, entry.name)) : entry.isFile() ? [join(dir, entry.name)] : []); }
export function verifyInstalledLinuxBundle(bundleRoot, target) {
  const files = walk(bundleRoot);
  const sidecar = files.find((path) => basename(path).startsWith("hifimule-daemon"));
  if (!sidecar) throw new Error(`No daemon sidecar under ${bundleRoot}`); assertElf(sidecar, target);
  const codec = files.find((path) => basename(path) === `libavcodec.so.${manifest.abiVersions.avcodec.split(".")[0]}`);
  if (!codec) throw new Error("Controlled FFmpeg library missing"); const libdir = dirname(codec);
  const libs = files.filter((path) => dirname(path) === libdir && /\.so(?:\.|$)/.test(basename(path)));
  const names = new Set(libs.map((path) => basename(path)));
  for (const [library, version] of Object.entries(manifest.abiVersions)) {
    const required = `lib${library}.so.${version.split(".")[0]}`;
    if (!names.has(required)) throw new Error(`Required controlled library is missing: ${required}`);
  }
  if (!names.has("libmtp.so.9")) throw new Error("Required controlled library is missing: libmtp.so.9");
  for (const path of libs) { assertElf(path, target, basename(path)); if (run("patchelf", ["--print-rpath", path]).trim() !== "$ORIGIN") throw new Error(`Invalid installed RUNPATH: ${path}`); }
  for (const path of libs) {
    for (const needed of elf(path).needed) {
      if (!baseline.has(needed) && !/^ld-linux/.test(needed) && !names.has(needed)) throw new Error(`Installed private closure is missing ${needed}, required by ${path}`);
    }
  }
  const sidecarRunpath = run("patchelf", ["--print-rpath", sidecar]).trim();
  if (!sidecarRunpath.includes("bundled-libs")) throw new Error(`Invalid installed sidecar RUNPATH: ${sidecarRunpath}`);
  const env = { ...process.env, LD_LIBRARY_PATH: libdir }; for (const path of libs) resolved(path, env); resolved(sidecar, process.env);
  return { sidecar, libdir, libraryCount: libs.length };
}
export function writeLinuxBuildEnvironment(prefix, path, target, options = {}) {
  const baseEnv = withoutAudioRuntimeVerification(options.env ?? process.env);
  const verify = options.verifyAudioRuntime ?? verifyAudioRuntime;
  const buildEnvironment = options.linuxBuildEnvironment ?? linuxBuildEnvironment;
  const native = buildEnvironment(target, baseEnv);
  verify({ prefix, env: native });
  const exactKey = `BINDGEN_EXTRA_CLANG_ARGS_${target}`;
  const normalizedKey = `BINDGEN_EXTRA_CLANG_ARGS_${target.replaceAll("-", "_")}`;
  const values = {
    HIFIMULE_FFMPEG_PREFIX: prefix,
    PKG_CONFIG_PATH: [posix.join(prefix, "lib/pkgconfig"), baseEnv.PKG_CONFIG_PATH].filter(Boolean).join(":"),
    LD_LIBRARY_PATH: [posix.join(prefix, "lib"), baseEnv.LD_LIBRARY_PATH].filter(Boolean).join(":"),
    LIBCLANG_PATH: native.LIBCLANG_PATH,
    [exactKey]: native[exactKey],
    [normalizedKey]: native[normalizedKey],
    [audioRuntimeVerification.environmentVariable]: audioRuntimeVerification.value,
  };
  for (const [key, value] of Object.entries(values)) appendFileSync(path, `${key}=${value}\n`);
}
if (process.argv[1] && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url))) {
  const [mode, target, arg] = process.argv.slice(2);
  if (mode === "ensure") console.log(ensureLinuxAudioRuntime(target));
  else if (mode === "env") writeLinuxBuildEnvironment(ensureLinuxAudioRuntime(target), arg, target);
  else if (mode === "verify-bundle") console.log(JSON.stringify(verifyInstalledLinuxBundle(arg, target)));
  else throw new Error("Usage: linux-audio-runtime.mjs <ensure TARGET | env TARGET OUTPUT | verify-bundle TARGET ROOT>");
}
