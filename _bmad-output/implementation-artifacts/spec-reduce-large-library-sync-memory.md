---
title: 'Reduce memory retained during long-running library sync'
type: 'refactor'
created: '2026-10-10'
status: 'done'
baseline_commit: '0ce0d8c348e13cc5b95c1c76862779de625ded00'
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** A 40,000-song sync retains about 500 MB in the daemon for hours (screenshot: 509.6 MB; not locally profiled).

**Approach:** Prioritize transfer memory: spool prepared metadata to temporary SQLite, release preparation collections, and stream bounded batches. Return summaries/plan IDs to the UI. Accept preparation peaks; take incidental savings.

## Boundaries & Constraints

**Always:** Preserve selection/budgets, routing, manual-first scheduling, compatibility/force-sync, managed deletion, ID changes, playlists, provenance/history/tier/pity behavior, DeviceIO, per-file durability, recovery and lifecycle fences. Cover manual/automatic sync; reuse tempfile/rusqlite.

**Ask First:** Changing selection semantics, manifest durability or adding a persistent library cache.

**Never:** Reload a full SyncDelta during transfer, truncate selections/failure details, or promise unmeasured memory limits. Preparation redesign is out of scope. Authoritative manifests/compact indexes may scale with library size.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Behavior | Error Handling |
|----------|---------------|-------------------|----------------|
| Large sync | 40,000+ songs | Release preparation data; bounded reads | No omissions |
| Preparation failure | Provider/disk error | No partial plan/device mutation | Clean up |
| Confirmation | Blocked/destructive changes | Exact counts/reasons; paged details | Decline discards |
| Stale token | Target/configuration/manifest changed | Reject before writes | Recalculate |
| Interruption | Cancel/disconnect/failure/shutdown | Drain; remove plan; preserve dirty/pending evidence | Accurate outcome |
| Invalid plan | Corruption/truncation | Validate before writes; stop on read failure | Preserve recovery |

</frozen-after-approval>

## Code Map

- `hifimule-daemon/src/rpc.rs:8142` — delta exchange/execution/history.
- `hifimule-daemon/src/main.rs:2131` — automatic preparation.
- `hifimule-daemon/src/sync.rs:2644` — copied queues/manifests/successes.
- `hifimule-daemon/src/device/mod.rs:1558` — durability snapshots.
- `hifimule-ui/src/components/BasketSidebar.ts:1276` — confirmations.

## Tasks & Acceptance

**Execution:**

- [x] `hifimule-daemon/src/sync_plan.rs` (new), `sync.rs` — consume typed deltas into operation/provenance/ordered-playlist tables with provider/priority indexes, summaries and integrity metadata. Batch limit: 500; SQLite cache: 4 MiB/connection; mmap disabled. Apply compatibility/force transformations before publication; release inputs/delta before transfer.
- [x] `hifimule-daemon/src/rpc.rs`, `main.rs` — add summaries, plan IDs, detail paging/discard and legacy delta adapter to file-backed execution. One pending plan; 30-minute expiry; single-use token bound to destination/connection/configuration/manifest fingerprints. Preserve admission fences; release during confirmation, revalidate before execution.
- [x] `hifimule-daemon/src/sync.rs`, `rpc.rs`, `main.rs` — stream operations/history/playlists through existing scheduling. Replace producer manifests/old target snapshots with required configuration/identity/path/provenance projections and unused success vectors with counts/compact receipts. Drop automatic-sync desired/Auto-Fill collections before transfer. Preserve rollback/per-file commits; bound repeated diagnostics without losing evidence.
- [x] `hifimule-ui/src/components/BasketSidebar.ts`, `hifimule-ui/tests/syncPlan.test.mjs` (new) — exchange summaries/IDs, page blocked details, discard abandoned plans and handle expiry while preserving confirmations.
- [x] Relevant Rust module tests, `scripts/measure-sync-memory.ps1` (new) — cover matrix/parity/bounds; compare deterministic 5,000/40,000/80,000-song jobs to `0ce0d8c` in fresh processes. Record preparation peak separately, transfer private bytes/working set, blocked-writer retention and extended-transfer samples.

**Acceptance Criteria:**

- Given identical fixtures, when old/new execution run, then identities/order/paths/reasons/history/playlists agree, including force-sync and overlapping sources.
- Given 40,000+ additions, when transfer blocks, then no full preparation/add collections, producer manifests or success lists remain; batch limits hold and UI responses are paged summaries.
- Given fresh-process benchmarks, when comparing, then transfer memory decreases and stays stable except manifest growth/bounded diagnostics. Preparation reduction is not a gate; investigate persistent plateaus before completion.
- Given failures, when handled, then temporary ownership is released and recovery/confirmation safeguards hold.

## Spec Change Log

## Design Notes

Audio already uses files/two-track queues. Spool before JSON; drop inputs before execution. Keep durability snapshots. Allocator retention can keep working set high after heap release; measure both.

## Verification

- `rtk cargo test -p hifimule-daemon` — regressions pass.
- `rtk cargo check -p hifimule-daemon` — no new compiler errors.
- `rtk npm run build --prefix hifimule-ui` — UI builds.
- `rtk proxy node --test hifimule-ui/tests/syncPlan.test.mjs hifimule-ui/tests/syncCapacity.test.mjs` — exchange/confirmation/capacity regressions pass.
- `rtk proxy powershell -NoProfile -File scripts/measure-sync-memory.ps1` — before/after memory evidence.

### Implementation verification (2026-10-11)

- Full daemon suite: `rtk proxy node scripts/build-daemon.mjs test -p hifimule-daemon -- --test-threads=8` — 1,410 unit tests and 5 contract tests passed; 8 ignored (including the external memory benchmark).
- Compiler: `rtk proxy node scripts/build-daemon.mjs check -p hifimule-daemon` — passed; 30 existing dead-code warnings.
- UI: TypeScript and Vite production build passed. Prepared-plan, capacity and Audiobookshelf confirmation tests: 18 passed.
- Broad JavaScript suite: 366 passed, 10 skipped, 2 existing Windows harness failures (`radioDefaults.test.mjs`, `serverSwitchBrowse.test.mjs`; `/C:/` URL path resolution in esbuild). Prepared-plan tests pass.
- Disk-backed playlist generation is exercised by existing playlist content, relocation, missing-track, unchanged-file, profile-change and cleanup tests through the plan adapter.
- Memory results and reproducible process sampler are documented in `sync-memory-evidence.md`; synthetic fixture results are not an absolute ceiling for real libraries. Extended blocked-writer samples are included.
- Independent review and focused follow-up are complete; the approved frozen intent was preserved.

## Suggested Review Order

**Plan preparation and ownership**

- Publish a compact summary only after validation and compatibility checks.
  [rpc.rs:5827](../../hifimule-daemon/src/rpc.rs#L5827)

- Consume metadata into temporary SQLite with bounded caches and integrity validation.
  [sync_plan.rs:238](../../hifimule-daemon/src/sync_plan.rs#L238)

- Bind cancellation cleanup to pending-plan ownership and expiry.
  [sync.rs:725](../../hifimule-daemon/src/sync.rs#L725)

- Revalidate destination and consume a confirmed plan before execution.
  [rpc.rs:8341](../../hifimule-daemon/src/rpc.rs#L8341)

**Transfer retention and durability**

- Stream provider batches through existing per-file durability and recovery.
  [sync.rs:2628](../../hifimule-daemon/src/sync.rs#L2628)

- Release automatic selection collections before the transfer starts.
  [main.rs:2037](../../hifimule-daemon/src/main.rs#L2037)

- Keep complete warning evidence on disk with bounded polling samples.
  [sync.rs:2530](../../hifimule-daemon/src/sync.rs#L2530)

- Resolve playlists in bounded passes and skip unchanged files.
  [sync.rs:4235](../../hifimule-daemon/src/sync.rs#L4235)

**Confirmation interface**

- Exchange plan tokens and discard abandoned preparation.
  [BasketSidebar.ts:1278](../../hifimule-ui/src/components/BasketSidebar.ts#L1278)

- Page blocked details and ignore late responses after closure.
  [BasketSidebar.ts:1502](../../hifimule-ui/src/components/BasketSidebar.ts#L1502)

**Regression coverage and measurement**

- Verify cancellation after publication releases both plan and expiry task.
  [sync_plan_tests.rs:640](../../hifimule-daemon/src/rpc/sync_plan_tests.rs#L640)

- Verify multi-page ordering and canonical plan metadata.
  [sync_plan.rs:642](../../hifimule-daemon/src/sync_plan.rs#L642)

- Exercise confirmation, discard and deferred response behavior.
  [syncPlan.test.mjs:1](../../hifimule-ui/tests/syncPlan.test.mjs#L1)

- Measure actual executor retention after durable file commits.
  [sync_memory_benchmark.rs:3](../../hifimule-daemon/src/sync_memory_benchmark.rs#L3)

- Sample fresh Windows processes and retain executable hashes and phase traces.
  [measure-sync-memory.ps1:24](../../scripts/measure-sync-memory.ps1#L24)
