import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import test from "node:test";

const root = resolve(import.meta.dirname, "../..");
const read = (path) => readFileSync(resolve(root, path), "utf8");
const json = (path) => JSON.parse(read(path));
const normalizeWorkflow = (workflow) => workflow.replace(/\r\n?/g, "\n");
const jobBlock = (workflow, jobName) => {
  const normalizedWorkflow = normalizeWorkflow(workflow);
  const match = normalizedWorkflow.match(new RegExp(`^  ${jobName}:\\n([\\s\\S]*?)(?=^  [a-zA-Z0-9_-]+:|$(?![\\s\\S]))`, "m"));
  assert.ok(match, `workflow must define the ${jobName} job`);
  return match[0];
};
const permissionMap = (workflow, indent) => {
  const normalizedWorkflow = normalizeWorkflow(workflow);
  const padding = " ".repeat(indent);
  const entryPadding = `${padding}  `;
  const header = `${padding}permissions:\n`;
  const start = normalizedWorkflow.indexOf(header);
  assert.notEqual(start, -1, "workflow block must define permissions");

  const permissions = {};
  for (const line of normalizedWorkflow.slice(start + header.length).split("\n")) {
    const match = line.match(new RegExp(`^${entryPadding}([a-z-]+): (read|write|none)$`));
    if (match) {
      assert.ok(!(match[1] in permissions), `workflow permissions contain duplicate entry: ${match[1]}`);
      permissions[match[1]] = match[2];
      continue;
    }
    if (line.trim() === "" || line.trimStart().startsWith("#")) continue;
    if (/^\s*\t/.test(line)) assert.fail(`workflow permissions contain tab-indented entry: ${line.trim()}`);
    if (line.match(/^ */)[0].length <= indent) break;
    assert.fail(`workflow permissions contain malformed entry: ${line.trim()}`);
  }
  assert.ok(Object.keys(permissions).length > 0, "workflow permissions must define read, write, or none entries");
  return permissions;
};

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

test("Cargo and Tauri versions agree with the platform bundle targets", () => {
  const workspacePackage = read("Cargo.toml").replace(/\r\n?/g, "\n").split("[workspace.package]")[1]?.split(/\n\[/)[0];
  assert.ok(workspacePackage, "Cargo must define [workspace.package]");
  const cargoVersion = workspacePackage.match(/^version = "([^"]+)"$/m)?.[1];
  assert.ok(cargoVersion, "Cargo workspace package must define a version");
  assert.equal(json("hifimule-ui/src-tauri/tauri.conf.json").version, cargoVersion);
  assert.deepEqual(json("hifimule-ui/src-tauri/tauri.windows.conf.json").bundle.targets, ["msi", "nsis"]);
  assert.deepEqual(json("hifimule-ui/src-tauri/tauri.linux.conf.json").bundle.targets, ["deb", "appimage", "rpm"]);
  assert.deepEqual(json("hifimule-ui/src-tauri/tauri.macos.conf.json").bundle.targets, ["app", "dmg"]);
});

test("Tauri runtime, JavaScript API and CLI share a stable minor release", () => {
  const runtime = read("Cargo.lock").match(/\[\[package\]\]\s*name = "tauri"\s*version = "([^"]+)"/)?.[1];
  assert.ok(runtime, "Cargo lockfile must resolve Tauri");
  const family = (version) => {
    const match = version.match(/^[~^]?(\d+\.\d+)(?:\.\d+)?$/);
    assert.ok(match, `expected a stable Tauri version: ${version}`);
    return match[1];
  };
  const manifest = json("hifimule-ui/package.json");
  const lock = json("hifimule-ui/package-lock.json");
  for (const [name, section] of [["@tauri-apps/api", "dependencies"], ["@tauri-apps/cli", "devDependencies"]]) {
    assert.equal(family(manifest[section][name]), family(runtime), `${name} manifest must match Rust runtime`);
    assert.equal(lock.packages[""][section][name], manifest[section][name], `${name} lock declaration must match manifest`);
    assert.equal(family(lock.packages[`node_modules/${name}`].version), family(runtime), `${name} resolved version must match Rust runtime`);
  }
});

test("Tauri opener versions are pinned together across Rust and JavaScript", () => {
  const cargoManifestVersion = read("hifimule-ui/src-tauri/Cargo.toml").match(/^tauri-plugin-opener = "=([^"]+)"$/m)?.[1];
  const cargoLockVersion = read("Cargo.lock").match(/\[\[package\]\]\s*name = "tauri-plugin-opener"\s*version = "([^"]+)"/)?.[1];
  const packageJsonVersion = json("hifimule-ui/package.json").dependencies["@tauri-apps/plugin-opener"];
  const packageLock = json("hifimule-ui/package-lock.json");
  assert.ok(cargoManifestVersion, "Rust opener version must be exact");
  assert.equal(cargoLockVersion, cargoManifestVersion);
  assert.equal(packageJsonVersion, cargoManifestVersion);
  assert.equal(packageLock.packages[""].dependencies["@tauri-apps/plugin-opener"], cargoManifestVersion);
  assert.equal(packageLock.packages["node_modules/@tauri-apps/plugin-opener"].version, cargoManifestVersion);
});

test("release verifies extracted RPM private libraries before candidate upload", () => {
  const workflow = read(".github/workflows/release.yml");
  const rpm = workflow.split(/(?=^      - )/m).find(step => step.includes("name: Verify Linux rpm"));
  assert.ok(rpm);
  assert.match(workflow, /libarchive-tools rpm/);
  assert.match(rpm, /set -euo pipefail/);
  assert.match(rpm, /bsdtar -xf "\$RPM" -C rpm-root/);
  assert.match(rpm, /linux-audio-runtime\.mjs verify-bundle.*rpm-root/);
  assert.ok(workflow.indexOf(rpm) < workflow.indexOf("name: Upload immutable candidate"));
  const build = normalizeWorkflow(read(".github/workflows/build.yml"));
  const install = build.indexOf("name: Install Linux RPM regression extractor");
  assert.ok(install >= 0 && install < build.indexOf("name: Run packaging and runtime script regressions"));
  assert.match(build.slice(install, build.indexOf("name: Run packaging and runtime script regressions")), /if: runner.os == 'Linux'[\s\S]*apt-get install -y libarchive-tools/);
});

const workflowRun = (stepName) => {
  const workflow = normalizeWorkflow(read(".github/workflows/release.yml"));
  const step = workflow.split(/(?=^      - )/m).find((entry) => entry.includes(`name: ${stepName}`));
  assert.ok(step, `workflow must define ${stepName}`);
  const run = step.match(/^        run: \|\n([\s\S]*)/m)?.[1];
  assert.ok(run, `${stepName} must define a shell script`);
  return run.replace(/^          /gm, "").trimEnd();
};
const discoveryScripts = () => [
  ...[["deb", "DEB", "Deb"], ["rpm", "RPM", "RPM"]].map(([extension, variable, label]) => ({
    name: extension,
    script: workflowRun(`Verify Linux ${extension}`).split(extension === "rpm" ? "rpm -qp" : "rm -rf deb-root")[0]
      + `printf '%s\\n' "$${variable}"\n`,
    missing: `${label} artifact not found`,
  })),
  {
    name: "checksums",
    // Exercise the entire checksum collection, stopping before unrelated source-revision metadata.
    script: workflowRun("Write immutable candidate checksums").split("git rev-parse HEAD")[0],
    missing: "Candidate bundle artifacts not found",
  },
];
const releaseFixture = (t) => {
  const cwd = mkdtempSync(join(tmpdir(), "hifimule-release-discovery-"));
  t.after(() => rmSync(cwd, { recursive: true, force: true }));
  return cwd;
};
const runDiscovery = (script, cwd) => spawnSync("bash", ["-c", script], {
  cwd,
  encoding: "utf8",
  env: { ...process.env, CANDIDATE_ROW: "linux-x64", CANDIDATE_VERSION: "0.17.0" },
});

test("release artifact discovery and candidate checksums accept either target directory alone", { skip: process.platform === "win32" }, (t) => {
  for (const targetDir of ["target", "hifimule-ui/src-tauri/target"]) {
    const cwd = releaseFixture(t);
    const artifacts = ["deb", "rpm"].map((extension) => `${targetDir}/x86_64-unknown-linux-gnu/release/bundle/${extension}/HifiMule release.${extension}`);
    for (const artifact of artifacts) {
      mkdirSync(dirname(join(cwd, artifact)), { recursive: true });
      writeFileSync(join(cwd, artifact), `package ${artifact}`);
    }
    for (const { name, script } of discoveryScripts()) {
      const result = runDiscovery(script, cwd);
      assert.equal(result.status, 0, `${targetDir}: ${name}: ${result.stdout}${result.stderr}`);
      if (name === "checksums") {
        const checksums = readFileSync(join(cwd, "candidate-metadata/linux-x64-0.17.0.sha256"), "utf8");
        const expected = artifacts.sort().map((artifact) => {
          const digest = createHash("sha256").update(readFileSync(join(cwd, artifact))).digest("hex");
          return `${digest}  ${artifact}\n`;
        }).join("");
        assert.equal(checksums, expected);
      } else {
        assert.equal(result.stdout.trim(), artifacts.find((artifact) => artifact.endsWith(`.${name}`)));
      }
    }
  }
});

test("release discovery fails explicitly when existing target directories have no packages", { skip: process.platform === "win32" }, (t) => {
  const cwd = releaseFixture(t);
  mkdirSync(join(cwd, "target/release"), { recursive: true });
  for (const { name, script, missing } of discoveryScripts()) {
    const result = runDiscovery(script, cwd);
    assert.notEqual(result.status, 0, `${name} must reject missing packages`);
    assert.ok(result.stdout.includes(missing), `${name}: ${result.stdout}${result.stderr}`);
  }
});

test("release discovery fails explicitly when no target directory exists", { skip: process.platform === "win32" }, (t) => {
  const cwd = releaseFixture(t);
  for (const { name, script } of discoveryScripts()) {
    const result = runDiscovery(script, cwd);
    assert.notEqual(result.status, 0, `${name} must reject missing target directories`);
    assert.ok(result.stdout.includes("No Tauri target directory found"), `${name}: ${result.stdout}${result.stderr}`);
  }
});

test("RPM verification extracts an rpm-rs package and preserves extraction and verifier failures", { skip: process.platform === "win32" }, (t) => {
  // macOS tar is libarchive's bsdtar; Ubuntu installs the bsdtar executable explicitly.
  const extractor = process.platform === "darwin" ? "/usr/bin/tar" : "bsdtar";
  const available = spawnSync(extractor, ["--version"], { encoding: "utf8" });
  if (available.error?.code === "ENOENT" && !process.env.CI) return t.skip("libarchive-tools is not installed");
  assert.equal(available.status, 0);
  assert.match(available.stdout, /bsdtar|libarchive/);
  const fixture = readFileSync(resolve(root, "scripts/tests/fixtures/rpm/rpm-rs-0.16.0.rpm"));
  assert.equal(createHash("sha256").update(fixture).digest("hex"), "57886c7ed2136f563e42a1c9386bf5cd5263e02e3aad97ae07fd2ab274adc154");
  for (const scenario of ["valid", "invalid", "extractor-failure", "verifier-failure"]) {
    const cwd = releaseFixture(t);
    const artifact = join(cwd, "target/release/bundle/rpm/HifiMule release.rpm");
    mkdirSync(dirname(artifact), { recursive: true });
    writeFileSync(artifact, scenario === "invalid" ? "invalid RPM archive" : fixture);
    const bin = join(cwd, "bin");
    mkdirSync(bin);
    const stubs = {
      rpm: "exit 0",
      rustc: 'echo "host: x86_64-unknown-linux-gnu"',
      "extract-then-fail": `"${extractor}" "$@" || exit 2\nexit 5`,
      node: `test "$*" = "scripts/linux-audio-runtime.mjs verify-bundle x86_64-unknown-linux-gnu rpm-root" || exit 2
test "$(cat rpm-root/usr/share/hifimule-fixture/payload.txt)" = "RPM extraction regression" || exit 3
touch verifier-ran
exit ${scenario === "verifier-failure" ? 7 : 0}`,
    };
    for (const [name, body] of Object.entries(stubs)) {
      const path = join(bin, name);
      writeFileSync(path, `#!/bin/sh\n${body}\n`);
      chmodSync(path, 0o755);
    }
    const command = scenario === "extractor-failure" ? "extract-then-fail" : extractor;
    const script = workflowRun("Verify Linux rpm").replace('bsdtar -xf', `${command} -xf`);
    const result = spawnSync("bash", ["-c", script], {
      cwd, encoding: "utf8", env: { ...process.env, PATH: `${bin}:${process.env.PATH}` },
    });
    if (scenario === "valid") {
      assert.equal(result.status, 0, result.stdout + result.stderr);
      assert.equal(readFileSync(join(cwd, "verifier-ran"), "utf8"), "");
    } else if (scenario === "invalid" || scenario === "extractor-failure") {
      assert.notEqual(result.status, 0, "extractor errors must block verification");
      if (scenario === "extractor-failure") {
        assert.equal(result.status, 5);
        assert.equal(readFileSync(join(cwd, "rpm-root/usr/share/hifimule-fixture/payload.txt"), "utf8"), "RPM extraction regression\n");
      }
      assert.throws(() => readFileSync(join(cwd, "verifier-ran")), { code: "ENOENT" });
    } else {
      assert.equal(result.status, 7, "private-runtime verifier failures must remain blocking");
    }
  }
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

test("smoke callers and callee retain draft-release visibility without publishing", () => {
  const releaseWorkflow = read(".github/workflows/release.yml");
  const smokeWorkflow = read(".github/workflows/smoke-test.yml");
  const draftPermissions = { contents: "write", actions: "read" };

  assert.deepEqual(permissionMap(smokeWorkflow, 0), draftPermissions);
  assert.deepEqual(permissionMap(jobBlock(releaseWorkflow, "smoke-release"), 4), draftPermissions);
  assert.deepEqual(permissionMap(jobBlock(releaseWorkflow, "smoke-candidate"), 4), draftPermissions);
  assert.deepEqual(permissionMap(jobBlock(releaseWorkflow, "release"), 4), { contents: "write" });
  assert.deepEqual(permissionMap(jobBlock(releaseWorkflow, "prepare-release"), 4), { contents: "write" });
  assert.equal(releaseWorkflow.match(/contents: write/g)?.length, 4);
  assert.match(releaseWorkflow, /releaseDraft: true/);
  assert.doesNotMatch(smokeWorkflow, /gh release (?:edit|create)|--draft=false/);
  assert.match(smokeWorkflow, /GH_TOKEN: \$\{\{ secrets\.GITHUB_TOKEN \}\}/);
});

test("tag builds use one prepared draft and an immutable source revision", () => {
  const workflow = normalizeWorkflow(read(".github/workflows/release.yml"));
  const prepare = jobBlock(workflow, "prepare-release");
  const release = jobBlock(workflow, "release");

  assert.match(prepare, /release-id: \$\{\{ steps\.draft\.outputs\.release-id \}\}/);
  assert.match(prepare, /contents: write/);
  assert.match(prepare, /prepare-draft-release\.cjs/);
  assert.match(prepare, /ref: \$\{\{ github\.sha \}\}/);
  assert.match(release, /needs: prepare-release/);
  assert.match(release, /ref: \$\{\{ inputs\.candidate_ref \|\| github\.sha \}\}/);
  const revalidate = release.split(/(?=^      - )/m).find((step) => step.includes('name: Revalidate prepared draft before packaging'));
  assert.ok(revalidate, 'failed platform reruns must revalidate saved preparation output');
  assert.match(revalidate, /if: github\.event_name == 'push'/);
  assert.match(revalidate, /PREPARED_RELEASE_ID: \$\{\{ needs\.prepare-release\.outputs\.release-id \}\}/);
  assert.match(revalidate, /releaseId: process\.env\.PREPARED_RELEASE_ID/);
  assert.ok(release.indexOf(revalidate) < release.indexOf('name: Build draft release'));

  const tauriSteps = release.split(/(?=^      - )/m).filter((step) => step.includes("uses: tauri-apps/tauri-action@"));
  assert.equal(tauriSteps.length, 3, "all distribution-signing branches remain covered");
  for (const step of tauriSteps) {
    assert.match(step, /if: github\.event_name == 'push'/);
    assert.match(step, /releaseId: \$\{\{ needs\.prepare-release\.outputs\.release-id \}\}/);
    assert.match(step, /releaseDraft: true/);
  }
  assert.match(workflow, /concurrency:\n  group: .*github\.ref/);
  assert.match(workflow, /cancel-in-progress: false/);
  assert.match(workflow, /group: .*github\.run_id/, "candidate runs must not compete with tag builds");
});

test("candidate preparation is a successful no-op, not a skipped dependency", () => {
  const workflow = normalizeWorkflow(read(".github/workflows/release.yml"));
  const prepare = jobBlock(workflow, "prepare-release");
  assert.doesNotMatch(prepare, /^    if:/m, "skipping the preparation job would skip its candidate dependents");
  const steps = prepare.split(/(?=^      - )/m).slice(1);
  assert.ok(steps.length > 0);
  for (const step of steps) {
    assert.match(step, /if: github\.event_name == 'push'/, "candidates must perform no release preparation operations");
  }
  const candidateSmoke = jobBlock(workflow, "smoke-candidate");
  assert.match(candidateSmoke, /if: github\.event_name == 'workflow_dispatch'/);
  assert.match(candidateSmoke, /needs: release/);
});

test("workflow permission parsing is independent of line endings", () => {
  const releaseWorkflow = normalizeWorkflow(read(".github/workflows/release.yml"));
  const smokeWorkflow = normalizeWorkflow(read(".github/workflows/smoke-test.yml"));
  const expectedPermissions = {
    smoke: { contents: "write", actions: "read" },
    release: { contents: "write" },
  };

  for (const [name, newline] of [["LF", "\n"], ["CRLF", "\r\n"], ["CR", "\r"]]) {
    const releaseVariant = releaseWorkflow.replaceAll("\n", newline);
    const smokeVariant = smokeWorkflow.replaceAll("\n", newline);
    const malformedVariant = ["permissions:", "  contents: read", "", "  actions: invalid", "jobs:"].join(newline);

    assert.deepEqual(permissionMap(smokeVariant, 0), expectedPermissions.smoke, `${name} callee permissions`);
    assert.deepEqual(permissionMap(jobBlock(releaseVariant, "smoke-release"), 4), expectedPermissions.smoke, `${name} smoke-release permissions`);
    assert.deepEqual(permissionMap(jobBlock(releaseVariant, "smoke-candidate"), 4), expectedPermissions.smoke, `${name} smoke-candidate permissions`);
    assert.deepEqual(permissionMap(jobBlock(releaseVariant, "release"), 4), expectedPermissions.release, `${name} release permissions`);
    assert.throws(() => permissionMap(malformedVariant, 0), /malformed entry: actions: invalid/, `${name} malformed permission entry`);
  }
});

test("workflow permission parsing still rejects missing and malformed-first blocks", () => {
  assert.throws(() => permissionMap("name: Missing permissions\n", 0), /workflow block must define permissions/);
  assert.throws(() => permissionMap("permissions:\r\n  contents: invalid\r\n", 0), /malformed entry: contents: invalid/);
  assert.throws(() => permissionMap("permissions:\n  contents: read\n\tactions: write\n", 0), /tab-indented entry: actions: write/);
  assert.throws(() => permissionMap("permissions:\n  contents: read\n  contents: write\n", 0), /duplicate entry: contents/);
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
