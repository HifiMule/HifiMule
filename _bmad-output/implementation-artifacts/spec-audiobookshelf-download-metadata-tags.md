---
title: 'Apply Audiobookshelf metadata to downloaded audiobook tags'
type: 'feature'
created: '2026-10-17'
status: 'done'
baseline_commit: '331a4a97bb1e3d4c5be0643ee567028b387ef5e9'
context:
  - '{project-root}/docs/audiobookshelf-implementation.md'
  - '{project-root}/docs/audiobookshelf-integration-contract.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Downloaded audiobooks retain stale embedded tags despite server corrections, causing incorrect player display and sorting.

**Approach:** Tag downloaded audiobook files during staging using the authenticated API metadata mirrored by `metadata.json`. Support admitted MP3 and AAC/MP4 (M4A/M4B) representations without server filesystem access.

## Boundaries & Constraints

**Always:** Scope tagging to Audiobookshelf audiobooks. Write book title as album, authors joined with `; ` as artist and album artist, current HifiMule part title as title, server part index as track number, valid part count as track total, and valid publication year when present. Normalize disc number/total to 1/1 so stale disc tags cannot override book-wide order. Preserve existing values for absent optional server fields. Preserve audio packets, embedded artwork, chapters and unrelated tags. Resolve metadata against the same validated book/media/file identity as the download. Retain managed-sync ownership, cancellation, bounded staging and final-byte capacity accounting.

**Ask First:** Expanding formats or fields beyond this contract, changing reconciliation to automatically refresh existing files, or relaxing preservation guarantees.

**Never:** Modify server files or metadata, read/write listening progress, re-encode audio, split files into chapters, fetch replacement covers, add series/narrator/genre tagging in this increment, change podcast/music transfers, or reinterpret chapter boundaries as separate files. No new settings screen. Existing files receive updated tags through the existing forced-sync or replacement path; ordinary keep decisions remain unchanged.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| Corrected book | Stale MP3/MP4 tags; current server title and authors | Correct album, artist, album artist and part title | Fail item if required tag write fails |
| Reordered files | Server indices differ from embedded track/disc values | Server track order and normalized disc tags | Reject ambiguous/invalid order instead of guessing |
| Sparse metadata | Missing authors or year | Preserve corresponding embedded fields; write available values | Do not write empty placeholders |
| Untagged MP3 | Valid audio without ID3 | Create tags and preserve audio | Distinguish missing tag from malformed tag |
| Existing rich metadata | Artwork, chapters, custom tags | Preserve untargeted content | Reject unsafe rewrite |
| Interrupted or invalid file | Cancellation, malformed container, write/capacity failure | No partial device file or successful manifest entry | Clean staged resources; use existing item failure policy |
| Existing copy / other roles | Kept book, music or podcast | Existing behavior retained | Forced book replacement uses current metadata |

</frozen-after-approval>

## Code Map

Paths below are relative to `hifimule-daemon/` unless prefixed otherwise.

- `src/providers/audiobookshelf.rs` — DTOs, book mapping and sync resolution.
- `src/providers/mod.rs` — private sync representation.
- `src/sync.rs` — staging, byte limits, atomic device writes and reconciliation.
- `src/rpc.rs` — existing forced-sync replacement behavior.

## Tasks & Acceptance

**Execution:**
- [x] `src/providers/mod.rs` — add an optional private typed sync-tag payload, absent by default.
- [x] `src/providers/audiobookshelf.rs` — obtain current tags during sync resolution with stable identity, positive unique indices and consistent total. Reuse part-title semantics; test sparse metadata, reordering and identity mismatch.
- [x] `src/sync_tags.rs` (new), `src/main.rs`, `Cargo.toml`, `../Cargo.lock` — implement native editing with `id3` and `mp4ameta`. Dispatch by admitted format (staging paths lack extensions). Retain supported ID3 versions; create v2.3 tags. Preserve MP4 chapters and artwork.
- [x] `src/sync.rs` — tag before enqueue using blocking workers and cancellation-safe ownership. Reserve rewrite overhead before editing; account final bytes in permits, device checks and manifest. Preserve explicit-selection/autofill failure policy.
- [x] `src/sync_tags.rs`, `src/sync.rs`, `tests/fixtures/audiobookshelf/` — test the matrix with real MP3/MP4 fixtures: audio-packet equality, artwork, chapter timings, custom tags, reopened values, cancellation and growth. Include MP4 chapter lists and tracks.
- [x] `../docs/audiobookshelf-implementation.md` — document supported fields/formats, automatic tagging on new transfers and forced-sync refresh for existing copies.

**Acceptance Criteria:**
- Given a corrected multi-part book, when synchronized to a device, then independently reopened files expose current book metadata and the server's track order.
- Given a previously synced book, when forced sync runs after a server edit, then replacement files contain current tags without changing remote identity or listening progress.
- Given valid rich audio fixtures, when tags are updated, then audio packets, embedded artwork and chapter timing remain equivalent and playback still decodes.
- Given concurrent staging near its configured limit, when tagging grows a file or is cancelled, then resources are released without deadlock, partial transfer or incorrect final-byte accounting.

## Spec Change Log

- Independent review patches: validate MPEG layer/version/rate and complete first frame; preflight ID3 frame sizes (including nested chapters), rejecting compressed or unsafe header layouts before native allocation. Added malformed-prefix/allocation regression cases and concurrent near-capacity staging with cancellation. Intent remains unchanged; preserve the API identity checks, local-only writes, packet/chapter fixtures and atomic transfer behavior.

## Design Notes

Native tag editing avoids re-encoding. MP4 chapter preservation requires fixture evidence; successful writes alone are insufficient. Keep public catalogue and manifest schemas unchanged.

## Verification

- `rtk cargo test -p hifimule-daemon sync_tags` — round-trip and preservation tests pass.
- `rtk cargo test -p hifimule-daemon audiobookshelf` — provider identity/order and contract regressions pass.
- `rtk cargo test -p hifimule-daemon sync` — staging, force, cancellation and accounting regressions pass.
- `rtk cargo check -p hifimule-daemon` — compile succeeds.
- `rtk cargo fmt --all -- --check` — formatting passes.

**Implementation evidence:** Focused tag suite after review fixes: 11 passed. Sync regressions: 231 passed, 1 existing ignored benchmark. Audiobookshelf provider regressions: 90 passed, 1 ignored live-server test. Offline contract: 5 passed. Tests ran through `rtk npm run build:daemon -- test -p hifimule-daemon ...` to use the bundled audio runtime. Local mock-server suites require localhost binding outside the filesystem sandbox. Bundled-runtime `cargo check`, new module rustfmt and `git diff --check` pass; workspace-wide `cargo fmt --all -- --check` encounters existing unrelated formatting drift. No physical-player test was performed.

**Review outcome:** Three independent reviewers completed. Deduplicated malformed-MPEG and ID3 allocation findings were fixed as implementation patches and confirmed by targeted re-review. Added legacy ID3v1 optional-field preservation. Concurrent reservation release and native copy cancellation are covered; cancellation during an active full blocking-worker pipeline remains an explicit coverage limitation. No intent changes or deferred unrelated fixes.

## Suggested Review Order

**Transfer boundary**

- Apply tags before enqueueing files for atomic device transfer.
  [sync.rs:2626](../../hifimule-daemon/src/sync.rs#L2626)

**Current server metadata**

- Validate file identity and authoritative book-wide order.
  [audiobookshelf.rs:2403](../../hifimule-daemon/src/providers/audiobookshelf.rs#L2403)

**Preservation and resource limits**

- Preflight ID3 frames before native parsing and preserve optional legacy fields.
  [sync_tags.rs:42](../../hifimule-daemon/src/sync_tags.rs#L42)

- Append MP4 metadata without moving audio or chapter sample offsets.
  [sync_tags.rs:314](../../hifimule-daemon/src/sync_tags.rs#L314)

**Verification and documentation**

- Exercise transfers, refreshed tags, and failed replacement preservation.
  [sync.rs:6234](../../hifimule-daemon/src/sync.rs#L6234)

- Compare original and rewritten audio, artwork, custom tags and chapters.
  [sync_tags.rs:564](../../hifimule-daemon/src/sync_tags.rs#L564)

- Document supported tags, limitations and forced-sync refresh.
  [audiobookshelf-implementation.md:44](../../docs/audiobookshelf-implementation.md#L44)
