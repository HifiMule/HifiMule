import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { pathToFileURL } from "node:url";
import { join, resolve } from "node:path";
import test from "node:test";
import { runDaemonBuild } from "../build-daemon.mjs";
import { audioRuntimeVerification } from "../verify-audio-runtime.mjs";

const root = resolve(import.meta.dirname, "../..");

test("daemon CLI preserves a failing Cargo subprocess exit code", (t) => {
  const scratch = mkdtempSync(join(tmpdir(), "hifimule-daemon-exit-"));
  t.after(() => rmSync(scratch, { recursive: true, force: true }));
  const preload = join(scratch, "preload.mjs");
  writeFileSync(preload, `
    import childProcess from "node:child_process";
    import { syncBuiltinESMExports } from "node:module";
    Object.defineProperty(process, "platform", { value: "darwin" });
    childProcess.execFileSync = (command) => {
      if (command === "rustc") return "host: aarch64-apple-darwin\\n";
      if (command === "node") return "";
      if (command === "cargo") throw Object.assign(new Error("fixture Cargo failure"), { status: 101 });
      throw new Error("Unexpected subprocess: " + command);
    };
    syncBuiltinESMExports();
  `);
  const result = spawnSync(process.execPath, ["--import", pathToFileURL(preload).href, join(root, "scripts/build-daemon.mjs"), "test", "-p", "hifimule-daemon"], { encoding: "utf8" });
  assert.equal(result.status, 101, result.stderr);
  assert.match(result.stderr, /fixture Cargo failure/);
});

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
    env: { PATH: "/controlled/bin", PKG_CONFIG_PATH: "/prior/pc", LD_LIBRARY_PATH: "/prior/lib", FFMPEG_DIR: "/old/ffmpeg8" },
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
  assert.equal(cargo.options.env.FFMPEG_DIR, "/controlled/ffmpeg");
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

test("Windows wrapper preserves Cargo search paths for every environment key casing", () => {
  for (const key of ["Path", "PATH", "path"]) {
    const calls = [];
    const original = { [key]: "C:\\Rust\\bin;C:\\Windows\\System32", KEEP: "yes" };
    runDaemonBuild(["test", "-p", "hifimule-daemon"], {
      platform: "win32",
      env: original,
      execFileSync: executor("x86_64-pc-windows-msvc", calls),
      ensureWindowsAudioRuntime: () => "C:\\ffmpeg",
    });
    const cargoEnv = calls.find((call) => call.command === "cargo").options.env;
    assert.deepEqual(Object.keys(cargoEnv).filter((name) => name.toUpperCase() === "PATH"), ["PATH"]);
    assert.equal(cargoEnv.PATH, `${join("C:\\ffmpeg", "bin")};${original[key]}`);
    assert.equal(cargoEnv.KEEP, "yes");
    assert.deepEqual(original, { [key]: "C:\\Rust\\bin;C:\\Windows\\System32", KEEP: "yes" });
  }
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
  const exportPrefix = prepare.indexOf("buildEnv.FFMPEG_DIR = audioPrefix", ensure);
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
  const privateSearch = build.indexOf("cargo:rustc-link-search=native=");
  assert.ok(privateSearch >= 0 && privateSearch < build.indexOf('pkg_config::probe_library("libmtp")'));
  assert.match(build, /cargo:rerun-if-env-changed=HIFIMULE_FFMPEG_PREFIX/);
});
