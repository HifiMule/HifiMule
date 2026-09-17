import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import test from "node:test";
import { assertAppImagePrerequisites, ensureAppImageRuntime, requestsAppImage, tauriEnvironment } from "../run-tauri.mjs";

const root = resolve(import.meta.dirname, "../..");

test("detects whether a Tauri build includes an AppImage", () => {
  assert.equal(requestsAppImage(["build"]), true);
  assert.equal(requestsAppImage(["build", "--bundles", "deb,rpm"]), false);
  assert.equal(requestsAppImage(["build", "--bundles=deb,appimage"]), true);
  assert.equal(requestsAppImage(["dev"]), false);
});

test("Linux AppImage builds pass a verified pinned runtime to linuxdeploy", (t) => {
  const cacheDir = mkdtempSync(join(tmpdir(), "hifimule-appimage-runtime-"));
  t.after(() => rmSync(cacheDir, { recursive: true, force: true }));
  let downloads = 0;
  assert.throws(
    () => ensureAppImageRuntime({
      arch: "arm64",
      cacheDir,
      download: (_url, destination) => {
        downloads += 1;
        writeFileSync(destination, "not the pinned runtime");
      },
    }),
    /checksum mismatch/,
  );
  assert.equal(downloads, 1);
  assert.equal(readFileSync(join(root, "hifimule-ui/package.json"), "utf8").includes("../scripts/run-tauri.mjs"), true);
});

test("an explicit runtime override is preserved without provisioning", () => {
  const env = tauriEnvironment(["build"], {
    platform: "linux",
    env: { LDAI_RUNTIME_FILE: "/controlled/runtime" },
    probe: () => ({ status: 0 }),
    fileExists: () => true,
    download: () => assert.fail("must not download"),
  });
  assert.equal(env.LDAI_RUNTIME_FILE, "/controlled/runtime");
});

test("AppImage preflight reports missing metadata and the complete Ubuntu package command", () => {
  assert.throws(
    () => assertAppImagePrerequisites({
      probe: (module) => ({ status: module === "librsvg-2.0" ? 1 : 0 }),
      fileExists: () => false,
    }),
    (error) => error.message.includes("xdo.h (libxdo-dev)")
      && error.message.includes("librsvg-2.0 (librsvg2-dev)")
      && [
        "libgtk-3-dev",
        "libxdo-dev",
        "libsoup-3.0-dev",
        "libwebkit2gtk-4.1-dev",
        "libfuse2",
        "librsvg2-dev",
      ].every((name) => error.message.includes(name)),
  );
});

test("release builds retain symbols required by Tauri bundle patching", () => {
  const cargo = readFileSync(join(root, "Cargo.toml"), "utf8");
  assert.match(cargo, /\[profile\.release\][\s\S]*strip = false/);
  assert.doesNotMatch(cargo, /\[profile\.release\][\s\S]*strip = true/);
});
