---
title: 'Preserve distinct selected music with stable collision filenames'
type: 'bugfix'
created: '2026-10-24'
status: 'done'
baseline_commit: 'fc307b37aa3bfa43b866614d0aa4abe81cd223c5'
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** The current one-file destination policy drops distinct music when incomplete tags produce equal filenames. A shared title and track number cannot establish recording identity.

**Approach:** Preserve distinct selected identities using stable collision suffixes. Retain manifest assignments across selection reorder, parallel completion and membership changes.

## Boundaries & Constraints

**Always:** Use portable server identity plus provider item ID. Retain existing assignments within the configured folder and compatible output format. Suffix only conflicting allocations. Check conflicts after sanitization, truncation and device path comparison. Preserve provider abstraction, bounded staging, checkpoints, cancellation, deletion safety and playlist order across music providers.

**Ask First:** Auto-Fill best-version changes, source-tag editing, fingerprinting or changes to untracked-file replacement.

**Never:** Infer audio equality from tags, duration, encoding or destination. Derive suffixes from enumeration/completion order, compact surviving suffixes, alias different identities to one audio file, or add a collision setting.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
| --- | --- | --- | --- |
| Incomplete tags | Three IDs, same title, track zero | Three owned files with distinct paths | Normal transfer errors remain per identity |
| Existing device | Previous coalesced owner and newcomers | Keep owner; transfer newcomers separately | Never adopt owner's bytes |
| Repeat / selection changes | Reorder, add or remove movements | Surviving assignments remain unchanged | Delete only removed ownership |
| Force / repair | Refresh, missing or empty assigned output | Refresh or repair that identity at its assignment | Never reuse another movement |
| Transcoding | Sources converge to one extension | Allocate against validated output format | Preserve format checks |
| Path conflict | Case, punctuation, truncation or suffix collision | Distinct safe bounded paths | Fail explicitly if allocation cannot succeed |
| Apparent rekey | Removed ID and new ID share tags | Transfer new music rather than relabel old audio | Metadata alone cannot prove identity |

</frozen-after-approval>

## Code Map

- `hifimule-daemon/src/sync.rs` — destinations, producer/writer reuse, ID matching, checkpoints and playlists.
- `hifimule-daemon/src/sync_plan.rs` — prepared additions and resident paths.
- `hifimule-daemon/src/device/mod.rs` — existing `SyncedItem` source identity/path mapping.
- `hifimule-daemon/src/rpc/sync_plan_tests.rs` — prepared execution regressions.
- `hifimule-daemon/src/device_music_scan.rs` — tag-based discovery and reconciliation regressions (user-requested reconciliation audit).

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-daemon/src/sync.rs` — replace cross-identity coalescing with one shared identity/path allocator. Reserve residents before additions; retain compatible same-owner paths. Allocate known-format additions in sorted identity order, then allocate response-only formats after validation. Hash the JSON-encoded optional server/item tuple using existing BLAKE3; compare case-folded final paths and reserve suffix space within component/absolute path limits. Extend a conflicting short hash and bound retries explicitly.
- [x] `hifimule-daemon/src/sync.rs` — disable metadata-only music rekeys in both delta matching and capacity estimation, preserving other roles. Remove reuse probes, cross-identity aliases and same-path manifest owner removal. Playlist generation reads each actual manifest assignment.
- [x] `hifimule-daemon/src/sync_plan.rs` — verified existing `ResidentIdentity` already projects server identity and `local_path`; no plan, database or manifest schema changes required.
- [x] `hifimule-daemon/src/sync.rs` and `hifimule-daemon/src/rpc/sync_plan_tests.rs` — replace one-file assertions and cover distinct fixture bytes, exact request counts, stable membership/Force/missing/empty recovery, cancellation/resume, checkpoint failure/retry, output-format and folder compatibility, case/truncation/hash conflicts and repeated playlists.
- [x] `hifimule-daemon/src/sync.rs` and `hifimule-daemon/src/device_music_scan.rs` — user-requested reconciliation audit: reserve adopted ID-change destinations before additions so a newcomer cannot overwrite adopted audio; test hash-looking names through tag-based matching, ambiguous equal tags remaining pending, owned files staying with their source and adopted-owner execution alongside a colliding new track. Preserve existing disabled-scan behavior and matching policy.

**Acceptance Criteria:**
- Given three supplied identities are selected, when sync completes, then three owned files exist and playlists reference their respective audio.
- Given committed assignments, when sync repeats or other selections change, then surviving paths stay identical and unchanged tracks incur no audio download.
- Given interruption or checkpoint failure, when sync resumes, then committed assignments remain stable and no identity inherits another track's audio.
- Given different music IDs with matching tags, when one replaces another in selection, then metadata matching cannot relabel old audio.

## Design Notes

Keep an existing `1-00 Concerto.m4a`; allocate `1-00 Concerto [a81f209c].m4a` for a newcomer (illustrative hash). Retain surviving suffixes when collisions disappear. Folder/format changes may require normal relocation. Stability depends on retained manifest assignments; ID resets and manifest loss cannot safely be reconciled from incomplete tags.

The export contains four repeated-title groups with distinct paths/durations. Source filenames are unnecessary for identity. The user confirmed best-version selection was disabled: it did not cause this issue. Its intended encoding-selection behavior remains outside this change.

## Verification

- Red before production changes: `rtk node scripts/build-daemon.mjs test -p hifimule-daemon distinct_music_collision_preserves_every_identity -- --test-threads=1` failed with one manifest owner instead of three; logs showed two identities coalescing into the third. Initial native compilation: 3m49s; test: 0.25s.
- `rtk node scripts/build-daemon.mjs test -p hifimule-daemon --bin hifimule-daemon sync:: -- --test-threads=1` — final result: 190 passed, 2 intentional benchmarks ignored; 26.18s after 52.84s compilation. Includes capacity planning, transport, cancellation, checkpoint, adopted-path and legacy server-provenance regressions. Captured raw output is in `stable-music-collision-verification.log` beside this spec.
- `rtk node scripts/build-daemon.mjs test -p hifimule-daemon --bin hifimule-daemon prepared_sync_ -- --test-threads=1` — 5 passed; 2.83s.
- `rtk node scripts/build-daemon.mjs test -p hifimule-daemon --bin hifimule-daemon sync_plan::tests -- --test-threads=1` — 8 passed; 1.33s.
- `rtk node scripts/build-daemon.mjs test -p hifimule-daemon --bin hifimule-daemon device_music_scan::tests -- --test-threads=1` — 15 passed; 0.23s. A parallel Windows DLL staging attempt hit EBUSY; sequential rerun passed.
- `rtk node scripts/build-daemon.mjs test -p hifimule-daemon --bin hifimule-daemon sync::protection::tests -- --test-threads=1` — 10 passed; 0.03s.
- Earlier full daemon run: library 15 passed; binary 1678 passed, 3 failed, 14 ignored (139.48s; 1m21s compilation). The changed-semantics capacity test was corrected and passes in the final focused suite. Two unchanged protection fixtures (`delivery_risk_without_a_shared_source_does_not_throttle_unrelated_sync`, `depleted_playback_without_a_shared_source_uses_the_global_minimum_delay`) expired their fixed `snapshot_at(1000)` timestamps after preceding long-running tests, returning `Stale`. Git blame attributes both to commit `989103c8`; they pass standalone. No protection code changed.
- `rtk node scripts/build-daemon.mjs test -p hifimule-daemon -- --test-threads=1 --skip delivery_risk_without_a_shared_source_does_not_throttle_unrelated_sync --skip depleted_playback_without_a_shared_source_uses_the_global_minimum_delay` — passed: binary 1682 passed, 14 ignored, 2 filtered (134.77s); library 15, mediacheck unit 1, contract integration 5 and CLI integration 6 passed; compilation 30.78s. This run preceded the final legacy unknown-to-known server-provenance compatibility follow-up, which has its own focused verification below.
- `rtk git diff --check` — passed before final evidence update.

## Implementation Notes

Implementation and independent review are complete. No active device/server actions were performed. All execution fixtures used temporary directories and mock providers. The native build wrapper required sandbox escalation because sandbox child-process spawning returned EPERM.

The existing cross-source raw-item-ID admission guard remains unchanged. Filename suffix identity includes portable server ID, but this work does not add concurrent admission for equal raw provider IDs on different servers or change playlist schemas. Retained manifest assignments remain the source of path stability; manifest loss cannot be resolved by guessing from incomplete tags.

Change log: implemented the approved collision policy; added reconciliation adopted-path reservation after the user's follow-up identified reconciliation impact. No frozen intent was edited.

## Independent Review

Blind, edge-case and acceptance reviewers found no actionable defects. Acceptance review traced all four criteria through the allocator, writer, reconciliation, deletion guards and regression coverage. Existing raw-ID admission guards remain necessary and unchanged.

The parent independently reran `rtk node scripts/build-daemon.mjs test -p hifimule-daemon --bin hifimule-daemon music_collision -- --test-threads=1`: five passed, zero failed. The final full sync group passed 190 tests with two benchmarks ignored. Reconciliation passed 15; prepared sync passed five; plan persistence passed eight. Two pre-existing full-suite protection timestamp fixtures remain recorded in `deferred-work.md`; they pass standalone. No physical MSC/MTP device smoke test was performed.

Review preparation follow-up: preserve a legacy same-item assignment when an admitted unknown server identity gains known server provenance, using an O(1) lookup and retaining current folder/output compatibility checks. Regression first failed (ordinary filename replaced retained suffix; 45.10s compilation), then passed in the final 190-test sync run. The membership/Force/repair integration now fills server provenance during Force and verifies unchanged assignments. Prepared, plan and reconciliation suites were rerun after this final code change.

## Suggested Review Order

**Stable assignments**

- Reserve existing paths and allocate colliding filenames from source identity.
  [sync.rs:2887](../../hifimule-daemon/src/sync.rs#L2887)

**Reconciliation and preparation**

- Reserve resident and reconciled paths before preparing new transfers.
  [sync.rs:3457](../../hifimule-daemon/src/sync.rs#L3457)

**Identity safety**

- Require transfers for different music IDs even when tags match.
  [sync.rs:446](../../hifimule-daemon/src/sync.rs#L446)

**Regression coverage**

- Verify distinct bytes and repeated playlist references.
  [sync.rs:12946](../../hifimule-daemon/src/sync.rs#L12946)

- Protect adopted audio while downloading a colliding newcomer.
  [sync.rs:12992](../../hifimule-daemon/src/sync.rs#L12992)

- Preserve committed assignments across failed checkpoint retries.
  [sync.rs:13032](../../hifimule-daemon/src/sync.rs#L13032)

- Keep ambiguous same-tag files pending and owned audio with its source.
  [device_music_scan.rs:1091](../../hifimule-daemon/src/device_music_scan.rs#L1091)
