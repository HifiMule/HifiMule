---
title: 'Avoid repeated downloads for coalesced selected music'
type: 'bugfix'
created: '2026-10-18'
status: 'done'
baseline_commit: '1529c1df14df336de9083ded2e512abd60e6a4e7'
context: []
---

<frozen-after-approval reason="human-owned intent — user approved the investigation fix direction and explicitly requested implementation">

## Intent

**Problem:** Auto-Fill can select several server identities sharing one device filename. The manifest correctly retains a single owner, but alternate IDs are downloaded and converted on every sync before the writer recognizes their reusable destination. The supplied excerpt contains 40 full downloads discarded this way.

**Approach:** Resolve reusable selected music destinations before requesting audio whenever their output path is safely known. Preserve the writer's established coalescing, ownership, cleanup and playlist behavior. Use the validated response destination before full staging when format uncertainty prevents an earlier decision. Keep diagnostics explicit about reuse and actual writing.

## Boundaries & Constraints

**Always:** Keep one physical output and one manifest owner per colliding selected music destination. Validate retained file existence, nonzero size and existing destination protections. Keep Force refresh of the retained owner once, including alias-before-owner ordering. Preserve missing/empty repair, unselected-owner replacement, moved-alias cleanup, ID rekeys, playlist references and manifest checkpoints. Make producer/writer coordination bounded and cancellation-aware. Retain provider abstraction and streamed temporary staging for actual transfers.

**Ask First:** Changing the established one-file collision policy, managed deletion semantics or persistent manifest ownership format.

**Never:** Persist aliases as duplicate file owners; infer identical recordings from equal names; skip a refresh solely because its own owner is resident; use source size as encoded size; silently suppress inspection errors; broadly assume requested and returned containers are identical; add new settings or modify the active device during development.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
| --- | --- | --- | --- |
| Repeat Auto-Fill | Selected MP3 resident and another selected ID at its deterministic MP3 path | Reuse before URL/audio request; preserve bytes and owner | Existing destination errors propagate |
| Same-run collision | First completed selected output already exists | Later aliases can reuse before full staging | Uncompleted output cannot establish reuse |
| Force | Alias arrives before the re-added retained owner | Alias reuses; owner refreshes exactly once | Normal write/checkpoint failures remain visible |
| Missing or empty | Retained path absent or zero bytes | Fetch and repair through normal transfer | Never claim reuse |
| Owner removed | Original owner is no longer selected | Normal replacement or identity reconciliation | Preserve deletion rules |
| Alias moves | Alias previously owned another path | Reuse new destination, clean old path and ownership | Failed cleanup retains old ownership and error |
| Output uncertainty | Destination depends on response container | Keep normal format validation; check before full body staging | Incompatible responses remain rejected |
| Other media | Podcast/audiobook shares a music path | Preserve cross-role collision protections | Never coalesce as music |
| Cancellation | Producer waits for writer decision | Waits terminate and staging resources release | No hung pipeline or false completion |

</frozen-after-approval>

## Code Map

- `hifimule-daemon/src/sync.rs` — producer request/staging, bounded writer queue, selected destination index, coalescing checkpoints, playlist aliases and lifecycle regressions.
- `hifimule-daemon/src/rpc/sync_plan_tests.rs` — prepared-plan execution regression for normal and Force sync.
- `_bmad-output/implementation-artifacts/investigations/autofill-mp3-repeat-sync-investigation.md` — log evidence, original diagnosis and correction record.
- `_bmad-output/implementation-artifacts/spec-deduplicate-selected-music-destinations.md` — established collision policy and previous staging limitation.

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-daemon/src/sync.rs` — add early reuse coordination and conservative destination selection; retain one authoritative coalescing lifecycle in the writer; log writing only for actual write attempts.
- [x] `hifimule-daemon/src/sync.rs` — extend regression coverage with request counts for repeat encoded MP3 aliases, Force owner refresh, missing/empty recovery, destination uncertainty and pipeline cancellation as applicable.
- [x] `hifimule-daemon/src/rpc/sync_plan_tests.rs` — run existing prepared-plan normal/Force coalescing regression; adjust fixtures only if required by earlier reuse.
- [x] Investigation and specification artifacts — record implementation, verification, review findings and remaining limits.

**Acceptance Criteria:**
- Given a completed MP3 sync with two colliding selected IDs, when the same selection syncs again, then the alternate ID incurs no audio URL resolution, HTTP request or staging when its destination is determinable before requesting audio.
- Given a supported output whose destination is ambiguous until response validation, when the destination matches reusable selected music, then the response body is not fully staged.
- Given either alias in a playlist, when a repeat sync reuses its owner, then every playlist occurrence still resolves to the retained path with a single durable owner.
- Given Force, missing/empty files, removed owners or a genuine inspection failure, when early reuse runs, then existing refresh, repair, cleanup and error behavior remains correct.

## Spec Change Log

- 2026-10-18 review: unconditional writer probes serialized ordinary downloads behind prior device writes. Add a conservative destination-hint filter; preserve the shared coalescing lifecycle, cancellation and ownership guards.
- 2026-10-18: Implementation authorized directly by the user following the investigation and fix direction; no further approval required for this scope.

## Design Notes

Only the writer should decide that a candidate has been handled: its retained destination index changes after writes, rekeys and cleanup. A bounded candidate message with a cancellation-aware reply can reuse the same coalescing path before source work without racing mutable ownership. Do not duplicate the lifecycle into a producer-only shortcut. A conservative shared destination-hint index must let unrelated tracks and the retained owner’s own refresh proceed without waiting for a writer reply, preserving download/write overlap. Hints only identify possible collisions; the writer remains authoritative. A conservative path prediction must account for actual output compatibility; uncertain formats retain response validation and a pre-staging fallback.

## Verification

- `rtk test node scripts/build-daemon.mjs test -p hifimule-daemon sync::tests -- --test-threads=1`
- `rtk test node scripts/build-daemon.mjs test -p hifimule-daemon prepared_sync_ -- --test-threads=1`
- `rtk git diff --check`

The existing macOS system-configuration dependency and local HTTP fixtures require sandbox escalation for these regressions. No physical MTP smoke test is available in this environment.

## Implementation Verification

- Full `sync::tests` suite: **128 passed, 1 ignored**. Covers reuse, transport, staging, cancellation, memory bounds, metadata and ownership.
- Final targeted collision lifecycle regression after pending-owner hint refinement: **passed**, including thirteen scenarios and the missing-resident Force alias-before-owner case.
- Final `prepared_sync_` subset: **5 passed**, including a repeated prepared plan with no additional audio request or transferred bytes.
- `git diff --check`: passed.
- New MP3 regression asserts one total Jellyfin negotiation and audio request across fresh and repeated selections, no alias negotiation, one durable owner, and playlist references for both IDs.
- Ambiguous-output regression keeps distinct MP3 and FLAC resident sentinels and checks reuse chooses the validated response path without staging.
- Existing blocked-write regression now verifies normal and Force staging overlap. Candidate cancellation covers a full bounded queue and an unanswered writer reply.

## Independent Review

Blind, edge-case and acceptance reviewers examined the change. Review found unconditional candidate requests serialized unrelated downloads behind device writes. A conservative owner-aware destination-hint index now bypasses impossible reuse decisions, preserving normal and Force overlap. Review also identified a stale hint after missing-file repair; publishing the pending staged owner avoids downloading the discarded former owner while the repair writes. All findings were implementation patches within the approved scope; no unresolved actionable findings remain.

Hints never establish reuse. The writer still validates paths, retained existence and nonzero size, performs replacement cleanup, checkpoints ownership and resolves playlist aliases. There are no manifest schema changes. Physical MTP smoke testing remains unavailable; existing transport regressions pass. An uncertain response format still requires opening the audio request before its destination can be checked, but avoids full staging when reuse succeeds.

## Suggested Review Order

- See when a producer can skip source negotiation.
  [sync.rs:1935](../../hifimule-daemon/src/sync.rs#L1935)

- Keep ownership decisions serialized with the writer.
  [sync.rs:2786](../../hifimule-daemon/src/sync.rs#L2786)

- Use conservative hints without blocking unrelated transfers.
  [sync.rs:2815](../../hifimule-daemon/src/sync.rs#L2815)

- Preserve cleanup and checkpoint semantics during early coalescing.
  [sync.rs:4092](../../hifimule-daemon/src/sync.rs#L4092)

- Prove repeated encoded MP3 selections make no additional requests.
  [sync.rs:10699](../../hifimule-daemon/src/sync.rs#L10699)

- Verify repeated prepared plans reuse their resident owner.
  [sync_plan_tests.rs:1112](../../hifimule-daemon/src/rpc/sync_plan_tests.rs#L1112)

