#!/usr/bin/env node
// Bundles macOS Homebrew libmtp dependencies beside the Tauri app and rewrites
// daemon load commands so local `npm run tauri build` matches release CI.

import { execFileSync } from "node:child_process";
import {
  copyFileSync,
  existsSync,
  lstatSync,
  mkdirSync,
  readdirSync,
} from "node:fs";
import { basename, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = fileURLToPath(new URL(".", import.meta.url));
const projectRoot = resolve(__dirname, "..");
const libDir = join(projectRoot, "hifimule-ui", "src-tauri", "bundled-libs");
const sidecarsDir = join(projectRoot, "hifimule-ui", "src-tauri", "sidecars");
const bundledLoadPrefix = "@executable_path/../Resources/bundled-libs";
const homebrewPrefixes = ["/opt/homebrew", "/usr/local"];

if (process.platform !== "darwin") {
  process.exit(0);
}

function run(command, args, options = {}) {
  const output = execFileSync(command, args, {
    encoding: "utf-8",
    stdio: options.stdio ?? ["ignore", "pipe", "pipe"],
  });

  return typeof output === "string" ? output.trim() : "";
}

function walkFiles(dir) {
  const entries = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) {
      entries.push(...walkFiles(path));
    } else if (entry.isFile()) {
      entries.push(path);
    }
  }
  return entries;
}

function dylibDependencies(path) {
  return run("otool", ["-L", path])
    .split("\n")
    .slice(1)
    .map((line) => line.trim().split(/\s+/)[0])
    .filter(Boolean);
}

function isHomebrewPath(path) {
  return homebrewPrefixes.some((prefix) => path.startsWith(`${prefix}/`));
}

function findLibmtpDylib() {
  const libmtpPrefix = run("brew", ["--prefix", "libmtp"]);
  const candidates = walkFiles(join(libmtpPrefix, "lib"))
    .filter((path) => /^libmtp.*\.dylib$/.test(basename(path)))
    .filter((path) => !lstatSync(path).isSymbolicLink());

  if (candidates.length === 0) {
    throw new Error(`No libmtp dylib found under ${libmtpPrefix}/lib`);
  }

  return candidates[0];
}

const copied = new Map();

function copyBrewDylib(source) {
  const name = basename(source);
  if (copied.has(name)) {
    return copied.get(name);
  }

  const target = join(libDir, name);
  copyFileSync(source, target);
  run("chmod", ["u+w", target]);
  copied.set(name, target);

  run("install_name_tool", [
    "-id",
    `${bundledLoadPrefix}/${name}`,
    target,
  ]);

  for (const dep of dylibDependencies(target).filter(isHomebrewPath)) {
    if (existsSync(dep)) {
      copyBrewDylib(dep);
    }
  }

  return target;
}

function rewriteHomebrewDependencies(path) {
  for (const dep of dylibDependencies(path).filter(isHomebrewPath)) {
    const depName = basename(dep);
    if (copied.has(depName)) {
      run("install_name_tool", [
        "-change",
        dep,
        `${bundledLoadPrefix}/${depName}`,
        path,
      ]);
    }
  }
}

function sign(path) {
  run("codesign", ["--force", "--sign", "-", path], { stdio: "inherit" });
}

mkdirSync(libDir, { recursive: true });

const libmtpDylib = findLibmtpDylib();
copyBrewDylib(libmtpDylib);

for (const dylib of [...copied.values()]) {
  rewriteHomebrewDependencies(dylib);
}

const sidecars = existsSync(sidecarsDir)
  ? walkFiles(sidecarsDir).filter((path) => /^hifimule-daemon-.*apple-darwin$/.test(basename(path)))
  : [];

for (const sidecar of sidecars) {
  rewriteHomebrewDependencies(sidecar);
}

for (const dylib of [...copied.values()]) {
  sign(dylib);
}

for (const sidecar of sidecars) {
  sign(sidecar);
}

for (const sidecar of sidecars) {
  const homebrewDeps = dylibDependencies(sidecar).filter(isHomebrewPath);
  if (homebrewDeps.length > 0) {
    throw new Error(
      `Homebrew dylib path still present in ${sidecar}:\n${homebrewDeps.join("\n")}`,
    );
  }
}

for (const dylib of [...copied.values()]) {
  const homebrewDeps = dylibDependencies(dylib).filter(isHomebrewPath);
  if (homebrewDeps.length > 0) {
    throw new Error(
      `Homebrew dylib path still present in ${dylib}:\n${homebrewDeps.join("\n")}`,
    );
  }
}

console.log(
  `Bundled macOS dylibs: ${[...copied.keys()].sort().join(", ")}`,
);
