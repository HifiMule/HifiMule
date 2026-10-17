---
title: 'Replace untracked files at selected music destinations'
type: 'bugfix'
created: '2026-10-17'
status: 'done'
baseline_commit: '9ab9dc34e62daa58973c0d67ee1f14d5bc097ba4'
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Pre-sync reconciliation can leave an existing music file out of the manifest when its tags or encoding do not meet reuse criteria. When a selected track resolves to that same destination, the writer currently cancels sync solely because the existing path is not manifest-owned. The reported example is `Music/Ace of Base/The Sign/01 - The Sign (radio edit).mp3`.

**Approach:** Keep discovery and safe reuse before normal sync, and allow a selected music transfer to replace an existing file at its exact destination inside the configured music folder. Record ownership after the replacement succeeds. Distinguish inspection failures from confirmed collisions in diagnostics.

## Boundaries & Constraints

**Always:** Use the configured music folder, including custom folder settings. Preserve reliable tag/profile matching for reuse, normal versus Force Sync behavior, transport checks, path containment, staging, verified writes, cancellation and manifest checkpoints. Keep reads during preparation non-mutating. Replace only the exact destination of a selected music transfer; other files remain untouched.

**Ask First:** Extending replacement to untracked podcast/audiobook destinations or changing deletion rules.

**Never:** Treat an inspection failure as a missing file; infer track identity from a filename alone and skip the transfer; adopt every file in Music; remove the existing file before staging completes; relax path traversal or symlink escape checks.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
| --- | --- | --- | --- |
| Reusable untracked file | Unique compatible tags match selected track | Preparation adopts through the existing plan; normal sync reuses actual path/bytes | No audio rewrite |
| Unrecognized same destination | Existing file inside configured Music, absent from manifest | Selected output replaces file; successful write records actual size and ownership; later tracks continue | Missing ownership does not cancel |
| Incompatible or missing tags | Existing destination cannot qualify for reuse | Remains scheduled for transfer and is replaced | Do not falsely claim reuse |
| Force Sync | Untracked file at selected music destination | Selected output replaces file | Same folder protections apply |
| Other untracked file | File does not occupy selected destination | Leave unchanged and unowned | No broad import or deletion |
| Unsafe destination | Traversal, directory collision, or symlink escaping configured music folder | Preserve existing content | Specific actionable error |
| Inspection fails | Parent creation or length query returns error | Preserve destination | Report original operation/error, not an ownership collision |
| Transfer fails | Staging or verified write fails | Preserve existing atomic-write guarantees; no successful manifest adoption | Existing failure handling |

</frozen-after-approval>

## Code Map

- `hifimule-daemon/src/device_music_scan.rs:367` — reconciliation enumerates audio and adopts unique compatible matches; filename presence alone does not create ownership.
- `hifimule-daemon/src/rpc.rs:5900` — preparation invokes reconciliation for normal sync; Force Sync bypasses reuse.
- `hifimule-daemon/src/main.rs:2032` — command-line sync invokes reconciliation.
- `hifimule-daemon/src/sync.rs:3803` — writer combines inspection errors and unowned collisions into one cancelling branch.
- `hifimule-daemon/src/device_io.rs:267` — mounted-device inspection distinguishes missing paths from inaccessible or unsafe paths.
- `hifimule-daemon/src/rpc/sync_plan_tests.rs:975` — existing preparation/reuse integration fixture.

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-daemon/src/sync.rs` — replace the unconditional unowned-collision rejection with a music-specific destination policy; validate configured folder containment, including mounted-device canonical paths. Keep existing ownership behavior for other media.
- [x] `hifimule-daemon/src/sync.rs` — split inspection failure diagnostics from rejected destinations and retain underlying error context. Log confirmed replacement of an untracked music destination.
- [x] `hifimule-daemon/src/device_music_scan.rs` — add regression fixtures demonstrating discovery/reuse of compatible untracked music and why incompatible or mismatched files remain pending transfers. Add focused diagnostics for skipped reuse where useful without changing identity rules.
- [x] `hifimule-daemon/src/rpc/sync_plan_tests.rs` and `hifimule-daemon/src/sync.rs` tests — cover prepare-through-execute replacement with an initially empty manifest, subsequent track completion, and successful manifest ownership. Cover custom music folders, Force Sync, unrelated files, inspection failures and containment rejection.
- [x] This specification and the investigation case — record implementation, tests and confirmed limits. The latest user request supersedes the earlier specification's requirement to protect unmatched collisions at selected music destinations.

**Acceptance Criteria:**
- Given a matching compatible file absent from the manifest, when normal sync prepares, then it is reused and its actual path/size enter the execution plan without audio copying.
- Given an untracked file at a selected music destination that is not reusable, when sync executes, then it replaces that destination, records ownership after success and continues to subsequent tracks.
- Given the same collision during Force Sync, when execution runs, then missing manifest ownership does not prevent replacement.
- Given an unrelated file or a destination outside the authorized music folder, when sync executes, then the new policy does not overwrite that file.
- Given failed destination inspection, when sync reports the failure, then its diagnostic identifies the underlying error separately from ownership.

## Spec Change Log

- 2026-10-17: Implemented approved replacement policy; frozen intent unchanged. The earlier collision-preservation requirement is superseded only for selected music destinations.

## Design Notes

The source confirms that pre-sync reconciliation is selective adoption, not a complete manifest rebuild. We cannot establish why this particular MP3 was not adopted without its tags, effective profile and runtime version. A filename collision is nevertheless sufficient to authorize replacement under the user's new policy; it is insufficient evidence for reusing existing bytes. Avoid broad scanner rewrites or weaker identity matching.

## Verification

- `rtk node scripts/build-daemon.mjs test -p hifimule-daemon device_music_scan -- --test-threads=1` — 9 passed.
- `rtk node scripts/build-daemon.mjs test -p hifimule-daemon sync_plan_tests -- --test-threads=1` — selected zero tests: this included file belongs to `rpc::tests`. Corrected verification to `rtk node scripts/build-daemon.mjs test -p hifimule-daemon rpc::tests -- --test-threads=1` — 169 passed, 1 ignored.
- `rtk node scripts/build-daemon.mjs test -p hifimule-daemon sync::tests -- --test-threads=1` — 121 passed, 1 ignored (including review collision regression).
- `rtk git diff --check` — no whitespace errors.

## Implementation Results

- Added a pre-write destination inspector that permits untracked collisions only for music in the configured folder, including custom and root folders. Mounted-device canonical ancestry is checked before parent creation; traversal, symlink escapes and directory collisions still fail.
- Parent creation, path resolution and file inspection errors preserve their operation and underlying cause. Confirmed untracked replacements are logged separately.
- Reconciliation identity and profile rules remain unchanged. Diagnostics explain metadata-probe, encoding and unique-match failures; regression fixtures confirm compatible adoption and pending incompatible/mismatched files.
- Prepared-plan integration covers empty manifests, normal and Force sync, custom folders, subsequent tracks, actual output size, durable ownership and untouched unrelated files. Writer tests cover rejected paths, inspection failures, source failures and verified-write failures without false ownership.
- The sync/RPC suites require an unsandboxed run for local HTTP fixture listeners and macOS SystemConfiguration; the initial sandbox-only failures disappeared on that run. No physical MTP device was tested. The reported MP3's precise reason for failed reuse remains unknown without its metadata/profile/runtime evidence.

## Review Patch

- Track generated destinations within the executing operation. If another selected transfer resolves to an already claimed destination, reject it before writing and preserve the first output and its manifest ownership. This does not prevent replacement of a preexisting untracked file.
- Regression executes two distinct provider IDs with identical destination metadata and distinct downloaded bytes; only the first output and ownership survive.
- Existing backend guarantees remain unchanged: MSC uses a deterministic sibling `.tmp` path with preexisting symlink/race exposure; MTP replacement can delete the old object before uploading the new object. The failure tests establish preservation for source acquisition failure and injected pre-write MSC failure, not universal preservation on every transport failure. Backend hardening is deferred separately.
- Review validation: full `sync::tests` suite passed 121 tests / 1 ignored; `rtk node scripts/build-daemon.mjs test -p hifimule-daemon prepared_sync_replaces_untracked -- --test-threads=1` passed its normal/custom/Force prepared execution regression; `rtk git diff --check` clean.

## Independent Review

Three independent reviewers examined the diff: blind adversarial, edge-case, and acceptance. The selected-output collision was classified as a patch and fixed with an execution regression. Existing MSC staging/race and MTP replacement/directory handling issues were classified as deferred backend hardening; they are documented in deferred-work.md. No unresolved introduced acceptance findings remain.

## Suggested Review Order

**Replacement policy**

- Validate selected music destinations while retaining explicit inspection errors.
  [sync.rs:2181](../../hifimule-daemon/src/sync.rs#L2181)

**Writer integration**

- Prevent selected tracks from overwriting each other during the same sync.
  [sync.rs:3789](../../hifimule-daemon/src/sync.rs#L3789)

**Reuse diagnostics**

- Explain why a discovered file could not be reused.
  [device_music_scan.rs:463](../../hifimule-daemon/src/device_music_scan.rs#L463)

**Regression coverage**

- Verify normal and Force replacement, custom folders, continuation, and durable ownership.
  [sync_plan_tests.rs:1048](../../hifimule-daemon/src/rpc/sync_plan_tests.rs#L1048)

**Failure and collision coverage**

- Verify conflicting outputs preserve the first completed transfer.
  [sync.rs:9937](../../hifimule-daemon/src/sync.rs#L9937)
