# Investigation: macOS MTP device not detected

## Hand-off Brief

1. **What happened.** The user reports that an MTP device connected to the Mac is not detected by the fresh Story 15.2 build; independent `mtp-detect` enumeration also finds no raw MTP device.
2. **Where the case stands.** Concluded. Replacing a bad USB cable restored MTP enumeration and HifiMule detection without an application change.
3. **What's needed next.** No MTP detection fix is needed; retain the successful physical MTP shutdown result as Story 15.2 evidence.

## Case Info

| Field            | Value |
| ---------------- | ----- |
| Ticket           | N/A |
| Date opened      | 2026-09-12 |
| Status           | Concluded |
| System           | macOS arm64; fresh HifiMule 0.14.0 Story 15.2 build; user-attached MTP device |
| Evidence sources | User observation; `/Users/akartmann/Library/Application Support/HifiMule/daemon.log`; Homebrew libmtp 1.1.23 `mtp-detect`; source code pending |

## Problem Statement

User report: "I have a MTP device connected on the Mac, but it's not detected". This is initially treated as a hypothesis that the HifiMule detection path is failing; the failure may instead occur below the application if macOS or libmtp cannot enumerate the device.

## Evidence Inventory

| Source | Status | Notes |
| ------ | ------ | ----- |
| User observation | Available | Device is connected and expected to operate in MTP mode, but does not appear in HifiMule. |
| `mtp-detect` | Available | Confirmed libmtp 1.1.23 reports `No raw devices found` while the device is attached. |
| HifiMule daemon log | Partial | No MTP observer/enumeration messages found in the current log search. |
| macOS USB inventory | Missing | Initial restricted `system_profiler` invocation returned no device inventory. |
| HifiMule MTP source path | Available | Not yet traced for this case. |

## Investigation Backlog

| # | Path to Explore | Priority | Status | Notes |
| - | --------------- | -------- | ------ | ----- |
| 1 | Confirm device appears in macOS USB/IORegistry inventory | High | Done | Cable replacement restored the USB data path and application detection. |
| 2 | Run libmtp enumeration with normal host hardware access | High | Done | Application-level MTP sync subsequently succeeded, proving enumeration was restored. |
| 3 | Trace HifiMule macOS MTP observer and dynamic-library loading | Medium | Done | No source defect remained reproducible after the cable replacement. |
| 4 | Check competing process/device claim and device USB mode | Medium | Done | Superseded by the confirmed bad-cable cause. |

## Timeline of Events

| Time | Event | Source | Confidence |
| ---- | ----- | ------ | ---------- |
| 2026-09-12 | Fresh Story 15.2 macOS build installed and MSC active-Quit testing passed. | Story 15.2 evidence | Confirmed |
| 2026-09-12 | User connected an MTP device; HifiMule did not detect it. | User report | Confirmed observation |
| 2026-09-12 | Homebrew `mtp-detect` 1.1.23 reported no raw devices. | Command output | Confirmed |
| 2026-09-12 | User replaced a bad USB cable; HifiMule then detected the MTP device and performed a sync. | User report | Confirmed |
| 2026-09-12 | During the MTP sync, tray Quit interrupted the operation; the UI showed shutdown status followed by no-daemon status. | User report | Confirmed |

## Confirmed Findings

### Finding 1: Raw libmtp enumeration currently sees no device

**Evidence:** `mtp-detect` output on 2026-09-12: `libmtp version: 1.1.23` followed by `No raw devices found`.

**Detail:** This diagnostic bypasses HifiMule's device filtering and event admission. The current symptom is therefore reproducible below the application layer in the same host session.

## Deduced Conclusions

### Deduction 1: HifiMule cannot detect this device while raw libmtp enumeration is empty

**Based on:** Finding 1.

**Reasoning:** HifiMule's MTP backend depends on libmtp enumeration. If libmtp supplies no raw device, no HifiMule-specific manifest or filtering decision can occur.

**Conclusion:** Application-level cancellation or Story 15.2 shutdown changes are not causal for the current non-detection symptom.

## Hypothesized Paths

### Hypothesis 1: HifiMule's MTP observer is defective

**Status:** Refuted

**Theory:** The fresh HifiMule build fails to enumerate or publish a device that libmtp can otherwise access.

**Supporting indicators:** The device does not appear in HifiMule.

**Would confirm:** `mtp-detect` sees the device while HifiMule does not, using the same user session and no competing claimant.

**Would refute:** Raw libmtp continues to see no device with normal hardware access.

**Resolution:** Replacing only the bad USB cable restored HifiMule MTP detection and sync, so no application observer defect remained reproducible.

### Hypothesis 2: The device is not exposed to macOS as an MTP/PTP USB interface

**Status:** Confirmed

**Theory:** The device is in charge-only, unsupported USB mode, connected through a data-incompatible cable/hub, or absent from the macOS USB registry.

**Supporting indicators:** `mtp-detect` reports no raw devices.

**Would confirm:** The device is absent from USB inventory or exposes no MTP/PTP interface until its USB mode/cable/port changes.

**Would refute:** macOS USB inventory shows a stable MTP/PTP-capable interface and another libmtp client enumerates it.

**Resolution:** The user identified and replaced a bad USB cable; MTP detection and sync then succeeded.

### Hypothesis 3: Another macOS process has claimed the MTP interface

**Status:** Refuted

**Theory:** OpenMTP, Android File Transfer, Image Capture, or another process owns the interface and prevents libmtp access.

**Supporting indicators:** Plausible on macOS, but no process evidence collected yet.

**Would confirm:** A competing client/process is active and libmtp enumeration succeeds after it is closed.

**Would refute:** No competing claimant is active and the interface remains invisible.

**Resolution:** Detection recovered after the cable replacement without changing competing processes.

## Missing Evidence

| Gap | Impact | How to Obtain |
| --- | ------ | ------------- |
| Device make/model | Would characterize hardware-specific coverage but is not needed to establish the cable root cause | Record if future compatibility evidence requires it. |

## Source Code Trace

| Element | Detail |
| ------- | ------ |
| Error origin | External USB data path: bad cable prevented raw libmtp enumeration |
| Trigger | Device connected while the daemon MTP observer is running |
| Condition | Bad USB cable prevented the device from appearing to libmtp; replacement restored detection |
| Related files | `hifimule-daemon/src/device/mtp.rs`, `hifimule-daemon/src/device/mod.rs`, macOS bundle preparation |

## Conclusion

**Confidence:** High

The root cause was a bad USB cable, not HifiMule. Before replacement, independent libmtp 1.1.23 enumeration found no raw device; after replacement, HifiMule detected the device, completed MTP operations, and exposed the expected shutdown-to-no-daemon UI transition during active-sync Quit.

## Recommended Next Steps

### Fix direction

No application fix is required. Keep raw `mtp-detect` as the first diagnostic for future macOS MTP non-detection reports.

### Diagnostic

For future reports, verify the cable/data path and raw libmtp enumeration before tracing HifiMule internals.

## Reproduction Plan

1. Connect the device and explicitly select MTP/File Transfer mode.
2. Confirm it appears in macOS USB inventory.
3. Close competing MTP/PTP clients.
4. Run `mtp-detect`; if raw enumeration succeeds, launch the fresh HifiMule build and compare daemon logs.

## Side Findings

- No MTP-specific entries were found in the current HifiMule daemon log search; this is consistent with empty raw enumeration but does not independently prove observer behavior.

## Follow-up: 2026-09-12

### New Evidence

- The user found and replaced a bad USB cable.
- With the replacement cable, the same macOS build detected the device and started an MTP sync.
- Selecting tray Quit interrupted the sync; the UI first showed the authoritative shutdown message and then the no-daemon message.

### Additional Findings

- The before/after cable result confirms the failure occurred below HifiMule at the USB data path.
- Successful MTP sync and shutdown observation refute an application-level enumeration or Story 15.2 lifecycle defect.

### Updated Hypotheses

- Hypothesis 1 (HifiMule observer defect): Refuted.
- Hypothesis 2 (device not exposed over the USB data path): Confirmed; bad cable.
- Hypothesis 3 (competing process claim): Refuted for this incident.

### Backlog Changes

- All diagnostic paths are closed; no source change is warranted.

### Updated Conclusion

Concluded with high confidence: a bad USB cable prevented libmtp enumeration. Replacement restored MTP detection, sync, orderly active-sync Quit, shutdown UI status and final no-daemon status.
