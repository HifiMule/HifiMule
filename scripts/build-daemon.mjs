#!/usr/bin/env node
import { execFileSync } from "node:child_process";
import { join, posix, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { ensureLinuxAudioRuntime, linuxBuildEnvironment } from "./linux-audio-runtime.mjs";
import { ensureWindowsAudioRuntime, prependWindowsPath } from "./windows-audio-runtime.mjs";
import {
  withAudioRuntimeVerification,
  withoutAudioRuntimeVerification,
} from "./verify-audio-runtime.mjs";

const root = resolve(import.meta.dirname, "..");
export const buildDaemonUsage = "Usage: node scripts/build-daemon.mjs [cargo arguments]\nDefault: cargo build --release -p hifimule-daemon\nExample: npm run build:daemon -- build -p hifimule-daemon";

function selectedTarget(cargoArgs, execute, env) {
  const inline = cargoArgs.find((arg) => arg.startsWith("--target="));
  if (inline) {
    const target = inline.slice("--target=".length);
    if (!target) throw new Error(`Cargo --target cannot be empty.\n${buildDaemonUsage}`);
    return target;
  }
  const targetIndex = cargoArgs.indexOf("--target");
  if (targetIndex >= 0) {
    const target = cargoArgs[targetIndex + 1];
    if (!target || target.startsWith("-")) throw new Error(`Cargo --target requires a target triple.\n${buildDaemonUsage}`);
    return target;
  }
  if (env.CARGO_BUILD_TARGET) return env.CARGO_BUILD_TARGET;
  const output = execute("rustc", ["-vV"], { encoding: "utf8", env });
  const target = output.match(/^host: (\S+)$/m)?.[1];
  if (!target) throw new Error(`Could not determine the Rust host target.\n${buildDaemonUsage}`);
  return target;
}

export function runDaemonBuild(cargoArgs = [], options = {}) {
  const execute = options.execFileSync ?? execFileSync;
  const platform = options.platform ?? process.platform;
  const dependencies = {
    ensureLinuxAudioRuntime: options.ensureLinuxAudioRuntime ?? ensureLinuxAudioRuntime,
    linuxBuildEnvironment: options.linuxBuildEnvironment ?? linuxBuildEnvironment,
    ensureWindowsAudioRuntime: options.ensureWindowsAudioRuntime ?? ensureWindowsAudioRuntime,
  };
  let env = withoutAudioRuntimeVerification(options.env ?? process.env);
  const target = selectedTarget(cargoArgs, execute, env);

  if (platform === "linux") {
    const prefix = dependencies.ensureLinuxAudioRuntime(target);
    env = dependencies.linuxBuildEnvironment(target, env);
    env.HIFIMULE_FFMPEG_PREFIX = prefix;
    // ffmpeg-sys-next checks FFMPEG_DIR before pkg-config.
    env.FFMPEG_DIR = prefix;
    env.PKG_CONFIG_PATH = [posix.join(prefix, "lib/pkgconfig"), env.PKG_CONFIG_PATH].filter(Boolean).join(":");
    env.LD_LIBRARY_PATH = [posix.join(prefix, "lib"), env.LD_LIBRARY_PATH].filter(Boolean).join(":");
    execute("node", ["scripts/verify-audio-runtime.mjs", "--prefix", prefix], { cwd: root, stdio: "inherit", env });
    env = withAudioRuntimeVerification(env);
  } else if (platform === "win32") {
    env.FFMPEG_DIR = dependencies.ensureWindowsAudioRuntime(target, { env });
    env = prependWindowsPath(env, join(env.FFMPEG_DIR, "bin"));
    env = withAudioRuntimeVerification(env);
  } else if (platform === "darwin") {
    execute("node", ["scripts/verify-audio-runtime.mjs"], { cwd: root, stdio: "inherit", env });
    env = withAudioRuntimeVerification(env);
  } else {
    throw new Error(`Unsupported daemon build platform: ${platform}\n${buildDaemonUsage}`);
  }

  const args = cargoArgs.length ? cargoArgs : ["build", "--release", "-p", "hifimule-daemon"];
  execute("cargo", args, { cwd: root, stdio: "inherit", env });
  return { target, args, env };
}

if (process.argv[1] && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url))) {
  const args = process.argv.slice(2);
  if (args.length === 1 && ["-h", "--help"].includes(args[0])) console.log(buildDaemonUsage);
  else {
    try { runDaemonBuild(args); }
    catch (error) {
      console.error(error);
      process.exitCode = Number.isInteger(error.status) && error.status > 0 ? error.status : 1;
    }
  }
}
