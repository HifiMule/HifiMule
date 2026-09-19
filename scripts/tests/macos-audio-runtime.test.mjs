import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import test from "node:test";
import { macosRuntimeReceipt, verifyInstalledMacosBundle, verifyMacosAudioRuntime } from "../macos-audio-runtime.mjs";

const root = resolve(import.meta.dirname, "../..");
const manifest = JSON.parse(readFileSync(join(root, "hifimule-daemon/audio-runtime.json"), "utf8"));

function controlledPrefix(target = "aarch64-apple-darwin") {
  const prefix = mkdtempSync(join(tmpdir(), "hifimule-macos-runtime-"));
  mkdirSync(join(prefix, "lib", "pkgconfig"), { recursive: true });
  writeFileSync(join(prefix, ".hifimule-audio-runtime.json"), JSON.stringify(macosRuntimeReceipt(target)));
  for (const [library, version] of Object.entries(manifest.abiVersions)) {
    writeFileSync(join(prefix, "lib", `lib${library}.${version}.dylib`), library);
  }
  return prefix;
}

test("macOS runtime accepts only the exact target receipt and ABI versions", (t) => {
  const target = "aarch64-apple-darwin";
  const prefix = controlledPrefix(target);
  t.after(() => rmSync(prefix, { recursive: true, force: true }));
  const architectures = [];
  const run = (command, args) => {
    if (command === "pkg-config") return `${manifest.abiVersions[args[1].slice(3)]}\n`;
    if (command === "lipo") { architectures.push(args.at(-1)); return ""; }
    if (command === "otool") return `${args.at(-1)}:\n\t/usr/lib/libSystem.B.dylib\n`;
    throw new Error(`unexpected command ${command}`);
  };
  assert.equal(verifyMacosAudioRuntime(prefix, target, { run }), prefix);
  assert.deepEqual(new Set(architectures), new Set(["arm64"]));
  const receipt = JSON.parse(readFileSync(join(prefix, ".hifimule-audio-runtime.json")));
  receipt.sourceSha256 = "0".repeat(64);
  writeFileSync(join(prefix, ".hifimule-audio-runtime.json"), JSON.stringify(receipt));
  assert.throws(() => verifyMacosAudioRuntime(prefix, target, { run }), /receipt is stale/);
});

function fakeApp() {
  const app = mkdtempSync(join(tmpdir(), "HifiMule.app-"));
  const macos = join(app, "Contents", "MacOS");
  const resources = join(app, "Contents", "Resources");
  const libs = join(resources, "bundled-libs");
  mkdirSync(macos, { recursive: true });
  mkdirSync(libs, { recursive: true });
  writeFileSync(join(macos, "hifimule-daemon"), "mach-o");
  writeFileSync(join(resources, "audio-runtime.json"), JSON.stringify(manifest));
  writeFileSync(join(resources, "THIRD_PARTY_AUDIO_NOTICES.md"), "notices");
  for (const [library, version] of Object.entries(manifest.abiVersions)) writeFileSync(join(libs, `lib${library}.${version}.dylib`), library);
  return app;
}

test("macOS installed verifier rejects developer load paths and requires notices", (t) => {
  const app = fakeApp();
  const libs = join(app, "Contents", "Resources", "bundled-libs");
  t.after(() => rmSync(app, { recursive: true, force: true }));
  const clean = (command) => command === "otool" ? "/usr/lib/libSystem.B.dylib\n" : "";
  assert.equal(verifyInstalledMacosBundle(app, "aarch64-apple-darwin", { run: clean }).dylibCount, 4);
  assert.throws(
    () => verifyInstalledMacosBundle(app, "aarch64-apple-darwin", { run: (command) => command === "otool" ? "/opt/homebrew/lib/libavcodec.63.dylib\n" : "" }),
    /Developer load path/,
  );
  writeFileSync(join(libs, "libavcodec.63.1.101.dylib"), "stale");
  assert.throws(() => verifyInstalledMacosBundle(app, "aarch64-apple-darwin", { run: clean }), /stale libavcodec runtime/);
});

test("macOS bundling clears its staging directory before copying the controlled closure", () => {
  const source = readFileSync(join(root, "scripts/bundle-macos-libs.mjs"), "utf8");
  assert.match(source, /rmSync\(libDir, \{ recursive: true, force: true \}\);\s*mkdirSync\(libDir/);
  assert.match(source, /join\(projectRoot, "target", "audio-runtime"\)/);
  assert.match(source, /return \[match, abiAlias\]/);
});
