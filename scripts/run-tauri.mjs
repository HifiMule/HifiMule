#!/usr/bin/env node
import { spawnSync } from "node:child_process";
import { createHash, randomUUID } from "node:crypto";
import {
  existsSync,
  mkdirSync,
  readFileSync,
  renameSync,
  rmSync,
} from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(import.meta.dirname, "..");
const uiDir = join(root, "hifimule-ui");
const runtimeRelease = "20251108";
const ubuntuAppImagePackages = Object.freeze([
  "libgtk-3-dev",
  "libxdo-dev",
  "libsoup-3.0-dev",
  "libwebkit2gtk-4.1-dev",
  "libfuse2",
  "librsvg2-dev",
]);
const appImagePkgConfigModules = Object.freeze({
  "gtk+-3.0": "libgtk-3-dev",
  "libsoup-3.0": "libsoup-3.0-dev",
  "webkit2gtk-4.1": "libwebkit2gtk-4.1-dev",
  "librsvg-2.0": "librsvg2-dev",
});
const runtimes = Object.freeze({
  arm64: {
    name: "aarch64",
    sha256: "00cbdfcf917cc6c0ff6d3347d59e0ca1f7f45a6df1a428a0d6d8a78664d87444",
  },
  x64: {
    name: "x86_64",
    sha256: "2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d",
  },
});

function sha256(path) {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

export function requestsAppImage(args) {
  if (args[0] !== "build") return false;
  const option = args.find((arg) => arg.startsWith("--bundles="));
  const index = args.findIndex((arg) => arg === "--bundles" || arg === "-b");
  const value = option?.slice(option.indexOf("=") + 1) ?? (index >= 0 ? args[index + 1] : undefined);
  return value === undefined || value.split(",").some((bundle) => bundle.trim() === "appimage");
}

export function ensureAppImageRuntime(options = {}) {
  const architecture = options.arch ?? process.arch;
  const runtime = runtimes[architecture];
  if (!runtime) throw new Error(`Unsupported AppImage architecture: ${architecture}`);

  const cacheDir = options.cacheDir ?? join(root, "target", "appimage-tools");
  const runtimePath = join(cacheDir, `runtime-${runtime.name}-${runtimeRelease}`);
  const valid = () => existsSync(runtimePath) && sha256(runtimePath) === runtime.sha256;
  if (valid()) return runtimePath;

  mkdirSync(cacheDir, { recursive: true });
  rmSync(runtimePath, { force: true });
  const temporaryPath = `${runtimePath}.${process.pid}.${randomUUID()}.tmp`;
  const url = `https://github.com/AppImage/type2-runtime/releases/download/${runtimeRelease}/runtime-${runtime.name}`;
  const download = options.download ?? ((source, destination) => {
    const result = spawnSync("curl", [
      "--fail",
      "--location",
      "--retry", "3",
      "--output", destination,
      source,
    ], { stdio: "inherit" });
    if (result.error) throw result.error;
    if (result.status !== 0) throw new Error(`Failed to download AppImage runtime (curl exit ${result.status})`);
  });

  try {
    console.log(`Downloading pinned AppImage runtime for ${runtime.name}...`);
    download(url, temporaryPath);
    const actual = sha256(temporaryPath);
    if (actual !== runtime.sha256) {
      throw new Error(`AppImage runtime checksum mismatch: expected ${runtime.sha256}, got ${actual}`);
    }
    renameSync(temporaryPath, runtimePath);
  } finally {
    rmSync(temporaryPath, { force: true });
  }
  return runtimePath;
}

export function assertAppImagePrerequisites(options = {}) {
  const probe = options.probe ?? ((module, env) => spawnSync("pkg-config", ["--exists", module], { env }));
  const fileExists = options.fileExists ?? existsSync;
  const env = options.env ?? process.env;
  const missing = [];
  for (const [module, packageName] of Object.entries(appImagePkgConfigModules)) {
    const result = probe(module, env);
    if (result.error) throw result.error;
    if (result.status !== 0) missing.push(`${module} (${packageName})`);
  }
  if (!fileExists("/usr/include/xdo.h")) missing.push("xdo.h (libxdo-dev)");
  if (missing.length) {
    throw new Error([
      `Missing Linux AppImage build prerequisites: ${missing.join(", ")}.`,
      `On Ubuntu/Debian install them with: sudo apt-get install -y ${ubuntuAppImagePackages.join(" ")}`,
    ].join("\n"));
  }
}

export function tauriEnvironment(args, options = {}) {
  const platform = options.platform ?? process.platform;
  const env = { ...(options.env ?? process.env) };
  if (platform === "linux" && requestsAppImage(args)) {
    assertAppImagePrerequisites({ ...options, env });
    if (!env.LDAI_RUNTIME_FILE) env.LDAI_RUNTIME_FILE = ensureAppImageRuntime(options);
  }
  return env;
}

export function runTauri(args, options = {}) {
  const cli = join(uiDir, "node_modules", "@tauri-apps", "cli", "tauri.js");
  const result = (options.spawn ?? spawnSync)(
    process.execPath,
    [cli, ...args],
    { cwd: uiDir, stdio: "inherit", env: tauriEnvironment(args, options) },
  );
  if (result.error) throw result.error;
  return result.status ?? 1;
}

const isMain = process.argv[1] && fileURLToPath(import.meta.url) === resolve(process.argv[1]);
if (isMain) process.exitCode = runTauri(process.argv.slice(2));
