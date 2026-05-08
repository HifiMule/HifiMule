#!/usr/bin/env node
// CI guardrail: keep macOS runtime compatibility visible when native deps change.

import { readFileSync } from "node:fs";
import { resolve } from "node:path";

const configPath = resolve("jellyfinsync-ui", "src-tauri", "tauri.conf.json");
const config = JSON.parse(readFileSync(configPath, "utf-8"));
const minimumSystemVersion = config.bundle?.macOS?.minimumSystemVersion;

if (!minimumSystemVersion) {
  console.error("bundle.macOS.minimumSystemVersion must be set in tauri.conf.json");
  process.exit(1);
}

console.log(
  `macOS minimumSystemVersion is ${minimumSystemVersion}; verify this when changing macOS native dependencies.`,
);
