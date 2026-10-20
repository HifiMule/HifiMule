---
title: 'Consistent device output estimates and Auto-Fill packing'
type: 'bugfix'
created: '2026-10-19'
status: 'done'
baseline_commit: '85f82dcac500ed902a4cfc9169fb281dd0dec2f5'
context: []
---

<frozen-after-approval reason="User requested all diagnostic issues be fixed in the current chat">

## Intent

**Problem:** Basket capacity, Auto-Fill selection and prepared sync can reserve different sizes for the same tracks. A real Jellyfin playlist displays 583 MiB from inconsistent parent duration while per-track planning gives 545 MiB. Additional confirmed defects affect fallback encoding bitrate, lossless reservations, goal-derived overrides and selection stopping.

**Approach:** Share authoritative track expansion and device output planning, preserve relevant source audio metadata, and use identical effective encoding decisions for selection and transfer. Fill available budgets with fitting candidates while retaining ranked order and atomic collections.

## Boundaries & Constraints

**Always:** Respect explicit budgets/headroom/duration goals, cancellation, server-qualified identity, device targeting, resident measured bytes, force/profile rewrites and unknown metadata. Default new serialized fields for old manifests/plans. Retain provider abstraction. Cache UI estimates by meaningful input changes rather than polling large libraries repeatedly. Keep estimates conservative where output is variable.

**Ask First:** Destructive device/server changes, publication or deployment.

**Never:** Encode/download media merely to estimate size; change server settings; claim a server cap or exact FLAC compression was measured when it was not; weaken deletion/path safety or album/artist atomicity.

## I/O & Edge-Case Matrix

| Scenario | Expected behavior |
| --- | --- |
| Playlist parent duration differs from tracks | Basket and preparation use identical expanded track durations and output decisions |
| Direct copy, resident reuse, overlapping containers | Count exact known bytes and each server-qualified track once |
| MP3 fallback without explicit bitrate | Request and reserve the same valid codec bitrate, never a lossless fallback |
| Known source resolution, lossless conversion | Use effective rate/depth/channels for a documented PCM upper bound |
| Missing duration with original size/average bitrate | Derive duration before conservative unknown fallback; never infer from rewritten output bytes |
| Oversized candidate followed by fitting candidates | Skip the oversized atomic unit and keep examining candidates within bounded fetch limits |
| Goal bitrate with capacity/headroom clipping | Emit the exact bitrate used during selection for execution |
| Profile/device/basket changes during estimation | Invalidate cache, reject stale replies and refresh authoritative totals |

</frozen-after-approval>

## Code Map

- `hifimule-daemon/src/rpc.rs`, `sync.rs` — basket expansion, output estimates, effective profile, capacity and preparation.
- `hifimule-daemon/src/api.rs`, `providers/{mod,jellyfin,subsonic}.rs`, `domain/models.rs` — source metadata, output constraints and provider requests.
- `hifimule-daemon/src/auto_fill/{mod,fetch,pipeline}.rs` — output reservations, budget normalization, candidate pagination and packing.
- `hifimule-ui/src/{rpc,library}.ts`, `state/{basket,syncCapacity}.ts`, `components/{BasketSidebar,MediaCard}.ts` — profile-aware estimate lifecycle, capacity and item displays.

## Tasks & Acceptance

**Execution:**
- [x] Add authoritative basket estimate RPC and UI caching/invalidation; repair stale persisted container duration and stale slot ceilings using current device capacity.
- [x] Preserve source audio properties through provider songs, Auto-Fill and sync plans; retain target constraints in provider requests and compatibility checks.
- [x] Normalize lossy codec bitrate defaults, derive missing durations from original evidence, and replace fixed lossless ceiling with known effective PCM properties.
- [x] Fix Jellyfin fallback `audioBitRate` units and add a mocked request regression.
- [x] Use one effective profile for device-preferred MTP formats; carry goal-derived encoding override from selection to execution.
- [x] Continue past unfitting tracks/collections, paginate configured priority sources beyond the first 2000 entries, and guard repeated/no-progress pages.
- [x] Add meaningful regressions, run focused suites/builds and review changes independently; update the investigation with fixed versus external/uncertain behavior.

**Acceptance Criteria:**
- Given the supplied playlist metadata, when the 192 kbps basket estimate is computed, then it uses 91 track durations and agrees with the 545 MiB prepared calculation instead of the 583 MiB parent-based result.
- Given direct, reused, transcoded and duplicate selections, when capacity is displayed and Auto-Fill is budgeted, then both use the same effective selected-byte total as preparation.
- Given a Jellyfin fallback request for 192 kbps, when its URL is built, then `audioBitRate=192000` is sent.
- Given available source audio evidence, when a transfer is planned, then compatibility, byte reservation and requested encoding settings agree; unknown/variable output remains explicitly estimated.
- Given remaining capacity and a ranked oversized unit followed by a fitting unit, when Auto-Fill runs, then the fitting unit is considered without exceeding budgets or splitting collections.
- Given goal-based encoding clipped by actual capacity, when selected tracks become sync additions, then the override equals the selection bitrate.

## Spec Change Log

- 2026-10-19: The user explicitly requested all diagnostic fixes together. This supersedes the historical first-oversized early-stop rule; accepted candidates retain priority order while later fitting candidates may now be selected.

## Design Notes

The real playlist has parent duration 24,469.9607951 s, child durations 22,929.2055291 s and per-track integer seconds totaling 22,885 s. At 192 kbps with existing allowances, parent-based prediction is 610,861,207 bytes; per-track prediction is 571,680,976 bytes. Preserve this regression without storing credentials or requiring the live server for tests. Navidrome server caps remain external uncertainty unless negotiated or observed; do not silently lower local estimates on an unverified assumption.

## Verification

- `rtk node --test hifimule-ui/tests/*.test.mjs` — focused UI request, cache, capacity and basket tests.
- `rtk npm run build --prefix hifimule-ui` — UI production build.
- `rtk node scripts/build-daemon.mjs test -p hifimule-daemon -- --test-threads=1` — controlled audio runtime daemon tests.
- `rtk git diff --check` — clean patch whitespace.


## Independent Review

Three independent reviews covered the code diff, edge cases, and acceptance criteria. The blind review found no actionable defects. Acceptance and edge reviews identified patch-level consistency gaps, all repaired and rechecked:

- Metadata ID reassignment now shares measured-byte estimates, consumes each old file once, and respects server ownership, source quality and profile rewrites.
- Exact audio bitrate survives provider conversion, and inferred source duration is enforced by Auto-Fill duration goals.
- Missing physical files receive no reusable/reclaimable capacity credit. Preparation recomputes available capacity, recovers missing rekey sources, and checks automatic-sync recovery before declaring no work.
- Smaller goal reservations replace oversized retained outputs with linked cleanup after a successful write. Automatic-sync preparation failures leave the syncing state.

All review findings were classified as routine patches within the captured intent. Final read-only verification found no remaining actionable blockers.


## Final Validation

- Controlled daemon suite: **1586 unit tests passed, 12 existing tests ignored**; **5 provider contract tests passed**.
- UI suite: **180 tests passed**.
- UI production build passed.
- Patch whitespace check passed.
- The live playlist metadata was inspected without downloading/encoding media or changing server settings. Copied-file size remains user-reported; no independent transfer-size measurement is claimed.

## Follow-up: Parallel Pipeline Test Failures

The user supplied a default parallel daemon run with 1581 passed, 4 failed, and 12 ignored. All four failures timed out waiting for the first device write in the same test fixture. Earlier full validation used `--test-threads=1`; a subsequent unmodified parallel run passed 1585 unit tests and 5 contracts. The reported logs show first-write setup taking over the fixture's two-second deadline under load.

- [x] Give pipeline integration progress waits a shared bounded allowance for loaded runners, preserving queue/overlap/cancellation assertions.
- [x] Make the one-writer release retain an early notification, and regress delayed startup plus early release.
- [x] Validate the targeted regression and full daemon suite with normal parallel execution.

Acceptance: delayed first-write startup must not be treated as deadlock; an early release must not be lost; the original pipeline concurrency, capacity and cancellation invariants remain enforced. No production transfer behavior changes are required by this follow-up.


Follow-up verification: the new delayed-start/early-release regression failed against the original two-second gate and passed after the repair. A complete **normal parallel run** passed **1586 unit tests and 5 provider contracts** (12 existing unit tests ignored); all four reported failing tests passed. Independent blind, edge and acceptance reviews found no remaining actionable issues. Changes are confined to test fixtures and tests; production transfer behavior is unchanged.

## Suggested Review Order

**Authoritative estimates**

- Expand actual provider tracks and fence results against destination changes.
  [rpc.rs:4647](../../hifimule-daemon/src/rpc.rs#L4647)

- Share output decisions and preserve measured reuse, including metadata reassignments.
  [sync.rs:1730](../../hifimule-daemon/src/sync.rs#L1730)

- Use effective source resolution, exact bitrate and conservative unknown-duration reservations.
  [sync.rs:1694](../../hifimule-daemon/src/sync.rs#L1694)

**Encoding and Auto-Fill**

- Send valid fallback bitrate units and supported output limits.
  [api.rs:1156](../../hifimule-daemon/src/api.rs#L1156)

- Derive encoding once and enforce recovered durations before selection.
  [fetch.rs:373](../../hifimule-daemon/src/auto_fill/fetch.rs#L373)

- Replace oversized retained encodings and reclaim old files after successful writes.
  [sync.rs:1811](../../hifimule-daemon/src/sync.rs#L1811)

**UI lifecycle and capacity safety**

- Cache meaningful inputs and reject stale asynchronous responses.
  [basketEstimate.ts:38](../../hifimule-ui/src/state/basketEstimate.ts#L38)

- Refresh authoritative capacity before sync and repair displayed basket metadata.
  [BasketSidebar.ts:196](../../hifimule-ui/src/components/BasketSidebar.ts#L196)

- Exclude absent device files from reusable and reclaimable capacity.
  [sync.rs:1782](../../hifimule-daemon/src/sync.rs#L1782)

**Supporting evidence**

- Preserve source properties without breaking legacy serialized plans.
  [models.rs:192](../../hifimule-daemon/src/domain/models.rs#L192)

- Reproduce the reported playlist duration mismatch without a live server dependency.
  [sync_plan_tests.rs:1191](../../hifimule-daemon/src/rpc/sync_plan_tests.rs#L1191)

- Cover replacement, reuse, identity, resolution and duration boundary behavior.
  [sync.rs:11550](../../hifimule-daemon/src/sync.rs#L11550)


**Pipeline test follow-up**

- Use bounded progress gates without assuming fast setup on loaded runners.
  [sync.rs:7264](../../hifimule-daemon/src/sync.rs#L7264)

- Regress slow startup and an early release without adding real test delays.
  [sync.rs:7329](../../hifimule-daemon/src/sync.rs#L7329)
