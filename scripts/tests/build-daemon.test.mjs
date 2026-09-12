import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import test from "node:test";
import { runDaemonBuild } from "../build-daemon.mjs";
import { audioRuntimeVerification } from "../verify-audio-runtime.mjs";

const root = resolve(import.meta.dirname, "../..");

function executor(target, calls) {
  return (command, args, options) => {
    calls.push({ command, args, options });
    if (command === "rustc") return `rustc 1.93.0\nhost: ${target}\n`;
    return "";
  };
}

test("daemon wrapper provisions Linux and passes controlled native environment to Cargo", () => {
  const calls = [];
  const result = runDaemonBuild([], {
    platform: "linux",
    env: { PATH: "/controlled/bin", PKG_CONFIG_PATH: "/prior/pc", LD_LIBRARY_PATH: "/prior/lib" },
    execFileSync: executor("aarch64-unknown-linux-gnu", calls),
    ensureLinuxAudioRuntime: (target) => {
      assert.equal(target, "aarch64-unknown-linux-gnu");
      return "/controlled/ffmpeg";
    },
    linuxBuildEnvironment: (target, env) => ({ ...env, LIBCLANG_PATH: `/llvm/${target}` }),
  });
  const cargo = calls.find((call) => call.command === "cargo");
  assert.deepEqual(cargo.args, ["build", "--release", "-p", "hifimule-daemon"]);
  assert.equal(cargo.options.env.HIFIMULE_FFMPEG_PREFIX, "/controlled/ffmpeg");
  assert.equal(cargo.options.env.PKG_CONFIG_PATH, "/controlled/ffmpeg/lib/pkgconfig:/prior/pc");
  assert.equal(cargo.options.env.LD_LIBRARY_PATH, "/controlled/ffmpeg/lib:/prior/lib");
  assert.equal(
    cargo.options.env[audioRuntimeVerification.environmentVariable],
    audioRuntimeVerification.value,
  );
  const verify = calls.find((call) => call.command === "node");
  assert.deepEqual(verify.args, ["scripts/verify-audio-runtime.mjs", "--prefix", "/controlled/ffmpeg"]);
  assert.equal(verify.options.env[audioRuntimeVerification.environmentVariable], undefined);
  assert.ok(calls.indexOf(verify) < calls.indexOf(cargo));
  assert.equal(result.target, "aarch64-unknown-linux-gnu");
});

test("daemon wrapper passes custom Cargo arguments through unchanged", () => {
  const calls = [];
  runDaemonBuild(["check", "-p", "hifimule-daemon"], {
    platform: "linux",
    execFileSync: executor("x86_64-unknown-linux-gnu", calls),
    ensureLinuxAudioRuntime: () => "/controlled/ffmpeg",
    linuxBuildEnvironment: (_target, env) => env,
  });
  assert.deepEqual(calls.find((call) => call.command === "cargo").args, ["check", "-p", "hifimule-daemon"]);
});

test("daemon wrapper ensures Windows FFmpeg and exports FFMPEG_DIR before Cargo", () => {
  const calls = [];
  let ensured;
  runDaemonBuild([], {
    platform: "win32",
    env: {},
    execFileSync: executor("aarch64-pc-windows-msvc", calls),
    ensureWindowsAudioRuntime: (target, options) => { ensured = { target, options }; return "C:\\ffmpeg"; },
  });
  assert.equal(ensured.target, "aarch64-pc-windows-msvc");
  const cargoEnv = calls.find((call) => call.command === "cargo").options.env;
  assert.equal(cargoEnv.FFMPEG_DIR, "C:\\ffmpeg");
  assert.match(cargoEnv.PATH, /^C:\\ffmpeg[/\\]bin(?:;|$)/);
  assert.equal(cargoEnv[audioRuntimeVerification.environmentVariable], audioRuntimeVerification.value);
});

test("daemon wrapper provisions the explicit Cargo target instead of the host", () => {
  const calls = [];
  let ensuredTarget;
  const result = runDaemonBuild(["build", "--target", "aarch64-pc-windows-msvc"], {
    platform: "win32",
    env: { PATH: "C:\\Windows\\System32" },
    execFileSync: executor("x86_64-pc-windows-msvc", calls),
    ensureWindowsAudioRuntime: (target) => { ensuredTarget = target; return "C:\\ffmpeg-arm64"; },
  });
  assert.equal(ensuredTarget, "aarch64-pc-windows-msvc");
  assert.equal(result.target, "aarch64-pc-windows-msvc");
});

test("daemon wrapper retains macOS runtime verification", () => {
  const calls = [];
  runDaemonBuild([], { platform: "darwin", execFileSync: executor("aarch64-apple-darwin", calls) });
  assert.ok(calls.some((call) => call.command === "node" && call.args[0] === "scripts/verify-audio-runtime.mjs"));
  const cargo = calls.find((call) => call.command === "cargo");
  assert.equal(cargo.options.env[audioRuntimeVerification.environmentVariable], audioRuntimeVerification.value);
});

test("daemon wrapper never trusts an inherited marker or reaches Cargo after verification failure", () => {
  const calls = [];
  const execute = (command, args, options) => {
    calls.push({ command, args, options });
    if (command === "rustc") return "rustc 1.93.0\nhost: aarch64-unknown-linux-gnu\n";
    if (command === "node") throw new Error("exact ABI validation failed");
    return "";
  };
  assert.throws(
    () => runDaemonBuild([], {
      platform: "linux",
      env: { [audioRuntimeVerification.environmentVariable]: audioRuntimeVerification.value },
      execFileSync: execute,
      ensureLinuxAudioRuntime: () => "/controlled/ffmpeg",
      linuxBuildEnvironment: (_target, env) => env,
    }),
    /exact ABI validation failed/,
  );
  const verify = calls.find((call) => call.command === "node");
  assert.equal(verify.options.env[audioRuntimeVerification.environmentVariable], undefined);
  assert.equal(calls.some((call) => call.command === "cargo"), false);
});

test("root build:daemon script uses the supported native wrapper", () => {
  const pkg = JSON.parse(readFileSync(join(root, "package.json"), "utf8"));
  assert.equal(pkg.scripts["build:daemon"], "node scripts/build-daemon.mjs");
});

test("sidecar and Build workflow export auto-provisioned Windows FFMPEG_DIR", () => {
  const prepare = readFileSync(join(root, "scripts/prepare-sidecar.mjs"), "utf8");
  const ensure = prepare.indexOf("ensureWindowsAudioRuntime(targetTriple");
  const exportPrefix = prepare.indexOf("buildEnv.FFMPEG_DIR = audioPrefix");
  const exportPath = prepare.indexOf("join(audioPrefix, \"bin\")");
  const cargo = prepare.indexOf("cargo build --release -p hifimule-daemon");
  assert.ok(ensure >= 0 && exportPrefix > ensure && exportPath > exportPrefix && cargo > exportPath);
  const workflow = readFileSync(join(root, ".github/workflows/build.yml"), "utf8");
  assert.match(workflow, /windows-audio-runtime\.mjs env \$TargetTriple \$env:GITHUB_ENV/);
});

test("sidecar marks Cargo only after the platform runtime verification succeeds", () => {
  const prepare = readFileSync(join(root, "scripts/prepare-sidecar.mjs"), "utf8");
  const sanitize = prepare.indexOf("withoutAudioRuntimeVerification(process.env)");
  const windowsVerify = prepare.indexOf("stageWindowsAudioRuntime(audioPrefix, targetTriple)");
  const verify = prepare.indexOf('"scripts/verify-audio-runtime.mjs"');
  const windowsMark = prepare.indexOf("buildEnv = withAudioRuntimeVerification(buildEnv)", windowsVerify);
  const unixMark = prepare.indexOf("buildEnv = withAudioRuntimeVerification(buildEnv)", verify);
  const cargo = prepare.indexOf("cargo build --release -p hifimule-daemon");
  assert.ok(sanitize >= 0);
  assert.ok(windowsVerify > sanitize && windowsMark > windowsVerify && cargo > windowsMark);
  assert.ok(verify > sanitize && unixMark > verify && cargo > unixMark);
});

test("Rust build gate exact-matches the shared marker and rejects unverified raw Cargo", () => {
  const build = readFileSync(join(root, "hifimule-daemon/build.rs"), "utf8");
  assert.match(build, new RegExp(`const VERIFIED_ENV: &str = "${audioRuntimeVerification.environmentVariable}"`));
  assert.match(build, new RegExp(`const VERIFIED_VALUE: &str = "${audioRuntimeVerification.value.replaceAll(".", "\\.")}"`));
  assert.match(build, /if !runtime_was_verified/);
  assert.doesNotMatch(build, /pkg_config::Config::new\(\)/);
  assert.match(build, /pkg_config::probe_library\("libmtp"\)/);
});
