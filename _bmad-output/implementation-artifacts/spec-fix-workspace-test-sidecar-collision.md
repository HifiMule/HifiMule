---
title: 'Prevent packaged sidecars from overwriting Cargo test binaries'
type: 'bugfix'
created: '2026-10-24'
status: 'done'
route: 'one-shot'
baseline_commit: 'e6352e1'
---

# Prevent packaged sidecars from overwriting Cargo test binaries

## Intent

**Problem:** Full workspace Cargo tests also build the Tauri UI. Tauri copies configured packaged sidecars into the shared Cargo output directory, replacing `target/debug/mediacheck` (and potentially `hifimule-daemon`) with packaged binaries. On macOS those binaries have app-bundle-relative library paths, so CLI integration tests abort in dyld before assertions. The supplied log reports missing `@executable_path/../Resources/bundled-libs/libmtp.9.dylib`. Targeted daemon CLI tests pass because they do not build the UI. This build-output collision is separate from the Jellyfin playlist implementation.

**Approach:** For the wrapper's Cargo `test` command, merge an empty `bundle.externalBin` array into the inherited `TAURI_CONFIG`. This prevents Tauri's sidecar copy step while retaining all workspace tests, existing configuration, native runtime verification, and normal build/packaging behavior. No fallback loader paths or skips were added to the CLI integration tests.

**Validation:** `rtk npm run build:daemon -- test` passed with **1,750 passed, 0 failed, 14 ignored**, including all five `mediacheck_cli` tests. The wrapper, macOS runtime, and mediacheck packaging Node tests passed via `rtk test node --test scripts/tests/build-daemon.test.mjs scripts/tests/macos-audio-runtime.test.mjs scripts/tests/mediacheck-packaging.test.mjs`. `rtk git diff --check` passed. The full workspace log is `/tmp/hifimule-workspace-tests.log` (temporary diagnostic output, not committed).

**Review:** An independent read-only review found no substantive defects. A speculative coverage concern about Cargo global arguments preceding the command was not expanded into this fix; the supported wrapper invocations exercised here put `test` first. No deferred defect was accepted.

## Suggested Review Order

- Disable sidecar copying only during Cargo tests while retaining inherited Tauri settings.
  [build-daemon.mjs:83](../../scripts/build-daemon.mjs#L83)
- Check test suppression, configuration preservation, caller environment isolation, and unchanged normal builds.
  [build-daemon.test.mjs:93](../../scripts/tests/build-daemon.test.mjs#L93)
