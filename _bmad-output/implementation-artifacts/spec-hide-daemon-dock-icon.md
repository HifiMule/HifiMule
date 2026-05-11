---
title: 'Hide Daemon Dock Icon on macOS'
type: 'feature'
created: '2026-05-11'
status: 'done'
route: 'one-shot'
---

## Intent

**Problem:** On macOS, the HifiMule daemon shows a Dock icon because `tao`'s event loop registers as a full `NSApplication` by default. Since the daemon already has a system tray icon, the Dock icon is redundant and misleading.

**Approach:** Set `ActivationPolicy::Accessory` on the `EventLoop` before calling `run()`, using `tao`'s `EventLoopExtMacOS` trait. This is the standard macOS pattern for tray-only apps (equivalent to `LSUIElement = true`).

## Suggested Review Order

- [hifimule-daemon/src/main.rs:353-359](../../hifimule-daemon/src/main.rs#L353) — the change: `mut` binding + macOS activation policy block

## Spec Change Log

## Design Notes

`tao` 0.31 exposes `EventLoopExtMacOS::set_activation_policy(&mut self, ActivationPolicy)` which must be called before `run()`. `ActivationPolicy::Accessory` is the correct value — it hides the Dock icon and removes the app from Cmd-Tab, while leaving the system tray icon fully functional.

The call is gated with `#[cfg(target_os = "macos")]` so Windows and Linux are unaffected. The `tao::platform::macos` module is itself `cfg`-gated, making the import safe to compile on all platforms.
