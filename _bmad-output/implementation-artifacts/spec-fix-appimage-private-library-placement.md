---
title: 'Preserve the private library closure in AppImage loader paths'
type: 'bugfix'
created: '2026-09-20'
status: 'done'
route: 'one-shot'
---

# Preserve the private library closure in AppImage loader paths

## Intent

**Problem:** Ubuntu release verification reports missing `libusb-1.0.so.0` required by `usr/lib/libmtp.so.9`. The staging script recursively collects libmtp dependencies, but ordinary Tauri resource mappings place those copies in nested resources. Linuxdeploy rewrites the daemon RUNPATH to `usr/lib` and its dependency discovery excludes some libraries, including libusb. A resource copy outside the effective loader directory does not satisfy the private closure contract.

**Approach:** Use Tauri's AppImage custom-file mapping, `usr/lib` -> `bundled-libs`, to copy the entire prepared closure into the loader directory before linuxdeploy runs. This covers excluded transitive libraries without an ever-growing per-library exception list. Preserve the deb resource layout and all architecture, SONAME, RUNPATH, symlink, and dependency checks.

A regression fixture reproduces the libmtp/libusb failure with libusb present only in nested resources, applies the actual AppImage custom-file mapping, verifies success, and confirms removal still fails. All 208 packaging-script tests pass locally. No native Ubuntu AppImage rebuild was performed on this macOS host; CI remains the native packaging confirmation.

Upstream implementation checked:
- [Tauri AppImage custom-file copy before linuxdeploy](https://raw.githubusercontent.com/tauri-apps/tauri/tauri-cli-v2.10.1/crates/tauri-bundler/src/bundle/linux/appimage/linuxdeploy.rs)
- [Tauri destination-to-source mapping and directory-copy semantics](https://raw.githubusercontent.com/tauri-apps/tauri/tauri-cli-v2.10.1/crates/tauri-bundler/src/utils/fs_utils.rs)
- [AppImage excluded dependencies, including libusb](https://raw.githubusercontent.com/AppImageCommunity/pkg2appimage/master/excludelist)

## Suggested Review Order

- Populate the final AppImage loader directory from the already prepared private closure.
  [tauri.linux.conf.json:9](../../hifimule-ui/src-tauri/tauri.linux.conf.json#L9)
- Reproduce missing reachable libusb, apply configured placement, and retain fail-closed verification.
  [linux-audio-runtime.test.mjs:367](../../scripts/tests/linux-audio-runtime.test.mjs#L367)
- Guard destination/source direction and retain the deb layout.
  [tauri-platform-config.test.mjs:33](../../scripts/tests/tauri-platform-config.test.mjs#L33)
