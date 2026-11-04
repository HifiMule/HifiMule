---
title: 'Fix Windows PROPVARIANT ownership'
type: 'bugfix'
created: '2026-11-04'
status: 'done'
route: 'one-shot'
---

# Fix Windows PROPVARIANT ownership

## Intent

**Problem:** Windows MSI installation aborts when shortcut registration destroys a VT_LPWSTR PROPVARIANT pointing into a borrowed HSTRING buffer. The windows 0.62.2 dependency owns and clears variants on drop. The MTP object-ID collection helper also manually frees memory owned by its variant.

**Approach:** Use `PROPVARIANT::from(&str)` for the shortcut AppUserModelID and let the MTP variant alone release its CoTaskMemAlloc buffer. Preserve checked installer registration and the MTP string type. Add a Windows regression test that creates a real shortcut, registers its identity twice, and reads the saved property; explicitly run that test in Windows CI.

## Suggested Review Order

- Use an owning string conversion to prevent the installer helper's invalid free.
  [main.rs:315](../../hifimule-daemon/src/main.rs#L315)
- Remove the duplicate release of a variant-owned MTP object-ID buffer.
  [mtp.rs:858](../../hifimule-daemon/src/device/mtp.rs#L858)
- Exercise helper cleanup and verify persisted shortcut identity on Windows.
  [tests.rs:4](../../hifimule-daemon/src/tests.rs#L4)
- Execute the Windows regression test in the existing provisioned CI environment.
  [build.yml:132](../../.github/workflows/build.yml#L132)
