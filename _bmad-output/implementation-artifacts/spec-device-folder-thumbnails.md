---
title: 'Device folder thumbnails and optional desktop icons'
type: 'feature'
created: '2026-10-21'
status: 'done'
baseline_commit: '05af441a2e30a5a2228d232efab762569d873ef2'
context:
  - '{project-root}/_bmad-output/planning-artifacts/research/folder-artwork-compatibility-2026-10-21.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Synced artist/album folders lack server artwork and recognizable desktop icons.

**Approach:** Add a per-device **Add thumbnail** switch, off by default. Enabled devices export `folder.jpg` and offer independent additions: Windows icon, macOS Finder icon, KDE `.directory`, and GNOME Files icon; additions default off.

## Boundaries & Constraints

**Always:** JPEG fits 500×500; preserve aspect ratio, never upscale. Fetch once per actual folder per sync through its source provider. Reuse successful exports across syncs/native-option changes. Preserve unmanaged files/icons and captured device/server identity. Include retained music needing no audio transfer.

**Ask First:** Changes to exported size, default activation, or unmanaged-file preservation.

**Never:** Embedded audio artwork edits; root/playlist artwork; name-based artwork lookup; treating MTP as a mounted filesystem; universal Linux-icon claims.

## I/O & Edge-Case Matrix

| State | Behavior | Failure handling |
| --- | --- | --- |
| Enabled, missing artwork | Album image in album folder; artist image in artist folder | No known source: skip |
| Shared folder | Keep prior winner; otherwise lowest server/entity ID wins | One image request; no racing fallbacks |
| Successful export exists | Reuse; no server image request, including later syncs | Missing owned file permits regeneration |
| Missing, invalid, oversized or failed image | No partial published output | One attempt this sync; warning; retry next sync |
| User output/custom icon | Preserve | Skip conflicting output; warning |
| Unsupported native adapter | Keep selection and JPEG | Explain unavailable host/transport; no audio failure |
| Option disabled | Stop generating; retain existing outputs | No immediate deletion |

</frozen-after-approval>

## Code Map

Paths are repository-relative; research context supplies compatibility details.

- `hifimule-daemon/src/device/{mod.rs,manifest_codec.rs}` — portable configuration/ownership and compact persistence.
- `hifimule-daemon/src/{rpc.rs,sync.rs,sync_plan.rs}` — device projections, provider identities, plans/execution.
- `hifimule-daemon/src/{device_io.rs,device/mtp.rs}` — mounted/object transport boundaries.
- `hifimule-ui/src/components/BasketSidebar.ts`, `hifimule-i18n/catalog.json` — settings and translations.

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-daemon/src/device/{mod.rs,manifest_codec.rs}` — persist serde-defaulted `folderArtwork` booleans (`enabled`, `windows`, `macos`, `kde`, `gnome`) and owned-output/source records, including polling/compact round trips.
- [x] `hifimule-daemon/src/{rpc.rs,providers/jellyfin.rs}` — strict optional-setting validation/state projections; preserve artist/cover IDs in sync conversion; ensure authenticated provider `fetch_cover_art`.
- [x] `hifimule-daemon/src/{sync.rs,sync_plan.rs}` — plan artwork from final sanitized paths/all desired retained music; resolve entity artwork and deduplicate before scheduling. Record ownership after verified publication. Retire unchanged owned files/native state before pruning folders without managed tracks; preserve user modifications.
- [x] `Cargo.toml`, `hifimule-daemon/src/{sync/artwork.rs,sync.rs}` — enable JPEG/ICO; new `artwork.rs` processes one image at a time: 8 MiB response, 32 MiB decoder allocation, 16 million pixels, JPEG quality 85, white alpha background. Honor cancellation/free space; sanitize URL logging.
- [x] `hifimule-daemon/src/{device_io.rs,folder_icons.rs,main.rs}`, `hifimule-daemon/Cargo.toml` — add native capability, unsupported by default, implemented inside MSC; new `folder_icons.rs` hosts adapters. Windows: relative ICO ≤256px, Unicode INI and Windows attributes. macOS: serialized NSWorkspace, platform dependency. KDE: `.directory` with `Icon=./folder.jpg` on any MSC host. GNOME: GIO metadata on Linux. Reuse JPEG without fetching; preserve user customization and record owned state.
- [x] `hifimule-daemon/src/device/mtp.rs` — classify JPEG appropriately; publish via verified transport/recovery semantics; no native adapters.
- [x] `hifimule-ui/src/components/BasketSidebar.ts`, `hifimule-i18n/catalog.json` — reveal JPEG base/optional additions; save preferences; localize host requirements/pending activation in en/fr/es/de. Apply on next sync.
- [x] `hifimule-daemon/src/{device/tests.rs,device/manifest_codec.rs,rpc.rs,sync/artwork.rs}`, `hifimule-ui/tests/deviceSettings.test.mjs` — cover matrix/defaults/validation/persistence/backfill/cleanup/native gates with fake providers/DeviceIO.

**Acceptance Criteria:**
- Given synced music, when enabled without audio changes, then its folders receive available provider artwork.
- Given successful artwork, when syncing again, then no image fetch occurs while the file exists.
- Given native selections, when synced on compatible MSC hosts, then supported file managers show the artwork; unavailable additions remain pending.
- Given an older manifest, when saved/reconnected, then defaults remain off and edited preferences persist.

## Spec Change Log

## Design Notes

JPEG is the shared base. Native execution depends on the host. Finder can generate 512px system representations from the ≤500px JPEG. No refresh until owned artwork disappears; failures retry next sync without blocking audio.

## Verification

- `rtk npm run build:daemon -- check -p hifimule-daemon` — passed on macOS with the controlled FFmpeg runtime.
- `rtk npm run build:daemon -- test -p hifimule-daemon` — passed, including 1,632 discovered unit cases and the integration suite. Local mock-server tests require the approved localhost-binding sandbox exception.
- Focused `sync::artwork` tests — 4 passed: authenticated retrieval, no-additions retained backfill through the shared executor, repeat/native-option reuse, cleanup/user preservation, bounded JPEG conversion, disk-full handling, cancellation after publication and ownership checkpointing.
- Focused `folder_icons` tests — 3 passed, including actual macOS NSWorkspace apply/verify/preserve/retire in a temporary folder, KDE ownership and symlink rejection.
- From `hifimule-ui`, `rtk proxy node --test tests/deviceSettings.test.mjs` — 6 passed; `rtk npm run build` — passed (TypeScript, localization and production bundle).
- Isolated `folder_icons.rs` checks with the installed rustup toolchain, offline dependencies and targets `aarch64-pc-windows-gnullvm` and `aarch64-unknown-linux-gnu` — passed. These validate native-adapter Rust/API types, not Windows/Linux file-manager behavior or the complete cross-platform daemon.
- `rtk git diff --check` — passed.
- At initial implementation, unavailable manual checks included Windows 11 Explorer, macOS FAT32/exFAT, KDE/GNOME removable mounts, cross-host/remount behavior and physical MTP devices. The exFAT regression verification below subsequently covered JPEG publication and KDE/Finder adapters on the user's mounted drive.

## Implementation Notes

- Artwork ownership/source/native tokens live in top-level manifest metadata, preserving the existing compressed track-column format. Polling exposes the portable preference; compact sync targets retain ownership records.
- The disk-backed addition records carry known artist/cover IDs. Retained legacy music resolves metadata through captured source providers only when a folder needs artwork. RPC captures retained artwork servers independently of audio additions; unavailable artwork sources remain nonfatal.
- The shared executor runs artwork after audio deletions/identity updates and before directory pruning. Verified publication is checkpointed even if cancellation arrives immediately afterward. Disabling options leaves live-folder outputs intact.
- MSC publication prefers staged no-clobber atomic files and rejects symlink folders. On Unix filesystems rejecting the required native primitives, exclusive destination creation preserves existing entries; see the interruption limitation below. MTP JPEG creation refuses the existing-object replacement branch; WPD shell-copy devices that cannot safely establish absence report artwork pending. Native adapters remain unsupported on MTP.
- Native options reuse the exported JPEG. Windows and macOS activate only on their hosts; KDE sidecars can be prepared on any MSC host. GNOME GIO metadata remains host-local, preserves themed customizations and has bounded command timeouts.

## Review Fixes and Final Verification

- Source election skips coverless sources and metadata failures/timeouts, preserving deterministic election among available server/entity identities and one image fetch per physical folder.
- Cleanup distinguishes confirmed user modification from transient inspection errors; failed stat/read/delete operations keep ownership evidence for retry.
- All retained managed descendants protect old artwork, including failed relocations outside the current root. New generation still requires physical membership under the configured music root.
- `DeviceIO::folder_identity` groups aliases using mounted-directory identity (canonical path plus Unix device/inode where applicable) or MTP object IDs. Existing record paths/source winners survive alias-track removal; case-sensitive directories stay distinct.
- WPD verifies the newly created object directly by object ID. Mismatch/read failures delete only that new object; publication no longer leaves an unowned JPEG after failed verification. A shared boundary test covers both failure modes and preserves a user replacement object.
- GNOME ownership records hashed machine/user scopes, merges per-host evidence when aliases coalesce, and retains foreign-host pending state during cleanup. A portable token cannot claim another user's existing customization.

Final checks after review fixes:

- `rtk npm run build:daemon -- test -p hifimule-daemon -- --quiet` — **1,626 passed, 14 ignored, 0 failed**; **5 integration tests passed**. Includes nine artwork lifecycle tests, GNOME scope/merge checks, transport identity checks and the WPD creation-verification boundary.
- Final mounted-directory identity regression after the Unix inode refinement — **1 passed**; current daemon production/test code compiled successfully.
- Isolated offline Windows check — passed for the native adapters **and the actual modified WPD module**, using minimal project-boundary stubs and the installed target standard library. Linux native-adapter check including hashed GNOME scopes — passed. These remain type/API checks rather than runtime file-manager/device validation.
- Root independently ran `rtk test node --test scripts/tests/*.test.mjs hifimule-ui/tests/*.test.mjs` — passed, no failures/cancellations, one skipped case. UI production build passed earlier and review fixes did not change UI code.
- Final `rtk git diff --check` — passed. Remaining physical/file-manager checks are listed above, with the subsequent exFAT exception below.

## exFAT Publication Regression

The reported sync produced no thumbnails because macOS FSKit exFAT rejects `renameatx_np(RENAME_EXCL)` and hard links with errno 45. The shared publisher now retains staged publication where supported and falls back only for Unix ENOTSUP/EOPNOTSUPP/ENOSYS. `create_new` preserves existing files, directories and symlinks. Failed ordinary writes/syncs remove only a destination still matching the open descriptor. Fresh descriptor metadata handles FSKit changing provisional inode numbers after the first sync. Warnings retain operation context and underlying causes.

The fallback destination is visible during writing. Process termination or device removal can leave an incomplete unowned file, which later syncs preserve; remove a confirmed incomplete file before retrying. This interruption case is outside the matrix's ordinary missing/invalid/failed-image handling. POSIX offers no portable conditional unlink, so cleanup cannot eliminate an external replacement precisely between identity inspection and removal.

Regression verification:

- Wrapped daemon `artwork` suite: **22 passed**, including 11 helper/native tests, one MSC publication integration and nine artwork lifecycle tests. Unsupported errors, no fallback for collision/permission failures, existing entries, failed-write/sync cleanup, replacements and concurrent publication are covered.
- Wrapped daemon check and runnable debug build passed; the running application was not restarted or installed.
- Exact shared publisher, existing-JPEG preservation, KDE `.directory` apply/verify/cleanup and Finder icon apply/verify/cleanup passed on **FLASHDISK exFAT**. Root independently repeated the probe successfully. Only unique temporary test directories were created and removed; no existing music or actual sync was changed.
- Three independent focused reviews found no ordinary-path regression. Interruption recovery and the conditional-unlink limitation are explicitly recorded above; no atomic/crash-recovery claim is made for the unsupported-filesystem fallback.
- `rustfmt --check` for the modified publisher and `git diff --check` passed.

The investigation record is [folder-thumbnails-not-created-investigation.md](investigations/folder-thumbnails-not-created-investigation.md). Relaunch the rebuilt application and sync again to backfill the actual library; no audio retransfers are required.

## Suggested Review Order

**exFAT publication**

- Preserve existing entries when removable filesystems reject native no-clobber publication.
  [folder_icons.rs:38](../../hifimule-daemon/src/folder_icons.rs#L38)

- Verify fresh descriptor identity before success or failed-write cleanup.
  [folder_icons.rs:78](../../hifimule-daemon/src/folder_icons.rs#L78)

**Artwork lifecycle**

- Follow optional artwork independently of audio transfers.
  [artwork.rs:400](../../hifimule-daemon/src/sync/artwork.rs#L400)

- Group physical folders and retain ownership across aliases and relocations.
  [artwork.rs:95](../../hifimule-daemon/src/sync/artwork.rs#L95)

- Run artwork before pruning empty device folders.
  [sync.rs:5019](../../hifimule-daemon/src/sync.rs#L5019)

**Settings and persistence**

- Expose the master switch and independent desktop additions.
  [BasketSidebar.ts:772](../../hifimule-ui/src/components/BasketSidebar.ts#L772)

- Persist portable choices separately from track columns.
  [mod.rs:139](../../hifimule-daemon/src/device/mod.rs#L139)

- Validate configuration and capture the target device.
  [rpc.rs:9394](../../hifimule-daemon/src/rpc.rs#L9394)

**Device boundaries**

- Keep native operations and no-clobber publication inside mounted-device I/O.
  [device_io.rs:44](../../hifimule-daemon/src/device_io.rs#L44)

- Preserve native customizations and scope GNOME ownership by computer and user.
  [folder_icons.rs:134](../../hifimule-daemon/src/folder_icons.rs#L134)

- Verify new MTP objects and retire failed publications by object identity.
  [mtp.rs:58](../../hifimule-daemon/src/device/mtp.rs#L58)

**Supporting checks**

- Exercise unsupported primitives, preservation, failure cleanup, replacement and concurrent publication.
  [folder_icons.rs:581](../../hifimule-daemon/src/folder_icons.rs#L581)

- Verify mounted-device artwork publication and collision preservation through DeviceIO.
  [device_io.rs:1370](../../hifimule-daemon/src/device_io.rs#L1370)

- Exercise artwork reuse, cancellation, preservation, aliases, and cleanup.
  [artwork.rs:599](../../hifimule-daemon/src/sync/artwork.rs#L599)

- Verify preference loading and persistence in the existing settings dialog.
  [deviceSettings.test.mjs:69](../../hifimule-ui/tests/deviceSettings.test.mjs#L69)
