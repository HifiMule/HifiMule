---
title: 'Fix daemon crash on unknown MTP device (null vendor/product)'
type: 'bugfix'
created: '2026-05-12'
status: 'done'
route: 'one-shot'
---

## Intent

**Problem:** Connecting any MTP device not in libmtp's device table (e.g. smartwatch VID=091e/PID=50a4) crashes the daemon with SIGSEGV (signal 11). libmtp sets `device_entry.vendor` and `device_entry.product` to NULL for unknown devices; `libmtp::enumerate()` passed these NULL pointers to `CStr::from_ptr()` unconditionally.

**Approach:** Guard both `CStr::from_ptr` calls with null-pointer checks. Substitute `"Unknown"` for a null vendor and `"USB Device (vid:pid)"` for a null product so the device still appears in the UI with a recognisable label.

## Suggested Review Order

1. [mtp.rs:1901-1929](../../../hifimule-daemon/src/device/mtp.rs) — null guards in `libmtp::enumerate()` map closure

## Spec Change Log

## Design Notes

`vendor_id` and `product_id` are `u16` integer fields — always safe to read even when the pointer fields are null — so the fallback `"USB Device (vid:pid)"` string is always well-formed. The friendly name is display-only; no code paths key on it.
