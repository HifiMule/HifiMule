---
title: 'Fix: Skip MTP Device Init When Storage Is Inaccessible (Android Charge-Only Mode)'
type: 'bugfix'
created: '2026-05-15'
status: 'done'
route: 'one-shot'
context: []
---

## Intent

**Problem:** When an Android phone is connected via USB in charge-only mode, the MTP interface is still advertised but `LIBMTP_Get_Storage` fails. The app detects the device, emits `DeviceEvent::Unrecognized`, and shows the Initialize button — but initialization fails because no storage is accessible.

**Approach:** Treat `LIBMTP_Get_Storage` failure as a hard error in `LibmtpHandle::open()`: release the device handle and return `Err`. The observer already handles `create_mtp_backend` errors gracefully (logs and skips), so the device is silently excluded. On the next 2-second poll the check is retried — once the user enables MTP mode the device is detected normally.

## Suggested Review Order

1. [hifimule-daemon/src/device/mtp.rs](../../../hifimule-daemon/src/device/mtp.rs) — `LibmtpHandle::open()`: the new early-return on `LIBMTP_Get_Storage` failure (~line 1592). Verify `LIBMTP_Release_Device` is called before the return and the device pointer is not subsequently wrapped in `Arc` (no double-free path).
2. [hifimule-daemon/src/device/mod.rs](../../../hifimule-daemon/src/device/mod.rs) — `run_mtp_observer` (~line 1524): confirm the `Err` branch only logs and does not insert into `known_ids`, so the device is re-evaluated on the next poll.
