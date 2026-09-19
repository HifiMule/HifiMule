import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import test from "node:test";

const root = resolve(import.meta.dirname, "../..");
const read = (path) => readFileSync(resolve(root, path), "utf8");
const json = (path) => JSON.parse(read(path));

test("0.15.0 release contract fixes the four shipping rows and package formats", () => {
  const contract = json("docs/playback-evidence/release-contract-0.15.0.json");
  assert.equal(contract.schemaVersion, 1);
  assert.equal(contract.releaseVersion, "0.15.0");
  assert.equal(contract.upgradeBaseline, "0.14.0");
  assert.deepEqual(
    contract.rows.map(({ id, os, architecture, packages, minimumOs }) => ({ id, os, architecture, packages, minimumOs })),
    [
      { id: "windows-x64", os: "windows", architecture: "x86_64", packages: ["msi", "nsis"], minimumOs: "Windows 10" },
      { id: "linux-x64", os: "linux", architecture: "x86_64", packages: ["deb", "appimage"], minimumOs: "Ubuntu 22.04" },
      { id: "macos-x64", os: "macos", architecture: "x86_64", packages: ["app", "dmg"], minimumOs: "macOS 10.15" },
      { id: "macos-arm64", os: "macos", architecture: "aarch64", packages: ["app", "dmg"], minimumOs: "macOS 11" },
    ],
  );
  assert.deepEqual(contract.decisionVocabulary, ["pass", "blocker", "unsupported"]);
  for (const row of contract.rows) {
    assert.ok(row.cleanInstallFixture);
    assert.ok(row.upgradeFixture);
    assert.ok(row.evidenceOwner);
    assert.ok(row.outputExpectation);
  }
  assert.ok(contract.providers.some(({ kind }) => kind === "jellyfin"));
  assert.ok(contract.providers.some(({ kind }) => kind === "subsonic"));
});

test("Cargo, Tauri and platform bundle targets agree with the release contract", () => {
  assert.match(read("Cargo.toml"), /\[workspace\.package\][\s\S]*?version = "0\.15\.0"/);
  assert.equal(json("hifimule-ui/src-tauri/tauri.conf.json").version, "0.15.0");
  assert.deepEqual(json("hifimule-ui/src-tauri/tauri.windows.conf.json").bundle.targets, ["msi", "nsis"]);
  assert.deepEqual(json("hifimule-ui/src-tauri/tauri.linux.conf.json").bundle.targets, ["deb", "appimage"]);
  assert.deepEqual(json("hifimule-ui/src-tauri/tauri.macos.conf.json").bundle.targets, ["app", "dmg"]);
});

test("release workflow supports explicit immutable candidates without publishing", () => {
  const workflow = read(".github/workflows/release.yml");
  assert.match(workflow, /workflow_dispatch:/);
  assert.match(workflow, /candidate_ref:/);
  assert.match(workflow, /candidate_version:/);
  assert.match(workflow, /actions\/checkout@v6[\s\S]*?ref: \$\{\{ inputs\.candidate_ref/);
  assert.match(workflow, /Install Rust 1\.93\.0/);
  assert.match(workflow, /dtolnay\/rust-toolchain@1\.93\.0/);
  assert.match(workflow, /Build immutable candidate/);
  assert.match(workflow, /Upload immutable candidate/);
  assert.match(workflow, /if: github\.event_name == 'push'/);
  assert.match(workflow, /releaseDraft: true/);
});

test("release guide states certification boundaries and the four-row workflow", () => {
  const guide = read("docs/release-guide.md");
  for (const phrase of [
    "Windows x64",
    "Linux x64",
    "macOS x64",
    "macOS ARM64",
    "Windows 10",
    "Ubuntu 22.04",
    "macOS 10.15",
    "macOS 11",
    "draft",
    "physical media keys",
    "does not certify",
  ]) assert.ok(guide.includes(phrase), `release guide must include ${phrase}`);
});
