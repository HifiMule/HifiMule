# HifiMule — Release Guide

This guide walks through publishing a new GitHub release from a clean `main` branch.

## 1. Pre-release checklist

- [ ] All stories for the milestone are merged to `main`
- [ ] `cargo test` passes locally
- [ ] Bump the version in [`hifimule-ui/src-tauri/tauri.conf.json`](../hifimule-ui/src-tauri/tauri.conf.json) — the `"version"` field at the top
- [ ] Bump the version in [`Cargo.toml`](../Cargo.toml) — the `version` field under `[workspace.package]` (must match `tauri.conf.json`)
- [ ] Commit the version bump: `git commit -m "chore: bump version to X.Y.Z"`
- [ ] Push the commit: `git push origin main`

## 2. Create and push the release tag

The release workflow triggers on any tag that matches `v*`.

```bash
git tag vX.Y.Z
git push origin vX.Y.Z
```

No further action is needed — the CI pipeline starts automatically.

## 3. Monitor the build

Open **Actions → Release** on GitHub. Three parallel jobs build the installers:

| Job | Platform | Artifacts |
|-----|----------|-----------|
| `release (windows-latest)` | Windows | `HifiMule_X.Y.Z_x64.msi` |
| `release (ubuntu-22.04)` | Ubuntu 22.04 | `hifimule_X.Y.Z_amd64.deb`, `hifimule_X.Y.Z_amd64.AppImage` |
| `release (macos-latest)` | macOS (universal) | `HifiMule_X.Y.Z.dmg` |

Jobs are independent (`fail-fast: false`), so a single platform failure doesn't cancel the others.

When all three jobs succeed a **draft release** is created automatically under **Releases** with all four installers attached. If a job fails, check its log — the most common causes are missing system libraries or a Rust compilation error.

Before triggering smoke tests, open the draft release and confirm all four artifacts are present. Each matrix job attaches its own artifacts, and the last job to finish completes the set — checking too early may show a partial list.

## 4. Run smoke tests

Once the draft release exists, trigger the automated smoke-test suite:

1. Go to **Actions → Smoke Test**
2. Click **Run workflow**
3. In the **release_tag** field enter the tag (e.g. `v0.1.0`) and click **Run workflow**

The workflow runs three parallel jobs (`smoke-windows`, `smoke-linux`, `smoke-macos`). Each job:

1. Downloads the installer for its platform from the GitHub release
2. Installs the app silently
3. Launches the app and polls `http://127.0.0.1:19140` with a `daemon.health` JSON-RPC call (30-second timeout)
4. Asserts that the response contains `"status": "ok"`
5. Uninstalls the app

A passing run means the daemon starts and responds correctly on all three platforms. If a job fails, the workflow uploads its log file as an artifact — download it from the **Artifacts** section at the bottom of the run summary page.

> **Note:** Smoke tests do not exercise MTP device detection. Physical device testing requires manual verification on each platform.

## 5. Publish the release

After all smoke tests pass:

1. Go to **Releases** and open the draft
2. Edit the release notes (add changelog, highlight breaking changes if any)
3. Click **Publish release**

The release is now public and visible to users.

## 6. Post-release

- Monitor the [Issues](../../../issues) tab for platform-specific installer reports over the first 24 hours
- If a critical regression is found, yank the release (mark it as pre-release or delete it) and cut a patch tag
