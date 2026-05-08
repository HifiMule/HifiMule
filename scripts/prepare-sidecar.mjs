#!/usr/bin/env node
// Builds the jellyfinsync-daemon and copies it to the Tauri sidecars directory
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
const uiDir = join(projectRoot, "jellyfinsync-ui");
const sidecarsDir = join(projectRoot, "jellyfinsync-ui", "src-tauri", "sidecars");

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
  console.log("jellyfinsync-ui/node_modules is missing; running npm install...");
  execSync("npm install", {
    cwd: uiDir,
    stdio: "inherit",
  });
}

// Build the daemon in release mode
console.log("Building jellyfinsync-daemon...");
execSync("cargo build --release -p jellyfinsync-daemon", {
  cwd: projectRoot,
  stdio: "inherit",
});

// Determine source and destination paths
const ext = process.platform === "win32" ? ".exe" : "";
const sourceBinary = join(projectRoot, "target", "release", `jellyfinsync-daemon${ext}`);
const destBinary = join(sidecarsDir, `jellyfinsync-daemon-${targetTriple}${ext}`);
const tempBinary = `${destBinary}.tmp`;

// Ensure sidecars directory exists
mkdirSync(sidecarsDir, { recursive: true });

for (const entry of readdirSync(sidecarsDir, { withFileTypes: true })) {
  if (entry.isFile() && entry.name.startsWith("jellyfinsync-daemon-")) {
    rmSync(join(sidecarsDir, entry.name), { force: true });
  }
}

try {
  rmSync(tempBinary, { force: true });
  copyFileSync(sourceBinary, tempBinary);
  renameSync(tempBinary, destBinary);
} catch (error) {
  rmSync(tempBinary, { force: true });
  rmSync(destBinary, { force: true });
  throw error;
}

console.log(`Sidecar copied: ${destBinary}`);
