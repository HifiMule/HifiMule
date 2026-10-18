---
title: 'Optional per-device sync reconciliation'
type: 'feature'
created: '2026-10-18'
status: 'done'
baseline_commit: '09076ec29f73ec8c852dd1a9e88e5aca5ba5451f'
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Matching device files against server tracks before sync can take a long time on large devices. This reconciliation is unnecessary when HifiMule is the only application managing those files.

**Approach:** Add a persisted, per-device setting controlling sync reconciliation, disabled by default for both new devices and existing manifests without the field. Keep the existing prompted scan during device setup independent of this setting.

When sync reconciliation runs, log its start and end. The completion summary must report the same result categories as the manual setup scan: imported/reused, unmatched, ambiguous, and incompatible files. Describe sync matches as reused, since reconciliation prepares a plan rather than committing an import.

## Boundaries & Constraints

**Always:** Apply the setting consistently to manual sync preparation, direct sync, and automatic sync. Save it through the existing device configuration and manifest persistence flow. Use a localized checkbox with the English label “Reconcile files before syncing — enable only if files are managed outside HifiMule.” Add helper text explaining that matching existing files to server tracks can take time on large devices. Preserve the user's existing setup scan choice.

**Ask First:** Any expansion into changing file ownership, deletion policy, or the setup scan consent flow.

**Never:** Disable the setup scan because this setting is off; automatically enable the setting after setup scanning; change unrelated playback reconciliation, server-ID migration, normal sync diff calculation, or transfer safety checks.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Legacy or new device | Field absent or default manifest | Reconciliation disabled | Existing manifests remain readable |
| Normal sync | Setting false | Skip reconciliation I/O; keep normal sync planning | Existing sync errors remain unchanged |
| External management | Setting true | Existing reconciliation behavior runs | Preserve current cancellation and scan error handling |
| Settings update | Explicit true or false | Persist and expose the selected value for this device | Failed writes follow existing settings error flow |
| Partial settings update | Setting omitted | Preserve the stored value | Reject supplied non-boolean values rather than silently coercing |
| Device setup | Setting false; user accepts scan | Existing matching scan still runs | Existing consent, cancellation, and error behavior preserved |
| Force sync | Either setting value | Preserve existing force-sync behavior | No additional scan introduced |
| Reconciliation logging | Enabled reconciliation starts and finishes | Start log identifies device; completion reports reused, unmatched, ambiguous, and incompatible counts, including zero-work runs | Failure/cancellation emits an end outcome without claiming successful completion; disabled runs do not claim a scan ran |

</frozen-after-approval>

## Code Map

- `hifimule-daemon/src/device/mod.rs` — `DeviceManifest`, defaults, initialization, and device persistence.
- `hifimule-daemon/src/device_music_scan.rs` — independent setup `scan` and sync `reconcile` functions; reconciliation unit tests.
- `hifimule-daemon/src/rpc.rs` — settings update validation/persistence, device summaries, `finish_sync_delta`, direct sync, and setup scan RPCs.
- `hifimule-daemon/src/main.rs` — automatic sync calls the same reconciliation function.
- `hifimule-daemon/src/device/tests.rs` — manifest compatibility and device persistence coverage.
- `hifimule-ui/src/components/BasketSidebar.ts` — device summary typing and Device Settings dialog load/save behavior.
- `hifimule-ui/src/components/InitDeviceModal.ts` — existing setup consent and scan flow to preserve.
- `hifimule-i18n/catalog.json` — shared English, French, Spanish, and German UI strings.
- `hifimule-ui/tests/deviceMusicScan.test.mjs` — existing setup scan regression coverage and UI test conventions.

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-daemon/src/device/mod.rs` — add `reconcile_before_sync: bool` with serde default false and the JSON name `reconcileBeforeSync`; preserve it in `compact_for_state` and update explicit manifest constructors as needed to compile.
- [x] `hifimule-daemon/src/rpc.rs` — accept an optional strictly boolean settings field, preserve it on omission, persist explicit values, and include it in device settings/summary responses. Changing only this setting must not request relocation or retranscoding.
- [x] `hifimule-daemon/src/device_music_scan.rs` — return immediately from sync reconciliation when disabled, before file access or cancellation worker creation. Keep setup scanning independent. Audit the RPC and automatic-sync call sites to confirm all use this gate.
- [x] `hifimule-daemon/src/device_music_scan.rs` — add start and end daemon logs around enabled reconciliation, including early successful returns and failure/cancellation outcomes. Track setup-equivalent result categories accurately, including duplicate-file ambiguity; report only final reusable matches as reused. Keep measured-size refreshes separate from matched-file totals. Cover summary accounting and terminal outcomes with focused tests.
- [x] `hifimule-ui/src/components/BasketSidebar.ts`, `hifimule-i18n/catalog.json` — add the localized device checkbox and helper text, load the saved value, send both checked and unchecked states on save, and include it in preview invalidation.
- [x] `hifimule-daemon/src/device/tests.rs`, `hifimule-daemon/src/device_music_scan.rs`, `hifimule-daemon/src/rpc.rs` — cover legacy defaults, persistence, invalid inputs, omission preservation, no reconciliation device access when disabled, and existing reconciliation when enabled. Update reconciliation fixtures to opt in explicitly.
- [x] `hifimule-ui/tests/deviceMusicScan.test.mjs` and a focused settings test alongside it — verify setting load/save/default behavior and preserve the prompted setup scan while disabled.
- [x] `hifimule-daemon/src/rpc/sync_plan_tests.rs` — opt existing reuse tests into reconciliation, verify disabled planning retains additions, and retain default-false setup import coverage. Verify settings changes invalidate prepared plans through the manifest fingerprint.

**Acceptance Criteria:**
- Given two devices with different saved settings, when each device is selected and synced, then its own setting controls reconciliation and appears correctly in Device Settings.
- Given a saved setting change, when the device reconnects and settings reopen, then the saved value is retained without triggering folder relocation or profile rewrites.
- Given manual preparation, direct sync, or automatic sync, when reconciliation is disabled, then each path skips the shared reconciliation process while retaining ordinary sync planning and safety checks.
- Given any supported UI language, when Device Settings opens, then the checkbox explains that it should be enabled only for files managed outside HifiMule.
- Given enabled sync reconciliation, when it runs, then logs identify its start and terminal outcome, with successful completion reporting the same result categories as the setup scan summary; given disabled reconciliation, when syncing, then no reconciliation start/completion pair is emitted.

## Spec Change Log

- 2026-10-18: User requested start/end logging for reconciliation during sync with the same information as the setup scan. Added lifecycle logging and setup-equivalent result counts; preserve the default-off setting and independent setup scan.

## Design Notes

A shared guard in `reconcile` avoids inconsistent behavior among sync entry points. The separate `scan` entry point supports initial device discovery and stays unaffected. Existing manifests opt out through serde defaults; no migration or database preference is necessary. Retain normal existence checks outside reconciliation, since those protect sync behavior rather than perform external-file matching.

## Verification

- `rtk npm run build:daemon -- test -p hifimule-daemon` — passed: 1,536 unit tests, 12 ignored; 5 integration tests. The repository requires this wrapper to validate the controlled FFmpeg runtime; raw Cargo is intentionally rejected by its build script. The successful run used permission to bind local mock HTTP servers after the sandboxed run could not bind them.
- Final focused rerun: `rtk npm run build:daemon -- test -p hifimule-daemon device_music_scan::tests` — all 12 passed, including measured-size refresh accounting.
- From `hifimule-ui`: `rtk proxy node --test tests/deviceMusicScan.test.mjs tests/deviceSettings.test.mjs` — all 13 tests passed.
- From `hifimule-ui`: `rtk npm run build` — TypeScript, localization, and production bundle passed (existing bundle warnings remain).
- `rtk git diff --check` — passed.
- Audited manual preparation (`finish_sync_delta`), direct sync, and automatic sync: all call the shared gated `reconcile`. Force sync bypasses it as before. Setup `scan` is independent; setup import regression asserts the setting remains false before and after import.
- Current compact state implementation is named `polling_metadata` (there is no `compact_for_state` in this revision). It and the exhaustive manifest storage codec both preserve the new setting.
- Focused regression coverage includes legacy defaults, saved true/false values and omitted updates, invalid boolean input, prepared-plan fingerprint invalidation, disabled device access, enabled reuse, final duplicate ambiguity counts, zero-work summaries, failure/cancellation terminal wording, and measured-size refresh accounting.

## Review Outcome

Acceptance and edge-case reviews found no actionable defects. The blind review concern about skipping measured-size refreshes was rejected: skipping all reconciliation I/O is intentional, and normal existence checks remain independent.

## Suggested Review Order

**Shared gate and logs**

- One guard covers all sync entry points while leaving setup scanning independent.
  [device_music_scan.rs:373](../../hifimule-daemon/src/device_music_scan.rs#L373)

**Persistence and configuration**

- Legacy devices default off; polling metadata retains the preference.
  [mod.rs:179](../../hifimule-daemon/src/device/mod.rs#L179)

- Compressed manifests preserve the setting across reconnects.
  [manifest_codec.rs:1](../../hifimule-daemon/src/device/manifest_codec.rs#L1)

- Reject invalid values and preserve omitted settings.
  [rpc.rs:9458](../../hifimule-daemon/src/rpc.rs#L9458)

**Device UI**

- Explain external management and save explicit checked or unchecked states.
  [BasketSidebar.ts:709](../../hifimule-ui/src/components/BasketSidebar.ts#L709)

**Regression coverage**

- Exercise default, enabled, and disabled settings interactions.
  [deviceSettings.test.mjs:13](../../hifimule-ui/tests/deviceSettings.test.mjs#L13)
