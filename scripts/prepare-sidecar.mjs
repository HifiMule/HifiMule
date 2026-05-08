#!/usr/bin/env node
// Builds the hifimule-daemon and copies it to the Tauri sidecars directory
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

// Build the daemon in release mode
console.log("Building hifimule-daemon...");
execSync("cargo build --release -p hifimule-daemon", {
  cwd: projectRoot,
  stdio: "inherit",
});

// Determine source and destination paths
const ext = process.platform === "win32" ? ".exe" : "";
const sourceBinary = join(projectRoot, "target", "release", `hifimule-daemon${ext}`);
const destBinary = join(sidecarsDir, `hifimule-daemon-${targetTriple}${ext}`);
const tempBinary = `${destBinary}.tmp`;

// Ensure sidecars directory exists
mkdirSync(sidecarsDir, { recursive: true });

try {
  rmSync(tempBinary, { force: true });
  copyFileSync(sourceBinary, tempBinary);
  renameSync(tempBinary, destBinary);
} catch (error) {
  rmSync(tempBinary, { force: true });
  throw error;
}

// Remove stale sidecars for other architectures after the new binary is in place
for (const entry of readdirSync(sidecarsDir, { withFileTypes: true })) {
  const fullPath = join(sidecarsDir, entry.name);
  if (entry.isFile() && entry.name.startsWith("hifimule-daemon-") && fullPath !== destBinary) {
    rmSync(fullPath, { force: true });
  }
}

console.log(`Sidecar copied: ${destBinary}`);
