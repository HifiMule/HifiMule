import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import test from "node:test";
import { linuxBuildEnvironment, linuxBuildPackages, preflightLinuxBuild, requiresHostAudioVerification, validateLinuxRuntimeReceipt, writeLinuxBuildEnvironment } from "../linux-audio-runtime.mjs";
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
    (error) => error.stderr?.toString().includes("libavcodec ABI mismatch: expected exact ABI 63.1.100, found 62.11.100") === true,
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

test("Linux bindgen uses matching Clang resource headers for native ARM64", () => {
  const resourceDir = "/opt/llvm/lib/clang/18";
  const libclangDir = "/opt/llvm/lib";
  const probeEnvironments = [];
  const options = {
    commandExists: () => true,
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
    for (const name of ["clang", "libclang-dev", "libc6-dev"]) assert.match(workflow, new RegExp(`\\b${name}\\b`), `${path} must install ${name}`);
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
