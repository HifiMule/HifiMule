#!/usr/bin/env node
// Builds the jellyfinsync-daemon and copies it to the Tauri sidecars directory
// with the correct target-triple naming convention required by Tauri v2.

import { execSync } from "node:child_process";
import { copyFileSync, mkdirSync } from "node:fs";
import { join, resolve } from "node:path";

const projectRoot = resolve(import.meta.dirname, "..");
const sidecarsDir = join(projectRoot, "jellyfinsync-ui", "src-tauri", "sidecars");

// Get the current Rust target triple
const rustcOutput = execSync("rustc -vV", { encoding: "utf-8" });
const targetTriple = rustcOutput
  .split("\n")
  .find((line) => line.startsWith("host:"))
  ?.split(": ")[1]
  ?.trim();

if (!targetTriple) {
  console.error("Failed to determine Rust target triple");
  process.exit(1);
}

console.log(`Target triple: ${targetTriple}`);

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

// Ensure sidecars directory exists
mkdirSync(sidecarsDir, { recursive: true });

// Copy the binary with the target-triple name
copyFileSync(sourceBinary, destBinary);
console.log(`Sidecar copied: ${destBinary}`);
