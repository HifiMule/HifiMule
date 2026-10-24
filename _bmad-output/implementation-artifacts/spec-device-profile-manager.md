---
title: 'Create, clone, and edit device profiles'
type: 'feature'
created: '2026-10-24'
status: 'done'
baseline_commit: '0300f5a13667c509702b6c46c876d04647518572'
context: ['{project-root}/DESIGN.md', '{project-root}/PRODUCT.md']
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Profiles require hand-edited JSON and lack system/custom ownership.

**Approach:** Selected design A: grouped Custom/System list beside a structured editor, supporting New/Edit/Clone through device dropdowns and the native menu, without a connected device.

## Boundaries & Constraints

**Always:** Existing app-data `device-profiles.json`; daemon-enforced system immutability; legacy IDs/unknown fields retained; existing styling, Shoelace, keyboard access, four locales. Planning only until implementation is requested.

**Ask First:** Data loss or changed layout/storage.

**Never:** Deletion, raw JSON editing, import/export, dependencies, retroactive folder changes.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected behavior | Error handling |
|---|---|---|---|
| Edit/clone | Custom/any source | Stable/new custom ID respectively | Preserve source/draft |
| System write | Reserved identity | Reject | Offer cloning |
| Conflict | Stale revision/active pipeline | Reject save | Retry; retain draft |
| Invalid | File/fields invalid | No write | Explain recovery |
| Upgrade | Legacy JSON | Backup; preserve uncertain entries | Abort safely on failure |

</frozen-after-approval>

## Code Map

- `hifimule-daemon/src/transcoding.rs`: JSON store/default merging.
- `hifimule-daemon/src/device/mod.rs`: ID-only rewrite detection.

## Tasks & Acceptance

**Execution:**

- [x] `hifimule-daemon/assets/device-profiles.json`, `hifimule-daemon/src/transcoding.rs`: add ownership, reconciliation, serialized atomic writes/tests. Systems use `system:<bundled-id>` with persisted collision alternatives; keep `passthrough`. Alias exact stock entries; retain uncertain entries as custom with original IDs/unknown fields. Back up once; freeze pre-migration audio baselines/aliases in JSON. Refresh only systems; ensure idempotence.
- [x] `hifimule-daemon/src/rpc.rs`: extend list; add detail/create/update/clone with optimistic revisions/custom UUIDs. Validate names, relative paths, limits, codecs/containers using encoding validators. Gate shutdown and audio-changing saves during active pipelines; invalidate plans. Return known usage, noting incomplete offline coverage. Test rejection/conflicts/failed writes.
- [x] `hifimule-daemon/src/device/mod.rs`, `hifimule-daemon/src/device/manifest_codec.rs`, `hifimule-daemon/src/sync.rs`, `hifimule-daemon/src/rpc.rs`, `hifimule-daemon/src/main.rs`: compare completed audio fingerprints before preview/manual/auto sync. Legacy fallback uses frozen last-synced-ID baseline, or selected-ID baseline for clean historyless manifests; unknown history rewrites conservatively. Exclude names/folders; preserve dirty/recovery flags and alias equivalence. Persist captured execution fingerprint only on success. Test same-ID edits, metadata, offline reconnection, compact manifests, failure/cancellation.
- [x] `hifimule-ui/src/deviceProfiles.ts`, `hifimule-ui/src/components/DeviceProfileManager.ts`, `hifimule-ui/src/styles.css`: add shared types/mapping, singleton manager, readable systems/Clone, New/Save, drafts/discard, errors, summary, narrow reflow. Preserve unsupported rules visibly; show shared-edit usage.
- [x] `hifimule-ui/src/components/BasketSidebar.ts`, `hifimule-ui/src/components/InitDeviceModal.ts`: add grouped options and Edit/New shortcuts; restore real selection before actions, reject sentinels, refresh in place, preserve all parent drafts/folder flags, restore focus. Select saved new profiles/clones; cancel retains selection.
- [x] `hifimule-daemon/src/main.rs`, `hifimule-lifecycle/src/lib.rs`, `hifimule-ui/src-tauri/src/lib.rs`, `hifimule-ui/src/main.ts`, `hifimule-lifecycle/tests/contract.rs`: add Device profiles menu activation for open/closed UI with backward-compatible metadata and readiness acknowledgment; test without device/server.
- [x] `hifimule-i18n/catalog.json`: translate labels/help/errors/menu into en/fr/es/de; preserve custom names/descriptions verbatim.
- [x] `hifimule-ui/tests/deviceProfiles.test.mjs`, `hifimule-ui/tests/deviceSettings.test.mjs`, `hifimule-ui/tests/initDeviceOptions.test.mjs`: test mapping, clones, immutable systems, validation/conflicts, discard handling, parent drafts, and sentinel exclusion.

**Acceptance Criteria:**

- Given no device, when the menu action runs, then the manager opens even if UI was closed.
- Given unsaved settings, when dropdown cloning succeeds, then the clone is selected and other inputs survive; cancel restores selection.
- Given a shared profile, when capabilities change, then later sync previews include required rewrites, including reconnected devices; metadata-only edits do not.
- Given a dirty editor, when switching/closing, then discard handling protects changes and focus returns correctly.

## Design Notes

Fields: name/description; MP3, AAC/M4A, ALAC/M4A, AAC/M4B, FLAC, Vorbis, Opus, WAV support; originals/incompatible-only/always conversion; output/bitrate; collapsed rate/depth/channels/original-bitrate limits; music/playlist defaults. New: MP3 support/fallback, incompatible-only, unlimited ceilings. Omit unlimited keys; bitrate uses bits/sec. Hide lossless bitrate; M4B is direct-play only.

## Spec Change Log

## Verification

- `rtk npm run build:daemon -- test -p hifimule-daemon -p hifimule-lifecycle -p hifimule-i18n -- --quiet` — passed: 1,749 tests; 16 existing ignored. The repository requires its verified FFmpeg wrapper; mock-server tests require local socket access.
- `rtk node --test scripts/tests/*.test.mjs hifimule-ui/tests/*.test.mjs` — passed: 598 tests; 1 existing skip. Final focused profile suite: 12 passed, including an additional future-rule preservation regression.
- `rtk npm run build --prefix hifimule-ui` — passed.
- `rtk npx --prefix hifimule-ui tsc --noEmit -p hifimule-ui/tsconfig.json` — passed.
- `rtk cargo check -p hifimule-ui` — passed; keep sequential with daemon CLI tests because Tauri stages sidecars into the same output directory.
- Browser checks cover desktop/compact layouts, system Save hidden/disabled, cloning/saving, parent settings draft preservation and focus restoration with real Shoelace. Evidence: `.impeccable/review/device-profile-manager/desktop.png` and `compact.png`.
- Fingerprint reconciliation is centralized in device sync snapshots, reusing the existing sync rewrite paths in `sync.rs`. Tests cover same-ID audio edits, metadata-only edits, offline/compact manifests, frozen legacy history and aliases, and existing success-only completion behavior.
- Three independent workflow reviews completed; all seven deduplicated findings were patched and verified resolved. No intent/spec loopback or deferred issues remained.
- Independent visual disposition: ship at the desktop/compact fixture scope. Documentation review confirmed an existing-system extension; DESIGN.md and its pre-existing sidecar were preserved.


### Review patch verification

- Escaped dynamic shared-device and unsupported-rule messages; partial/unknown direct-play rules stay locked and intact.
- Protected both profile switching and the complete save/refresh transaction against edits during asynchronous loads.
- Preserved whole-bit precision for fractional-kbps values, validated store metadata containers before reconciliation, and reused parent selection effects for saved profiles.
- Refreshed saved profile details to retain shared-usage warnings, with truthful saved-state recovery when refresh fails.
- Focused UI tests: 32 passed. Focused Rust store tests: 7 passed. Profile/RPC/device regression filter: 25 passed. TypeScript and production UI build passed.
- Final full Node/UI suite rerun after review fixes passed with zero failures and one existing skip; final whitespace check passed.

## Suggested Review Order

**Editor and capability model**

- Start with the two-pane editor and its create/edit/clone lifecycle.
  [DeviceProfileManager.ts:8](../../hifimule-ui/src/components/DeviceProfileManager.ts#L8)

- Follow capability mapping and preservation of unsupported legacy rules.
  [deviceProfiles.ts:73](../../hifimule-ui/src/deviceProfiles.ts#L73)

**Persistence and trust boundary**

- Review ownership reconciliation, backups, frozen baselines, and atomic persistence.
  [transcoding.rs:116](../../hifimule-daemon/src/transcoding.rs#L116)

- Inspect RPC validation, optimistic conflicts, and synchronization admission.
  [rpc.rs:10340](../../hifimule-daemon/src/rpc.rs#L10340)

**Sync correctness**

- Check completed fingerprints and offline-device rewrite detection.
  [device/mod.rs:330](../../hifimule-daemon/src/device/mod.rs#L330)

**Entry points and device drafts**

- Trace native-menu activation through existing lifecycle metadata.
  [lib.rs:598](../../hifimule-lifecycle/src/lib.rs#L598)

- Verify dropdown actions restore real selections before opening the manager.
  [deviceProfiles.ts:121](../../hifimule-ui/src/deviceProfiles.ts#L121)

- Check device settings preserve drafts and refresh recovery controls.
  [BasketSidebar.ts:697](../../hifimule-ui/src/components/BasketSidebar.ts#L697)

**Supporting evidence**

- Read regressions for imported rules, loading races, and shared usage.
  [deviceProfiles.test.mjs:9](../../hifimule-ui/tests/deviceProfiles.test.mjs#L9)

- Review translated labels and recovery guidance across four locales.
  [catalog.json:9](../../hifimule-i18n/catalog.json#L9)

