#!/usr/bin/env node
// Bump the release version in every place it must stay in sync, then stop.
// Touches: Cargo.toml ([workspace.package] version), tauri.conf.json (version),
// and the hifimule-* entries in Cargo.lock. Does NOT touch hifimule-ui/package.json,
// and performs no git operations. Usage: node scripts/bump-version.mjs <X.Y.Z>

import { readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";

const SEMVER_RE = /^(\d+)\.(\d+)\.(\d+)$/;

function fail(message) {
  console.error(`bump-version: ${message}`);
  process.exit(1);
}

const nextVersion = process.argv[2];
if (!nextVersion) {
  fail("missing version argument. Usage: node scripts/bump-version.mjs <X.Y.Z>");
}

const nextMatch = SEMVER_RE.exec(nextVersion);
if (!nextMatch) {
  fail(`"${nextVersion}" is not a valid semver version (expected X.Y.Z)`);
}

// --- Read current workspace version from Cargo.toml ------------------------
const cargoTomlPath = resolve("Cargo.toml");
const cargoToml = readFileSync(cargoTomlPath, "utf-8");

// Match the first `version = "..."` inside the [workspace.package] table.
const workspaceVersionRe = /(\[workspace\.package\][\s\S]*?\nversion\s*=\s*")([^"]+)(")/;
const currentMatch = workspaceVersionRe.exec(cargoToml);
if (!currentMatch) {
  fail("could not find [workspace.package] version in Cargo.toml");
}
const currentVersion = currentMatch[2];

// --- Guard: new version must be strictly greater than the current one ------
function toParts(v) {
  return SEMVER_RE.exec(v).slice(1, 4).map(Number);
}
function compare(a, b) {
  const [pa, pb] = [toParts(a), toParts(b)];
  for (let i = 0; i < 3; i++) {
    if (pa[i] !== pb[i]) return pa[i] - pb[i];
  }
  return 0;
}
if (compare(nextVersion, currentVersion) <= 0) {
  fail(`new version ${nextVersion} must be greater than current version ${currentVersion}`);
}

// --- Cargo.toml ------------------------------------------------------------
const nextCargoToml = cargoToml.replace(workspaceVersionRe, `$1${nextVersion}$3`);
writeFileSync(cargoTomlPath, nextCargoToml);

// --- tauri.conf.json (surgical replace to preserve CRLF and formatting) ----
// The top-level "version" is the first "version" key in the file; a non-global
// replace targets only that first occurrence and leaves line endings intact.
const tauriConfPath = resolve("hifimule-ui", "src-tauri", "tauri.conf.json");
const tauriConf = readFileSync(tauriConfPath, "utf-8");
const tauriVersionRe = /("version"\s*:\s*")[^"]+(")/;
if (!tauriVersionRe.test(tauriConf)) {
  fail("could not find version in tauri.conf.json");
}
writeFileSync(tauriConfPath, tauriConf.replace(tauriVersionRe, `$1${nextVersion}$2`));

// --- Cargo.lock (every hifimule-* member tracks the workspace version) -----
const cargoLockPath = resolve("Cargo.lock");
const cargoLock = readFileSync(cargoLockPath, "utf-8");
const lockMemberRe = /(name = "hifimule-[^"]*"\nversion = ")[^"]+(")/g;
const lockMembers = cargoLock.match(lockMemberRe) ?? [];
if (lockMembers.length === 0) {
  fail("could not find any hifimule-* entries in Cargo.lock");
}
const nextCargoLock = cargoLock.replace(lockMemberRe, `$1${nextVersion}$2`);
writeFileSync(cargoLockPath, nextCargoLock);

// --- Summary ---------------------------------------------------------------
console.log(`Bumped ${currentVersion} -> ${nextVersion}`);
console.log("  Cargo.toml                       [workspace.package] version");
console.log("  hifimule-ui/src-tauri/tauri.conf.json  version");
console.log(`  Cargo.lock                       ${lockMembers.length} hifimule-* entr${lockMembers.length === 1 ? "y" : "ies"}`);
console.log("\nNo files were committed. Review, then commit/tag/push manually.");
