---
title: 'Fix Garmin MTP initialization and device selection'
type: 'bugfix'
created: '2026-09-17'
status: 'done'
baseline_commit: '91acf7e1ca012bd813186970962f8a946fecc071'
context:
  - '{project-root}/_bmad-output/planning-artifacts/project-context.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Garmin MTP initialization can report a failed manifest write even after the Shell copy succeeds, because verification compares Explorer's extension-hidden display name. Separately, an empty Windows card-reader slot can replace an attached watch as the sole pending device, leading the UI to treat the watch as undetected and allowing initialization to target the unusable drive.

**Approach:** Use a parsing Shell name for file identity, make initial MTP manifest persistence honor the existing authoritative-local-cache policy, and prevent mediumless removable drives from becoming candidates. Preserve the selected pending device when competing candidates are observed, with deterministic preference for MTP over MSC where an unexpected collision remains possible.

## Boundaries & Constraints

**Always:** Treat full filesystem names—not Explorer display labels—as device file identities; retain normal Shell display names when locating the device itself under This PC; keep local MTP manifest cache commits authoritative and atomic; ensure a failed best-effort on-device MTP mirror cannot abort a successful initialization; reject Windows removable roots that cannot report present media; retain coherent path/IO/friendly-name snapshots; add regression tests before implementation.

**Ask First:** Reworking the UI or RPC response shape to present a multi-device initialization picker; changing non-Windows mount detection; changing Garmin Shell-copy selection criteria beyond fallback/error handling.

**Never:** Rely on the user's `HideFileExt` setting; initialize or enumerate an empty card-reader slot; silently pair one pending path with another device's IO; weaken failure reporting for local-cache persistence; delete user files or manifests as part of migration.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Shell-written manifest | Garmin Shell copy succeeds and the extension is hidden | Verification locates `.hifimule.json` by parsing name and initialization succeeds | No direct-WPD fallback required for the successful write |
| On-device MTP mirror fails | Local authoritative cache commits, device mirror returns an error | Initialization registers the device and retains the cache | Log the mirror failure as best effort; do not return an initialization error |
| Cache fails | Initial MTP manifest cache cannot be atomically committed | Initialization fails without registering the device | Return the cache persistence error |
| Empty card reader | `GetDriveTypeW` reports removable but querying the root reports no media | Drive is ignored and cannot become pending | Continue scanning remaining devices |
| Competing pending devices | An MTP watch and a removable candidate are observed | The usable MTP watch remains the pending initialization target | Do not overwrite the snapshot with a less-preferred candidate |

</frozen-after-approval>

## Code Map

- `hifimule-daemon/src/device/mtp.rs` -- Windows WPD/Shell write and verification helpers; filesystem-child lookup currently compares `SHGDN_NORMAL` display names, while This-PC device lookup uses a friendly display name.
- `hifimule-daemon/src/device/mod.rs` -- pending-device lifecycle, Windows removable-drive filter, and initial manifest persistence.
- `hifimule-daemon/src/device/tests.rs` -- DeviceManager and initialization regression coverage.
- `hifimule-daemon/src/device/mtp.rs` test module -- Windows-specific Shell-name/verification coverage where the COM boundary can be isolated.

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-daemon/src/device/mtp.rs` -- obtain filesystem-child names with `SHGDN_FORPARSING` for identity comparisons while retaining `SHGDN_NORMAL` for This-PC friendly-device lookup -- fixes extension-hidden manifest verification and deletion lookup without breaking device resolution.
- [x] `hifimule-daemon/src/device/mod.rs` -- commit the initial MTP manifest to the local authoritative cache before attempting the on-device mirror; make mirror failures non-fatal after a cache commit -- aligns initialization with later MTP manifest updates.
- [x] `hifimule-daemon/src/device/mod.rs` -- require usable media in the Windows removable-drive candidate check and preserve the higher-quality pending MTP snapshot over a conflicting MSC candidate -- prevents empty readers from masking a watch or becoming initialization targets.
- [x] `hifimule-daemon/src/device/tests.rs` and relevant `mtp.rs` tests -- add focused red tests for parsing-name selection, MTP cache-first initialization, mirror failure tolerance, media absence, and pending-device precedence -- locks the behaviors above against regression.

**Acceptance Criteria:**
- Given Windows hides known extensions, when Shell copy writes `.hifimule.json`, then its verification recognizes the file and initialization completes.
- Given an MTP manifest cache commit succeeds but the device copy fails, when initialization runs, then the device is initialized from the cached manifest and the mirror failure is logged without failing the operation.
- Given an MTP cache commit fails, when initialization runs, then the operation fails and no connected device is registered.
- Given an assigned empty removable-reader letter and an attached MTP watch, when device discovery runs in either order, then the watch is the pending device and the reader is never initialized.

## Spec Change Log

## Design Notes

The Shell namespace has two distinct name contracts: `SHGDN_NORMAL` is a user-facing display name and obeys Explorer's extension-hiding option, while `SHGDN_FORPARSING` is suitable for an exact filename identity comparison. The latter is required for all Shell child lookups that accept paths from the application.

Initial MTP setup must follow the same persistence ordering as `update_manifest_for_device_with_cache_path`: the cache is the source of truth, and a device mirror is recovery/best-effort evidence. Device selection should stay a coherent snapshot rather than independently selecting a path and IO object.

## Verification

**Commands:**
- `rtk cargo test` from `hifimule-daemon` -- expected: new focused regressions and the full daemon suite pass.
- `rtk cargo clippy --all-targets -- -D warnings` from `hifimule-daemon` -- expected: no diagnostics.

## Suggested Review Order

**Manifest identity and persistence**

- Separate parsing-name file identity from friendly device lookup.
  [`mtp.rs:480`](../../hifimule-daemon/src/device/mtp.rs#L480)

- Commit the MTP cache before attempting its best-effort device mirror.
  [`mod.rs:1272`](../../hifimule-daemon/src/device/mod.rs#L1272)

**Device candidacy**

- Reject removable drive letters that do not have usable media.
  [`mod.rs:1811`](../../hifimule-daemon/src/device/mod.rs#L1811)

**Regression coverage**

- Covers MTP mirror failure, cache failure, discovery order, and absent media.
  [`tests.rs:1491`](../../hifimule-daemon/src/device/tests.rs#L1491)
