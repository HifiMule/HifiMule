---
title: 'Reduce Rockbox tagcache device I/O'
type: 'refactor'
created: '2026-11-02'
status: 'done'
baseline_commit: '8233ca27565e9a09ff74cb5a747cd6dfbd3cd010'
context:
  - '{project-root}/docs/rockbox-database.md'
  - '{project-root}/docs/sync-performance.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** The user reports over ten minutes for an empty tagcache with 40,000 resident files and 200 transfers, and four minutes with an existing tagcache. Code rereads transferred audio, inspects every managed file and republishes unchanged database files. Timing attribution remains unmeasured.

**Approach:** Reuse final locally staged audio metadata, retain validated entries and publish only changed tagcache files. Instrument cold builds and incremental updates separately. This is a proposal; the current request authorizes analysis only.

## Boundaries & Constraints

**Always:** Preserve formats, endian, legacy tags, ownership, unmanaged/other-volume entries, runtime history, account-scoped ratings, cancellation, external-change detection and device-local recovery. Bound memory/staging. Use destination timestamps. Observe device ratings before media changes; persist backfill intent before publication.

**Ask First:** Weaker recovery/change detection or persistent cross-run caching.

**Never:** Infer final tags from server originals, retain all audio until finalization, substitute a lock for conflict checks, or keep the only recovery backup on the computer.

## I/O & Edge-Case Matrix

| Scenario | Input/state | Behavior | Failure handling |
| --- | --- | --- | --- |
| Transfer | Final local artifact and accepted device/manifest write | Reuse tags with destination timestamp | Failed writes never supply entries |
| Resident | Unchanged validated entry | Retain tags/history | Keep external-change checks |
| Missing index | Resident managed audio | Probe device after transfers | Warn; preserve prior history or leave unindexed |
| No changes | Identical database and ownership | No commit bump/publication | Recovery/rating observation still runs |
| Rating only | Existing database | Replace master; invalidate RAM snapshot | Preserve pending intents |
| Interruption | Journal and backups | Recover changed files, master last | Preserve external modifications |

</frozen-after-approval>

## Code Map

All source paths below are relative to `hifimule-daemon/src/`.

- `sync.rs`: `StagedTrack`, producers, verified writer, checkpoint and finalization.
- `device_music_scan.rs`: FFmpeg/tag probe, including ID3-prefixed WAVE.
- `device_io.rs`: destination metadata and durable writes.
- `rockbox.rs`: snapshot, ownership, codec, ratings, publication/recovery.
- `sync_logging.rs`: correlated stage timers.

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-daemon/src/rockbox.rs`, `hifimule-daemon/src/sync.rs` — instrument snapshot, resident inspection, local/device probes, ratings, encoding, backup, publication and verification; count probes, changed files and I/O bytes.
- [x] `hifimule-daemon/src/device_music_scan.rs` — extract a shared path-based probe preserving parsing, cancellation and warnings.
- [x] `hifimule-daemon/src/sync.rs` — probe both producer routes after final encoding/tag edits, only for maintenance; retain bounded results at the shared `StagedTrack` writer boundary. Admit results after verified copy and accepted manifest update. Join workers before cleanup; release audio promptly.
- [x] `hifimule-daemon/src/device_io.rs`, `hifimule-daemon/src/sync.rs` — capture destination size/time after write; never use staging mtime.
- [x] `hifimule-daemon/src/rockbox.rs` — prefer successful-transfer metadata under exact source/path identity, then validated unchanged entries, then device probing. Track semantic changes; handle ownership-only updates separately and avoid unconditional commit increments.
- [x] `hifimule-daemon/src/rockbox.rs` — journal/back up/replace only byte-changed outputs. Validate the complete candidate and preserve original-set conflict checks, including unchanged files. Keep master-last ordering and RAM-snapshot invalidation. Do not patch master words in place.
- [x] Tests in `hifimule-daemon/src/{rockbox,sync,device_music_scan}.rs` — cover the matrix, both producers, legacy encodings, missing/moved tracks, external modifications, subset transactions and cancellation.
- [x] `docs/rockbox-tagcache-data-flow.md`, `docs/sync-performance.md` — update implemented data origins and measured performance.

**Acceptance Criteria:**
- Given successful transfers, when maintenance runs, then final audio tags are probed locally without reopening destination audio for tags.
- Given 40,000 managed tracks, an empty database and 200 local results, when building, then only the remaining required entries need device probes: approximately 39,800 without another valid source.
- Given unchanged database/ownership, when maintenance completes, then tagcache bytes/commit remain unchanged and no publication journal is created.
- Given a rating-only update, when publishing, then string-file bytes/timestamps remain unchanged.
- Given changed/missing resident files or interrupted publication, when maintaining/recovering, then current refresh, history-retention and conflict guarantees remain intact.
- Given equivalent benchmark workloads, when comparing, then report phase medians, device bytes and probe counts without inventing a speedup.

## Spec Change Log

- 2026-11-02 review: no-op maintenance retains native tombstone rows, unlike fresh encoding. Backfill mapping must always derive live row offsets from the actual candidate master; checking only ratings-only mode can patch a deleted row. KEEP local metadata reuse and unchanged serialization. Added both-format/endian maintenance-plus-backfill regression.
- 2026-11-02 review: selective transactions must persist unchanged dependency hashes in the journal and verify them before recovery or cleanup. An in-memory guard alone cannot detect a dependency modified across interruption. New field defaults empty for legacy journals; conflicting dependencies retain journal/backups. KEEP selective writes, device-local backups and master-last ordering. Regression covers prepared and committed transactions.

## Design Notes

Database editing already happens in RAM. An SSD copy alone adds little. Moving 200 probes locally covers only 0.5% of a 40,000-track first build. Sorted shared strings can change offsets, so incremental metadata processing still requires whole affected output files.

Keep resident-file checks initially. Defer persistent metadata caching, rename-based backups and adaptive rating queries until measured. A validated persistent metadata cache could accelerate later rebuilds, but cannot eliminate first-ever reads. Avoid copying entire resident audio merely to extract tags without evidence of benefit.

## Verification

- `rtk proxy node scripts/build-daemon.mjs test -p hifimule-daemon --bin hifimule-daemon rockbox -- --quiet`: existing/new tests pass; run targeted producer/probe/cancellation tests with the same wrapper.
- `rtk git diff --check`: clean.
- Compare repeated same-device cold builds, 200-track updates, unchanged syncs and ratings-only updates. Restore equivalent starting states; distinguish warm caches; record phases, bytes, probes, resource use and errors. No fixed speedup is promised.

## Implementation evidence

- Common writer-entry probing covers final output from both producer routes without expanding every staged-track constructor. Cancellation joins the probe worker before discarding audio; accepted metadata is limited to 64 MiB and validated against actual destination size/time. Overflow uses resident fallback.
- `rockbox` tests: 58 passed, 3 ignored. Sandbox initially prevented mock HTTP servers from binding; rerun with loopback permissions passed.
- `device_music_scan` tests: 15 passed. `test_execute_provider_sync` tests: 15 passed.
- Regression coverage added for local metadata avoiding device probing, unchanged commit/bytes/RAM snapshot, and ratings-only string timestamps across both formats/endian variants.
- `rtk git diff --check` passed. Real-device 40,000-track benchmarks remain unavailable; documentation explicitly avoids a speedup claim. Logged phase times, probe counts and requested database I/O bytes support that follow-up measurement.

Review-fix verification: Rockbox suite **59 passed, 3 ignored**; diff whitespace checks passed. Local encoding suite **10 passed, 1 ignored**.

## Suggested Review Order

- Capture final staged metadata before copying.
  [sync.rs:4593](../../hifimule-daemon/src/sync.rs#L4593)

- Share metadata parsing across local and resident audio.
  [device_music_scan.rs:129](../../hifimule-daemon/src/device_music_scan.rs#L129)

- Publish changed files while retaining full dependency guards.
  [rockbox.rs:712](../../hifimule-daemon/src/rockbox.rs#L712)

- Validate journal dependencies before rollback or cleanup.
  [rockbox.rs:631](../../hifimule-daemon/src/rockbox.rs#L631)

- Map ratings through actual live disk rows.
  [rockbox.rs:1438](../../hifimule-daemon/src/rockbox.rs#L1438)

- Test interrupted subset recovery with external edits.
  [rockbox.rs:2161](../../hifimule-daemon/src/rockbox.rs#L2161)

