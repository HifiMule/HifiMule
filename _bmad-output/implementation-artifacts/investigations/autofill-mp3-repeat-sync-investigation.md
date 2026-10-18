# Investigation: Repeated Auto-Fill MP3 sync preparation

## Hand-off Brief

The supplied excerpt confirms repeated download/conversion of alternate selected IDs followed by reuse of existing device files. Destination collision handling happens after full staging, while only the retained owner ID persists across runs. Move that reuse decision ahead of staging and add request/body-count assertions to the repeat-sync regression.

## Case Info

- Date opened: 2026-10-18
- Status: Concluded
- Environment: macOS; Auto-Fill; MP3 conversion; checkout 1529c1d
- Evidence: `/Users/akartmann/.codex/attachments/240cc6e3-037b-4674-88c2-be1e7fc1206f/Texte collé.txt`

## Problem Statement

User reports that running Auto-Fill with MP3 conversion appears to copy all tracks again roughly an hour after a previous sync with apparently the same tracks.

## Evidence Inventory

| Source | Status | Notes |
| --- | --- | --- |
| Supplied log | Partial | 500 lines, 14:23:01–14:25:22; begins during managed deletions, ends mid-staging |
| Current source | Available | Clean checkout at start |
| Prior run log and device manifest | Missing | Needed to establish prior selection and physical device state |
| Plan/start diagnostics and final summary | Missing | Excerpt omits reasons for additions and final transfer counts |

## Confirmed Findings

1. The log schedules 4,707 preparation items. It includes 41 preparation starts, 40 staged items, and 40 coalescing messages.
2. At 14:23:07, `3BcCgp68UBgCWP6VxzOHIm` coalesces with `7hWWdrozk2UGOEUVWTe9M1` at `Music/AC_DC/Back in Black/10 - Rock and Roll Ain’t Noise Pollution.mp3` after staging.
3. At 14:23:08, two distinct incoming IDs for “The Sign (radio edit)” coalesce with a third ID at the same MP3 destination.

## Hypothesized Paths

### Hypothesis 1: All files shown are overwritten on the device

- Status: Refuted for the excerpt
- Supporting indicator: log says “Writing” after staging each item.
- Would confirm: actual device write call and transfer completion for those items.
- Would refute: coalescing returns before device writes.
- Resolution: all 40 completed items coalesce. `sync.rs:4055` discards the staging file and continues before the device write at `sync.rs:4069`. The “Writing” message at `sync.rs:3937` precedes this decision.

### Hypothesis 2: Distinct provider IDs sharing destinations bypass early reuse

- Status: Confirmed
- Supporting indicator: every staged item in the excerpt coalesces with another ID.
- Would confirm: delta/reuse uses identity while writer later deduplicates by path.
- Would refute: early reuse already excludes these exact aliases and another condition forces transfers.
- Resolution: `calculate_delta` checks manifest IDs at `sync.rs:5035` and creates additions for absent IDs at `sync.rs:5078`. The writer retains one owner and removes redundant manifest entries (`sync.rs:4034`). Its alias map is execution-local (`sync.rs:3047`), so alternate selected IDs can be added again on subsequent runs.

## Investigation Backlog

| Task | Status |
| --- | --- |
| Trace planner identity and reuse | Done |
| Trace staging/coalescing/write ordering | Done |
| Determine minimal safe correction and regression scenario | Done; direction below |

## Timeline

- 14:23:01: excerpt starts; file 1/4707 begins preparation.
- 14:23:07: first staged file coalesces with another ID at its destination.
- 14:25:22: excerpt ends while staging file 41.

## Source Code Trace (investigation baseline 1529c1d)

| Element | Evidence |
| --- | --- |
| Selection identity | `hifimule-daemon/src/sync.rs:5035`: current IDs come from manifest owners; different selected IDs become additions at 5078. |
| Identity reconciliation limitation | `hifimule-daemon/src/sync.rs:5189`: metadata identity matching pairs additions with deletions. A still-selected resident has no deletion to pair with. |
| Retained destination index | `hifimule-daemon/src/sync.rs:3050`: selected residents seed the destination index. |
| Source request | `hifimule-daemon/src/sync.rs:3256`: resolve URL and open response; validate response output format at 3448–3470. |
| Destination construction | `hifimule-daemon/src/sync.rs:3524`: final path is known before staging. |
| Full local staging | `hifimule-daemon/src/sync.rs:3644`: stream the complete response to disk. |
| Late reuse decision | `hifimule-daemon/src/sync.rs:3991`: inspect existing nonzero destination; different selected IDs coalesce at 4006. |
| Avoided device write | `hifimule-daemon/src/sync.rs:4055`: remove staged file and continue before device transfer at 4069. |
| Transient aliases | `hifimule-daemon/src/sync.rs:4585`: aliases resolve playlists within this run. |

## Conclusion

**Confidence: High for the excerpt and source mechanism.** All 40 fully staged items in this excerpt are discarded in favor of another selected identity's existing output. HifiMule wastes server conversion, network transfer and temporary-disk work before recognizing duplicate music destinations. It does not overwrite those 40 device files. The saved manifest retains one owner per output and the alias mapping is temporary, so the alternate IDs can incur the same work again next sync.

This does not establish that every one of the 4,707 scheduled items behaves identically: the log ends at item 41 and omits plan diagnostics and the final summary. It also does not prove that provider IDs changed between runs or that colliding tracks are identical recordings. The distinct IDs sharing filenames are confirmed; their catalog origin is outside the supplied evidence.

The previous implementation explicitly records this limitation in `spec-deduplicate-selected-music-destinations.md`, under “Confirmed Limits.” MP3 conversion adds cost to the repeated staging but is not itself the identity-matching failure.

## Recommended Next Steps

Move selected-music destination reuse ahead of full source staging. Preserve the one-file policy, playlist aliases, single manifest owner, Force refresh of that owner, missing/zero-byte recovery and cross-media protections. Do not weaken matching or simply compare source sizes with encoded sizes.

The final destination is available at `sync.rs:3524`, before full staging at 3644. Checking there can avoid downloading the full response body, although response opening may already trigger server conversion. Avoiding even the request requires conservative output-format prediction: the current code uses validated response format, which can differ from the requested format. Keep the existing fallback when the destination cannot be known safely in advance.

Add a repeated-sync regression that counts HTTP requests/bodies and device writes, in addition to checking final bytes and ownership. The existing lifecycle regression verifies preserved bytes, retries, Force and missing/zero-byte cases, but does not assert that duplicate staging was avoided.

## Verification

- `prepared_sync_coalesces_selected_music_in_normal_and_force_modes`: passed through the project's build wrapper.
- `selected_music_collisions_keep_one_owner_across_retry_force_and_missing_files`: passed through the project's build wrapper outside the sandbox.
- `git diff --check`: passed.
- The first sandboxed run failed in macOS `system-configuration` initialization (`Attempted to create a NULL object`); the same test passed outside the sandbox.
- No application code was changed; this artifact records the diagnosis.

## Reproduction Plan

1. Select two music records with different provider IDs but identical generated destination metadata.
2. Complete a normal sync; confirm one physical output and one manifest owner.
3. Repeat normal sync with both still selected.
4. Observe the non-owner ID planned as an addition, downloaded/staged, then coalesced with the retained output; verify device bytes remain unchanged.
5. After optimization, assert the non-owner no longer incurs full audio staging, while Force and missing-file recovery retain their existing behavior.

## Follow-up: 2026-10-18

The user approved implementing the fix. The implementation adds a bounded writer-owned reuse request before audio URL resolution for predictable destinations, with a response-validated fallback before staging for uncertain formats. Coalescing still runs through the original ownership, replacement cleanup, checkpoint and playlist path logic. “Writing” is now logged only immediately before a device write attempt.

A conservative shared destination-hint index avoids making unrelated source preparation wait behind device writes. Hints are only an optimization: existing file safety and nonzero-size checks remain authoritative in the writer. Review specifically identified and corrected an initial serialization regression; existing blocked-writer tests verify source staging can still overlap device transfers.

New regression coverage counts Jellyfin transcode negotiations and audio requests across fresh and repeated MP3 selections, checks uncertain response-format reuse without staging, and cancels both full-queue and unanswered-reply waits. The twelve existing collision lifecycle scenarios now also assert per-ID HTTP request counts. Prepared-plan regression coverage repeats the plan with the discarded alias and checks zero additional requests and zero transferred bytes.

Implementation and final validation are tracked in `../spec-avoid-repeated-duplicate-music-downloads.md`.

Final validation: sync suite **128 passed / 1 ignored**; final thirteen-scenario collision lifecycle passed; final prepared-plan subset **5 passed**; whitespace check passed. Independent review patches preserve producer/writer overlap and pending-owner hint correctness. No unresolved actionable findings remain.
