---
title: 'Recognize existing device music during setup and before sync'
type: 'feature'
created: '2026-10-16'
status: 'done'
baseline_commit: '0863c5a625929802cd70ed31ee3af2c94360c9e6'
context: ['{project-root}/DESIGN.md']
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Existing music is not recognized, and capacity uses source bytes instead of planned output for the device's format/compression profile.

**Approach:** After configuration, offer to scan a nonempty music folder against the active server. Import unambiguous matches into the synced manifest and basket, grouping complete artists, then complete albums, then remaining tracks. Reuse the scanner during existence verification before every sync to recognize later-added files matching the selection. Normal sync reuses profile-compatible files; Force Sync overwrites managed copies.

## Boundaries & Constraints

**Always:** Bind scans to device, folder and captured active server. Explain that imported matches become managed music subject to ordinary removal rules. Compare scanned extensions/container/codec and available bitrate against the effective device profile; incompatible files are not reusable matches. Store actual paths/sizes; group only complete collections. Deduplicate server-qualified IDs. Preserve unmatched files, cancellation, atomic writes and freshness checks. Later scans reconcile desired tracks only. Force bypasses reuse; automatic sync shares normal behavior.

**Ask First:** Changing removal semantics or automatically importing unrelated files during later syncs.

**Never:** Transcode to estimate size; use source bytes for planned transcoding; equate extension with compression quality; treat errors as absence; adopt ambiguous matches; mutate manifests during preview; escape the music folder; add retention/bypass settings.

## I/O & Edge-Case Matrix

| State | Expected behavior |
|-------|-------------------|
| Populated folder at setup | Offer scan; Yes imports matches; No finishes setup |
| Complete/partial collections | Complete artists, then albums, then tracks; no overlap |
| Matching track and profile-compatible extension/format | Reuse; record actual bytes and identity |
| Wrong extension/codec or incompatible bitrate | Keep unmanaged scan result untouched; report incompatibility |
| Ambiguous, unmatched or duplicate file | Leave unmanaged; report unresolved result |
| Later-added desired track | Reconcile and reuse before normal sync |
| Force Sync | Replace managed selected copies; protect unmatched collisions |
| Planned transcode (any source format) | Estimate duration × target bitrate / 8 plus overhead; no audio conversion |
| Direct copy or reusable resident | Use source bytes only for direct copy; measured bytes for resident |
| Unknown duration/bitrate or variable compression | Explicit conservative estimate/range; never claim exact output size |
| Failed/incomplete/cancelled scan | No partial setup import or inferred absence; retry safely |

</frozen-after-approval>

## Code Map

Paths below are relative to `hifimule-daemon/src/` unless prefixed otherwise.

- `rpc.rs` — initialization, provider mapping, preparation and force handling.
- `sync.rs` — existence check, path construction, reconciliation and writes.
- `providers/mod.rs` — search and complete album/artist membership.

## Tasks & Acceptance

**Execution:**
- [x] `device_music_scan.rs` (new), `main.rs` — Add cancellable scanner, matcher and grouping. Probe tags and format through existing FFmpeg facilities; gate reuse on the effective device profile. Match unique IDs or consistent artist/album/title/track/disc metadata with available duration. Reject truncated searches as uniqueness evidence. Cache bounded provider queries; group from complete membership IDs, never approximate counts.
- [x] `device_io.rs` — Add reliable enumeration, lengths and bounded metadata access for MSC/MTP. Distinguish absence from access failures; enforce folder boundaries and avoid whole-library audio buffering.
- [x] `rpc.rs`, `device/mod.rs` — Add target-bound setup scan/progress/cancel operations. Capture active server; block Auto-Sync until setup resolves. Atomically publish matched synced records and grouped basket. Decline/no matches leaves initialized data unchanged. Return imported/unmatched/ambiguous counts.
- [x] `sync.rs`, `sync_plan.rs`, `main.rs` — Reconcile desired matches and measurements before manual/automatic sync. Keep preview read-only; commit on execution. Preserve rekeys and force overwrites; do not replace compatible adopted files merely for absent source-quality metadata. Revalidate destinations before writing.
- [x] `rpc.rs`, `hifimule-ui/src/state/syncCapacity.ts`, `hifimule-ui/src/components/BasketSidebar.ts` — Share effective profile/transfer decisions with execution. Estimate new/replacement output from duration and target bitrate plus documented overhead, without transcoding; handle unknown/VBR output explicitly. Use actual reused sizes and source sizes only for direct copy. Apply consistently to basket, preview, admission and Auto-Fill; allow preparation despite coarse source totals.
- [x] `hifimule-ui/src/components/InitDeviceModal.ts`, `hifimule-ui/src/state/basket.ts`, `hifimule-i18n/catalog.json` — Add setup scan prompt, progress/cancel/results and basket refresh; explain managed ownership; translate all locales.
- [x] `device_music_scan.rs`, `rpc/sync_plan_tests.rs`, `hifimule-ui/tests/deviceMusicScan.test.mjs` (new), `hifimule-ui/tests/syncCapacity.test.mjs` — Test matrix using tagged fixtures, duplicate editions, partial catalogs, multi-server IDs, bounded scans, setup cancellation and normal/force write traces.

**Acceptance Criteria:**
- Given files in a newly configured music folder, when the user accepts scanning, then unique active-server matches enter both manifest and basket without copying audio.
- Given complete and partial collections, when grouping finishes, then expanding the initial basket yields exactly matched tracks.
- Given a matching profile-compatible file added later, when normal sync prepares, then it reuses the file and counts actual bytes.
- Given the same managed file, when Force Sync runs, then it overwrites it and records actual output bytes.

- Given any source requiring transcoding, when capacity is calculated, then it uses estimated target output without encoding or downloading audio.
- Given a scanned file with incompatible extension/format, when matching runs, then it is not adopted as a reusable copy.

## Spec Change Log

- 2026-10-16: User replaced optional retention with setup import, complete-set grouping and default pre-sync reuse; Force Sync still overwrites. Matched imports now become managed files.

- 2026-10-16: User required profile-aware scan extensions and planned-output sizing for all transcoding, without performing conversion.

## Design Notes

At 192 kbps, 240 seconds estimates 5,760,000 audio bytes plus overhead, regardless of source size. Reuse uses measured bytes. Match actual direct/transcode policy, including per-slot overrides; invalidate estimates on profile changes. Setup imports compatible matches; later scans reconcile selection only. Grouped baskets can include future server additions.

## Verification

- `rtk node --test hifimule-ui/tests/deviceMusicScan.test.mjs hifimule-ui/tests/syncCapacity.test.mjs hifimule-ui/tests/syncPlan.test.mjs`
- `rtk npm run build --prefix hifimule-ui`
- `rtk node scripts/build-daemon.mjs test -p hifimule-daemon -- --test-threads=1`
- `rtk git diff --check`

## Implementation verification (2026-10-16)

- Daemon full wrapper suite after review fixes: 1500 passed, 12 ignored; provider contracts: 5 passed.
- UI production build passes; 26 focused setup, capacity and prepared-plan tests pass.
- `git diff --check` passes.
- Prepared-capacity summary compiles; RPC integration asserts measured `selectedBytes` after preparation.
- Legacy Jellyfin Auto-Fill now uses the shared output estimator; regression proves a 50 MB FLAC selection fits the 7 MB encoded budget while direct copy does not.
- Normal reuse RPC integration executes the prepared plan, checks byte-identical existing audio and actual manifest path/length, and checks Force Sync schedules replacement. Existing same-path replacement and refused force cleanup tests cover write/delete ordering.
- Scanner fixtures cover metadata probing, ambiguous editions/duplicate files, incomplete search results, exact complete/partial grouping, cancellation, captured server IDs and stale destination rejection. UI tests cover consent/decline/progress/cancel/retry.
- No physical MTP device or Windows runtime was available. Production MTP access is streamed to disk with bounded buffers; cancellation is checked within the WPD read loop and the libmtp transfer progress callback. Hardware smoke testing remains an environmental validation limitation, not an unimplemented scan path.

## Independent Review

Three independent reviewers (blind, edge-case, acceptance) identified eight deduplicated implementation defects: MTP missing destination parents; missing disc metadata in later matching; Auto-Fill preview lacking output profile; abandoned setup proposals blocking sync; no-audio MP4 aborting scans; post-initialization discovery errors reported as initialization failure; MTP cancellation not reaching transport; and cancellation racing import publication. Classified as patches: the approved intent already defines safe matching, cancellation, profile sizing and target-bound import, and each defect is locally correctable without changing scope. All eight corrections are complete with regression verification; no unresolved review findings remain.

## Review fixes (2026-10-16)

- New MTP destination parents are handled through strict absence confirmation; root enumeration failures still propagate. Two transport regressions cover missing-parent/error distinction and cancellation token delivery.
- Disc numbers now flow through provider/Jellyfin selection, Auto-Fill and delta additions; later reuse rejects a conflicting disc number.
- Both Auto-Fill preview branches use the selected output profile.
- Idle setup proposals expire after five minutes and are bounded to 128; an expired dialog can be dismissed without reinitializing the configured device.
- A container with no audio stream is skipped as incompatible, like other unsupported media. A generated 16×16 video-only MP4 regression asserts the specific no-audio-stream classification.
- Post-initialization discovery failure is returned separately as `musicScanWarning`, displayed with translated configured-device text.
- Cancellation and ownership commit share a gate; cancellation after the commit decision reports committed rather than cancelled. Backend and UI regressions cover this outcome.

## Suggested Review Order

**Setup and ownership**

- Offer import after configuration, with progress and cancellation.
  [InitDeviceModal.ts:316](../../hifimule-ui/src/components/InitDeviceModal.ts#L316)

- Bind import to captured device and server; serialize cancellation with publication.
  [rpc.rs:9636](../../hifimule-daemon/src/rpc.rs#L9636)

**Matching and reuse**

- Match metadata and profile before adopting files; preserve ambiguous content.
  [device_music_scan.rs:144](../../hifimule-daemon/src/device_music_scan.rs#L144)

- Reuse selected compatible files and refresh measured resident sizes.
  [device_music_scan.rs:367](../../hifimule-daemon/src/device_music_scan.rs#L367)

**Capacity and transfer safety**

- Estimate target output without encoding; retain direct-copy sizing.
  [sync.rs:1648](../../hifimule-daemon/src/sync.rs#L1648)

- Distinguish absent files from inspection failures.
  [device_io.rs:83](../../hifimule-daemon/src/device_io.rs#L83)

**Regression coverage**

- Verify disc identity survives provider and delta mapping.
  [sync_plan_tests.rs:1037](../../hifimule-daemon/src/rpc/sync_plan_tests.rs#L1037)

- Exercise setup consent and cancellation UI.
  [deviceMusicScan.test.mjs:24](../../hifimule-ui/tests/deviceMusicScan.test.mjs#L24)
