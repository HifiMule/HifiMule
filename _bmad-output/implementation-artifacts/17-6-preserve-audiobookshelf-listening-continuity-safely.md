---
baseline_commit: afcc2d8ab57bec43654fa8dab762c8db6ac12572
---
# Story 17.6: Preserve Audiobookshelf listening continuity safely

Status: in-progress

## Story

As a listener,
I want HifiMule playback to resume and report a book at the correct whole-book position,
so that I can move safely between HifiMule and Audiobookshelf.

## Acceptance Criteria

1. **Stable identity.** A player-owned continuity record binds the configured server, selected Books library, library-item ID, media ID, and active file/part identity to the current playback occurrence. It persists the stable item identity and whole-book offset needed for safe recovery. Title, path, part index, artwork, and stream URL never establish identity. Missing, replaced, cross-library, or stale mappings inhibit remote writes.
2. **Remote resume at admission.** Starting an Audiobookshelf book or part reads the current user's remote progress through the provider before audible playback. A valid unfinished whole-book position maps to the correct real audio file and local position. Absent progress starts at the requested beginning; finished, malformed, out-of-range, or inaccessible progress has an explicit safe outcome. A user-requested part start or explicit seek must not be silently overridden by a late resume response. Existing local session recovery retains its generation/intent fences.
3. **Correct position conversion.** File durations and the deterministic audio-file order from 17.3 define cumulative whole-book offsets. Convert whole-book seconds to a file plus local milliseconds and back, including single-file and multipart books, boundary positions, sparse or invalid durations, and chapters that cross files. Chapter markers are timing metadata, never fictitious tracks. Do not guess an offset when the file identity/order/duration cannot be proven; disable write-back for that occurrence and explain recoverably.
4. **Player-owned reporting.** While the daemon's current, audibly qualified Audiobookshelf occurrence plays a proven matching book, report bounded whole-book position through the verified progress endpoint. Commit seek results before reporting; do not report requested-but-uncommitted seeks, buffering, paused time, preview, stale generations, abandoned preparations, technical failures, or old queued items. Recheck current server, library, item, media, and file mapping before each write, including asynchronous completion. Coalesce routine updates and flush a final valid position on stop/replacement when possible without delaying playback or shutdown indefinitely.
5. **Completion integrity.** Mark a book finished only after natural completion of the final real audio file with the same proven identity. Completing an intermediate file advances the book position without marking completion. Skip, stop, retry, seeking to the end, and failure must not fabricate completion. Preserve existing album continuation, queue semantics, playback attempt classification, and music-provider behavior.
6. **Failure and privacy.** A 404 or changed remote identity disables write-back; 401 refreshes at most once, 403 is permission denial, and 429/5xx/transport failures use bounded, sanitized recovery without an unbounded queue of stale writes. Local playback remains recoverable if progress service fails. Tokens, raw item/library/media IDs, authenticated URLs, response bodies, and upstream session IDs stay out of UI/RPC/logs. Give a recoverable re-link or refresh explanation when identity cannot be proven, with no implied successful write.
7. **Scope and evidence.** Progress is read on direct player start and written only by that player. No device-sync progress import/propagation, podcast progress, Autofill, device transfer, new parallel player, or general background polling. Offline provider, playback, persistence, and UI tests cover the above, including restart, races, multipart boundaries, stale mappings, and unchanged Jellyfin/Subsonic behavior. Distinguish offline tests from installed-app and controlled-server evidence.

## Tasks / Subtasks

- [x] **Define a private continuity contract and persistence** (AC: 1, 3, 7)
  - [x] Add the narrow provider capability/DTO for reading and writing a book's whole-item progress. Keep raw upstream IDs and auth behind the daemon provider boundary. Reuse the existing `ProviderIdentity`, opaque album/track IDs, and playback source-server routing.
  - [x] Persist the item identity, active part identity, cumulative offset, and mapping validity with the player-owned occurrence, using existing SQLite migration and checkpoint patterns. Invalidate on server/library/media/file changes; do not persist credentials or temporary playback-session data.
- [x] **Resume on playback admission** (AC: 2, 3, 6)
  - [x] Implement bounded `GET /api/me/progress/{libraryItem}` via the existing authenticated Audiobookshelf client, validate returned identity/shape and finite nonnegative time, and classify absent progress versus actual failures.
  - [x] Resolve current book detail and real file durations/order, then map remote whole-book time to the correct file/local time. Apply only while the initiating playback generation and user intent are still current; preserve explicit part selection, seeks, and local restart behavior.
- [x] **Report proven playback and completion** (AC: 1, 3–6)
  - [x] Use daemon playback owner events and committed positions to schedule serialized, coalesced `PATCH /api/me/progress/{libraryItem}` writes with `currentTime`, validated `duration`, and `isFinished`. Fence each queued/in-flight operation to its occurrence and stable identity; do not let a delayed write overwrite newer progress from this session.
  - [x] Convert local file position to whole-book time using the proven mapping. Handle file advancement, pause/stop/replacement, restart, seek, and final natural completion; keep network work outside owner locks and bound shutdown flushing.
  - [x] Show a scoped, localized recoverable explanation for unprovable identity or rejected progress, without exposing raw upstream identifiers. Preserve Book/Part controls and all existing music UI.
- [x] **Verify contract and regression behavior** (AC: 1–7)
  - [x] Add fixture-backed `mockito` provider tests for 200/404/401-refresh/403/429/5xx, malformed DTOs, duplicate writes, redaction, and no speculative 409 conflict handling. Add mapping property/boundary tests for single and multipart books, invalid durations, chapters, and changed file IDs.
  - [x] Add playback/persistence race tests for admission replacement, explicit seek versus late resume, paused/buffering/preview exclusion, committed seek, intermediate/final natural completion, skip/failure, restart, and stale async writes. Run relevant daemon, UI, type-check/build, format/lint, and diff checks; record any pre-existing drift separately.

### Review Findings

- [ ] [Review][Patch] Fast replacement can discard a final position or natural completion before the reporter writes it [hifimule-daemon/src/playback/book_progress.rs:251]
- [x] [Review][Patch] Stop can discard the final valid position, including short sessions [hifimule-daemon/src/playback/book_progress.rs:316]
- [x] [Review][Patch] Optional progress lookups can exhaust direct playback admission time [hifimule-daemon/src/rpc.rs:1068]
- [x] [Review][Patch] Rejected progress writes lack a scoped recovery explanation [hifimule-daemon/src/playback/book_progress.rs:452]

## Dev Notes

### Binding context and current implementation

- Story 17.5 already provides Book and Part direct playback through `playback.play_album`/`playback.play_track`, scoped provider resolution, verified direct MP3/AAC range reads, and private upstream session cleanup. It intentionally performs no progress write. Do not replace its stream/session lifecycle, decoder checks, or source-server routing.
- `hifimule-daemon/src/providers/audiobookshelf.rs` maps book detail into ordered real `Song` parts. Opaque album IDs encode library/item/media; opaque track IDs add the file `ino`. `catalogue_book` checks library/item/media, and `get_song` checks the part. Extend the existing protected GET/POST auth client and bounded JSON/status handling for progress GET/PATCH. The provider currently has no progress methods; its `download_url` remains unsupported.
- `hifimule-daemon/src/providers/mod.rs` defines `MediaProvider`, `PlaybackDescription`, private `PlaybackRequest`, and cleanup. Add only the provider-neutral interface genuinely required by player-owned progress; other providers should preserve existing behavior. Never put bearer headers or raw ABS fields in public playback/RPC DTOs.
- `hifimule-daemon/src/playback/session.rs` is the serialized playback owner. It already checkpoints local positions, gates progress samples by generation/occurrence, tracks committed seeks, distinguishes `naturalCompletion` from skip/failure, and advances album occurrences. `playback/commands.rs` handles admission and audio resume. Place continuity decisions at these boundaries; do not equate a requested seek with the actual committed position.
- Existing DB playback session/attempt persistence is the migration pattern. Inspect actual schema and transaction boundaries before extending it; the continuity record must be invalidated atomically with an occurrence transition. `rpc.rs` already routes a playback item through its portable server identity. The UI in `hifimule-ui/src/library.ts` and Book/Part play controls already works; only a scoped error/status affordance is expected if needed.
- Controlled Audiobookshelf v2.36.1 evidence verifies `GET` and `PATCH /api/me/progress/{libraryItem}` with `currentTime`, `duration`, `isFinished`; duplicate PATCHes returned 200, and a deleted progress record returned 404. The contract found no progress 409 semantics. Use the pinned contract and fixtures; newer upstream APIs do not silently broaden the supported server version. A current upstream issue reports that PATCH progress may not notify other clients live, so verify persisted read-back rather than promising instant websocket propagation. [Audiobookshelf issue #4977](https://github.com/advplyr/audiobookshelf/issues/4977)
- Whole-item time must derive from the current ordered files and their verified durations. Define one rounding/clamping policy at ms/seconds boundaries and test it. If metadata cannot prove a unique file offset, preserve local playback and suppress remote mutation. An explicit seek backwards is valid; avoid a blanket monotonic-time rule that would prevent it, while still fencing older asynchronous writes.

### Previous story and repository intelligence

- Story 17.5 review fixed malformed non-ASCII track IDs, upstream session close outcome checks, shutdown cleanup draining, and oversized playback response cleanup. Its offline daemon/UI checks passed; installed-app playback was not exercised. HLS transcode remains unsupported. Recent commits `7d5aefb` (story), `ba2c49b` (development), and `56f30f5` (review) establish the current direct-playback seams.

### References

- [Source: `_bmad-output/planning-artifacts/epics.md` — Epic 17, Story 17.6]
- [Source: `_bmad-output/planning-artifacts/prd.md` — FR85]
- [Source: `_bmad-output/planning-artifacts/architecture.md` — Epic 17 progress bridge]
- [Source: `_bmad-output/planning-artifacts/ux-design-specification.md` — Audiobookshelf progress feedback]
- [Source: `_bmad-output/planning-artifacts/sprint-change-proposal-2026-09-20-audiobookshelf.md` — Story 17.6 and architecture amendment]
- [Source: `docs/audiobookshelf-integration-contract.md` — stable identity and progress endpoint register]
- [Source: `_bmad-output/implementation-artifacts/17-5-directly-play-audiobookshelf-audiobooks-through-hifimule.md` — prior implementation and review]
- [Source: `hifimule-daemon/src/providers/audiobookshelf.rs`, `hifimule-daemon/src/providers/mod.rs`, `hifimule-daemon/src/playback/session.rs`, `hifimule-daemon/src/playback/commands.rs` — current implementation seams]

## Dev Agent Record

### Agent Model Used

GPT-6 Codex

### Completion Notes List

- Review follow-up: bounded optional progress admission lookups; retained the last qualified position across Stop for a final report; surfaced rejected writes through the scoped refresh notice. A fast occurrence replacement can still remove the outgoing binding before the asynchronous reporter writes its final position, so the story remains in progress.
- Review follow-up verification: `cargo fmt --all -- --check` and `git diff --check` passed. Daemon compilation was blocked on this Windows host because the FFmpeg native runtime is absent and `npm` is unavailable for the documented build wrapper; WSL access was denied.

- Added a daemon-private Audiobookshelf whole-book progress capability with bounded authenticated GET/PATCH, strict identity and timing checks, sanitized failures, and one token refresh.
- Added a durable player occurrence binding and version 7 playback migration. Queue changes invalidate the mapping; natural part completion carries it transactionally to the next real file.
- Admission resolves remote progress before audio starts. Explicit part play keeps the requested part start. A serialized player reporter uses committed positions, checks the current generation and mapping before writes, and marks completion only after a natural final-file finish.
- Added a scoped, localized recovery notice for refresh/re-link cases without exposing upstream identifiers.
- Offline evidence: daemon 1,106 passed, 6 ignored; Audiobookshelf contract 5 passed; UI 19 passed; i18n 7 passed; TypeScript check, Vite build, rustfmt, daemon binary Clippy, and git diff check passed. All-target Clippy remains red on the pre-existing `unused_io_amount` deny in `playback/audio/queue_edit_tests.rs`; `-D warnings` exposes 130 baseline lint errors across the repository. Installed-app and controlled-server playback were not exercised.

### File List

- `_bmad-output/implementation-artifacts/17-6-preserve-audiobookshelf-listening-continuity-safely.md`
- `_bmad-output/implementation-artifacts/sprint-status.yaml`
- `hifimule-daemon/src/domain/models.rs`
- `hifimule-daemon/src/providers/mod.rs`
- `hifimule-daemon/src/providers/audiobookshelf.rs`
- `hifimule-daemon/src/playback/mod.rs`
- `hifimule-daemon/src/playback/book_progress.rs`
- `hifimule-daemon/src/playback/model.rs`
- `hifimule-daemon/src/playback/native.rs`
- `hifimule-daemon/src/playback/persistence.rs`
- `hifimule-daemon/src/playback/session.rs`
- `hifimule-daemon/src/playback/session/album_admission.rs`
- `hifimule-daemon/src/playback/session/album_admission_tests.rs`
- `hifimule-daemon/src/rpc.rs`
- `hifimule-i18n/catalog.json`
- `hifimule-ui/src/rpc.ts`
- `hifimule-ui/src/components/PlaybackControls.ts`
- `hifimule-ui/tests/audiobookshelfBrowse.test.mjs`

## Change Log

- 2026-09-23: Implemented private Audiobookshelf listening continuity, durable occurrence mapping, safe resume and write-back, localized recovery guidance, and offline verification.
