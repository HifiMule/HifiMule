---
title: 'Rockbox database maintenance and Navidrome ratings import'
type: 'feature'
created: '2026-10-31'
status: 'done'
baseline_commit: 'db9ac3d79e4a1f39a2f3996c04bc7d793e6ab54d'
context: ['{project-root}/DESIGN.md']
---

<frozen-after-approval reason="human-owned intent">

## Intent

**Problem:** Sync neither maintains Rockbox's database nor imports its track ratings into Navidrome.

**Approach:** Native RBDB read/write, optional maintenance during sync, and independent ratings import, configurable through profile defaults and device settings.

## Boundaries & Constraints

**Always:** Opt-ins default off. Support 3.15/v0x0f and 4.0/v0x10, both endiannesses; preserve existing format/endian. Fresh creation format is selectable, default 4.0. Preserve runtime statistics and unmanaged indexed entries. Use manifest ownership and provider APIs. Mounted storage/default `.rockbox` location only; reject incompatible custom locations.

**Ask First:** Format migration, destructive recovery, or broader transport/location support.

**Never:** Modify unmanaged audio, install firmware, scan the whole device, rate other providers, match titles, change favorites/scrobbles, or overwrite corrupt/unknown databases.

## I/O & Edge-Case Matrix

| State | Behavior | Failure handling |
|---|---|---|
| No database | Index successfully synced managed audio | Require firmware directory |
| Existing database | Merge managed changes; remove only successfully deleted managed paths; retain other entries/statistics | Reject invalid/unsupported/pending database |
| Interrupted publication | Recover journaled prior set before access | Retain backup; report failure |
| Mixed sources | Unique manifest match routes to originating Navidrome account | Skip ambiguous/unavailable sources visibly |
| Rating 0–10 | `(rating + 1) / 2`; initial zero establishes baseline only; later nonzero-to-zero clears | Skip invalid values |
| Unchanged rating | Preserve later server edits | Confirm before advancing baseline |
| Failed import | Durably retain latest pending value, even for subsequently removed tracks | Retry on later enabled sync; warn |

</frozen-after-approval>

## Code Map

Daemon paths below resolve under `hifimule-daemon/src/`; UI paths under `hifimule-ui/src/`.

- `device/mod.rs`, `device/manifest_codec.rs`: persistence/polling.
- `transcoding.rs`, `rpc.rs`: profiles/settings.
- `sync.rs`, `main.rs`, `sync_plan.rs`: shared execution; compact plans omit tracks.
- `device_music_scan.rs`, `device_io.rs`: metadata/access.
- `providers/mod.rs`, `providers/subsonic.rs`, `db.rs`: API/import state.

## Tasks & Acceptance

**Execution:**

- [x] Daemon `rockbox.rs` (new), `main.rs`: bounded codec; validate headers/counts/offsets, filename UTF-8, alignment, sorting/flags. Preserve ratings/plays/playtime/last-played/resume; regenerate build/file metadata. Validate staged output, back up, journal replacement/master last, roll back/recover. Invalidate stale RAM state; refuse pending commits.
- [x] Daemon `device_music_scan.rs`, `rockbox.rs`: reuse FFmpeg tags; normalize paths, preserve volume prefixes, configure fresh-entry prefix (default `/`). Reject traversal, escaping symlinks and ambiguous mappings; retain other-volume entries.
- [x] Daemon `device/mod.rs`, `device/manifest_codec.rs`, `transcoding.rs`, `rpc.rs`, `hifimule-daemon/assets/device-profiles.json`: persist separate maintenance/import booleans, format and prefix through polling/state/profile CRUD. Prefill setup from profiles; preserve existing device choices. Reject enabling MTP; exclude settings from audio fingerprints.
- [x] Daemon `providers/mod.rs`, `providers/subsonic.rs`, `db.rs`, `rockbox.rs`: numeric methods, default unsupported; authenticated Navidrome detection, bounded requests/readback. Persist observed/pending/confirmed raw ratings by device/server/account/track before deletion. Read back uncertain writes before retrying; never replay unchanged intent.
- [x] Daemon `sync.rs`, `main.rs`, `rpc.rs`, `sync_plan.rs`: capture ratings before mutations; publish after successful media work, before transport finalization. Use authoritative checkpoint manifests. Admit metadata-only manual/auto sync; serialize, respect cancellation/disconnection, report partial failures. Ratings-only mode leaves RBDB unchanged.
- [x] UI `deviceProfiles.ts`, `components/DeviceProfileManager.ts`, `components/InitDeviceModal.ts`, `components/BasketSidebar.ts`, `state/basketEstimate.ts`; `hifimule-i18n/catalog.json`: localized controls/help; preserve drafts/accessibility/compact layout. Explain rating policy, runtime-data prerequisite, location and managed-only creation.
- [x] Rust module tests, `hifimule-daemon/tests/fixtures/rockbox/`, `hifimule-ui/tests/{deviceProfiles,deviceSettings,initDeviceOptions}.test.mjs`; new `docs/rockbox-database.md`: cover matrix, independent fixtures, preservation/recovery, no-op sync and provider exclusion.

**Acceptance Criteria:**

- Given opt-in, when manual/automatic sync succeeds, then RBDB reflects managed changes and retains statistics.
- Given mixed sources/ratings-only mode, when syncing, then only Navidrome ratings change; RBDB stays byte-identical.
- Given disabled/legacy settings, when syncing, then existing behavior remains unchanged.
- Given interrupted publication, when reconnecting, then recovery restores a validated complete set and reports failures truthfully.

## Spec Change Log

- 2026-11-01: The connected device exposed legacy-encoded metadata in structurally valid records. Limit UTF-8 validation to filenames and preserve metadata bytes without transcoding. KEEP format validation, ownership checks, allocation limits, runtime values, and recoverable publication.
- 2026-11-01: A malformed audio container exposed that a per-track metadata rejection aborted the entire database update. Downgrade recognized invalid/unsupported audio failures to streamed warnings when source ownership is safe. Retain existing source history (including failed moves) and leave new unreadable tracks unindexed. KEEP I/O errors, source conflicts, database validation, warning persistence and publication failures fatal.
- 2026-11-01: A 10,244-byte podcast comment passed metadata probing but exceeded the encoder's tag guard. Normalize newly scanned text at the Rockbox boundary: first NUL terminates text and TAG_MAXLEN is 520 bytes, cut only at complete UTF-8 characters. Report field/path/byte-count warnings. KEEP existing raw database metadata, filenames, numeric history, byte budgets and strict codec validation.

## Verification

- `rtk npm run build:daemon -- test -p hifimule-daemon -p hifimule-i18n -- --quiet`
- `rtk node --test scripts/tests/*.test.mjs hifimule-ui/tests/*.test.mjs`
- `rtk npm run build --prefix hifimule-ui`
- `rtk npx --prefix hifimule-ui tsc --noEmit -p hifimule-ui/tsconfig.json`
- Inspect compact/desktop controls; independently parse fixtures; report missing hardware/server checks.

## Verification Evidence (2026-10-31)

- `rtk proxy node scripts/build-daemon.mjs test -p hifimule-daemon -p hifimule-i18n -- --quiet`: passed 1,736 tests; 14 existing ignored. Includes both release/endian fixtures, validation, recoverable publication, external-mutation preservation, deletion evidence across interrupted sync, metadata-only execution, rating retry/readback/zero policy, and provider exclusion. Existing compiler warnings remain.
- `rtk proxy node --test scripts/tests/*.test.mjs hifimule-ui/tests/*.test.mjs`: 629 passed, zero failures, 10 existing platform skips (639 total).
- `rtk proxy node hifimule-ui/node_modules/typescript/bin/tsc --noEmit -p hifimule-ui/tsconfig.json`: passed.
- `rtk proxy node node_modules/vite/bin/vite.js build` from `hifimule-ui/`: passed; existing mixed static/dynamic import warnings remain. Direct Node entry points substitute for npm, which is absent from this environment's PATH.
- Final `rtk proxy node scripts/build-daemon.mjs check -p hifimule-daemon --tests`: passed after metadata date/album-artist fallback cleanup and formatting.
- Final `rtk proxy node scripts/build-daemon.mjs test -p hifimule-daemon --bin hifimule-daemon rockbox -- --quiet`: 17 passed after requiring changed manifest records to refresh tags even within timestamp tolerance. `rtk proxy git diff --check` passed; temporary research/edit helpers were removed.
- Parent browser fixture checks passed at 1280x900 and 520x900: settings save/reopen, independent ratings-only mode, invalid-prefix draft retention, existing device preferences, immutable system controls and cloning. Screenshots are at the session visualization root as `rockbox-profile-desktop.jpg`, `rockbox-profile-compact.jpg`, `rockbox-ratings-only-compact.jpg`. This is fixture-scope evidence.
- Parent independent Python parser verified all four packed fixture directories: lengths, magic, master aggregate size, string offsets/ownership, Unicode/path, rating and play count.
- Physical Rockbox and live Navidrome testing were unavailable. Mounted/default-location limitations are documented in `docs/rockbox-database.md`.

## Format References

- [Firmware](https://github.com/Rockbox/rockbox/blob/v4.0-final/apps/tagcache.c), [3.15](https://github.com/Rockbox/rockbox/blob/v3.15-final/apps/tagcache.h).
- [rockbase](https://github.com/iksteen/rockbase), [DAP-DB-Manager](https://github.com/vakintosh/DAP-DB-Manager): references only.
- [Numeric rating contract](https://opensubsonic.netlify.app/docs/endpoints/setrating/).

## Review Resolution

Three independent reviews (blind hunter, edge-case hunter, acceptance auditor) completed.
Nine confirmed findings were deduplicated into eight implementation patch groups; the approved intent was unchanged.

- Match the configured prefix on component boundaries; preserve other-volume entries and ownership evidence.
- Retain every changed rating intent, including zero after an uncertain positive import.
- Reset unrelated source history when an output path is reused; reject ambiguous source evidence.
- Transfer history from the unique indexed candidate after an interrupted move.
- Fence first observations and retries against identity-preserving account changes; never rewrite server identities.
- Recover owned interrupted backup stages while preserving unrelated files.
- Revalidate device connection, transport identity and cancellation after preparation, immediately before publication.
- Budget decoded allocations and encoded output before buffers grow; borrow shared tag strings during encoding.

Each group has a regression reproducing its failure. No unresolved findings or deferred work remain.

## Final Verification After Review

- Full Rust daemon/i18n suite: **1,745 passed, 14 ignored, zero failures**. Includes reconnect/IO replacement/cancellation publication guards.
- Full Node/UI suite: **629 passed, 10 platform skips, zero failures**.
- TypeScript and production Vite build passed; existing compiler/bundler warnings remain.
- Additional per-record output-budget assertion passed separately after the full run; production code was unchanged.
- Independent Python fixture parser passed all four release/endian directories.
- Final browser captures at desktop/compact widths verify saved switches and updated policy/prefix help: `rockbox-final-desktop.jpg`, `rockbox-final-compact.jpg` in the session visualization directory. Browser data was an isolated in-memory fixture.
- Physical Rockbox and live Navidrome validation were unavailable; supported transport/location limits remain documented.

## Suggested Review Order

**Sync lifecycle and connection ownership**

- Follow shared manual and automatic sync admission, capture, and maintenance.
  [sync.rs:3388](../../hifimule-daemon/src/sync.rs#L3388)

- Recheck the admitted connection after asynchronous metadata preparation.
  [sync.rs:5181](../../hifimule-daemon/src/sync.rs#L5181)

**Binary format and recoverable writes**

- Validate both release layouts and bound decoded allocations.
  [rockbox.rs:185](../../hifimule-daemon/src/rockbox.rs#L185)

- Recover interrupted preparation, replacement, and cleanup without deleting unrelated files.
  [rockbox.rs:577](../../hifimule-daemon/src/rockbox.rs#L577)

- Validate and back up the complete set before publishing the master last.
  [rockbox.rs:650](../../hifimule-daemon/src/rockbox.rs#L650)

**Source identity and ratings**

- Match only the selected firmware-visible prefix.
  [rockbox.rs:967](../../hifimule-daemon/src/rockbox.rs#L967)

- Route exact ownership to the original Navidrome account and retain pending intent.
  [rockbox.rs:975](../../hifimule-daemon/src/rockbox.rs#L975)

- Distinguish an initial zero from later rating changes.
  [rockbox.rs:1449](../../hifimule-daemon/src/rockbox.rs#L1449)

**Settings and supporting evidence**

- Expose independent defaults using the existing editor controls.
  [DeviceProfileManager.ts:129](../../hifimule-ui/src/components/DeviceProfileManager.ts#L129)

- Validate portable settings at the RPC boundary.
  [rpc.rs:9590](../../hifimule-daemon/src/rpc.rs#L9590)

- Start regression review with volume and prefix isolation.
  [rockbox.rs:2066](../../hifimule-daemon/src/rockbox.rs#L2066)

- Read prerequisites, rating policy, preservation, and recovery limits.
  [rockbox-database.md:1](../../docs/rockbox-database.md#L1)

## Follow-up 2026-11-01: Legacy Metadata Encoding

- Reproduced the reported `sync_execute[multi-server]: Invalid Rockbox UTF-8` failure using legacy metadata byte sequences from the connected device.
- Read-only inspection of `H:/.rockbox` found TCH v0x10, 45,644 live tracks, and 47 non-UTF-8 metadata records. Filename records were valid UTF-8. All firmware paths use `/<microSD0>/`.
- Store metadata as raw byte vectors; preserve it through sorting, deduplication, and writing. Validate filenames as UTF-8 before path use; retain structural and memory validation.
- Regression covers mixed legacy metadata across both supported layouts/endian variants and rejects invalid UTF-8 filenames.
- Explicit connected-device test passed: decode, encode in memory, decode again, compare every track's metadata and numeric fields, and re-read every input file to confirm byte identity. No device files were written.
- Full Rust suite: 1,747 passed, 15 ignored, zero failures. The new connected-device test is opt-in/ignored by default and was separately executed successfully.
- Release daemon build passed: target/release/hifimule-daemon.exe. The running installation was not modified during diagnosis.

## Follow-up 2026-11-01: ID3-prefixed WAVE metadata

- Reproduced `rockbox-database: Invalid data found when processing input` with a read-only probe of the connected device. The first failing manifest record (index 1250) is `Music/Chic/Risqué/1-04 My Forbidden Lover.mp3`.
- The file starts with a valid ID3v2.4 tag followed by a RIFF/WAVE container holding MP3 audio. FFmpeg detects WAVE but reads its header from the leading ID3 bytes. Explicitly skipping the validated tag boundary allows the same bytes to be read as WAVE; the file does not need rewriting.
- Reuse the decoder's existing bounded ID3/WAVE recognition and explicit container opening in the metadata scanner, retaining cancellation callbacks. Share the existing bounded tag parser with transcoding so the skipped tag still supplies title, artist, album, year and other metadata.
- Regression tests reproduce the failure with ordinary and 8 MiB metadata, verify indexed fields and duration, and preserve ratings/play count and byte-identical source audio. Probe failures now identify the device-relative filename and include the underlying error; the existing database is retained.
- The connected manifest currently contains 45,372 tracks and has prefix `/`. The physical database uses `/<microSD0>`; the device preference must be corrected before retrying maintenance or ratings import. Diagnosis does not change device settings or files.
- Verification: full Rust suite passed with 1,749 tests and 16 ignored. The focused Rockbox suite passed 30 tests with two opt-in hardware tests ignored. The new read-only scanner was separately exercised on H:, passed the original failure and over 6,000 tracks, then was stopped intentionally to avoid rereading the entire library; this was not a completed full-device scan or sync.
- Release daemon build passed (`target/release/hifimule-daemon.exe`, SHA-256 `2d24210d0cd108a9e530e86f5a04d8cefbe89b14c55dc56202deecb02f90c58f`). No installation or device writes were performed.

## Follow-up 2026-11-01: Per-track metadata failures

- The reported Phil Collins file has 5,105,468 bytes after its leading ID3 tag, but its RIFF header declares 5,116,812 bytes (11,344 beyond EOF). FFmpeg also reports an invalid packet during a read-only decode. This is a distinct malformed-input case; the strict audio/container validator remains unchanged.
- Recognized FFmpeg invalid-data or unsupported-audio probe failures now warn per track and allow other database updates. Existing entries keep their metadata and runtime values. New unreadable tracks stay unindexed. Failed moves retain both old history and ownership evidence for a later readable retry.
- Warnings stream into the existing journal with a bounded polling sample. Warning-journal failures still prevent publication. Unsafe source conflicts, file I/O failures and invalid databases retain their fatal behavior.
- Regressions cover retained history, other valid tracks, new unindexed audio, moved-track retry, changed-source refusal, and failed warning storage. An end-to-end metadata-only sync test covers paging 102 warnings while keeping the polling sample bounded.
- Review additionally checks repeated unreadable retries for stable ownership size and refuses reuse of an indexed history path by a different track while its original source remains selected. Regression covers both processing orders in the same sync and after reconnecting.
- Indexed active paths and historical candidates with ambiguous ownership are both fatal, including a retry after removal of a conflicting file. Focused review approved the final guards after reproducing and addressing ownership growth and history-loss cases.
- Final full Rust suite passed: 1,755 tests, 17 ignored, zero failures. The ignored connected-track test was separately executed against a temporary copy of the actual Phil Collins file and full H: database. It verified one warning, 45,644 retained entries with identical metadata/non-offset numeric values, and byte-identical original device database, manifest and audio. Firmware transient flags and rebuilt string offsets are intentionally normalized by the existing encoder.

- Release build passed: target/release/hifimule-daemon.exe, SHA-256 2b162c2eac35d6d48d47164942736c00ecd2a4466fd26df1dabcd6dfb19e6226. Installation and H: files were not modified.

## Follow-up 2026-11-01: Bounded generated metadata

- Read-only inspection of all 53 synced podcasts/audiobook files identified a 10,244-byte comment in `Podcasts/The Rock and Roll Geek Show/20261004 - Handsome Dick Manitoba 2026 California Tour Kickoff.mp3`. It exceeded the encoder's 8 KiB legacy-record guard after probing had completed.
- Rockbox's own builder caps new text at `TAG_MAXLEN = MAX_PATH * 2` (normally 520 bytes). Normalize newly probed text before record construction: end at the first NUL, cap at 520 bytes on a complete UTF-8 boundary, and stream a warning naming the track/field and original/result byte counts. Empty text keeps the existing untagged fallback. Existing raw database values, filenames, codec budgets and runtime preservation are unchanged.
- Strict encoder diagnostics now identify the tag, byte length and a source path if invalid data still reaches encoding.
- Regression reproduced the exact old error, then passed for both formats with the real comment length, 519/520-byte boundaries, multibyte characters and embedded NULs. It preserves a 6,000-byte raw legacy tag, ratings/play count and source audio.
- Full Rust suite passed: 1,756 tests, 17 ignored. The opt-in connected-track copy test also passed for the actual podcast against all 45,644 database entries, verifying the bounded source-comment prefix, retained runtime values, unchanged other metadata, and byte-identical originals on H:. Focused code review approved the change.
- Firmware references: [4.0 tag limit](https://github.com/Rockbox/rockbox/blob/v4.0-final/apps/tagcache.h), [filesystem constants](https://github.com/Rockbox/rockbox/blob/v4.0-final/firmware/include/fs_defines.h), [builder truncation](https://github.com/Rockbox/rockbox/blob/v4.0-final/apps/tagcache.c), [3.15 tag limit](https://github.com/Rockbox/rockbox/blob/v3.15-final/apps/tagcache.h).

- Release build passed: target/release/hifimule-daemon.exe, SHA-256 3ba95e5fedb483894fd170dabd1a61e9e95b7356a01c6850061ea3e18c54cecc. No installation or H: files were modified.
