import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
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
