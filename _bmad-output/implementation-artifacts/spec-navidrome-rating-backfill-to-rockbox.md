---
title: 'Backfill unrated Rockbox tracks from supported servers'
type: 'feature'
created: '2026-11-01'
status: 'done'
baseline_commit: 'db9ac3d79e4a1f39a2f3996c04bc7d793e6ab54d'
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Rockbox ratings reach Navidrome, but Navidrome ratings do not populate unrated device tracks.

**Approach:** Extend the existing ratings option to synchronize device edits and backfill unrated Rockbox tracks from their original server/account when its provider supports numeric ratings. Navidrome is currently the only supported server; keep the integration extensible through provider capabilities. Preserve deliberate device clears and later server edits.

## Boundaries & Constraints

**Always:** Backfill only rating zero with a valid positive provider rating; normalize provider ratings to 1–5 and map to 2,4,6,8,10. Require unique manifest/source ownership, the configured firmware prefix and the original server/account. Include newly indexed tracks during the same sync. Keep maintenance and ratings independently selectable. Persist backfill intent before publication and acknowledge its baseline after publication. Keep database validation, cancellation, connection checks, backups/recovery, runtime history and existing outbound retries.

**Ask First:** Overwriting nonzero Rockbox ratings or changing the established deliberate-clear policy.

**Never:** Backfill through providers without numeric-rating support, infer matches from titles, change favorites, rewrite audio, recreate a missing database in ratings-only mode, or modify live devices/accounts during automated verification. Adding support to another provider is outside this change.

## I/O & Edge-Case Matrix

| State | Expected behavior | Failure handling |
|---|---|---|
| Rockbox 0, Navidrome 4 | Publish Rockbox 8; record synchronized baseline | Leave server unchanged |
| Rockbox nonzero | Existing outbound observation/retry rules apply; no backfill | Preserve later server edits when device rating is unchanged |
| Previously rated device becomes 0 | Existing clear takes precedence; exclude from backfill that sync | Pending/uncertain clears remain pending |
| Stable confirmed 0; server later gains a rating | Backfill on a subsequent sync | No cross-sync cache hiding server changes |
| Server 0, missing track, invalid response or unavailable account | Do not seed a rating | Warn for failures; preserve device data |
| Ratings enabled, maintenance disabled | Update existing rating fields only when needed | No audio probes or empty database creation |
| Interrupted device publication | Recover pending backfill without treating it as a device edit | Uncertain provenance leaves both sides unchanged and warns |

</frozen-after-approval>

## Code Map

Daemon: `hifimule-daemon/src/{rockbox,db,sync}.rs` and `providers/{mod,subsonic}.rs`. UI text: `hifimule-i18n/catalog.json`. Documentation: `docs/rockbox-database.md`.

## Tasks & Acceptance

**Execution:**

- [x] `db.rs`: migrate pending inbound intent/publication evidence idempotently; preserve outbound rows.
- [x] Providers: capability-based paged numeric lookup; implement Navidrome only. Use empty-query `search3` for large sets; bound responses, validate IDs/ratings/paging. Qualify once per lookup; cancel between pages; retain requested IDs only.
- [x] `rockbox.rs`: select eligible zero ratings after media changes, exclude device clears, stage backfills and prepare ratings-only updates. Establish published baselines to prevent outbound echoes; preserve metadata/history.
- [x] `sync.rs`: retain pre-transfer outbound capture; backfill at successful finalization against the authoritative manifest. Publish once through existing fences; journal warnings. Keep unchanged ratings-only databases byte-identical.
- [x] Catalog/docs: display “Sync ratings when supported” in all four languages; retain stored settings. Explain conversion, clears, backfill, ratings-only writes and current Navidrome support.
- [x] Tests: matrix, both formats/endians, new tracks, ratings-only mode, mixed sources/accounts, echo prevention, publication retry/migration, bounded paging and no requests without candidates.

**Acceptance Criteria:**

- Given a successful manual/automatic sync and an eligible unrated managed track, when its original server/account supports numeric ratings and has a positive rating, then Rockbox receives twice the normalized rating with history preserved.
- Given a seeded rating and a later server edit, when the unchanged device reconnects, then the seed is not replayed into Navidrome.
- Given ratings enabled without maintenance, when backfill is needed, then only rating-related database state changes and no audio metadata is probed.
- Given disabled ratings or unsafe ownership/account/prefix, when syncing, then no inbound rating is applied and the existing outbound safeguards remain effective.

## Spec Change Log

- 2026-11-01: User requested “Sync ratings when supported”; use provider capabilities so future server support can reuse the integration. Navidrome remains the current implementation.

## Design Notes

Persist inbound intent separately from outbound confirmation, with intended rating and original database fingerprint. An unchanged original snapshot permits retry; a matching applied rating establishes the baseline. Ambiguous evidence must not clear the server or invent a device edit.

The existing ratings flag now permits reverse writes. The original frozen spec remains unchanged.

Primary API references: [paged empty-query search](https://opensubsonic.netlify.app/docs/endpoints/search3/), [per-user numeric ratings](https://opensubsonic.netlify.app/docs/responses/child/).

## Verification

- `rtk proxy node scripts/build-daemon.mjs test -p hifimule-daemon -p hifimule-i18n -- --quiet` — complete Rust suite passes.
- `rtk proxy node --test scripts/tests/*.test.mjs hifimule-ui/tests/*.test.mjs` — UI/catalog contracts pass.
- `rtk proxy node hifimule-ui/node_modules/typescript/bin/tsc --noEmit -p hifimule-ui/tsconfig.json` — types pass.
- `rtk proxy git diff --check` — clean whitespace; review only this extension over the current authorized Rockbox work.
- Use packed database fixtures and mock providers; report physical-device/live-account validation separately.

## Suggested Review Order

- Validate source history before inspecting or skipping an audio path.
  [rockbox.rs:1393](../../hifimule-daemon/src/rockbox.rs#L1393)

- Backfill newly initialized entries, including restored tracks.
  [rockbox.rs:1229](../../hifimule-daemon/src/rockbox.rs#L1229)

- Confirm exact-path absence after FFmpeg reports a missing file.
  [rockbox.rs:1548](../../hifimule-daemon/src/rockbox.rs#L1548)

- Attach filename and operation to fatal filesystem errors.
  [rockbox.rs:518](../../hifimule-daemon/src/rockbox.rs#L518)

- Reproduce the collision-suffixed missing-file failure.
  [rockbox.rs:2960](../../hifimule-daemon/src/rockbox.rs#L2960)

- Verify restored-track ratings in an existing database.
  [rockbox.rs:2689](../../hifimule-daemon/src/rockbox.rs#L2689)

- Verify disappeared files, retained history and fatal recheck errors.
  [rockbox.rs:3074](../../hifimule-daemon/src/rockbox.rs#L3074)

- Explain missing-path warnings and restoration behavior.
  [rockbox-database.md:115](../../docs/rockbox-database.md#L115)

## Review and Verification Results

Three independent reviewers audited the extension. Four implementation patches resolved: warning on failed provider construction, response-limit errors distinguished from unsupported capability, normalized path aliases rejected, and retryable inbound intents released before maintenance changes their snapshot evidence. Follow-up edge/acceptance reviews found no remaining blockers. No intent gaps, spec renegotiation or deferred findings.

Final checks: Rust 1,766 passed / 17 ignored; Node/UI 629 passed / 10 skipped; TypeScript and whitespace checks passed. Tests use temporary fixtures and mock servers. H: and live accounts were not modified or exercised for this extension. No new dependencies.

No local commit was created: signing was unavailable earlier in this session; signing was not bypassed.

Release build passed: [hifimule-daemon.exe](../../target/release/hifimule-daemon.exe), 12526080 bytes, SHA-256 00483d5506715f74bfed8f0406c778f5a33bff384210cc4bcf98e9d59571660e.

## Reopened: Missing Database Backfill

The user reports that rebuilding a deleted database does not restore server ratings, whereas an existing database receives them. This repairs the approved same-sync indexing/backfill requirement; frozen intent remains unchanged. Reproduce with retained confirmed rating observations, pending positive/clear intents, and missing RBDB files. Freshly initialized zero ratings must not be interpreted as device clears or be blocked by stale confirmed observations. Preserve retry semantics, cancellation/recovery and exact ownership/account checks. Verify missing-database creation from scratch and rebuilds with retained history.

## Missing Database Repair Verification

- [x] Reproduce missing-database rebuild with retained confirmed ratings (failed 0 versus expected 8 before repair).
- [x] Preserve pending positives and clears through creation, unavailable lookup and interrupted publication.
- [x] Migrate initialized-zero provenance idempotently without changing existing observations.
- [x] Recover positive device edits after initialization publication but before local acknowledgement.
- [x] Full Rust suite: 1,771 passed, 17 ignored, zero failures. Whitespace checks passed.

Blind and edge reviews identified one recovery patch for a positive device edit after an unacknowledged initialization. It was reproduced before fixing; follow-up edge and acceptance reviews approved the correction. No intent gap, spec change or deferred issue. Verification used temporary databases and mock servers. No live device, account or local application database was modified. UI code was unchanged, so prior Node/TypeScript checks were not repeated.

Final repair release build passed. No changes were installed or started automatically; the executable above contains the missing-database fix and follow-up recovery correction.

## Reopened: Stale Managed Audio Path

Fresh database construction returned bare OS error 2 after a long metadata scan. Read-only native filesystem checks identify a stale manifest collision-suffixed path for Legion of the Damned; the unsuffixed audio exists, but the exact managed suffixed path cannot be opened. Do not infer another source by filename. Add a failing fixture for a missing manifest-owned audio path. Skip only confirmed missing audio with a contextual per-track warning, preserve existing source history and pending ratings, and continue indexing available tracks. Retain source-conflict, root/connection, cancellation, warning-journal, other IO and publication guards. Improve operation/path context for fatal metadata and publication IO errors so future failures are actionable.

Read-only native audit of H: inspected 45,372 managed paths and found 18 NotFound audio paths. A restored exact path must also get server-rating backfill when newly indexed into an existing database; it must never become an unintended device clear. Temporary tests reproduce both failures and verify safe restoration.

## Stale Audio Path Repair Verification

- [x] Reproduce the exact bare OS error 2 from a missing collision-suffixed managed file.
- [x] Warn and continue for confirmed missing audio; preserve same-path and moved source history.
- [x] Keep root loss, source conflicts, other IO and warning-storage failures fatal.
- [x] Restore numeric-rating backfill when an exact missing file is newly indexed later.
- [x] Handle FFmpeg ENOENT only after exact-path absence is confirmed; retain fatal behavior for existing audio or other recheck errors.
- [x] Final focused Rockbox suite: 56 passed, 3 ignored.
- [x] Final full Rust suite: 1,778 passed, 17 ignored, zero failures. Whitespace checks passed.

Independent blind, edge and acceptance review produced one deduplicated patch: the FFmpeg open-time missing-file race. It was reproduced before repair and verified across fresh/same/moved history and existing/missing/recheck-error outcomes. Follow-up edge and acceptance reviews approved the exact-path check. No intent gap or deferred issue. Temporary fixtures and mock servers verified writes; physical-device diagnostics were read-only. UI code was unchanged, so UI tests were not repeated for this repair.

The final stale-audio-path repair release build passed. The source includes the confirmed-missing FFmpeg recheck, and all 1,778 Rust tests pass (17 ignored). No live files were repaired, deleted or replaced by the assistant. No local commit was created because signing was unavailable earlier in this session.
