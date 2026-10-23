#!/usr/bin/env node
// Builds the daemon and mediacheck and copies them to the Tauri sidecars directory
// with the correct target-triple naming convention required by Tauri v2.

import { execSync } from "node:child_process";
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  readdirSync,
  renameSync,
  rmSync,
} from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { bundleLinuxAudioRuntime } from "./linux-audio-runtime.mjs";
import { stageWindowsAudioRuntime } from "./windows-audio-runtime.mjs";
import { runDaemonBuild } from "./build-daemon.mjs";

const __dirname = fileURLToPath(new URL('.', import.meta.url));
const projectRoot = resolve(__dirname, "..");
const uiDir = join(projectRoot, "hifimule-ui");
const sidecarsDir = join(projectRoot, "hifimule-ui", "src-tauri", "sidecars");

if (process.env.HIFIMULE_SKIP_SIDECAR_PREP === "1") {
  console.log("Skipping sidecar preparation because HIFIMULE_SKIP_SIDECAR_PREP=1");
  process.exit(0);
}

// Get the current Rust target triple
const rustcOutput = execSync("rustc -vV", { encoding: "utf-8" });
const hostMatch = rustcOutput.match(/^host: (\S+)$/m);
const targetTriple = hostMatch?.[1];

if (!targetTriple) {
  console.error("Failed to determine Rust target triple");
  console.error(rustcOutput);
  process.exit(1);
}

console.log(`Target triple: ${targetTriple}`);

if (!existsSync(join(uiDir, "node_modules"))) {
  console.log("hifimule-ui/node_modules is missing; running pnpm install...");
  execSync("pnpm install", {
    cwd: uiDir,
    stdio: "inherit",
  });
}

console.log("Building hifimule-daemon and mediacheck...");
const { env: buildEnv } = runDaemonBuild();
const audioPrefix = buildEnv.FFMPEG_DIR;
if (process.platform === "win32") {
  stageWindowsAudioRuntime(audioPrefix, targetTriple);
}

// Determine source and destination paths
const ext = process.platform === "win32" ? ".exe" : "";
const binaries = ["hifimule-daemon", "mediacheck"];
const destinations = [];
mkdirSync(sidecarsDir, { recursive: true });

for (const name of binaries) {
  const sourceBinary = join(projectRoot, "target", "release", `${name}${ext}`);
  const destBinary = join(sidecarsDir, `${name}-${targetTriple}${ext}`);
  const tempBinary = `${destBinary}.tmp`;
  try {
    rmSync(tempBinary, { force: true });
    copyFileSync(sourceBinary, tempBinary);
    renameSync(tempBinary, destBinary);
  } catch (error) {
    rmSync(tempBinary, { force: true });
    throw error;
  }
  destinations.push(destBinary);
  console.log(`Sidecar copied: ${destBinary}`);
}

// Remove stale architectures only after both new binaries are in place.
for (const entry of readdirSync(sidecarsDir, { withFileTypes: true })) {
  const fullPath = join(sidecarsDir, entry.name);
  if (entry.isFile() && binaries.some((name) => entry.name.startsWith(`${name}-`)) && !destinations.includes(fullPath)) {
    rmSync(fullPath, { force: true });
  }
}

if (process.platform === "linux") {
  bundleLinuxAudioRuntime(audioPrefix, destinations, targetTriple);
}

if (process.platform === "darwin") {
  console.log("Bundling macOS libmtp dylibs...");
  execSync("node scripts/bundle-macos-libs.mjs", {
    cwd: projectRoot,
    stdio: "inherit",
    env: buildEnv,
  });
}
