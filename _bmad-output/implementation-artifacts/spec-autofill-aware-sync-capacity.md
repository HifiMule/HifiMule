---
title: 'Allow selected content to reclaim autofill capacity before synchronization'
type: 'bugfix'
created: '2026-10-10'
status: 'done'
baseline_commit: 'd4fd7356fa21c36a35f3e5cb80a4fff0ad00d5d7'
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Start Sync compares the basket, including autofill budget cards, with physical free space. Autofill can therefore block manual sync although its files can be replaced. Track provenance is lost when writing the manifest.

**Approach:** Persist manual/autofill origin per track. Separate their bytes, exclude autofill ceilings from required selection size, and credit existing managed capacity against the full desired manual selection. After each successful copy, delete one pending obsolete file to control temporary usage; finish remaining cleanup afterward.

## Boundaries & Constraints

**Always:** Manual selection wins for tracks, playlists, albums, artists, genres, and other supported containers. Record origin across manual sync and autosync, including unchanged tracks and identifier changes. Persist actual written sizes for new transfers; preserve existing sizes for metadata-only changes. Read older manifests without repair or re-download. Preserve device, dirty-manifest, cancellation, and cleanup safeguards.

**Ask First:** Deletion beyond the calculated delta, changing copy-before-delete ordering, or changes to transcoding estimates and autofill ranking.

**Never:** Treat budgets as actual bytes, reclaim unmanaged content, infer origin from enabled settings or tiers, or modify manifests during delta preview.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Autofill occupies device | Free 1 GB; selected 2 GB; autofill 7 GB; desired manual 4 GB | Start Sync enabled | Runtime checks remain authoritative |
| Already synced selection | Free 1 GB; selected 4 GB; desired manual 4 GB | Enabled; no double counting | Normal reconciliation |
| Large autofill ceilings | Multiple budgets exceed capacity; manual content fits | Enabled | Runtime shared budget caps fill |
| Real overflow | Manual estimate exceeds free plus managed bytes | Disabled; correct excess shown | Reduce selection |
| Origin changes | Unchanged track moves manual↔autofill | Update origin without download | Preview remains read-only |
| Legacy manifest | No origin field | Default selected; classify next resolved sync | No reset required |
| Device unavailable | No storage information or disconnected | Preserve unavailable behavior | Do not invent capacity |
| Interleaved cleanup | New copies and obsolete delta files | Copy one successfully, delete one obsolete file; drain leftovers at end | Failed copies/cancellation preserve pending files |

</frozen-after-approval>

## Code Map

- `hifimule-daemon/src/device/mod.rs` — `SyncedItem` and compatibility.
- `hifimule-daemon/src/sync.rs` — delta and manifest reconciliation.
- `hifimule-daemon/src/rpc.rs` — three calculation routes, recovery, force sync.
- `hifimule-daemon/src/main.rs` — independent provider autosync.
- `hifimule-ui/src/state/basket.ts` — manual size excludes virtual slots.
- `hifimule-ui/src/components/BasketSidebar.ts` — capacity display and admission.
- `hifimule-ui/src/state/syncCapacity.ts` — proposed pure capacity helper.

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-daemon/src/device/mod.rs` — Add defaulted provenance and separate byte totals; update fixtures and round-trip tests.
- [x] `hifimule-daemon/src/sync.rs` — Carry retained-track origin through the delta; persist during execution, including ID changes. Store actual staged bytes. Interleave one eligible delta deletion per successful copy; drain leftovers; preserve replacement safety, ownership checks, manifest writes, and cancellation.
- [x] `hifimule-daemon/src/rpc.rs` — Classify complete desired sets after manual-wins deduplication in every route; preserve effective origin through recovery and force sync.
- [x] `hifimule-daemon/src/main.rs` — Capture autofill origin and admit metadata-only autosync work.
- [x] `hifimule-ui/src/state/syncCapacity.ts`, `hifimule-ui/src/components/BasketSidebar.ts` — Share accounting across admission, excess messages, capacity bar, slot/preview readouts, and descriptor estimates.
- [x] `hifimule-ui/tests/syncCapacity.test.mjs`, Rust test modules — Cover matrix, overlapping selection, ID changes, force sync, actual sizes, preview immutability, and write/delete ordering with failed/cancelled copies.

**Acceptance Criteria:**
- Given overlapping manual/autofill candidates, when sync resolves them, then manual origin wins.
- Given replaceable autofill, when manual content fits, then admission and capacity display agree.
- Given an old manifest, when sync resolves unchanged tracks, then origin updates without transfers.
- Given a preview, when execution is cancelled, then stored provenance remains unchanged.
- Given obsolete delta files, when a copy succeeds, then at most one pending obsolete file is deleted before the next copy; remaining cleanup runs after transfers.

## Spec Change Log

- 2026-10-10: User explicitly authorized one obsolete-file deletion after each successful copy. Extended execution and regression coverage; preserved calculated deletion scope and copy-before-delete safety.
- Review patches: avoid duplicate forced adds after missing-file recovery; preserve an old ID's manifest record when replacement cleanup fails; let same-path overwrites consume a pending obsolete deletion when paired cleanup deletes no physical file.

## Design Notes

Reuse `is_auto_fill`, defaulting to false. Carry explicit updates for resolved unchanged tracks: `adds` alone cannot represent origin. Apply only during execution; missing updates preserve previous origin.

The UI estimates the full manual basket, including other-server selections. Available capacity is `freeBytes + syncedSelectedBytes + syncedAutofillBytes`; remaining fill capacity subtracts the manual estimate, clamped at zero. Credit selected files once and treat autofill as replaceable. Separate physical usage from projected managed usage; clamp bar segments. Runtime ceilings remain limits. Existing source estimates and legacy recorded sizes remain approximate; new transfers use actual staged size, and ID-only changes retain the old file size.

## Verification

**Commands:**
- `rtk cargo test -p hifimule-daemon` — manifest, reconciliation, autosync and existing daemon regressions pass.
- `rtk node --test hifimule-ui/tests/syncCapacity.test.mjs` — capacity scenarios pass.
- `rtk npm run build --prefix hifimule-ui` — TypeScript and Vite build succeed.
- `rtk git -c safe.directory=C:/Workspaces/HifiMule diff --check` — no whitespace errors.

**Execution evidence (2026-10-10):**
- Observed capacity renderer fail with 4 GB desired, 1 GB free, 2 GB selected and 7 GB autofill: it reported 3 GB excess instead of 6 GB remaining. Shared accounting now passes all 6 UI behavioral cases, including multiple budgets, other-server selection, unavailable storage and MTP free-space-only reporting.
- Observed Rust regressions fail for manifest origin serialization, retained origin updates, ID-only file sizes, actual staged bytes (99-byte estimate versus 2,500,000-byte file), deletion ordering and force-sync origin/identifier preservation. These regressions now pass.
- `rtk node scripts/build-daemon.mjs test -p hifimule-daemon -- --test-threads=1` passed: 1,381 unit tests, 7 ignored; 5 integration tests passed. The repository wrapper supplies cached controlled FFmpeg required on Windows.
- The parallel full daemon run passed all sync changes but exposed `rpc::tests::rejected_control_does_not_supersede_pending_selection` returning `PLAYBACK_SELECTION_CANCELLED` instead of `PLAYBACK_SELECTION_SOURCE_UNAVAILABLE`; the serial full suite and an isolated rerun both passed this existing playback test. No playback code was changed.
- `rtk node --test hifimule-ui/tests/syncCapacity.test.mjs` passed (6 tests).
- npm is unavailable on PATH, so the build's exact project-local CLIs ran instead: `rtk node hifimule-ui/node_modules/typescript/bin/tsc -p hifimule-ui/tsconfig.json` and, from `hifimule-ui`, `rtk node node_modules/vite/bin/vite.js build`. Both passed; Vite emitted its existing chunk/import warnings.
- `rtk git -c safe.directory=C:/Workspaces/HifiMule diff --check` passed.

The first copy still requires temporary physical space. One-for-one interleaving controls file count; files of different sizes can still increase intermediate byte usage. Legacy manifest sizes and manual selection estimates remain approximate.

**Review patch verification (2026-10-10):**
- Missing-file recovery followed by force sync failed with autofill origin `true` instead of manual `false`; force now retains the already-classified recovery add.
- Refused cleanup of a distinct old ID failed with only one tracked manifest entry instead of two; both written files now remain managed until obsolete-file removal succeeds.
- Same-path overwrite failed with operation trace `write, write, delete, delete`; eligible scheduled cleanup now produces `write, delete, write, delete`.
- Focused suites passed: 104 sync tests and all 3 force-sync route tests.
- Final `rtk node scripts/build-daemon.mjs test -p hifimule-daemon -- --test-threads=1` passed: 1,384 unit tests, 7 ignored, plus all 5 integration tests.
- UI and build output are unchanged from the earlier verified runs. `rtk git -c safe.directory=C:/Workspaces/HifiMule diff --check` passed.

**Independent verification:** The serial daemon suite, TypeScript typecheck, Vite build, and whitespace check passed. The broader UI suite passed 51 tests; two unchanged test files failed on Windows because their esbuild entrypoints use URL `.pathname` instead of a filesystem path. Recorded in `deferred-work.md` alongside the parallel playback-test failure. Three independent reviews identified the patches above. Their first-copy headroom finding is the documented consequence of the explicitly requested copy-before-delete ordering.

## Suggested Review Order

**Selection capacity**

- Compare selected demand against free and managed bytes without counting autofill ceilings.
  [`syncCapacity.ts:13`](../../hifimule-ui/src/state/syncCapacity.ts#L13)
- Bind admission and capacity display to manual selection size.
  [`BasketSidebar.ts:1059`](../../hifimule-ui/src/components/BasketSidebar.ts#L1059)

**Persistent provenance**

- Default old manifests to selected and persist the autofill flag.
  [`mod.rs:64`](../../hifimule-daemon/src/device/mod.rs#L64)
- Classify resolved additions, identifier changes, and retained metadata without preview writes.
  [`sync.rs:239`](../../hifimule-daemon/src/sync.rs#L239)
- Preserve recovered additions instead of duplicating forced transfers.
  [`rpc.rs:8182`](../../hifimule-daemon/src/rpc.rs#L8182)
- Apply the same classification to automatic synchronization.
  [`main.rs:1946`](../../hifimule-daemon/src/main.rs#L1946)

**Copy and cleanup safety**

- Preserve ownership when paired replacement cleanup fails.
  [`sync.rs:3570`](../../hifimule-daemon/src/sync.rs#L3570)
- Delete one eligible obsolete file after a successful copy.
  [`sync.rs:3620`](../../hifimule-daemon/src/sync.rs#L3620)
- Share validated deletion and per-file manifest persistence with final cleanup.
  [`sync.rs:2424`](../../hifimule-daemon/src/sync.rs#L2424)

**Behavioral regressions**

- Verify same-path replacement cleanup occurs before the next copy.
  [`sync.rs:5946`](../../hifimule-daemon/src/sync.rs#L5946)
- Exercise capacity reclamation, overflow, unavailable storage, multiple slots, and MTP.
  [`syncCapacity.test.mjs:15`](../../hifimule-ui/tests/syncCapacity.test.mjs#L15)
