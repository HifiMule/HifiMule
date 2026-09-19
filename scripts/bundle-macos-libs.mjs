#!/usr/bin/env node
// Bundles the controlled FFmpeg runtime plus Homebrew libmtp dependencies and
// rewrites daemon load commands so no developer path reaches a shipping app.

import { execFileSync } from "node:child_process";
import {
  copyFileSync,
  existsSync,
  lstatSync,
  mkdirSync,
  readdirSync,
  rmSync,
  writeFileSync,
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
const audioPrefix = process.env.HIFIMULE_FFMPEG_PREFIX;
if (!audioPrefix) throw new Error("HIFIMULE_FFMPEG_PREFIX is required for macOS packaging");
const privatePrefixes = [
  ...homebrewPrefixes,
  resolve(audioPrefix),
  join(projectRoot, "target", "audio-runtime"),
];

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

function isPrivatePath(path) {
  return privatePrefixes.some((prefix) => path.startsWith(`${prefix}/`));
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

function findFfmpegDylibs() {
  const prefix = resolve(audioPrefix);
  const required = ["libavcodec", "libavformat", "libavutil", "libswresample"];
  return required.flatMap((stem) => {
    const match = walkFiles(join(prefix, "lib"))
      .find((path) => basename(path).startsWith(`${stem}.`) && path.endsWith(".dylib") && !lstatSync(path).isSymbolicLink());
    if (!match) throw new Error(`Controlled FFmpeg library ${stem} not found under ${prefix}/lib`);
    const major = basename(match).split(".")[1];
    const abiAlias = join(prefix, "lib", `${stem}.${major}.dylib`);
    if (!existsSync(abiAlias)) throw new Error(`Controlled FFmpeg ABI alias is missing: ${abiAlias}`);
    return [match, abiAlias];
  });
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
    `@loader_path/${name}`,
    target,
  ]);

  for (const dep of dylibDependencies(target).filter(isPrivatePath)) {
    if (existsSync(dep)) {
      copyBrewDylib(dep);
    }
  }

  return target;
}

function rewritePrivateDependencies(path, prefix) {
  for (const dep of dylibDependencies(path).filter(isPrivatePath)) {
    const depName = basename(dep);
    if (copied.has(depName)) {
      run("install_name_tool", [
        "-change",
        dep,
        `${prefix}/${depName}`,
        path,
      ]);
    }
  }
}

function sign(path) {
  run("codesign", ["--force", "--sign", "-", path], { stdio: "inherit" });
}

// The directory is a packaging staging area. Reusing it would retain dylibs
// from a previous FFmpeg/Homebrew closure and silently ship both runtimes.
rmSync(libDir, { recursive: true, force: true });
mkdirSync(libDir, { recursive: true });
writeFileSync(join(libDir, ".gitkeep"), "");

const libmtpDylib = findLibmtpDylib();
copyBrewDylib(libmtpDylib);
for (const dylib of findFfmpegDylibs()) copyBrewDylib(dylib);

const sidecars = existsSync(sidecarsDir)
  ? walkFiles(sidecarsDir).filter((path) => /^hifimule-daemon-.*apple-darwin$/.test(basename(path)))
  : [];

// Linkers commonly record the ABI symlink (for example libavformat.63.dylib)
// while the initial discovery above selects the fully-versioned regular file.
// Copy every private direct dependency under its recorded basename before
// rewriting so the packaged load command always has a matching resource.
for (const sidecar of sidecars) {
  for (const dependency of dylibDependencies(sidecar).filter(isPrivatePath)) {
    if (existsSync(dependency)) copyBrewDylib(dependency);
  }
}

for (const dylib of [...copied.values()]) {
  rewritePrivateDependencies(dylib, "@loader_path");
}

for (const sidecar of sidecars) {
  rewritePrivateDependencies(sidecar, bundledLoadPrefix);
}

for (const dylib of [...copied.values()]) {
  sign(dylib);
}

for (const sidecar of sidecars) {
  sign(sidecar);
}

for (const sidecar of sidecars) {
  const developerDeps = dylibDependencies(sidecar).filter(isPrivatePath);
  if (developerDeps.length > 0) {
    throw new Error(
      `Developer dylib path still present in ${sidecar}:\n${developerDeps.join("\n")}`,
    );
  }
}

for (const dylib of [...copied.values()]) {
  const developerDeps = dylibDependencies(dylib).filter(isPrivatePath);
  if (developerDeps.length > 0) {
    throw new Error(
      `Developer dylib path still present in ${dylib}:\n${developerDeps.join("\n")}`,
    );
  }
}

console.log(
  `Bundled macOS dylibs: ${[...copied.keys()].sort().join(", ")}`,
);
