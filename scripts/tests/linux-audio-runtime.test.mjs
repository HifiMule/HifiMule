import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { chmodSync, cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { basename, dirname, join, resolve } from "node:path";
import test from "node:test";
import { stageLinuxDependencyClosure, verifyInstalledLinuxBundle, verifyLinuxAudioLinkage, acquireLinuxAudioRuntimeLock, linuxBuildEnvironment, linuxBuildPackages, preflightLinuxBuild, requiresHostAudioVerification, validateInstalledSidecarRunpath, validateLinuxRuntimeReceipt, writeLinuxBuildEnvironment } from "../linux-audio-runtime.mjs";
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
  writeFileSync(pkgConfig, `#!/bin/sh\ncase "$1:$2" in\n  --modversion:libavcodec) echo 62.11.100 ;;\n  --modversion:libavformat) echo ${manifest.abiVersions.avformat} ;;\n  --modversion:libavutil) echo ${manifest.abiVersions.avutil} ;;\n  --modversion:libswresample) echo ${manifest.abiVersions.swresample} ;;\n  --variable=libdir:*) printf '%s\\n' "$HIFIMULE_TEST_LIBDIR" ;;\n  *) exit 1 ;;\nesac\n`);
  chmodSync(pkgConfig, 0o755);
  assert.throws(
    () => execFileSync("node", [join(root, "scripts/verify-audio-runtime.mjs"), "--prefix", prefix], { env: { ...process.env, PATH: `${bin}:${process.env.PATH || ""}`, HIFIMULE_TEST_LIBDIR: join(prefix, "lib"), PKG_CONFIG_PATH: join(prefix, "lib/pkgconfig") }, stdio: "pipe" }),
    (error) => error.stderr?.toString().includes(`libavcodec ABI mismatch: expected exact ABI ${manifest.abiVersions.avcodec}, found 62.11.100`) === true,
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

test("installed Linux sidecar RUNPATH reaches AppImage and deb private libraries", (t) => {
  const scratch = mkdtempSync(join(tmpdir(), "hifimule-installed-runpath-"));
  t.after(() => rmSync(scratch, { recursive: true, force: true }));

  const appImageRoot = join(scratch, "appimage");
  const appImageSidecar = join(appImageRoot, "usr/bin/hifimule-daemon");
  const appImageLibdir = join(appImageRoot, "usr/lib");
  mkdirSync(dirname(appImageSidecar), { recursive: true });
  mkdirSync(appImageLibdir, { recursive: true });
  assert.equal(
    validateInstalledSidecarRunpath(appImageRoot, appImageSidecar, "$ORIGIN/../lib"),
    realpathSync(appImageLibdir),
  );

  const debRoot = join(scratch, "deb");
  const debSidecar = join(debRoot, "usr/bin/hifimule-daemon");
  const debLibdir = join(debRoot, "usr/lib/HifiMule/bundled-libs");
  mkdirSync(dirname(debSidecar), { recursive: true });
  mkdirSync(debLibdir, { recursive: true });
  assert.equal(
    validateInstalledSidecarRunpath(
      debRoot,
      debSidecar,
      "$ORIGIN/../bundled-libs:$ORIGIN/bundled-libs:$ORIGIN/../lib/HifiMule/bundled-libs:$ORIGIN/../lib/hifimule/bundled-libs",
    ),
    realpathSync(debLibdir),
  );
});

test("installed Linux sidecar RUNPATH rejects unreachable, absolute, and escaping entries", (t) => {
  const bundleRoot = mkdtempSync(join(tmpdir(), "hifimule-invalid-runpath-"));
  t.after(() => rmSync(bundleRoot, { recursive: true, force: true }));
  const sidecar = join(bundleRoot, "usr/bin/hifimule-daemon");
  const libdir = join(bundleRoot, "usr/lib");
  mkdirSync(dirname(sidecar), { recursive: true });
  mkdirSync(libdir, { recursive: true });
  mkdirSync(join(bundleRoot, "usr/collision"));
  writeFileSync(join(dirname(sidecar), "not-a-directory"), "");

  for (const runpath of ["$ORIGIN/../share", "/usr/lib", "$ORIGIN/../../../outside", "$ORIGIN/../lib:/usr/lib", "$ORIGIN/../collision:$ORIGIN/../lib", "$ORIGIN/missing/../../lib:$ORIGIN/../lib", "$ORIGIN/not-a-directory/../../lib:$ORIGIN/../lib"]) {
    assert.throws(
      () => validateInstalledSidecarRunpath(bundleRoot, sidecar, runpath),
      (error) => error.message === `Invalid installed sidecar RUNPATH: ${runpath}`,
    );
  }
});

test("installed Linux sidecar RUNPATH cannot escape through a symlink before parent traversal", { skip: process.platform === "win32" }, (t) => {
  const scratch = mkdtempSync(join(tmpdir(), "hifimule-symlink-runpath-"));
  t.after(() => rmSync(scratch, { recursive: true, force: true }));
  const bundleRoot = join(scratch, "bundle");
  const sidecar = join(bundleRoot, "usr/bin/hifimule-daemon");
  const libdir = join(bundleRoot, "usr/lib");
  const outside = join(scratch, "outside");
  mkdirSync(dirname(sidecar), { recursive: true });
  mkdirSync(libdir, { recursive: true });
  mkdirSync(outside);
  symlinkSync(outside, join(bundleRoot, "usr/escape-link"));
  assert.throws(
    () => validateInstalledSidecarRunpath(bundleRoot, sidecar, "$ORIGIN/../escape-link/../lib"),
    (error) => error.message === "Invalid installed sidecar RUNPATH: $ORIGIN/../escape-link/../lib",
  );
});

function installedBundleFixture(t, reachableNames, nestedNames) {
  const bundleRoot = mkdtempSync(join(tmpdir(), "hifimule-installed-bundle-"));
  t.after(() => rmSync(bundleRoot, { recursive: true, force: true }));
  const sidecar = join(bundleRoot, "usr/bin/hifimule-daemon");
  const libdir = join(bundleRoot, "usr/lib");
  const nestedLibdir = join(bundleRoot, "usr/lib/HifiMule/resources/bundled-libs");
  for (const path of [sidecar, ...reachableNames.map((name) => join(libdir, name)), ...nestedNames.map((name) => join(nestedLibdir, name))]) {
    mkdirSync(dirname(path), { recursive: true });
    writeFileSync(path, "ELF fixture");
  }
  const allFiles = [sidecar, ...reachableNames.map((name) => join(libdir, name)), ...nestedNames.map((name) => join(nestedLibdir, name))];
  const requiredFfmpeg = Object.entries(manifest.abiVersions).map(([library, version]) => `lib${library}.so.${version.split(".")[0]}`);
  const options = {
    assertElf: (path) => ({ needed: path === sidecar ? requiredFfmpeg : [], soname: basename(path) }),
    elf: () => ({ needed: [] }),
    resolved: () => {},
    run: (_command, args) => args[1] === sidecar ? "$ORIGIN/../lib\n" : "$ORIGIN\n",
  };
  return { allFiles, bundleRoot, libdir, options };
}

test("installed Linux verification uses the RUNPATH closure despite duplicate nested libraries and traversal order", (t) => {
  const controlledNames = [
    ...Object.entries(manifest.abiVersions).map(([library, version]) => `lib${library}.so.${version.split(".")[0]}`),
    "libmtp.so.9",
    "libpulse.so.0",
  ];
  const fixture = installedBundleFixture(t, controlledNames, [controlledNames[0]]);
  for (const files of [fixture.allFiles, [...fixture.allFiles].reverse()]) {
    const result = verifyInstalledLinuxBundle(fixture.bundleRoot, target, { ...fixture.options, walk: () => files });
    assert.equal(result.libdir, realpathSync(fixture.libdir));
    assert.equal(result.libraryCount, controlledNames.length);
  }
});

test("installed Linux verification ignores linuxdeploy GTK modules and unrelated libraries outside the controlled closure", { skip: process.platform === "win32" }, (t) => {
  const controlledNames = [
    ...Object.entries(manifest.abiVersions).map(([library, version]) => `lib${library}.so.${version.split(".")[0]}`),
    "libmtp.so.9",
    "libpulse.so.0",
  ];
  const fixture = installedBundleFixture(t, controlledNames, []);
  const gtkModule = join(fixture.libdir, "gtk-3.0/3.0.0/immodules/im-am-et.so");
  const gtkLink = join(fixture.libdir, "im-am-et.so");
  const unrelatedLibrary = join(fixture.libdir, "libgstreamer-1.0.so.0");
  mkdirSync(dirname(gtkModule), { recursive: true });
  writeFileSync(gtkModule, "linuxdeploy GTK module");
  symlinkSync("gtk-3.0/3.0.0/immodules/im-am-et.so", gtkLink);
  writeFileSync(unrelatedLibrary, "unrelated linuxdeploy library");

  const result = verifyInstalledLinuxBundle(fixture.bundleRoot, target, {
    ...fixture.options,
    walk: () => [...fixture.allFiles, gtkLink, gtkModule, unrelatedLibrary],
    assertElf: (path, ...args) => {
      assert.notEqual(path, gtkModule, "unrelated GTK module must not be inspected");
      assert.notEqual(path, unrelatedLibrary, "unrelated library must not be inspected");
      return fixture.options.assertElf(path, ...args);
    },
    run: (command, args) => {
      assert.notEqual(args[1], gtkModule, "unrelated GTK RUNPATH must not be inspected");
      assert.notEqual(args[1], unrelatedLibrary, "unrelated library RUNPATH must not be inspected");
      return fixture.options.run(command, args);
    },
  });
  assert.equal(result.libraryCount, controlledNames.length);
});

test("installed Linux verification walks reachable transitive dependencies by exact SONAME", (t) => {
  const controlledNames = [
    ...Object.entries(manifest.abiVersions).map(([library, version]) => `lib${library}.so.${version.split(".")[0]}`),
    "libmtp.so.9",
    "libpulse.so.0",
  ];
  const direct = "libhifimule-device.so.1";
  const transitive = "libhifimule-transport.so.2";
  const fixture = installedBundleFixture(t, [...controlledNames, direct, transitive], []);
  const result = verifyInstalledLinuxBundle(fixture.bundleRoot, target, {
    ...fixture.options,
    assertElf: (path) => ({
      needed: path === fixture.allFiles[0]
        ? [...controlledNames.slice(0, 4), direct]
        : basename(path) === direct ? [transitive] : [],
      soname: basename(path),
    }),
  });
  assert.equal(result.libraryCount, controlledNames.length + 2);
});

test("AppImage custom files preserve libmtp's excluded libusb dependency in the effective closure", (t) => {
  const controlledNames = [
    ...Object.entries(manifest.abiVersions).map(([library, version]) => `lib${library}.so.${version.split(".")[0]}`),
    "libmtp.so.9", "libpulse.so.0",
  ];
  const usb = "libusb-1.0.so.0";
  // Discovery copied libmtp to usr/lib but left its excluded dependency in resources.
  const fixture = installedBundleFixture(t, controlledNames, [usb]);
  const options = {
    ...fixture.options,
    assertElf: (path) => ({
      ...fixture.options.assertElf(path),
      ...(basename(path) === "libmtp.so.9" ? { needed: [usb] } : {}),
    }),
  };
  assert.throws(() => verifyInstalledLinuxBundle(fixture.bundleRoot, target, options),
    /Installed private closure is missing libusb-1\.0\.so\.0, required by .*libmtp\.so\.9/);

  const staging = mkdtempSync(join(tmpdir(), "hifimule-appimage-staging-"));
  t.after(() => rmSync(staging, { recursive: true, force: true }));
  mkdirSync(join(staging, "bundled-libs"));
  for (const name of [...controlledNames, usb]) writeFileSync(join(staging, "bundled-libs", name), "ELF fixture");
  const config = JSON.parse(readFileSync(join(root, "hifimule-ui/src-tauri/tauri.linux.conf.json"), "utf8"));
  // Match Tauri copy_custom_files: package destination -> source relative to src-tauri.
  for (const [destination, source] of Object.entries(config.bundle.linux.appimage.files)) {
    cpSync(join(staging, source), join(fixture.bundleRoot, destination), { recursive: true });
  }
  const result = verifyInstalledLinuxBundle(fixture.bundleRoot, target, options);
  assert.equal(result.libraryCount, controlledNames.length + 1);
  rmSync(join(fixture.libdir, usb));
  assert.throws(() => verifyInstalledLinuxBundle(fixture.bundleRoot, target, options),
    /Installed private closure is missing libusb-1\.0\.so\.0/);
});

test("installed Linux verification rejects missing and path-containing reachable dependencies", (t) => {
  const controlledNames = [
    ...Object.entries(manifest.abiVersions).map(([library, version]) => `lib${library}.so.${version.split(".")[0]}`),
    "libmtp.so.9",
    "libpulse.so.0",
  ];
  const direct = "libhifimule-device.so.1";
  const fixture = installedBundleFixture(t, [...controlledNames, direct], []);
  for (const invalid of ["libmissing-transport.so.2", "../libescape.so.1", "nested/libescape.so.1"]) {
    assert.throws(
      () => verifyInstalledLinuxBundle(fixture.bundleRoot, target, {
        ...fixture.options,
        assertElf: (path) => ({
          needed: path === fixture.allFiles[0]
            ? [...controlledNames.slice(0, 4), direct]
            : basename(path) === direct ? [invalid] : [],
          soname: basename(path),
        }),
      }),
      invalid.includes("/")
        ? /Invalid installed dependency name/
        : new RegExp(`Installed private closure is missing ${invalid.replaceAll(".", "\\.")}, required by .*${direct.replaceAll(".", "\\.")}`),
    );
  }
});

test("installed Linux verification validates loader-like dependency names before classification", (t) => {
  const controlledNames = [
    ...Object.entries(manifest.abiVersions).map(([library, version]) => `lib${library}.so.${version.split(".")[0]}`),
    "libmtp.so.9",
    "libpulse.so.0",
  ];
  const direct = "libhifimule-device.so.1";
  const fixture = installedBundleFixture(t, [...controlledNames, direct], []);
  const ffmpegNames = controlledNames.slice(0, 4);
  for (const invalid of ["ld-linux/../../escape.so", "", "ld-linux\nmalicious.so.2"]) {
    assert.throws(
      () => verifyInstalledLinuxBundle(fixture.bundleRoot, target, {
        ...fixture.options,
        assertElf: (path) => ({ needed: path === fixture.allFiles[0] ? [...ffmpegNames, invalid] : [], soname: basename(path) }),
      }),
      /Invalid installed dependency name/,
      `direct dependency ${JSON.stringify(invalid)}`,
    );
    assert.throws(
      () => verifyInstalledLinuxBundle(fixture.bundleRoot, target, {
        ...fixture.options,
        assertElf: (path) => ({
          needed: path === fixture.allFiles[0] ? [...ffmpegNames, direct] : basename(path) === direct ? [invalid] : [],
          soname: basename(path),
        }),
      }),
      /Invalid installed dependency name/,
      `transitive dependency ${JSON.stringify(invalid)}`,
    );
  }
  for (const prefixed of ["ld-linux-malicious.so.2", "ld-linux-x86-64.so.2.extra"]) {
    assert.throws(
      () => verifyInstalledLinuxBundle(fixture.bundleRoot, target, {
        ...fixture.options,
        assertElf: (path) => ({ needed: path === fixture.allFiles[0] ? [...ffmpegNames, prefixed] : [], soname: basename(path) }),
      }),
      new RegExp(`Installed private closure is missing ${prefixed.replaceAll(".", "\\.")}`),
      `direct dependency ${prefixed}`,
    );
    assert.throws(
      () => verifyInstalledLinuxBundle(fixture.bundleRoot, target, {
        ...fixture.options,
        assertElf: (path) => ({
          needed: path === fixture.allFiles[0] ? [...ffmpegNames, direct] : basename(path) === direct ? [prefixed] : [],
          soname: basename(path),
        }),
      }),
      new RegExp(`Installed private closure is missing ${prefixed.replaceAll(".", "\\.")}`),
      `transitive dependency ${prefixed}`,
    );
  }
});

test("installed Linux verification validates bundled baseline libraries that can shadow the system", (t) => {
  const controlledNames = [
    ...Object.entries(manifest.abiVersions).map(([library, version]) => `lib${library}.so.${version.split(".")[0]}`),
    "libmtp.so.9",
    "libpulse.so.0",
  ];
  const absent = installedBundleFixture(t, controlledNames, []);
  const sidecarDependencies = [...controlledNames.slice(0, 4), "libc.so.6"];
  const absentResult = verifyInstalledLinuxBundle(absent.bundleRoot, target, {
    ...absent.options,
    assertElf: (path) => ({ needed: path === absent.allFiles[0] ? sidecarDependencies : [], soname: basename(path) }),
  });
  assert.equal(absentResult.libraryCount, controlledNames.length);

  const shadowed = installedBundleFixture(t, [...controlledNames, "libc.so.6"], []);
  assert.throws(
    () => verifyInstalledLinuxBundle(shadowed.bundleRoot, target, {
      ...shadowed.options,
      assertElf: (path) => {
        if (basename(path) === "libc.so.6") throw new Error(`Malformed bundled baseline library: ${path}`);
        return { needed: path === shadowed.allFiles[0] ? sidecarDependencies : [], soname: basename(path) };
      },
    }),
    /Malformed bundled baseline library/,
  );
});

test("installed Linux verification rejects a reachable SONAME symlink into a nested directory", { skip: process.platform === "win32" }, (t) => {
  const controlledNames = [
    ...Object.entries(manifest.abiVersions).map(([library, version]) => `lib${library}.so.${version.split(".")[0]}`),
    "libmtp.so.9",
    "libpulse.so.0",
  ];
  const fixture = installedBundleFixture(t, controlledNames, []);
  const soname = controlledNames[0];
  const nestedTarget = join(fixture.libdir, "nested", `${soname}.1.0`);
  mkdirSync(dirname(nestedTarget));
  writeFileSync(nestedTarget, "nested ELF fixture");
  rmSync(join(fixture.libdir, soname));
  symlinkSync(join("nested", basename(nestedTarget)), join(fixture.libdir, soname));
  assert.throws(
    () => verifyInstalledLinuxBundle(fixture.bundleRoot, target, fixture.options),
    new RegExp(`Invalid installed library symlink: .*${soname.replaceAll(".", "\\.")}`),
  );
});

test("installed Linux verification rejects broken and escaping reachable SONAME symlinks", { skip: process.platform === "win32" }, (t) => {
  const controlledNames = [
    ...Object.entries(manifest.abiVersions).map(([library, version]) => `lib${library}.so.${version.split(".")[0]}`),
    "libmtp.so.9",
    "libpulse.so.0",
  ];
  for (const mode of ["broken", "escaping"]) {
    const fixture = installedBundleFixture(t, controlledNames, []);
    const soname = controlledNames[0];
    const sonamePath = join(fixture.libdir, soname);
    rmSync(sonamePath);
    if (mode === "broken") {
      symlinkSync(`${soname}.missing`, sonamePath);
    } else {
      const outside = join(dirname(fixture.bundleRoot), `${basename(fixture.bundleRoot)}-${soname}`);
      writeFileSync(outside, "outside ELF fixture");
      t.after(() => rmSync(outside, { force: true }));
      symlinkSync(outside, sonamePath);
    }
    assert.throws(
      () => verifyInstalledLinuxBundle(fixture.bundleRoot, target, fixture.options),
      new RegExp(`Invalid installed library symlink: .*${soname.replaceAll(".", "\\.")}`),
      mode,
    );
  }
});

test("installed Linux verification rejects invalid RUNPATH and ldd failures in the reachable closure", (t) => {
  const controlledNames = [
    ...Object.entries(manifest.abiVersions).map(([library, version]) => `lib${library}.so.${version.split(".")[0]}`),
    "libmtp.so.9",
    "libpulse.so.0",
  ];
  const fixture = installedBundleFixture(t, controlledNames, []);
  const required = controlledNames[0];
  assert.throws(
    () => verifyInstalledLinuxBundle(fixture.bundleRoot, target, {
      ...fixture.options,
      run: (_command, args) => args[1] === fixture.allFiles[0] ? "$ORIGIN/../lib\n" : basename(args[1]) === required ? "/usr/lib\n" : "$ORIGIN\n",
    }),
    new RegExp(`Invalid installed RUNPATH: .*${required.replaceAll(".", "\\.")}`),
  );
  assert.throws(
    () => verifyInstalledLinuxBundle(fixture.bundleRoot, target, {
      ...fixture.options,
      resolved: (path) => { if (basename(path) === required) throw new Error(`Unresolved dependency for ${path}`); },
    }),
    new RegExp(`Unresolved dependency for .*${required.replaceAll(".", "\\.")}`),
  );
});

test("installed Linux verification rejects a complete closure that is only in unreachable nested resources", (t) => {
  const controlledNames = [
    ...Object.entries(manifest.abiVersions).map(([library, version]) => `lib${library}.so.${version.split(".")[0]}`),
    "libmtp.so.9",
    "libpulse.so.0",
  ];
  const fixture = installedBundleFixture(t, controlledNames.slice(1), controlledNames);
  assert.throws(
    () => verifyInstalledLinuxBundle(fixture.bundleRoot, target, { ...fixture.options, walk: () => fixture.allFiles }),
    new RegExp(`Installed private closure is missing ${controlledNames[0].replaceAll(".", "\\.")}, required by`),
  );
});

test("installed Linux verification accepts an in-directory SONAME symlink to a versioned backing file", (t) => {
  const controlledNames = [
    ...Object.entries(manifest.abiVersions).map(([library, version]) => `lib${library}.so.${version.split(".")[0]}`),
    "libmtp.so.9",
    "libpulse.so.0",
  ];
  const fixture = installedBundleFixture(t, controlledNames, []);
  const soname = controlledNames[0];
  const sonamePath = join(fixture.libdir, soname);
  const backingPath = `${sonamePath}.1.0`;
  rmSync(sonamePath);
  writeFileSync(backingPath, "ELF fixture");
  symlinkSync(basename(backingPath), sonamePath);
  const inspections = [];
  const result = verifyInstalledLinuxBundle(fixture.bundleRoot, target, {
    ...fixture.options,
    assertElf: (path, elfTarget, expectedSoname) => {
      inspections.push({ path, elfTarget, expectedSoname });
      if (path === fixture.allFiles[0]) return fixture.options.assertElf(path, elfTarget, expectedSoname);
      const actualSoname = path === realpathSync(backingPath) ? soname : basename(path);
      if (expectedSoname && expectedSoname !== actualSoname) throw new Error(`ELF SONAME mismatch for ${path}: ${actualSoname}`);
      return { needed: [], soname: actualSoname };
    },
  });
  assert.equal(result.libdir, realpathSync(fixture.libdir));
  assert.deepEqual(
    inspections.filter((inspection) => inspection.path === realpathSync(backingPath)).map(({ expectedSoname }) => expectedSoname),
    [soname],
  );
});

test("installed Linux verification rejects a regular required library with the wrong SONAME", (t) => {
  const controlledNames = [
    ...Object.entries(manifest.abiVersions).map(([library, version]) => `lib${library}.so.${version.split(".")[0]}`),
    "libmtp.so.9",
    "libpulse.so.0",
  ];
  const fixture = installedBundleFixture(t, controlledNames, []);
  const requiredName = controlledNames[0];
  assert.throws(
    () => verifyInstalledLinuxBundle(fixture.bundleRoot, target, {
      ...fixture.options,
      assertElf: (path, elfTarget, expectedSoname) => {
        if (path === fixture.allFiles[0]) return fixture.options.assertElf(path, elfTarget, expectedSoname);
        const actualSoname = basename(path) === requiredName ? "libwrong.so.1" : basename(path);
        if (expectedSoname && expectedSoname !== actualSoname) throw new Error(`ELF SONAME mismatch for ${path}: ${actualSoname}`);
        return { needed: [], soname: actualSoname };
      },
    }),
    new RegExp(`ELF SONAME mismatch for .*${requiredName.replaceAll(".", "\\.")}: libwrong\\.so\\.1`),
  );
});

test("installed Linux verification rejects a missing direct sidecar dependency", (t) => {
  const controlledNames = [
    ...Object.entries(manifest.abiVersions).map(([library, version]) => `lib${library}.so.${version.split(".")[0]}`),
    "libmtp.so.9",
    "libpulse.so.0",
  ];
  const fixture = installedBundleFixture(t, controlledNames, []);
  const directDependency = "libhifimule-device.so.1";
  assert.throws(
    () => verifyInstalledLinuxBundle(fixture.bundleRoot, target, {
      ...fixture.options,
      assertElf: (path) => ({
        needed: path === fixture.allFiles[0]
          ? [...Object.entries(manifest.abiVersions).map(([library, version]) => `lib${library}.so.${version.split(".")[0]}`), directDependency]
          : [],
        soname: basename(path),
      }),
    }),
    new RegExp(`Installed private closure is missing ${directDependency.replaceAll(".", "\\.")}, required by`),
  );
});


test("Linux preflight requires Pulse shared-output development metadata", () => {
  assert.throws(() => preflightLinuxBuild(target, {
    commandExists: () => true,
    spawn: (_command, args) => ({ status: args.includes("libpulse") ? 1 : 0 }),
  }), /PulseAudio development files are missing/);
  assert.ok(linuxBuildPackages.includes("libpulse-dev"));
});

test("Linux preflight requires D-Bus native-controls development metadata", () => {
  assert.throws(() => preflightLinuxBuild(target, {
    commandExists: () => true,
    spawn: (_command, args) => ({ status: args.includes("dbus-1") ? 1 : 0 }),
  }), /D-Bus development files are missing/);
  assert.ok(linuxBuildPackages.includes("libdbus-1-dev"));
});


test("staging follows the daemon GTK graph beyond the explicit audio and MTP roots", (t) => {
  const dir = mkdtempSync(join(tmpdir(), "hifimule-stage-closure-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const sources = join(dir, "sources"), out = join(dir, "staged");
  mkdirSync(sources); mkdirSync(out);
  const graph = {
    "hifimule-daemon": ["libaudio.so.1", "libgdk-3.so.0", "libc.so.6", "ld-linux-x86-64.so.2"],
    "libaudio.so.1": [],
    "libmtp.so.9": ["libusb-1.0.so.0"],
    "libusb-1.0.so.0": ["libc.so.6"],
    "libgdk-3.so.0": ["libfontconfig.so.1"],
    "libfontconfig.so.1": ["libfreetype.so.6"],
    "libfreetype.so.6": ["libfontconfig.so.1"],
  };
  for (const name of Object.keys(graph)) writeFileSync(join(sources, name), name);
  const options = {
    assertElf: (path) => ({ soname: basename(path), needed: graph[basename(path)] }),
    resolveNeeded: (_source, name) => {
      assert.ok(name in graph, `must not resolve host baseline ${name}`);
      return join(sources, name);
    },
  };
  const stage = () => stageLinuxDependencyClosure(join(sources, "hifimule-daemon"),
    [join(sources, "libaudio.so.1"), join(sources, "libmtp.so.9")], out, target, [sources], options);
  const copied = stage();
  assert.deepEqual([...copied.keys()].sort(), Object.keys(graph).filter(name => name !== "hifimule-daemon").sort());
  for (const name of copied.keys()) assert.equal(readFileSync(join(out, name), "utf8"), name);
  assert.equal(existsSync(join(out, "hifimule-daemon")), false);
  rmSync(join(sources, "libfontconfig.so.1"));
  assert.throws(stage, /ENOENT/);
});


test("native ELF staging collects a daemon-only GDK/fontconfig dependency chain", { skip: process.platform !== "linux" }, (t) => {
  const dir = mkdtempSync(join(tmpdir(), "hifimule-native-closure-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const out = join(dir, "staged");
  mkdirSync(out);
  const compile = (name, code, args) => {
    const source = join(dir, `${name}.c`);
    writeFileSync(source, code);
    execFileSync("cc", [source, "-o", join(dir, name), ...args]);
  };
  compile("libfontconfig.so.1", "int font(void) { return 0; }", ["-shared", "-fPIC", "-Wl,-soname,libfontconfig.so.1"]);
  compile("libgdk-3.so.0", "extern int font(void); int gdk(void) { return font(); }",
    ["-shared", "-fPIC", "-Wl,-soname,libgdk-3.so.0", `-L${dir}`, "-l:libfontconfig.so.1", `-Wl,-rpath,${dir}`]);
  compile("hifimule-daemon", "extern int gdk(void); int main(void) { return gdk(); }",
    [`-L${dir}`, "-l:libgdk-3.so.0", `-Wl,-rpath,${dir}`]);
  const nativeTarget = process.arch === "arm64" ? "aarch64-unknown-linux-gnu" : target;
  const copied = stageLinuxDependencyClosure(join(dir, "hifimule-daemon"), [], out, nativeTarget, []);
  assert.deepEqual([...copied.keys()].sort(), ["libfontconfig.so.1", "libgdk-3.so.0"]);
  for (const name of copied.keys()) assert.ok(existsSync(join(out, name)));
});
