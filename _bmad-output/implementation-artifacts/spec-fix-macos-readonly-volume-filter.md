---
title: 'Fix: ignore read-only volumes in macOS device scan'
type: 'bugfix'
created: '2026-05-11'
status: 'done'
route: 'one-shot'
---

## Intent

**Problem:** On macOS, mounted DMG files appear in `/Volumes` and pass the existing device-scan filters, causing them to be emitted as `DeviceEvent::Unrecognized`. Because DMGs are read-only, initialization always fails and the user sees a spurious "unrecognized device" prompt. The log warning `overwriting pending unrecognized device` is a symptom of two read-only volumes (e.g., `/Volumes/HifiMule 1` and `/Volumes/HifiMule`) racing through the same unrecognized-device slot.

**Approach:** Add `is_readonly_mount()` (macOS-only) using `statvfs` to check `ST_RDONLY`, and call it inside `get_mounts()` to skip read-only volumes before they reach the device event queue.

## Suggested Review Order

1. [is_readonly_mount helper](../../hifimule-daemon/src/device/mod.rs#L1656) — new helper; verify `statvfs`/`ST_RDONLY` usage
2. [get_mounts() filter call](../../hifimule-daemon/src/device/mod.rs#L1682) — where `is_readonly_mount` is called inside the volume loop
