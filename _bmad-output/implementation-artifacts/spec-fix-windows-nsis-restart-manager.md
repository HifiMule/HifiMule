---
title: 'Fix Windows NSIS Restart Manager compatibility'
type: 'bugfix'
created: '2026-10-05'
status: 'done'
route: 'one-shot'
---

# Fix Windows NSIS Restart Manager compatibility

## Intent

**Problem:** Windows release packaging fails with missing RestartManager_StartSession. The custom installer originated from Tauri 2.10.1 and is missing the header required by the 2.12.1 CLI's running-app utility. Its calls also pass bare filenames where the updated utility requires installed executable paths.

**Approach:** Add Win/RestartManager.nsh before installer hooks and macro use; pass $INSTDIR-qualified paths for daemon preinstall and UI install/uninstall checks. Preserve daemon shutdown, startup registration and same-version uninstall behavior. Confirmed against the exact [Tauri CLI 2.12.1 template](https://github.com/tauri-apps/tauri/blob/tauri-cli-v2.12.1/crates/tauri-bundler/src/bundle/windows/nsis/installer.nsi) and [utility macros](https://github.com/tauri-apps/tauri/blob/tauri-cli-v2.12.1/crates/tauri-bundler/src/bundle/windows/nsis/utils.nsh).

Verification: combined Linux runtime, release contract, signing contract and platform configuration suites pass with 78 tests, zero failures and one existing Linux-only test skipped on macOS. All 11 platform configuration tests pass independently in review; no introduced defects found. Source-contract regressions cover header ordering and installed executable paths. Native Windows makensis compilation and installer execution remain unverified because this macOS host has no makensis. One pre-existing uninstall-cancellation registration issue was recorded in deferred-work.md.

## Suggested Review Order

- Provide the Restart Manager prerequisites required by the updated Tauri utility macros.
  [installer.nsi:25](../../hifimule-ui/src-tauri/nsis/installer.nsi#L25)

- Register the installed UI executable path with Restart Manager before replacement or removal.
  [installer.nsi:632](../../hifimule-ui/src-tauri/nsis/installer.nsi#L632)

- Register the installed daemon path before copying private runtime DLLs.
  [hooks.nsh:7](../../hifimule-ui/src-tauri/nsis/hooks.nsh#L7)

- Guard macro prerequisites and path arguments while preserving existing installer behavior checks.
  [tauri-platform-config.test.mjs:68](../../scripts/tests/tauri-platform-config.test.mjs#L68)
