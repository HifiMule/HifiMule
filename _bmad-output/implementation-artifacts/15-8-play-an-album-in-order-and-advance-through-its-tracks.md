# Story 15.8: Play an album in order and advance through its tracks

Status: ready-for-dev

## Story

As a HifiMule user,
I want to start an album and hear its tracks in their intended order,
so that I can listen to the complete album without starting every track individually.

**Requirements:** Album-order portion of FR61; Next portion of FR58; album failure behavior of FR73; session continuity portions of FR56 and FR60; P-NFR4–6; applicable P-AR3–4 and P-AR10; P-UX-DR11, P-UX-DR13–14.

**Dependencies:** Stories 15.1–15.7 are marked done. Preparation date: 2026-09-18; inspected baseline: `b54751a0bcbb5bc06100b7d1f788e9264baa445b`. Respect those statuses while retaining the installed-seek evidence limitations recorded in 15.7.

**Scope:** Explicit Play album in existing album grid/list surfaces, atomic ordered queue construction, daemon-owned natural advancement, shared UI/native Next, same-occurrence retry, and durable local outcomes. Reuse the working player and selected output. Prepared gapless boundaries (15.9), album gain (15.10), Preview (15.11), Playback destination/queue editor/floating redesign (15.12–14), Radio (15.15–20), reporting (15.21) and snapshots/exports (15.23–25) remain separate. No browser audio, new event loop, dependency upgrade, physical-device requirement, or automatic server scrobbling.

## Acceptance Criteria

1. **Start the whole ordered album.** Given an album with source-provided disc/track ordering, explicit Play album atomically replaces the main listening queue with its ordered occurrences and attempts the first occurrence. Each occurrence retains its portable source-server ID plus provider track ID independently of browsing. This is replacement, not Preview. An unplayable first track is retained and fails visibly under AC6 rather than being filtered out.
2. **Use deterministic fallback without losing entries.** Given multiple discs, missing/zero ordering fields, ties or repeated source tracks, honor valid numbers using the ordering contract below and provider ordinal as the final fallback. Never sort by title/ID, deduplicate source IDs, truncate the album, or collapse deliberate occurrences. Empty, oversized, invalid or unsuccessfully enumerated albums leave the existing queue intact with an actionable error.
3. **Advance exactly once at actual completion.** Given an active album track reaches natural completion after audio presentation drains, the daemon records its natural outcome and selects/starts exactly one successor through the common effect path, even with the UI closed. Late/duplicate completion, old progress and obsolete preparation cannot skip another track, overwrite metadata, or affect a replacement session.
4. **Make Next consistent and intentional.** Given a successor, UI Next and supported native Next use the same serialized transition, mark the departed occurrence explicitly skipped, reset the successor position to zero and preserve playing/paused intent. The queue order and occurrence identities remain intact. Local disposition does not submit a server play, dislike or preference.
5. **Retain a completed album.** Given the final occurrence finishes naturally, retain queue/current occurrence and actual terminal cursor, expose completed transport with no audio, and neither repeat nor start Radio. Next is unavailable without a successor; delivery despite a disabled control cannot mutate any queue, cursor or disposition.
6. **Stop at technical failure and retry explicitly.** Given initial load, source availability or unrecoverable decode failure, pause at that occurrence with a sanitized error and explicit Retry. Retain all remaining entries. Retry targets the same occurrence and committed cursor under the contract below; technical failure is distinct from natural completion or explicit skip/dislike. Explicit Next remains possible when a successor exists.
7. **Preserve continuity and safe restoration.** UI close/reopen preserves ongoing playback, current occurrence and the complete paged queue. Quit/relaunch restores paused at the last coherent committed occurrence/position without requiring the source online. Local outcomes survive restoration. Failed transactions retain the previous coherent state and expose failure instead of claiming successful advancement.
8. **Preserve usable browser and native interactions.** Play album, Next and Retry have localized accessible names, keyboard operation and visible focus. Current metadata/source badge and native Next capability reflect the authoritative occurrence and real successor availability. Grid/list navigation, identity-based multi-selection, basket/playlist actions, output selection and seeking continue to work without the later Playback destination.
9. **Verify ordered transport on supported platforms.** Deterministic and integration checks cover multi-disc/missing/tied metadata, repeated occurrences, duplicate/stale completion, paused Next, final completion, failed-track retry, offline restoration and UI/native parity on Windows, macOS and Linux. Record platform/architecture/backend and real outcomes. Source tests, native API calls and audible sequence checks are not physical-gaplessness or physical-media-key certification.

## Tasks / Subtasks

- [ ] Record the scoped contracts in the API/test documentation before changing implementation (AC: 1–9).
  - [ ] Adopt the album request, ordering/fallback, resolution bounds, transition table, persistence migration and explicit error contracts below.
  - [ ] Record EOF/Next/seek/Stop/output-loss race outcomes and the owner-to-audio effect bridge; do not leave automatic playback dependent on a UI poll.
- [ ] Add source-safe album actions and daemon resolution (AC: 1–2, 8).
  - [ ] Tag album browse results with portable source identity captured together with their provider; propagate through normal/artist/recent/favorite album mappings and synthesized favorite albums.
  - [ ] Add explicit Play album to existing album cards and rows using the item's captured identity; preserve navigation and multi-selection events.
  - [ ] Resolve full album through `get_provider_by_server_id` and `MediaProvider::get_album`, validate/order it off the owner thread, then commit once with current admission fences.
- [ ] Extend the owner and persistence for ordered transport (AC: 1–7).
  - [ ] Add bounded successor lookup, local occurrence outcomes, schema migration and atomic transition writes.
  - [ ] Add Next/Retry and authoritative successor capability; preserve existing queue-revision, command-ID, source and occurrence rules.
  - [ ] Consume matching presentation completion once, including the final occurrence; fence stale progress/events and reset per-track seek/metadata state.
- [ ] Deliver successor audio through the common daemon service (AC: 3–7).
  - [ ] Wire one bounded owner-effect bridge for automatic transitions to `PlaybackCommandService`; ensure effect execution occurs outside owner/database locks and without UI/native-registration dependency.
  - [ ] Reuse selected-output policy, cancellable provider preparation, pipeline retirement and generation/control-epoch activation checks.
  - [ ] Preserve silence on paused Next, output loss, superseded work and shutdown; report transition/preparation failures truthfully.
- [ ] Wire UI/native state and recovery (AC: 4–8).
  - [ ] Extend typed RPC and mounted `PlaybackControls` with Next, explicit Retry, correct current source and immediate stale-metadata clearing.
  - [ ] Add native Next intent/capability using the existing vendored platform support; retain Previous disabled and existing seek/receipt/teardown behavior.
  - [ ] Add English/French/Spanish/German strings and focused responsive styles; preserve output dropdown and scrub state handling.
- [ ] Validate and record evidence (AC: 1–9).
  - [ ] Add owner/persistence/provider/RPC/effect/native race and failure tests; exercise more than 200 album entries and a successor beyond the first snapshot page.
  - [ ] Add browser/component behavior tests for captured source identity, complete albums from favorite-only views, accessibility, selection preservation, retry, final Next and old responses. The current playback UI suite covers PlaybackControls only; add executable card/row action coverage instead of assuming it covers library actions.
  - [ ] Extend installed checklist/evidence collection with album occurrence sequence, transition causes and actual transport outcomes; run platform checks and leave unavailable rows explicitly unverified.

## Dev Notes

### Implementation contract: album resolution and ordering

These are preparation decisions for this story, not statements that the feature already exists.

**Wire entry:** Add strict `playback.playAlbum` with `{ schemaVersion: 1, instanceId, sessionId, commandId, expectedQueueRevision, expectedGenerationId, source: { serverId, albumId } }`. `serverId` is portable, never the local vault/cache ID. Validate nonempty IDs with the existing 1024-byte identity limit and UUID command identity; reject unknown fields. Return the current versioned session snapshot after queue admission; actual audio may still be loading. Add the method to authenticated RPC dispatch and `is_mutating_method`, and retain the lifecycle mutation guard until resolution/commit is settled.

**Admission and races:** Reserve one album-resolution operation in the serialized owner after checking identity/revision/generation/restoration/shutdown. Capture its control epoch internally. Resolve asynchronously outside the owner and server-manager lock. Revalidate the reservation, queue revision, generation and control epoch at commit; intervening Pause/Stop/seek/Next/output change/session replacement/Quit supersedes it rather than allowing late playback. Keep existing audio and queue untouched until successful commit. Source enumeration failure, cancellation, timeout, empty album and validation failure are pre-commit errors. After commit, first-track playback failure belongs to the new queue under AC6.

**Bounded work:** One album resolution per owner; distinct concurrent album requests return `PLAYBACK_BUSY` rather than spawning unbounded tasks. Same-command pending retries share the reservation or return a documented pending/busy result, never duplicate provider work. Use a named 60-second album-resolution deadline. Admit 1–10,000 occurrences per album, with explicit `ALBUM_EMPTY` / `ALBUM_TOO_LARGE` / `ALBUM_INVALID` errors; never truncate. The existing 200-source limit is a public insert-batch limit, not an album-size limit: use one dedicated atomic replacement, not a chain of Replace/Append RPCs. Keep ordinary append/replace limits unchanged. `get_album` currently materializes provider metadata as a Vec; the 10,000-entry guard bounds subsequent queue construction, not the provider HTTP response's peak memory. Retain existing HTTP limits/timeouts, measure metadata peak in the large-album fixture, and do not claim a new hard whole-response memory guarantee. Audio budgets remain separately bounded.

**Ordering:** Enumerate the provider Vec before sorting. Normalize positive disc numbers as valid; missing/zero disc uses effective disc 1. Within each effective disc, positive track numbers precede missing/zero track numbers; sort numbered tracks ascending. Original provider ordinal breaks all ties and orders unnumbered tracks. A concrete key is `(effective_disc, missing_track, positive_track_or_zero, original_ordinal)`. Negative provider numbers normalize to missing at adapters. This is the project's fallback policy, not a server-order guarantee. Keep every valid source occurrence, including identical track IDs appearing twice; mint unique occurrence UUIDs. Missing/unusable source IDs fail the album atomically rather than silently removing entries. Do not probe playability to filter the queue or skip ahead.

Reuse existing `Song.track_number` / `disc_number` and `AlbumWithTracks`. Jellyfin `get_album` delegates to `api.rs::get_child_items_with_sizes`, whose current request has no explicit sort; Subsonic maps `album.song` in response order. Normalize/order once in a playback helper (a small new `playback/album.rs` is appropriate). Do not reuse title-sorted sync expansion or change playlist/sync ordering as a side effect. Test complete album enumeration against each adapter; do not substitute a visible paginated list of tracks for the full album.

**Deduplication:** Include album, Next and Retry in the existing cross-operation command-ID namespace and payload-mismatch rejection. Album reservations and completed results must be bounded (at most 1024 completed results, at most 10 minutes, matching apply retention). Same-ID successful replay never remints occurrences or starts audio again; include authoritative current metadata when replaying an older result. Next/Retry retain the current control cache's bounded 1024-entry behavior; document it without promising indefinite deduplication. Refresh after conflict, never automatically replay a mutation against a newly selected occurrence.

### Implementation contract: transitions and effects

Add `next` and `retry` to `playback.control` using the existing strict instance/session/command/expected-generation/occurrence envelope. Native Next is an intent resolved against current owner state at execution, like native Toggle; UI requests remain occurrence/generation fenced. Both call the same internal transition. Successor lookup is indexed by `(session_id, ordinal)` with `ordinal > current.ordinal ORDER BY ordinal LIMIT 1`, not array indexing into a page or assuming contiguous ordinals. Project additive `canGoNext` from the owner; it remains true for a failed occurrence with a successor, but false during shutdown/restoration failure.

| Trigger | Current outcome | Successor / resulting state |
| --- | --- | --- |
| Play album commits | Old queue explicitly replaced; no invented skip/listen report | First ordered occurrence at 0, buffering/loading, subject to selected-output policy |
| Natural presentation completion with successor | `naturalCompletion` once for matching occurrence/generation | Successor at 0; continue if intent remains playing, otherwise remain paused |
| UI/native Next with successor | `explicitSkip`, including deliberate Next from a failed occurrence | Successor at 0; Playing/Buffering intent continues; Paused/Error remains paused; Stopped/Completed remains nonplaying with Stopped status |
| Natural presentation completion without successor | `naturalCompletion` once | Retain last current and final cursor; transport Paused + playback Completed; output closed |
| Next without successor | No mutation | UI/native capability false; strict RPC `NEXT_UNAVAILABLE`; delivered native command has no effect |
| Load/decode/source failure | `technicalFailure`, with sanitized reason | Same current/cursor, Paused + Error, no automatic skip/retry |
| Explicit Retry | Same occurrence; prior failure remains distinguishable from rejection | Fresh generation, resume same committed actual cursor through existing bounded reopen path, buffering/loading; output policy must permit it |
| Stop | No invented completion/skip | Existing stopped behavior: same current at 0, new generation, no auto-advance |
| Absolute seek exactly to duration | No natural-completion disposition | Preserve 15.7 terminal-cursor behavior; do not synthesize EOF/advance |

The last-track Paused/Completed representation deliberately reuses the current model; `TransportState::Idle` currently implies an empty persisted queue. Do not persist Idle with retained entries without changing and testing that invariant. Restart is always paused. Explicit Resume after completed final track replays that current occurrence from zero, preserving current behavior; replaying the album from its first track requires Play album. A Retry reuses occurrence identity; new Play album creates new occurrences.

Retry is available after source/decode failure once configuration/output permits another attempt. Reuse existing reopen/sequential-discard or qualified seek capabilities; do not promise an unsupported random-access resume. If recovery cannot reach the committed cursor within the existing deadline, remain paused with an actionable failure; never claim a successful cursor or silently restart at zero. Keep output-loss recovery labeled as deliberate Resume on selected output, without routing to default speakers. Failed Retry must remain same-occurrence and must not accumulate duplicate history or background retries.

**Natural completion proof:** Reuse the CPAL `PresentationClock` drain and Linux Pulse `PresentationLedger`/drain contracts. Decoder EOF, downloaded-byte count, provider duration and UI timeline reaching its maximum are not natural completion. Consume terminal events once per occurrence/generation; a terminal failure cannot later be converted into success by a delayed EOF. Late metadata/activity/progress from a terminal or replaced pipeline must not revive it. Final completion also needs a consumed-terminal fence even though there is no successor.

**Races:** A transition rotates generation, closes the old gate, retires old work, clears old metadata/duration/representation/seek capability/pending seek, and resets progress ingress to the new cursor. Preserve latest owner-ordered Pause intent during preparation. If EOF wins before a stale UI Next, reject Next by generation and refresh; never retarget it automatically. If Next wins, old EOF is ignored. Native requests deliberately resolve in owner order; separate native button presses may advance separately, but replay of one internal effect cannot. Output loss must prevent automatic playback on the successor until deliberate recovery, even at an EOF boundary. Reconcile captured output-loss signals before dispatch and test both event orders. Shutdown has priority over pending effects.

**Seek interaction:** Existing absolute UI seeks, including exactly-duration, preserve occurrence and do not record a natural listen. MPRIS relative Seek beyond the end now has a successor available: after validating lifecycle admission and the existing qualified seek capability, normalize a positive target strictly greater than duration to the common Next path when one exists (explicit navigation/skip, current paused intent preserved), as required by its protocol. Equality remains exact-end seek; unsupported seeking must not bypass its capability gate by becoming Next. Without a successor retain documented terminal clamp behavior. Update the existing unconditional relative clamp test; negative overshoot still clamps to zero. Keep strict absolute RPC validation and committed-only native discontinuity publication.

**Actual audio effects:** `PlaybackCommandService::dispatch_effect` currently runs only after RPC/native control/seek responses. Completed events go straight to `PlaybackSession`, so changing the current row alone produces no audio. Add a single daemon-lifetime bounded owner-effect bridge installed at startup independently of UI and native registration. The owner emits a transition effect only after its transaction commits; consume outside owner/DB locks through the shared service's source-resolution/output path. Use a bounded latest-generation slot or channel with explicit supersession/acknowledgment: no unbounded task-per-event queue, no silent loss of the one current effect on saturation, and no duplicate start from both RPC return and bridge. Fence by instance/session/occurrence/generation/control epoch at dequeue, after provider resolution and before output activation. Stop/join this bridge during existing shutdown. Keep the queue transition independent of output reopening so 15.9 can later prepare boundaries without replacing queue ownership.

### Persistence and state consistency

Extend playback persistence from version 1 to version 2 transactionally in `playback/persistence.rs` (the `Database` implementation already owns playback DDL there). Add minimal occurrence disposition fields: `pending`, `naturalCompletion`, `explicitSkip`, `technicalFailure`, plus a nullable sanitized failure code. Store no credentials, URLs, decoder objects, or metadata blobs. Existing v1 entries migrate to `pending`; do not infer listening from old ordinals or cursors. Unknown versions and invalid values remain recoverable errors preserving evidence. Keep wire schema version 1 for additive projections and version the SQLite schema independently.

Disposition describes the latest attempt at that occurrence, not a reporting ledger. Retry can reset `technicalFailure` to `pending` while retaining the last failure code until successful recovery; explicit replay can reset a completed current occurrence to pending. Do not add unbounded attempt history, Radio exclusions, server reporting or export tables. An absolute exact-end seek leaves disposition unchanged, preserving the distinction from genuine completion. Later reporting must measure listening separately; `naturalCompletion` after seeking alone does not prove that skipped-over audio was heard.

For advancement, commit departed disposition, successor current ID, position 0, checkpoint sequence and durable transport together in one transaction. For final completion/failure, commit terminal cursor/outcome together. Queue order/membership unchanged means `queueRevision` unchanged; current/outcome changes increment `stateSequence`. Play album replacement increments `queueRevision` exactly once. Position/progress does not invalidate queue edits. Retain paged 100-default/200-max reads and indexed successor lookups; do not load the whole queue on every tick/Next.

Do fallible validation/response preparation before commit where possible; after a committed transition do not report an unrelated projection failure as if the transaction never happened. On transaction failure, close audio safely, retain the old coherent occurrence/queue, publish persistence error, and do not dispatch successor audio. Retain one bounded pending terminal transition for explicit Retry after storage recovers, so final EOF is neither silently lost nor processed twice. In this persistence-error case, Retry attempts that exact frozen transaction once, then leaves its resulting occurrence paused (or final Completed); subsequent Resume explicitly starts audio. Do not reinterpret persistence Retry as a source retry or mark it skipped. Bind recovery to the retained instance/session/occurrence/generation and validate it again inside the owner. A later replacement/clear/Stop or other generation invalidation discards the pending transition; Next/seek must reject while it remains unresolved. Stop still closes output immediately even when its checkpoint fails. Recovery must never overwrite a superseding queue or reopen ended audio. A crash restores the last commit; process-local pending work never replays automatically. Ordinary 5-second position checkpoints and final shutdown checkpoint remain in place outside callbacks.

### Existing implementation: changes and preservation

| File / area | Current behavior | Required change and preservation |
| --- | --- | --- |
| `hifimule-daemon/src/playback/model.rs` | Strict v1 envelope; source/occurrence IDs; Pause/Resume/Stop; paged snapshots; seek outcomes | Add album request, Next/Retry, disposition and authoritative successor projection. Keep decimal-string counters and integer ms, private effect flags off the wire. |
| `playback/session.rs` | One 64-command owner; bounded dedup; generation/epoch gates; Completed pauses same track; error pauses | Add album admission/commit and shared transition ownership. Preserve progress/reset, duplicate-effect suppression, shutdown guard lifetime and independent metadata/seek event slots. |
| `playback/persistence.rs` | v1 DDL, paged reads, atomic structure writes; checkpoint updates only position/sequence | Add v1→v2 migration, successor query and atomic transition/disposition write; retain corruption/rollback/orphan validation. Do not rely on position-only checkpointing to save a changed current ID. |
| `playback/commands.rs`, `commands_tests.rs`, `mod.rs` | Shared cancellable transport effects; 60-second preparation; PlayTrack path also exists in RPC | Add album resolution and owner-effect bridge without duplicating mutation authority or starting effects twice. Preserve provider barriers, source identity, output admission and cancellation. |
| `hifimule-daemon/src/main.rs` | Daemon startup, shared service/native lifecycle, safe shutdown | Install and retire the owner-effect bridge with existing lifecycle. Preserve single instance, sync safety, tray and native registration; no second loop. |
| `playback/audio.rs`, `audio/pulse_output.rs` | Completion after presentation drain; selected output, bounded worker retirement | Reuse as effect targets. Change only event/effect integration needed; never move completion to decoder EOF or bypass selected endpoint. No gapless DSP/prefetch work here. |
| `playback/native.rs` | Coalesced snapshot publication, correlated receipts; Next rejected/mask absent | Add Next mask/intent from owner `canGoNext`; preserve native Stopped projection for Next from stopped/terminal state. Preserve Previous disabled, occurrence MPRIS ID, seek acknowledgment, 64-request ingress and registration/teardown. |
| `hifimule-daemon/src/rpc.rs` | Auth/lifecycle admission; track-only browse source tagging; playback dispatch | Add album method to mutation classifier/dispatch and safe album tagging. Preserve provider-aware credentials fix and existing browse response fields. |
| `domain/models.rs`, `providers/{mod,jellyfin,subsonic}.rs`, `api.rs` | Song already has optional disc/track; adapters return album tracks in provider Vec order | Reuse types/trait. Add normalization/completeness tests; change adapters only if a tested completeness defect requires it. No source-specific HTTP outside provider boundary. |
| `hifimule-ui/src/rpc.ts` | Typed playback helpers; `BrowseAlbum` lacks portable identity | Add album provenance and request/control projections; preserve strict response/identity handling and Tauri proxy. |
| `hifimule-ui/src/library.ts`, `components/MediaCard.ts` | Album cards/rows browse/add; Play currently Audio-only; several album mappers omit source | Add Play album consistently and preserve captured source through normal and favorite mappings, including synthesized albums. Stop action propagation; retain row/card navigation, keyboard and selection. |
| `hifimule-ui/src/components/PlaybackControls.ts` | Mounted transport/output nodes; 500 ms polling; scrub fencing; bounded 750 ms interpolation | Add Next/Retry/source display without remounting focus. Reset old scrub/anchors when occurrence changes, refresh conflict without replay and show failures immediately. Paused successor must not display prior metadata. |
| `hifimule-ui/src/styles.css`, `hifimule-i18n/catalog.json` | Responsive controls and EN/FR/ES/DE catalog | Add only required labels/focus/layout. Preserve hoisted bottom-end output dropdown and existing visual tokens. |
| `scripts/tests/playback-ui.test.mjs`, `docs/api-contracts-hifimule-daemon.md`, `docs/playback-installed-test-checklist.md` | Behavioral fake-DOM tests and scoped evidence contracts | Add album/Next/Retry behavior and state assertions; avoid implementation-string-only tests. |
| `scripts/playback-installed-evidence.py`, `scripts/tests/test_playback_installed_evidence.py` | Sanitized collector, versioned seek/output/native evidence | Add distinct album evidence version/mode; old single-track/seek records cannot satisfy album acceptance. |

RPC album-tagging locations: `handle_browse_get_artist`, `handle_browse_list_albums`, `handle_browse_get_album`, `handle_browse_list_recently_added`, `handle_browse_list_favorite_items`. Capture the provider and portable ID together; do not tag after awaiting with whichever server is currently selected. Reuse the existing track-tagging pattern. Preserve source when constructing favorite album nodes from tagged tracks.

Album actions use the real provider album ID, never a synthetic `favorites:album:…` basket ID. A favorite album view may display only favorite tracks; Play album still resolves the complete source album. Disable an action whose source provenance is absent rather than guessing the selected server.

Paused Next can select immediately without downloading/decoding successor audio. Populate safe metadata from the resolved album or bounded generation-fenced metadata resolution; otherwise show an honest current-source/track fallback while loading metadata, never the previous title. Native metadata follows the current occurrence too. Source labels must use the queued portable ID mapped to configured server identity, not selected browser context. Preserve no-device playback while leaving physical basket/sync locks intact.

### Architecture, libraries and project structure

Keep playback in the existing daemon modules; a small `playback/album.rs` helper is the only new module suggested. Reuse SQLite, provider abstractions, `ServerManager`, authenticated Tauri RPC, vanilla TypeScript, Shoelace and current native runtime. No `PlaybackQueue.ts`/Radio/reporting scaffolding is required to deliver this story.

Repository pins supersede exploratory architecture examples: Rust edition 2024/minimum 1.93.0; CPAL `=0.18.2`; `ffmpeg-next`/`ffmpeg-sys-next` `=9.0.0`; controlled FFmpeg `9.0.1`; `crossbeam-queue =0.3.12`; locally patched Souvlaki `=0.8.3`; Linux `libpulse-binding =2.30.1`. No package changes are needed. Vendored Souvlaki already supports Next/dynamic capability on Windows SMTC, macOS remote commands and default MPRIS D-Bus. Do not replace that patch to enable Next.

Retain `audio-runtime.json` budgets: aggregate compressed 8 MiB (including 1 MiB network chunk capacity and remaining read window), PCM target 500 ms capped at 1 MiB, startup/refill 100 ms and preparation deadline 60 seconds. These are audio budgets, separate from album metadata/SQLite pages. Do not instantiate one decoder per album entry. Callbacks perform no allocations, provider/DB IO or acquisition of sync-held locks. Selected-output loss pauses with no automatic rerouting; audio and existing managed-device sync remain independent.

`project-context.md` supplies provider/managed-zone principles but its greenfield label is historical. The January UX document predates playback: approved playback amendments override no-device locking and discretionary entity ordering. Use current components/tokens, not old mockup styles. Current portable/local server identity rules override early single-provider examples.

### Previous-story and git intelligence

- Story 15.7 review fixed backward-seek ingress reset, post-seek pipeline errors, pending seek supersession on output change, metadata/qualification coalescing, scrub reset and cross-occurrence queued seek, invisible RPC rejection, operation-based native discontinuity and no-op evidence acceptance. Preserve all of these through advancement; old-position monotonic guards must reset for every successor generation.
- Current 15.7 header/sprint and commit `b54751a` mark it done. Older paragraphs still say in-progress and installed numeric platform qualification remains incomplete. Respect done while carrying the evidence limitation; do not turn qualitative field reports into certified installed rows or reopen 15.7 during this preparation.
- `9cd69fd` made device credentials provider-aware in `api.rs`/`rpc.rs`; avoid overwriting this with assumptions that every provider supplies Jellyfin tokens.
- `4c886ce` fixed the Linux Pulse seek call/build and CI. Any common audio signature change must compile both cfg-selected CPAL and Pulse paths.
- `5f51549` added Navidrome raw seek qualification; `45f3bdc` extended compressed Jellyfin seek/duration handling. Do not restore the earlier WAV-only contract or enable generic Subsonic seeking based on a suffix/range header. Album progression must work for ordinary nonseekable playback too.

### Testing requirements

| Layer | Required scenarios and assertions |
| --- | --- |
| Ordering/providers | Single/multi-disc, absent/zero/negative/tied numbering, original provider fallback, repeated identical IDs, all-unnumbered album, empty/invalid source, completeness for both adapters; exact expected occurrence sequence. |
| Album admission | One atomic replacement; 201 tracks and 10,000 boundary accepted, 10,001 rejected without truncation; provider failure/timeout/caller drop; same-ID replay and cross-operation reuse; stale queue/generation/epoch after Pause/Next/Stop/seek/replacement/Quit. Existing session unchanged on pre-commit rejection. |
| Owner/DB | One successor per terminal event, final terminal consumed once, indexed successor beyond first 100/200 entries, disposition transitions, no queue revision churn, atomic outcome/current/cursor/checkpoint write; v1 migration, invalid v2/future version, injected rollback and restart; failed final-completion write followed by storage Retry; recovery after Stop/replacement must reject and never revive old state. |
| Effects/races | Real shared service starts successor with UI absent and native registration unavailable; old EOF after Next, Next after EOF, failure then EOF, late progress/metadata after final completion, slow preparation then Pause/Stop/output loss/Quit; no obsolete audio and no duplicate effect. Bounded tasks and queue saturation recover explicitly. |
| Retry | First/middle/final load or decode failure, unavailable source, output lost, failure after successful seek, Retry success/failure at committed cursor; same occurrence and untouched remaining entries. Explicit Next after error marks skip and stays paused. |
| Seek/regression | Absolute exact-end does not become natural completion; qualified native relative overshoot uses Next when available; unqualified overshoot never mutates, and equality retains exact-end semantics; stale pending/queued seek cannot target successor; preserve backward seek and current format gates. |
| Native/UI | Next capability from owner, no successor no-op, paused Next silence, API parity, source A album action after browse B, album cards and virtual rows preserve selection/basket/playlist behavior, source badge correct, Retry accessible, metadata not stale, focus/timers/dropdown intact and four-locale parity. |
| Installed | Ordered short distinguishable tracks from configured Jellyfin and Subsonic/Navidrome, natural sequence with UI closed, native/API Next and paused Next, induced track failure and retry, final completion, Quit/restart offline queue retention on Windows/macOS/Linux. Record OS/architecture/backend/runtime/provider versions and limitations. |

Use existing deterministic fixture generation/provenance under `hifimule-daemon/tests/fixtures/` and actual service seams in `commands_tests.rs`. Local disposition assertions must inspect SQLite after restart, not merely an in-memory event label. For terminal-race tests use barriers/injected events rather than unreliable sleeps. A successor's first PCM marker or instrumented audio-effect invocation must confirm audio startup; a changed queue cursor alone is insufficient. Paused Next must verify zero audio activation. Check unrelated physical-device selection/basket/playlist regressions explicitly.

Album evidence must identify session/generation/occurrence, portable source pseudonym, queue revision, before/after cursor/state, transition cause/disposition, duplicate terminal delivery count and actual successor audio outcome. Retain ordinal sequence/total count without exporting titles, credentials or URLs. Distinguish normal completion from Next/seekEnd/technical failure. Validator must reject empty/no-op alleged advancement, wrong order, dropped repeated occurrences, extra advance, missing failed occurrence, paused Next with audio, and unsupported or contradictory evidence versions. Record actual platform/architecture; a VM result does not certify another architecture, native API delivery does not certify physical keys, and ordered audible tracks do not prove gaplessness.

Suggested implementation checks (not executed during story preparation):

```sh
rtk proxy node scripts/build-daemon.mjs test -p hifimule-daemon playback -- --test-threads=1
rtk proxy node scripts/build-daemon.mjs test -p hifimule-daemon providers -- --test-threads=1
rtk proxy node scripts/build-daemon.mjs test -p hifimule-daemon rpc -- --test-threads=1
rtk proxy node --test scripts/tests/playback-ui.test.mjs
rtk proxy python3 -m unittest discover -s scripts/tests -p 'test_playback*evidence.py'
rtk cargo test -p hifimule-i18n
rtk npm --prefix hifimule-ui run build
rtk cargo fmt --all -- --check
rtk git diff --check
```

Run additional browse tests and lifecycle checks for changed entry points; compile applicable Windows/Linux/macOS target paths and record required installed evidence. Known unrelated repository formatting drift must be reported separately from formatting of touched files, not silently “fixed” across the repository. Do not reuse old test counts as evidence for this implementation.

### Current technical research

Checked 2026-09-18 against primary documentation; retain repository versions and validate existing APIs rather than upgrading.

- OpenSubsonic `getAlbum(id)` returns the album and song entries; `Child.track` and `discNumber` are optional. Therefore missing-order policy belongs explicitly in HifiMule. [getAlbum](https://opensubsonic.netlify.app/docs/endpoints/getalbum/), [Child fields](https://opensubsonic.netlify.app/docs/responses/child/).
- MPRIS Next must have no effect when `CanGoNext` is false and otherwise preserves Playing/Paused/Stopped status. Relative Seek beyond track length acts like Next if another track exists. Keep protocol normalization at native ingress and common owner semantics. [MPRIS Player specification](https://specifications.freedesktop.org/mpris/latest/Player_Interface.html).
- The version-specific CPAL 0.18.2 documentation confirms device-bound streams and fallible/disconnected-device configuration; keep the project's concrete selected output and cancellation checks. Search-index “latest” results can lag pins, so use versioned docs. [CPAL 0.18.2](https://docs.rs/cpal/0.18.2/cpal/). The binding source listing reports ffmpeg-next 9.0.0; no decoder API change is needed for queue advancement. [ffmpeg-next source](https://docs.rs/crate/ffmpeg-next/9.0.0/source/).

### References

- [Source: `_bmad-output/planning-artifacts/epics.md` — Story 15.8; Epic 15 requirements/UX and stories 15.1–15.29]
- [Source: `_bmad-output/planning-artifacts/prd.md` — Desktop Playback amendment, FR56/58/60/61/73 and playback NFRs]
- [Source: `_bmad-output/planning-artifacts/architecture.md` — Playback State and Ownership; Audio Pipeline; Implementation Contracts; Validation Refinements; Project Structure; portable server identity]
- [Source: `_bmad-output/planning-artifacts/ux-design-specification.md` — responsive/accessibility/selection foundation, superseded where playback amendments differ]
- [Source: `_bmad-output/planning-artifacts/project-context.md`; `playback-epic-validation.md`; `playback-story-review.md` — durable principles, approved scope and story-specific readiness gates]
- [Source: `_bmad-output/implementation-artifacts/15-7-seek-within-a-track-and-see-the-actual-playback-position.md` — review fixes, current extension notes and installed evidence limitations]
- [Source: `hifimule-daemon/src/playback/{model,session,persistence,commands,audio,native}.rs`; `playback/audio/pulse_output.rs` — inspected current contracts and missing advancement effect]
- [Source: `hifimule-daemon/src/domain/models.rs`; `providers/{mod,jellyfin,subsonic}.rs`; `api.rs`; `rpc.rs` — album metadata, routing and browse provenance]
- [Source: `hifimule-ui/src/{rpc,library}.ts`; `components/{MediaCard,PlaybackControls}.ts` — existing action/control seams]
- [Source: `Cargo.toml`; `hifimule-daemon/audio-runtime.json`; `third_party/souvlaki` — runtime pins and existing Next support]
- [Source: `docs/api-contracts-hifimule-daemon.md`; `docs/playback-installed-test-checklist.md`; `scripts/tests/playback-ui.test.mjs` — implementation documentation and validation conventions]

## Dev Agent Record

### Agent Model Used

Story preparation: Codex (GPT-6), with parallel planning, provider/UI and transport/native research.

### Debug Log References

- Preparation inspected baseline `b54751a` and five recent commits; initial working tree clean.
- Reviewed planning sources, preceding story, current production seams and primary technical documentation.
- Independent checklist reviews checked all nine acceptance groups and transport contracts. Applied fixes for stopped Next, persistence-error recovery, and seek capability checks before native overshoot navigation. No implementation, audio test or installed acceptance run is claimed by preparation.

### Completion Notes List

- Ultimate context engine analysis completed - comprehensive developer guide created.
- Story-specific ordering, admission, advancement, retry, terminal persistence, native capability and test contracts defined for implementation.
- Checklist validation complete; sprint status updated to ready-for-dev. Installed acceptance evidence remains implementation work.

### File List

- `_bmad-output/implementation-artifacts/15-8-play-an-album-in-order-and-advance-through-its-tracks.md`
- `_bmad-output/implementation-artifacts/sprint-status.yaml`
