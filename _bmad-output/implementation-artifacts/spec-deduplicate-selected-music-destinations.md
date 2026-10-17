---
title: 'Keep one file for colliding selected music tracks'
type: 'bugfix'
created: '2026-10-17'
status: 'done'
baseline_commit: 'b8d71966f01edb67501e34143c5e574fbd645cb5'
context: []
---

<frozen-after-approval reason="human-owned intent — user explicitly selected one file and requested root-cause investigation">

## Intent

**Problem:** Two distinct track IDs resolve to the same music filename. The new guard aborts sync instead of keeping one file. The user explicitly requests one file, not distinct filenames, and investigation of why both records were selected.

**Approach:** Keep a single completed file per selected music destination, continue sync when another ID resolves there, and preserve that decision across retries. Prefer a valid existing selected resident; otherwise keep the first successfully written selected track. Explain the observed source metadata and the difference between ID deduplication and filename collisions.

## Boundaries & Constraints

**Always:** Preserve untracked-file replacement from the previous fix. Keep unrelated files and path/inspection protections. Keep manifest identity, deletion cleanup and playlist paths consistent with the retained file. Respect cancellation and existing transport guarantees. Report duplicate coalescing as a nonfatal diagnostic with both identities and destination.

**Never:** Create a second filename for this collision, cancel solely for duplicate selected music destinations, claim duplicate bytes were written, delete a retained file because a discarded alias was removed, or expose credentials during investigation.

## I/O & Edge-Case Matrix

| Situation | Expected behavior |
| --- | --- |
| Two new selected music IDs, same destination | One output and manifest owner, no sync error, subsequent tracks continue |
| Prior partial sync retained first ID | Reuse retained output; colliding addition does not overwrite it |
| Force Sync selects both | Refresh retained owner once, skip duplicate without aborting |
| Playlist references either ID | Both resolve to retained audio path; no missing entries caused by coalescing |
| Only former duplicate remains selected | Existing file is retained or replaced through normal identity/reconciliation rules |
| Missing former resident file | Recover it rather than falsely claiming reuse |
| Different destinations | Continue normal transfer behavior |
| Real inspection/write failure | Existing error handling and ownership guarantees |

</frozen-after-approval>

## Code Map

- `hifimule-daemon/src/sync.rs` — generated destination, selected collision guard, manifest checkpoint, cleanup, playlist path map.
- `hifimule-daemon/src/sync_plan.rs` — resident identities and persisted plan iteration.
- `hifimule-daemon/src/rpc.rs` — selection deduplication uses track IDs.
- `hifimule-daemon/src/rpc/sync_plan_tests.rs` — preparation/execution regression coverage.

## Tasks & Acceptance

- [x] Trace both reported IDs using local read-only diagnostics; distinguish confirmed evidence from remaining catalog uncertainty.
- [x] Replace aborting duplicate handling with one-file coalescing for selected music, including resident ownership and repeated/Force sync.
- [x] Preserve playlist references and deletion safety for coalesced identities without false manifest ownership.
- [x] Add execution regressions covering fresh, partial/repeated, Force, playlist, missing-file and subsequent-track cases.
- [x] Run relevant daemon tests and independent review; record findings and limits.

**Acceptance Criteria:**
- Given both reported IDs resolve to one destination, when sync runs, then one physical file remains and collision alone does not fail sync.
- Given the first ID is already a resident, when the second remains selected, then retry retains valid existing output instead of overwriting or aborting.
- Given a playlist references both IDs, when sync completes, then both entries can resolve to the retained file.
- Given runtime evidence, when reporting the cause, then source facts and unconfirmed catalog explanations are explicitly distinguished.

## Verification

Use `rtk node scripts/build-daemon.mjs test -p hifimule-daemon sync::tests -- --test-threads=1`, relevant `rpc::tests`, and `rtk git diff --check`. Local mock HTTP/macOS tests may require the previously established sandbox escalation.

## Implementation Evidence

- Selected music destination collisions now produce nonfatal diagnostics with both identities and the destination. A valid selected resident retains ownership; re-added resident owners may refresh once, including when a Force Sync alias arrives first. New destinations choose the first successfully written/checkpointed owner.
- Resident roles are included in the ephemeral plan projection with a serde default. No persistent manifest or catalog schema changed. File inspection is deferred until an addition collides, avoiding an extra full-library metadata scan.
- Skipped aliases resolve to the retained owner's path for playlist generation and do not become extra manifest owners or transferred bytes. Duplicate ownership is removed during successful checkpointing; scheduled alias deletion preserves retained music files. Removing all selections still deletes the single owned output normally.
- Execution scenarios cover fresh duplicates plus a subsequent track, existing first owner with only the second ID added, Force alias-before-owner ordering, missing resident recovery, obsolete-alias deletion, only-alias selection, and final removal of all selections. Prepared-plan regressions cover normal, custom-folder and Force execution. Distinct downloaded bytes and different source duration deliberately test filename policy without assuming identical content.
- `rtk node scripts/build-daemon.mjs test -p hifimule-daemon sync::tests -- --test-threads=1`: 121 passed, 1 ignored.
- `rtk node scripts/build-daemon.mjs test -p hifimule-daemon rpc::tests -- --test-threads=1`: 170 passed, 1 ignored.
- `rtk node scripts/build-daemon.mjs test -p hifimule-daemon sync_plan::tests -- --test-threads=1`: 8 passed after adding the resident-role compatibility default.
- `rtk git diff --check`: clean. Local HTTP/macOS tests ran with the established sandbox escalation.

## Confirmed Limits

Final destination coalescing occurs after source staging, so duplicate candidates can still incur download/staging work and its existing failures. Preparation estimates remain conservative and can show the existing overridable UI capacity confirmation or reduce autofill budget. Daemon execution has no aggregate-estimate rejection; its actual destination-space check occurs after duplicate skipping. Transport atomicity guarantees are unchanged; no hardware MTP validation was performed. Independent review and the read-only source investigation are tracked separately by the parent workflow.

## Independent Review Patches

- Coalesced ID changes checkpoint removal of the superseded ID instead of leaving stale duplicate ownership.
- Re-added aliases moving from a distinct old path run replacement cleanup only after retained output inspection. Failed cleanup preserves old ownership and reports the error.
- Successful writes clear stale aliases for the identity's real output; removed selected resident identities also receive playlist aliases.
- Collision sharing requires music on both sides, preserving a previously selected podcast/audiobook output even when folders overlap.
- Owner-to-path indexing replaces a full retained-library scan on each successful write. Zero-byte retained music is treated as unavailable for coalescing; arbitrary nonzero size/encoding corruption semantics are unchanged.
- Expanded execution regression covers 12 lifecycle scenarios, including ID rekey, moved aliases, cleanup failure, stale alias replacement, redundant resident playlist references and zero-byte recovery. A separate role-pair policy regression covers all music/podcast/audiobook combinations.
- Post-review verification: `sync::tests` **122 passed / 1 ignored**; `prepared_sync_` RPC filter **5 passed**; whitespace check clean. Earlier complete RPC suite passed 170 / 1 ignored; plan suite passed 8.

## Confirmed Runtime Cause

Read-only local album RPC confirmed two distinct records from the same server, album ID and artist ID. Both carry the same title, disc 1, track 1, MP3 format and 128 kbps bitrate. `558mY8oZtpZyqR7qeuJqP1` lasts 253 seconds (4:13); `3MPvUNc43yMrRVPjTst6nl` lasts 210 seconds (3:30). Auto-Fill admitted both because selection deduplication uses source IDs. Filename construction uses artist/album/track/title/extension, so both resolve to the same path. They are not proven identical recordings; the source catalog exposes differing durations with identical filename metadata. The first is already resident on FLASHDISK and therefore remains the representative under the user's explicit one-file policy. See the investigation for local evidence sources.

## Independent Review

Blind, edge-case and acceptance reviews identified lifecycle patches: obsolete paths on coalesced moves, stale ownership after rekey, missing aliases for removed resident identities, and stale aliases after a later distinct write. All were corrected with regression scenarios. Additional guards retain cross-media collision protection, recover zero-byte residents, and avoid per-write full-library scans. Acceptance follow-up confirmed its concrete findings are addressed. Arbitrary nonzero audio corruption checks remain outside the existing reuse guarantees.

## Suggested Review Order

- Reserve selected resident ownership so retries keep the same physical file.
  [sync.rs:2987](../../hifimule-daemon/src/sync.rs#L2987)

- Skip colliding music without failing sync; preserve cleanup and playlist identity.
  [sync.rs:3938](../../hifimule-daemon/src/sync.rs#L3938)

- Resolve both source identities to the retained playlist path.
  [sync.rs:4494](../../hifimule-daemon/src/sync.rs#L4494)

- Exercise twelve ownership, retry, Force, recovery and removal scenarios.
  [sync.rs:10104](../../hifimule-daemon/src/sync.rs#L10104)

- Verify normal and Force prepared-plan coalescing with durable ownership.
  [sync_plan_tests.rs:1104](../../hifimule-daemon/src/rpc/sync_plan_tests.rs#L1104)
