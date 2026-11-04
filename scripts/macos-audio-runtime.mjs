import { buildSourceAudioRuntime } from "./source-audio-runtime.mjs";
import { encodingDependencyReceipt } from "./encoding-dependencies.mjs";
import { ensurePrebuiltAudioRuntime, usesPrebuiltAudioRuntime } from "./prebuilt-audio-runtime.mjs";
import { execFileSync } from "node:child_process";

import { appendFileSync, existsSync, readFileSync, readdirSync } from "node:fs";

import { basename, dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { audioRuntimeVerification, verifyAudioRuntime, withoutAudioRuntimeVerification } from "./verify-audio-runtime.mjs";

const root = resolve(import.meta.dirname, "..");
const manifest = JSON.parse(readFileSync(join(root, "hifimule-daemon/audio-runtime.json"), "utf8"));
const targets = {
  "x86_64-apple-darwin": { arch: "x86_64", minimum: "10.15" },
  "aarch64-apple-darwin": { arch: "arm64", minimum: "11.0" },
};
const receiptName = ".hifimule-audio-runtime.json";

function run(command, args, options = {}) {
  return execFileSync(command, args, { cwd: options.cwd, encoding: "utf8", stdio: options.stdio ?? ["ignore", "pipe", "pipe"], env: options.env ?? process.env });
}
function expectedReceipt(target) {
  const info = targets[target];
  return {
    schemaVersion: 1,
    targetTriple: target,
    ffmpegRelease: manifest.ffmpegRelease,
    sourceUrl: manifest.sourceUrl,
    signatureUrl: manifest.signatureUrl,
    signingKey: manifest.signingKey,
    sourceSha256: manifest.sourceSha256,
    configureFlags: ["--disable-autodetect", `--arch=${info.arch}`, "--target-os=darwin", ...manifest.configureFlags],
    abiVersions: manifest.abiVersions,
    encodingDependencies: encodingDependencyReceipt,
    minimumSystemVersion: info.minimum,
  };
}
export function macosRuntimeReceipt(target) {
  if (!targets[target]) throw new Error(`Unsupported macOS audio runtime target: ${target}`);
  return expectedReceipt(target);
}
function dylibFor(prefix, library, major) {
  const candidates = readdirSync(join(prefix, "lib"))
    .filter((name) => name.startsWith(`lib${library}.`) && name.endsWith(".dylib"));
  const exact = candidates.find((name) => name === `lib${library}.${major}.dylib`)
    ?? candidates.find((name) => name.includes(`.${major}.`));
  if (!exact) throw new Error(`Controlled macOS runtime is missing lib${library}.${major}.dylib`);
  return join(prefix, "lib", exact);
}
export function verifyMacosAudioRuntime(prefix, target, options = {}) {
  const info = targets[target];
  if (!info) throw new Error(`Unsupported macOS audio runtime target: ${target}`);
  prefix = resolve(prefix);
  const expectedLoadPrefix = resolve(options.expectedLoadPrefix ?? prefix);
  const receiptPath = join(prefix, receiptName);
  if (!existsSync(receiptPath)) throw new Error(`Controlled macOS runtime receipt is missing: ${receiptPath}`);
  const receipt = JSON.parse(readFileSync(receiptPath, "utf8"));
  if (JSON.stringify(receipt) !== JSON.stringify(expectedReceipt(target))) throw new Error(`Controlled macOS runtime receipt is stale: ${receiptPath}`);
  const env = { ...process.env, PKG_CONFIG_PATH: join(prefix, "lib/pkgconfig") };
  const execute = options.run ?? run;
  for (const [library, version] of Object.entries(manifest.abiVersions)) {
    const actual = execute("pkg-config", ["--modversion", `lib${library}`], { env }).trim();
    if (actual !== version) throw new Error(`lib${library} version mismatch: expected ${version}, found ${actual}`);
    execute("lipo", [dylibFor(prefix, library, version.split(".")[0]), "-verify_arch", info.arch]);
    const loadCommands = execute("otool", ["-L", dylibFor(prefix, library, version.split(".")[0])]);
    const dependencies = loadCommands.split("\n").slice(1).map((line) => line.trim().split(/\s+/)[0]).filter(Boolean);
    if (dependencies.some((dependency) => dependency.includes(".staging-")
      || dependency.includes("target/audio-runtime") && !dependency.startsWith(`${expectedLoadPrefix}/`))) {
      throw new Error(`lib${library} contains stale controlled-runtime load paths:\n${dependencies.join("\n")}`);
    }
  }
  return prefix;
}
export function ensureMacosAudioRuntime(target, options = {}) {
  if ((options.platform ?? process.platform) !== "darwin") throw new Error("The controlled macOS audio runtime can only be prepared on macOS");
  if (!targets[target]) throw new Error(`Unsupported macOS audio runtime target: ${target}`);
  const env = options.env ?? process.env;
  if (env.HIFIMULE_FFMPEG_PREFIX) return verifyMacosAudioRuntime(env.HIFIMULE_FFMPEG_PREFIX, target, options);
  if (usesPrebuiltAudioRuntime(env)) return ensurePrebuiltAudioRuntime(target, verifyMacosAudioRuntime);
  return verifyMacosAudioRuntime(buildSourceAudioRuntime(target, { env }), target);
}
function walk(dir) {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => entry.isDirectory() ? walk(join(dir, entry.name)) : entry.isFile() ? [join(dir, entry.name)] : []);
}
export function verifyInstalledMacosBundle(appRoot, target, options = {}) {
  const info = targets[target];
  if (!info) throw new Error(`Unsupported macOS audio runtime target: ${target}`);
  const files = walk(appRoot);
  const sidecar = files.find((path) => basename(path) === "hifimule-daemon"
    || basename(path).startsWith("hifimule-daemon-"));
  if (!sidecar) throw new Error("macOS bundle is missing the daemon sidecar");
  const mediacheck = files.find((path) => basename(path) === "mediacheck");
  if (!mediacheck || dirname(mediacheck) !== dirname(sidecar)) throw new Error("macOS bundle is missing mediacheck beside the daemon");
  for (const required of ["audio-runtime.json", "THIRD_PARTY_AUDIO_NOTICES.md"]) {
    if (!files.some((path) => basename(path) === required)) throw new Error(`macOS bundle is missing ${required}`);
  }
  const execute = options.run ?? run;
  execute("lipo", [sidecar, "-verify_arch", info.arch]);
  const dylibs = files.filter((path) => /[\\/]bundled-libs[\\/]/.test(path) && path.endsWith(".dylib"));
  for (const [library, version] of Object.entries(manifest.abiVersions)) {
    const major = version.split(".")[0];
    const names = dylibs.map((path) => basename(path)).filter((name) => name.startsWith(`lib${library}.`));
    if (!names.includes(`lib${library}.${version}.dylib`)) throw new Error(`macOS bundle is missing exact controlled lib${library} ${version}`);
    if (names.some((name) => name !== `lib${library}.${version}.dylib` && name !== `lib${library}.${major}.dylib`)) {
      throw new Error(`macOS bundle contains stale lib${library} runtime: ${names.join(", ")}`);
    }
  }
  for (const path of [sidecar, mediacheck, ...dylibs]) {
    execute("lipo", [path, "-verify_arch", info.arch]);
    const dependencies = execute("otool", ["-L", path]);
    if (/\/opt\/homebrew|\/usr\/local|target\/audio-runtime|\.cargo/.test(dependencies)) throw new Error(`Developer load path remains in ${path}`);
  }
  return { sidecar, mediacheck, dylibCount: dylibs.length };
}
export function writeMacosBuildEnvironment(prefix, target, output, options = {}) {
  const env = withoutAudioRuntimeVerification(options.env ?? process.env);
  verifyMacosAudioRuntime(prefix, target);
  verifyAudioRuntime({ prefix, env: { ...env, PKG_CONFIG_PATH: join(prefix, "lib/pkgconfig") } });
  const values = {
    HIFIMULE_FFMPEG_PREFIX: prefix,
    FFMPEG_DIR: prefix,
    PKG_CONFIG_PATH: [join(prefix, "lib/pkgconfig"), env.PKG_CONFIG_PATH].filter(Boolean).join(":"),
    DYLD_LIBRARY_PATH: [join(prefix, "lib"), env.DYLD_LIBRARY_PATH].filter(Boolean).join(":"),
    MACOSX_DEPLOYMENT_TARGET: targets[target].minimum,
    [audioRuntimeVerification.environmentVariable]: audioRuntimeVerification.value,
  };
  for (const [key, value] of Object.entries(values)) appendFileSync(output, `${key}=${value}\n`);
}

if (process.argv[1] && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url))) {
  const [mode, target, value] = process.argv.slice(2);
  if (mode === "ensure") console.log(ensureMacosAudioRuntime(target));
  else if (mode === "env") writeMacosBuildEnvironment(ensureMacosAudioRuntime(target), target, value);
  else if (mode === "verify-bundle") {
    const installed = verifyInstalledMacosBundle(value, target);
    const env = { ...process.env };
    delete env.DYLD_LIBRARY_PATH;
    const help = run(installed.mediacheck, ["--help"], { env });
    if (!help.includes("Usage: mediacheck")) throw new Error("Installed mediacheck console help was not displayed");
    console.log(JSON.stringify(installed));
  }
  else throw new Error("Usage: macos-audio-runtime.mjs <ensure TARGET | env TARGET OUTPUT | verify-bundle TARGET APP>");
}
