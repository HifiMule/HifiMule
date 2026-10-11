---
title: 'Compress device manifests with column-based JSON and Zstd-9'
type: 'feature'
created: '2026-10-11'
status: 'done'
baseline_commit: 'c36a7298a6a6260f98554bd51bd27b3b87c0ee34'
context:
  - '{project-root}/_bmad-output/implementation-artifacts/spec-large-sync-checkpoints-and-state-polling.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Manifests can reach tens of megabytes, making batched checkpoints expensive to copy to slow devices.

**Approach:** Use lossless column-based JSON compressed with Zstd level 9, as selected by the user. Read legacy JSON and migrate on the next successful write. Provide a documented read-only command to display either format as ordinary readable JSON.

## Boundaries & Constraints

**Always:** Preserve values, order, duplicates, integer precision, Unicode and defaults. Write `.hifimule.json.zst` and `<device_id>.json.zst` caches; read legacy `.json` only when compressed files are absent. Preserve atomic replacement, recovery evidence, connection fences, cache-first MTP authority and verification. Retain initial admission, 1000-update/ten-minute checkpoints and finalization. Keep RPC and basket hashing unchanged. Encode outside Tokio workers/state locks, under the commit lock; reuse bytes.

**Ask First:** Changing codec, checkpoint bounds, filenames or sync behavior; destructive migration.

**Never:** Interpret corruption as an unrecognized device, fall back from corrupt authoritative caches to stale mirrors, discard recovery evidence, modify files during inspection, or build a full track JSON Value graph solely for compression.

## I/O & Edge-Case Matrix

| Scenario | Expected behavior | Error handling |
| --- | --- | --- |
| Legacy device/cache JSON | Write/verify compressed file before removing legacy | Failed write leaves legacy intact; cleanup failures are surfaced |
| Supported compressed manifest | Restore every value and exact track order | Reject malformed columns/counts/types |
| Empty or full-field library | Lossless round trip, including settings and recovery | No truncation or precision loss |
| Both formats or corrupt compressed file | Compressed file takes precedence | Never fall back to stale legacy JSON |
| Disconnect or persistence failure | Existing recovery and commit semantics survive | Report required failures; preserve dirty/pending evidence |
| Inspection of either format | Pretty JSON on stdout; source unchanged | Diagnostic on stderr and nonzero exit |

</frozen-after-approval>

## Code Map

Paths are repository-relative; `src/` means `hifimule-daemon/src/`.

- `src/device/mod.rs` — models, MSC probe, MTP cache/device reads, ordinary updates, initialization and persistence.
- `src/device/checkpoint.rs` — batched serialization, connection validation and cache-first commits.
- `src/paths.rs` — compressed and legacy MTP cache paths.
- `docs/development-guide.md` — existing display and manual JSON editing instructions.

## Tasks & Acceptance

**Execution:**

- [x] `src/device/manifest_codec.rs` — centralize column encoding, reconstruction, version checks, bounded decoding and blocking-worker execution.
- [x] `hifimule-daemon/Cargo.toml`, `Cargo.lock` — add Zstd with level 9 and frame checksum support.
- [x] `src/device/mod.rs`, `src/device/checkpoint.rs`, `src/paths.rs` — route storage through codec/new filenames, migrate safely, release state locks, reuse bytes, log sizes and distinguish missing/invalid caches.
- [x] `src/device/tests.rs`, `src/sync.rs`, `src/rpc.rs` — test matrix, interrupted migration/cleanup, legacy/full-field parity, both cache identities, thresholds, rollback and reconnect.
- [x] `scripts/inspect-manifest.mjs`, `scripts/tests/inspect-manifest.test.mjs` — add read-only inspection; test parity, malformed data and unchanged sources against Rust-produced fixtures.
- [x] `docs/development-guide.md`, `docs/data-models-hifimule-daemon.md`, `docs/architecture-hifimule-daemon.md`, `README.md` — document storage, migration, downgrade limits and inspection. Replace manual JSON edits with Repair UI/RPC guidance.

**Acceptance Criteria:**

- Given a managed device, when initialization, settings, basket or sync persistence succeeds, then compressed storage restores identical state at rediscovery.
- Given a legacy dirty manifest, when it is migrated, then recovery evidence survives without additional media transfers or deletion.
- Given the 45,522-track fixture, when benchmarking Rust, then record storage size and CPU timings against pretty JSON.
- Given either supported format, when following the inspection documentation, then readable JSON appears without daemon startup or source modification.

## Spec Change Log

- 2026-10-11: User selected option B, column-based JSON plus Zstd-9, and requested documentation explaining manifest display.
- 2026-10-11: User requested visible migration; use `.json.zst` filenames with compressed-first discovery and verified legacy cleanup.

## Design Notes

Use a standard checksummed Zstd frame containing a versioned JSON wrapper and manifest body. Transpose `synced_items` into a record count and named field arrays; retain other fields. Validate equal column counts before reconstruction. Recognize legacy JSON or Zstd magic, reject unsupported wrappers, and enforce a documented 512 MiB decoded-size ceiling. Storage version is independent of `DeviceManifest.version`.

Remove legacy files only after authoritative compressed persistence succeeds. Retry/report local cleanup failures; MTP mirror/cleanup remains best effort. Compressed corruption must never expose an older clean legacy copy.

Document `rtk node scripts/inspect-manifest.mjs "E:/.hifimule.json.zst"`, Node 24+, legacy inputs and copied MTP caches. Show ordinary JSON; redirection targets a separate file. Explain older-build incompatibility.

## Verification

- `rtk node scripts/build-daemon.mjs test -p hifimule-daemon` — full daemon and contract tests pass.
- `rtk node scripts/build-daemon.mjs check -p hifimule-daemon` — controlled FFmpeg build check passes.
- `rtk node --test scripts/tests/inspect-manifest.test.mjs` — inspection tests pass.
- Benchmark release-mode storage and inspect its output; record size, timings and limits.

Implementation verification completed on 2026-10-11: 1,441 unit tests and 5 contract
tests passed serially (10 ignored); controlled build check passed; 10 inspector tests passed.
The refreshed production Rust release benchmark reduced the 45,522-track fixture
from 38,723,346 to 1,937,120 bytes (95.0% smaller), and the inspector restored all
45,522 tracks and pending IDs with dirty state intact. Full commands, timings,
environment, boundary regressions and limitations are recorded in
[manifest-compression-evidence.md](./manifest-compression-evidence.md).
The default concurrent suite exposed the previously documented RPC global-ticket
test race; that case passed in isolation and the full serial run passed. Details
and the preserved failure log are included in the evidence document.

## Review Patches

- 2026-10-11: The parent completed the formal acceptance audit and requested local
  corrections for clean-state preservation, explicit manifest readback/directory
  durability before legacy retirement, inspector metadata/bounded-descriptor
  validation (including duplicate path-alias rejection), and moving ordinary-update snapshots into the encoder. These patches
  and their red/green regressions are complete; existing platform directory-sync
  semantics and the approved codec are retained. See the evidence document for
  verification and the non-Unix durability limitation.

Status remains in review for the parent task's final reconciliation.

## Suggested Review Order

**Storage format**

- Start with the versioned storage wrapper and named columns preserving typed track values.
  [manifest_codec.rs:77](../../hifimule-daemon/src/device/manifest_codec.rs#L77)

- Stream checksummed Zstd while retaining exact metadata number tokens and bounded output.
  [manifest_codec.rs:112](../../hifimule-daemon/src/device/manifest_codec.rs#L112)

- Reject corrupt frames, unsupported versions and inconsistent typed column lengths.
  [manifest_codec.rs:219](../../hifimule-daemon/src/device/manifest_codec.rs#L219)

**Publication, authority and recovery**

- Verify compressed publication before legacy retirement; preserve recovery evidence after uncertain commits.
  [mod.rs:592](../../hifimule-daemon/src/device/mod.rs#L592)

- Release state guards before compression while retaining checkpoint commit ownership.
  [checkpoint.rs:270](../../hifimule-daemon/src/device/checkpoint.rs#L270)

- Resolve authoritative MTP caches without falling back from corrupt compressed storage.
  [mod.rs:3503](../../hifimule-daemon/src/device/mod.rs#L3503)

- Apply established platform directory-sync behavior specifically to manifest publication and retirement.
  [device_io.rs:208](../../hifimule-daemon/src/device_io.rs#L208)

**Inspection and documentation**

- Inspect either format through bounded reads, preserving exact numbers and unchanged source bytes.
  [inspect-manifest.mjs:10](../../scripts/inspect-manifest.mjs#L10)

- Follow device and MTP-cache inspection commands, migration behavior and older-build limitations.
  [development-guide.md:317](../../docs/development-guide.md#L317)

**Supporting verification and dependencies**

- Exercise corrupt publication, durability failures and verified legacy retirement.
  [tests.rs:433](../../hifimule-daemon/src/device/tests.rs#L433)

- Compare Rust-generated fixtures against lossless inspector output and unchanged sources.
  [inspect-manifest.test.mjs:70](../../scripts/tests/inspect-manifest.test.mjs#L70)

- Add Zstd alongside raw JSON metadata token support.
  [Cargo.toml:33](../../hifimule-daemon/Cargo.toml#L33)
