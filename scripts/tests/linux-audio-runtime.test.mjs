import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import test from "node:test";
import { verifyLinuxAudioLinkage, acquireLinuxAudioRuntimeLock, linuxBuildEnvironment, linuxBuildPackages, preflightLinuxBuild, requiresHostAudioVerification, validateLinuxRuntimeReceipt, writeLinuxBuildEnvironment } from "../linux-audio-runtime.mjs";
import { audioRuntimeVerification } from "../verify-audio-runtime.mjs";

const root = resolve(import.meta.dirname, "../..");
const manifest = JSON.parse(readFileSync(join(root, "hifimule-daemon/audio-runtime.json"), "utf8"));
const target = "x86_64-unknown-linux-gnu";
const receipt = { schemaVersion: 1, targetTriple: target, ffmpegRelease: manifest.ffmpegRelease, sourceSha256: manifest.sourceSha256, configureFlags: ["--disable-autodetect", ...manifest.configureFlags], abiVersions: manifest.abiVersions };

function prefixWithReceipt(value) {
  const prefix = mkdtempSync(join(tmpdir(), "hifimule-ffmpeg-receipt-"));
  writeFileSync(join(prefix, ".hifimule-audio-runtime.json"), JSON.stringify(value));
  return prefix;
}
function fakePcPrefix(overrides = {}) {
  const prefix = mkdtempSync(join(tmpdir(), "hifimule-ffmpeg-pc-"));
  const pcDir = join(prefix, "lib/pkgconfig"); mkdirSync(pcDir, { recursive: true });
  for (const [library, version] of Object.entries({ ...manifest.abiVersions, ...overrides })) {
    const name = `lib${library}`;
    writeFileSync(join(pcDir, `${name}.pc`), [`prefix=${prefix}`, "libdir=${prefix}/lib", "includedir=${prefix}/include", "", `Name: ${name}`, `Description: ${name} fixture`, `Version: ${version}`, `Libs: -L${prefix}/lib -l${library}`, ""].join("\n"));
  }
  return prefix;
}

test("accepts only a complete manifest-derived receipt", () => {
  const prefix = prefixWithReceipt(receipt);
  assert.deepEqual(validateLinuxRuntimeReceipt(prefix, target), receipt);
});
test("rejects partial and stale receipts", () => {
  assert.throws(() => validateLinuxRuntimeReceipt(prefixWithReceipt({ schemaVersion: 1 }), target), /stale/);
  assert.throws(() => validateLinuxRuntimeReceipt(prefixWithReceipt({ ...receipt, sourceSha256: "0".repeat(64) }), target), /stale/);
});
test("standalone verifier rejects Ubuntu FFmpeg 8 from the selected prefix", { skip: process.platform === "win32" }, () => {
  const prefix = fakePcPrefix({ avcodec: "62.11.100" });
  const bin = mkdtempSync(join(tmpdir(), "hifimule-fake-pkg-config-"));
  const pkgConfig = join(bin, "pkg-config");
  writeFileSync(pkgConfig, "#!/bin/sh\ncase \"$1:$2\" in\n  --modversion:libavcodec) echo 62.11.100 ;;\n  --modversion:libavformat) echo 63.1.101 ;;\n  --modversion:libavutil) echo 61.1.101 ;;\n  --modversion:libswresample) echo 7.1.101 ;;\n  --variable=libdir:*) printf '%s\\n' \"$HIFIMULE_TEST_LIBDIR\" ;;\n  *) exit 1 ;;\nesac\n");
  chmodSync(pkgConfig, 0o755);
  assert.throws(
    () => execFileSync("node", [join(root, "scripts/verify-audio-runtime.mjs"), "--prefix", prefix], { env: { ...process.env, PATH: `${bin}:${process.env.PATH || ""}`, HIFIMULE_TEST_LIBDIR: join(prefix, "lib"), PKG_CONFIG_PATH: join(prefix, "lib/pkgconfig") }, stdio: "pipe" }),
    (error) => error.stderr?.toString().includes("libavcodec ABI mismatch: expected exact ABI 63.1.101, found 62.11.100") === true,
  );
});
test("Windows skips Unix pkg-config verification while macOS retains it", () => {
  assert.equal(requiresHostAudioVerification("win32"), false);
  assert.equal(requiresHostAudioVerification("linux"), true);
  assert.equal(requiresHostAudioVerification("darwin"), true);
});

test("Linux preflight reports the complete Ubuntu compiler prerequisites", () => {
  assert.throws(
    () => preflightLinuxBuild("aarch64-unknown-linux-gnu", { commandExists: (tool) => tool !== "clang" }),
    (error) => error.message.includes("Missing Linux build tools: clang")
      && ["clang", "libclang-dev", "libc6-dev"].every((name) => error.message.includes(name)),
  );
  assert.deepEqual(
    ["clang", "libclang-dev", "libc6-dev"].every((name) => linuxBuildPackages.includes(name)),
    true,
  );
});

test("Linux preflight turns a broken native header chain into setup guidance", () => {
  const resourceDir = "/opt/llvm/lib/clang/18";
  assert.throws(
    () => preflightLinuxBuild("aarch64-unknown-linux-gnu", {
      commandExists: () => true,
      spawn: (command) => command === "clang" ? { status: 1, stderr: "fatal error: 'limits.h' file not found" } : { status: 0 },
      run: (command) => command === "clang" ? `${resourceDir}\n` : "aarch64-linux-gnu\n",
      exists: (path) => path === "/usr/include/limits.h" || path === `${resourceDir}/include/limits.h`,
    }),
    (error) => error.message.includes("cannot compile against the native aarch64-unknown-linux-gnu libc headers")
      && error.message.includes("libclang-dev")
      && error.message.includes("libc6-dev"),
  );
});

test("Linux x64 preflight requires NASM before configuring FFmpeg", () => {
  assert.throws(
    () => preflightLinuxBuild("x86_64-unknown-linux-gnu", { commandExists: (tool) => tool !== "nasm" }),
    (error) => error.message.includes("Missing Linux build tools: nasm") && error.message.includes("apt-get install"),
  );
  assert.ok(linuxBuildPackages.includes("nasm"));
});

test("Linux preflight reports missing ALSA development metadata before building", () => {
  assert.throws(
    () => preflightLinuxBuild(target, {
      commandExists: () => true,
      spawn: (_command, args) => ({ status: args.includes("alsa") ? 1 : 0 }),
    }),
    (error) => error.message.includes("ALSA development files are missing") && error.message.includes("libasound2-dev"),
  );
  assert.ok(linuxBuildPackages.includes("libasound2-dev"));
  for (const file of [".github/workflows/build.yml", ".github/workflows/release.yml"]) {
    assert.match(readFileSync(join(root, file), "utf8"), /\blibasound2-dev\b/);
  }
});

test("Linux bindgen uses matching Clang resource headers for native ARM64", () => {
  const resourceDir = "/opt/llvm/lib/clang/18";
  const libclangDir = "/opt/llvm/lib";
  const probeEnvironments = [];
  const options = {
    commandExists: (tool) => tool !== "nasm",
    spawn: (_command, _args, spawnOptions) => { probeEnvironments.push(spawnOptions.env); return { status: 0 }; },
    run: (command, _args, runOptions) => { probeEnvironments.push(runOptions.env); return command === "clang" ? `${resourceDir}\n` : "aarch64-linux-gnu\n"; },
    exists: (path) => path === "/usr/include/limits.h" || path === `${resourceDir}/include/limits.h`,
    readdir: (path) => path === libclangDir ? ["libclang.so"] : [],
  };
  const env = linuxBuildEnvironment("aarch64-unknown-linux-gnu", { PATH: "/controlled/bin", BINDGEN_EXTRA_CLANG_ARGS: "-DKEEP_ME", LIBCLANG_PATH: "/unrelated/libclang" }, options);
  assert.equal(env.LIBCLANG_PATH, libclangDir);
  assert.match(env["BINDGEN_EXTRA_CLANG_ARGS_aarch64-unknown-linux-gnu"], /--target=aarch64-unknown-linux-gnu/);
  assert.match(env.BINDGEN_EXTRA_CLANG_ARGS_aarch64_unknown_linux_gnu, /--target=aarch64-unknown-linux-gnu/);
  assert.match(env.BINDGEN_EXTRA_CLANG_ARGS_aarch64_unknown_linux_gnu, /-resource-dir=\/opt\/llvm\/lib\/clang\/18/);
  assert.match(env.BINDGEN_EXTRA_CLANG_ARGS_aarch64_unknown_linux_gnu, /-DKEEP_ME/);
  assert.ok(probeEnvironments.every((probeEnv) => probeEnv.PATH === "/controlled/bin"));
});

test("Build and release jobs install bindgen's Linux header toolchain", () => {
  for (const path of [".github/workflows/build.yml", ".github/workflows/release.yml"]) {
    const workflow = readFileSync(join(root, path), "utf8");
    for (const name of ["clang", "libclang-dev", "libc6-dev", "nasm"]) assert.match(workflow, new RegExp(`\\b${name}\\b`), `${path} must install ${name}`);
  }
});

test("sidecar cargo build receives the validated Linux bindgen environment", () => {
  const prepare = readFileSync(join(root, "scripts/prepare-sidecar.mjs"), "utf8");
  const configure = prepare.indexOf("Object.assign(buildEnv, linuxBuildEnvironment(targetTriple, buildEnv))");
  const cargoBuild = prepare.indexOf("cargo build --release -p hifimule-daemon");
  assert.ok(configure >= 0 && cargoBuild > configure, "Linux bindgen environment must be configured before cargo build");
  assert.match(prepare.slice(cargoBuild), /env: buildEnv/);
});

test("Linux CI environment writes the marker only after exact verification", () => {
  const output = join(mkdtempSync(join(tmpdir(), "hifimule-linux-env-")), "github-env");
  const events = [];
  writeLinuxBuildEnvironment("/controlled/ffmpeg", output, target, {
    env: {
      PKG_CONFIG_PATH: "/prior/pc",
      FFMPEG_DIR: "/old/ffmpeg8",
      [audioRuntimeVerification.environmentVariable]: audioRuntimeVerification.value,
    },
    linuxBuildEnvironment: (_target, env) => {
      assert.equal(env[audioRuntimeVerification.environmentVariable], undefined);
      return {
        ...env,
        LIBCLANG_PATH: "/llvm/lib",
        [`BINDGEN_EXTRA_CLANG_ARGS_${target}`]: "--target=x86_64-unknown-linux-gnu",
        BINDGEN_EXTRA_CLANG_ARGS_x86_64_unknown_linux_gnu: "--target=x86_64-unknown-linux-gnu",
      };
    },
    verifyAudioRuntime: ({ prefix, env }) => {
      events.push("verified");
      assert.equal(prefix, "/controlled/ffmpeg");
      assert.equal(env[audioRuntimeVerification.environmentVariable], undefined);
    },
  });
  assert.deepEqual(events, ["verified"]);
  const lines = readFileSync(output, "utf8").trim().split("\n");
  assert.ok(lines.includes("PKG_CONFIG_PATH=/controlled/ffmpeg/lib/pkgconfig:/prior/pc"));
  assert.ok(lines.includes("FFMPEG_DIR=/controlled/ffmpeg"));
  assert.ok(lines.includes("LD_LIBRARY_PATH=/controlled/ffmpeg/lib"));
  assert.equal(lines.at(-1), `${audioRuntimeVerification.environmentVariable}=${audioRuntimeVerification.value}`);
});

test("Linux CI environment accepts no marker when exact verification fails", () => {
  const output = join(mkdtempSync(join(tmpdir(), "hifimule-linux-env-fail-")), "github-env");
  assert.throws(
    () => writeLinuxBuildEnvironment("/controlled/ffmpeg", output, target, {
      env: { [audioRuntimeVerification.environmentVariable]: audioRuntimeVerification.value },
      linuxBuildEnvironment: (_target, env) => ({
        ...env,
        LIBCLANG_PATH: "/llvm/lib",
        [`BINDGEN_EXTRA_CLANG_ARGS_${target}`]: "args",
        BINDGEN_EXTRA_CLANG_ARGS_x86_64_unknown_linux_gnu: "args",
      }),
      verifyAudioRuntime: () => { throw new Error("verification rejected"); },
    }),
    /verification rejected/,
  );
  assert.equal(readFileSync(output, { encoding: "utf8", flag: "a+" }), "");
});

test("Linux runtime lock creates a fresh parent and retains exclusive ownership", (t) => {
  const scratch = mkdtempSync(join(tmpdir(), "hifimule-linux-lock-"));
  t.after(() => rmSync(scratch, { recursive: true, force: true }));
  const parent = join(scratch, "target", "audio-runtime");
  const lockPath = join(parent, "ffmpeg.lock");
  assert.equal(existsSync(parent), false);
  const unlock = acquireLinuxAudioRuntimeLock(lockPath);
  try {
    assert.equal(JSON.parse(readFileSync(join(lockPath, "owner.json"), "utf8")).pid, process.pid);
    assert.throws(() => mkdirSync(lockPath), { code: "EEXIST" });
  } finally {
    unlock();
  }
  assert.equal(existsSync(lockPath), false);
  assert.equal(existsSync(parent), true);
  acquireLinuxAudioRuntimeLock(lockPath)();
});

test("Linux packaging rejects a daemon linked against the system FFmpeg ABI", () => {
  const correct = ["libavcodec.so.63", "libavformat.so.63", "libavutil.so.61", "libswresample.so.7"];
  assert.doesNotThrow(() => verifyLinuxAudioLinkage(correct));
  for (const invalid of [correct.slice(1), ["libavcodec.so.62", ...correct.slice(1)], [...correct, "libavcodec.so.62"]]) {
    assert.throws(() => verifyLinuxAudioLinkage(invalid), /Daemon FFmpeg linkage mismatch/);
  }
});


test("Linux preflight requires Pulse shared-output development metadata", () => {
  assert.throws(() => preflightLinuxBuild(target, {
    commandExists: () => true,
    spawn: (_command, args) => ({ status: args.includes("libpulse") ? 1 : 0 }),
  }), /PulseAudio development files are missing/);
  assert.ok(linuxBuildPackages.includes("libpulse-dev"));
});
