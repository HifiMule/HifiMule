#!/usr/bin/env node
import { execFileSync, spawnSync } from "node:child_process";
import { createHash, randomUUID } from "node:crypto";
import { appendFileSync, copyFileSync, existsSync, lstatSync, mkdirSync, readFileSync, readdirSync, realpathSync, renameSync, rmSync, statSync, writeFileSync } from "node:fs";
import { availableParallelism } from "node:os";
import { basename, dirname, isAbsolute, join, posix, relative, resolve, sep } from "node:path";
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
const linuxLoaders = { "aarch64-unknown-linux-gnu": "ld-linux-aarch64.so.1", "x86_64-unknown-linux-gnu": "ld-linux-x86-64.so.2" };
const hostRuntimeLibraryPrefixes = Object.freeze([
  "libgtk-3.so.", "libgdk-3.so.", "libgdk_pixbuf-2.0.so.", "libgio-2.0.so.",
  "libglib-2.0.so.", "libgobject-2.0.so.", "libpango", "libpangocairo-", "libcairo.so.",
  "libatk-1.0.so.", "libatk-bridge-2.0.so.", "libatspi.so.", "libX11.so.", "libXcursor.so.",
  "libXi.so.", "libXrandr.so.", "libXrender.so.", "libXfixes.so.", "libXext.so.",
  "libxkbcommon.so.", "libwayland-", "libssl.so.", "libcrypto.so.", "libxdo.so.",
]);
const isHostRuntimeLibrary = (name) => hostRuntimeLibraryPrefixes.some((prefix) => name.startsWith(prefix));
export const linuxBuildPackages = Object.freeze(["build-essential", "clang", "libclang-dev", "libc6-dev", "nasm", "curl", "xz-utils", "pkg-config", "binutils", "patchelf", "libmtp-dev", "libasound2-dev", "libpulse-dev", "libdbus-1-dev"]);
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
  const tools = ["cc", "make", "curl", "tar", "pkg-config", "readelf", "patchelf", "clang"];
  if (target === "x86_64-unknown-linux-gnu") tools.push("nasm");
  const missing = tools.filter((tool) => !commandExists(tool));
  if (missing.length) throw new Error(`Missing Linux build tools: ${missing.join(", ")}\nInstall with: sudo apt-get install ${packages}`);
  if (spawn("pkg-config", ["--exists", "libmtp"], { env: probeEnv }).status !== 0) throw new Error(`libmtp development files are missing.\nInstall with: sudo apt-get install ${packages}`);
  if (spawn("pkg-config", ["--exists", "alsa"], { env: probeEnv }).status !== 0) throw new Error(`ALSA development files are missing.\nInstall with: sudo apt-get install ${packages}`);
  if (spawn("pkg-config", ["--exists", "libpulse"], { env: probeEnv }).status !== 0) throw new Error(`PulseAudio development files are missing.\nInstall with: sudo apt-get install ${packages}`);
  if (spawn("pkg-config", ["--exists", "dbus-1"], { env: probeEnv }).status !== 0) throw new Error(`D-Bus development files are missing.\nInstall with: sudo apt-get install ${packages}`);
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
export function stageLinuxDependencyClosure(sidecar, roots, out, target, dirs, options = {}) {
  const inspectElf = options.assertElf ?? assertElf;
  const resolveDependency = options.resolveNeeded ?? resolveNeeded;
  const isSystem = (name) => baseline.has(name) || name === linuxLoaders[target];
  const queue = [...roots], copied = new Map();
  // The daemon's GUI and tray dependencies must resolve from the host. Bundling
  // Ubuntu 22 GLib/GTK libraries makes their older ABI override newer distro
  // modules (for example Ubuntu 26's GVfs and dconf modules). Only stage the
  // explicit audio, MTP, and Pulse roots and their private dependency closure.
  while (queue.length) {
    const source = realpathSync(queue.shift()), meta = inspectElf(source, target);
    if (!meta.soname) throw new Error(`ELF library has no SONAME: ${source}`);
    if (copied.has(meta.soname)) { if (sha(source) !== sha(copied.get(meta.soname))) throw new Error(`Conflicting SONAME ${meta.soname}`); continue; }
    copyFileSync(source, join(out, meta.soname)); copied.set(meta.soname, source);
    for (const name of meta.needed) if (!isSystem(name)) queue.push(resolveDependency(source, name, dirs));
  }
  return copied;
}
function resolved(path, env) { const output = run("ldd", [path], { env }); if (/=>\s+not found/.test(output)) throw new Error(`Unresolved dependency for ${path}:\n${output}`); }
export function verifyLinuxAudioLinkage(needed) {
  for (const [library, version] of Object.entries(manifest.abiVersions)) {
    const stem = `lib${library}.so.`;
    const expected = `${stem}${version.split(".")[0]}`;
    const actual = needed.filter((name) => name.startsWith(stem));
    if (actual.length !== 1 || actual[0] !== expected) {
      throw new Error(`Daemon FFmpeg linkage mismatch: expected ${expected}, found ${actual.join(", ") || "none"}. Rebuild the daemon with the controlled FFmpeg prefix before packaging.`);
    }
  }
}
export function bundleLinuxAudioRuntime(prefix, sidecar, target) {
  verifyLinuxAudioLinkage(assertElf(sidecar, target).needed);
  prefix = verifyLinuxAudioPrefix(prefix, target);
  const env = pcEnv(prefix), out = join(root, "hifimule-ui/src-tauri/bundled-libs"); mkdirSync(out, { recursive: true });
  for (const name of readdirSync(out)) if (/\.so(?:\.|$)/.test(name)) rmSync(join(out, name), { force: true });
  const roots = [], dirs = [join(prefix, "lib")];
  for (const library of Object.keys(manifest.abiVersions)) { const dir = run("pkg-config", ["--variable=libdir", `lib${library}`], { env }).trim(); roots.push(realpathSync(join(dir, `lib${library}.so`))); }
  const mtpDir = run("pkg-config", ["--variable=libdir", "libmtp"], { env }).trim(); dirs.push(mtpDir); roots.push(realpathSync(join(mtpDir, "libmtp.so")));
  const pulseDir = run("pkg-config", ["--variable=libdir", "libpulse"], { env }).trim(); dirs.push(pulseDir); roots.push(realpathSync(join(pulseDir, "libpulse.so")));
  const copied = stageLinuxDependencyClosure(sidecar, roots, out, target, dirs);
  for (const name of copied.keys()) { const path = join(out, name); run("patchelf", ["--set-rpath", "$ORIGIN", path]); if (run("patchelf", ["--print-rpath", path]).trim() !== "$ORIGIN") throw new Error(`Invalid RUNPATH: ${path}`); }
  const sidecarPath = ["$ORIGIN/../bundled-libs", "$ORIGIN/bundled-libs", "$ORIGIN/../lib/HifiMule/bundled-libs", "$ORIGIN/../lib/hifimule/bundled-libs"].join(":");
  run("patchelf", ["--set-rpath", sidecarPath, sidecar]);
  const resolveEnv = { ...process.env, LD_LIBRARY_PATH: out }; for (const name of copied.keys()) resolved(join(out, name), resolveEnv); resolved(sidecar, process.env);
}
function walk(dir) { return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => entry.isDirectory() ? walk(join(dir, entry.name)) : entry.isFile() ? [join(dir, entry.name)] : []); }
function isWithin(root, candidate) {
  const path = relative(root, candidate);
  return path === "" || (path !== ".." && !path.startsWith(`..${sep}`) && !isAbsolute(path));
}
function isSameFile(left, right) {
  const leftStat = statSync(left);
  const rightStat = statSync(right);
  return leftStat.dev === rightStat.dev && leftStat.ino === rightStat.ino;
}
function resolveRunpathEntry(root, origin, suffix, invalid) {
  let candidate = origin;
  let hasMissingComponent = false;
  for (const component of suffix.split("/")) {
    if (!component || component === ".") continue;
    if (component === "..") {
      if (hasMissingComponent) throw invalid();
      candidate = dirname(candidate);
    } else {
      candidate = join(candidate, component);
      if (existsSync(candidate)) {
        candidate = realpathSync(candidate);
        if (!statSync(candidate).isDirectory()) throw invalid();
      } else hasMissingComponent = true;
    }
    if (!isWithin(root, candidate)) throw invalid();
  }
  return candidate;
}
export function validateInstalledSidecarRunpath(bundleRoot, sidecar, runpath) {
  const root = realpathSync(bundleRoot);
  const origin = realpathSync(dirname(sidecar));
  const invalid = () => new Error(`Invalid installed sidecar RUNPATH: ${runpath}`);
  if (!isWithin(root, origin)) throw invalid();
  const entries = runpath.split(":");
  if (!runpath || entries.some((entry) => !entry)) throw invalid();
  const existingDirectories = [];
  for (const entry of entries) {
    if (isAbsolute(entry)) throw invalid();
    let suffix;
    if (entry === "$ORIGIN" || entry === "${ORIGIN}") suffix = ".";
    else if (entry.startsWith("$ORIGIN/")) suffix = entry.slice("$ORIGIN/".length);
    else if (entry.startsWith("${ORIGIN}/")) suffix = entry.slice("${ORIGIN}/".length);
    else throw invalid();
    const candidate = resolveRunpathEntry(root, origin, suffix, invalid);
    if (existsSync(candidate) && !existingDirectories.some((directory) => isSameFile(candidate, directory))) {
      existingDirectories.push(candidate);
    }
  }
  if (existingDirectories.length !== 1) throw invalid();
  return existingDirectories[0];
}
export function verifyInstalledLinuxBundle(bundleRoot, target, options = {}) {
  const listFiles = options.walk ?? walk;
  const inspectElf = options.assertElf ?? assertElf;
  const execute = options.run ?? run;
  const verifyResolved = options.resolved ?? resolved;
  const files = listFiles(bundleRoot);
  const sidecar = files.find((path) => basename(path).startsWith("hifimule-daemon"));
  if (!sidecar) throw new Error(`No daemon sidecar under ${bundleRoot}`);
  const sidecarMeta = inspectElf(sidecar, target);
  verifyLinuxAudioLinkage(sidecarMeta.needed);
  const sidecarRunpath = execute("patchelf", ["--print-rpath", sidecar]).trim();
  const libdir = validateInstalledSidecarRunpath(bundleRoot, sidecar, sidecarRunpath);
  const controlledRoots = [
    ...Object.entries(manifest.abiVersions).map(([library, version]) => `lib${library}.so.${version.split(".")[0]}`),
    "libmtp.so.9",
    "libpulse.so.0",
  ];
  const queue = [];
  const systemDependencies = new Set([...baseline, linuxLoaders[target]]);
  const validateDependencyName = (name, requiredBy) => {
    if (typeof name !== "string" || !name || name.includes("/") || name === "." || name === ".." || /[\x00-\x1f\x7f]/.test(name)) {
      throw new Error(`Invalid installed dependency name ${JSON.stringify(name)}, required by ${requiredBy}`);
    }
  };
  const enqueue = (name, requiredBy) => {
    validateDependencyName(name, requiredBy);
    if (systemDependencies.has(name)) {
      try { lstatSync(join(libdir, name)); } catch (error) {
        if (error.code === "ENOENT") return;
        throw error;
      }
    }
    queue.push({ name, requiredBy });
  };
  for (const name of sidecarMeta.needed) if (!isHostRuntimeLibrary(name)) enqueue(name, sidecar);
  for (const name of controlledRoots) enqueue(name, "the controlled Linux runtime");
  const libraries = new Map();
  while (queue.length) {
    const { name, requiredBy } = queue.shift();
    if (libraries.has(name)) continue;
    const path = join(libdir, name);
    let pathStat;
    try { pathStat = lstatSync(path); } catch (error) {
      if (error.code === "ENOENT") throw new Error(`Installed private closure is missing ${name}, required by ${requiredBy}`);
      throw error;
    }
    let targetPath = path;
    if (pathStat.isSymbolicLink()) {
      try { targetPath = realpathSync(path); } catch { throw new Error(`Invalid installed library symlink: ${path}`); }
      if (dirname(targetPath) !== libdir || !statSync(targetPath).isFile()) throw new Error(`Invalid installed library symlink: ${path}`);
    } else if (!pathStat.isFile()) {
      throw new Error(`Expected regular ELF file: ${path}`);
    }
    const meta = inspectElf(targetPath, target, name);
    if (execute("patchelf", ["--print-rpath", targetPath]).trim() !== "$ORIGIN") throw new Error(`Invalid installed RUNPATH: ${path}`);
    libraries.set(name, { path, target: targetPath });
    for (const dependency of meta.needed) enqueue(dependency, path);
  }
  const env = { ...process.env, LD_LIBRARY_PATH: libdir };
  for (const { path } of libraries.values()) verifyResolved(path, env);
  verifyResolved(sidecar, process.env);
  return { sidecar, libdir, libraryCount: libraries.size };
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
    FFMPEG_DIR: prefix,
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
