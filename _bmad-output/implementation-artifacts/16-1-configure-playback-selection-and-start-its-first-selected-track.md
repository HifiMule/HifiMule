---
baseline_commit: 30209880f64c5382cbd10537e27c30a20d0fe2cb
---
# Story 16.1: Configure Playback selection and start its first selected track

Status: review

## Story

As a HifiMule user,
I want independent Playback source and selection settings using HifiMule's existing selection engine,
so that I can start listening from my curated music without choosing the first track manually or changing device sync preferences.

## Acceptance Criteria

1. Playback selection settings offer participating servers and supported playlist, artist and genre inputs through suitable existing controls. Unsupported capabilities are disabled with an explanation. No physical capacity, folder layout or device transcoding settings appear.
2. Validated settings survive UI reconnect and application restart in local Playback configuration. Saving never changes device, server or portable manifest preferences and never implicitly copies device defaults.
3. Given equivalent candidates, ordering and explicit seed, sync and Playback call the same pure selector and produce equivalent order before consumer constraints. Existing sync results remain unchanged for existing inputs; Playback needs no mounted device or capacity.
4. An explicit settings-view start action prepares the first eligible result and starts it through the existing main-session Play path, preserving its source identity. Saving alone never replaces the session; this action is not an audition.
5. Candidates from several servers, including identical provider-local track IDs, preserve portable server ID plus track ID across normalization, selection and playback routing. Titles or equal local IDs do not imply recording identity.
6. Missing sources, unavailable required sources and empty results show distinct actionable explanations. A failed preparation preserves the current main session and is not recorded as a skip or taste signal.
7. A newer start request or cancellation fences off older results before they can publish session state or audio. Retrieval and cache work have explicit independent bounds; no library-sized queue is constructed.
8. Concurrent sync and Playback selection do not share mutable config, random state or selection memory. Playback adds no durable taste profile.
9. Windows, macOS and Linux verification covers settings round trip, selector equivalence, source ID collisions, empty/unavailable results, concurrent requests, keyboard controls, accessible names and status feedback, and existing sync behavior.

## Tasks / Subtasks

- [x] Define the implementation contract before editing behavior (AC: 2, 3, 5, 7, 8).
  - [x] Record pure selector input/output, portable source identity, per-consumer eligibility, deterministic seed, defaults, invalid-setting handling, and candidate/page/cache bounds.
  - [x] Specify a versioned Playback-only settings schema and wire shape; preserve existing output preference and migration behavior.
- [x] Extend the shared selection boundary for Playback (AC: 3, 5, 7, 8).
  - [x] Carry source server identity through candidate and result types without altering the ordering or sync path for existing inputs.
  - [x] Reuse `run_pipeline` and provider candidate retrieval; adapt device-shaped budget/history inputs into Playback-owned eligibility and track-count limits.
  - [x] Route each source through its own provider, with bounded retrieval and no dependency on currently browsed server.
- [x] Add Playback settings persistence and RPCs (AC: 1, 2, 6, 8).
  - [x] Validate servers, capabilities, input references, bounds and schema version at the daemon boundary; return distinguishable setup, source and empty errors.
  - [x] Make saved settings available in authoritative reconnect state without touching `autoFill.setPipeline`, device manifests or sync defaults.
- [x] Add settings UI and explicit first-track action (AC: 1, 4, 6, 7, 9).
  - [x] Reuse appropriate selection controls and localization patterns, omitting device-only fields.
  - [x] Fence/cancel pending requests and only commit a prepared result through the main-session Play command. Preserve current session on every failure.
  - [x] Provide keyboard operation, visible focus, accessible names and status/error feedback at responsive sizes.
- [x] Verify behavior (AC: 2–9).
  - [x] Test stable ordering equivalence, identity collisions, settings isolation/round trip, stale completion, cancellation, empty/unavailable sources, current-session preservation and simultaneous sync selection.
  - [x] Run relevant Rust/UI tests and cross-platform checks; document any platform not actually verified.

## Dev Notes

### Implementation contract and guardrails

- This is a finite first-result start. Continuous replenishment, artist transitions, logical-session exclusions and final bar/menu **Play something** belong to Stories 16.2–16.6. Do not call this Radio in the UI.
- The shared engine is currently sync/device shaped: `AutoFillPipeline`, `SourceEntry`, `Candidate`, `SourceKey`, `PipelineInput` and `run_pipeline` are in `hifimule-daemon/src/auto_fill/pipeline.rs`; candidate materialization is in `auto_fill/fetch.rs`. `Candidate`/`SourceKey` and `AutoFillItem` currently lack a server dimension. Add a deliberate typed boundary, then retain that identity all the way into Playback's source routing. Avoid changing existing sync order or interpreting raw IDs across servers.
- Sync calls `expand_auto_fill_slot` from `rpc.rs`, uses manifest pipeline settings, and exposes `autoFill.setPipeline`. These remain device-facing. Existing UI `AutoFillPanel.ts` and `state/autoFill.ts` offer control/type patterns, but Playback owns its own state and settings surface.
- `playback/config.rs` already stores a versioned local `PlaybackConfig` with output selection. Extend it compatibly or use an explicitly separate Playback config; retain output preference, defaults, and migration. `playback/session.rs` owns the authoritative main session. `rpc.rs` has `playback.applySession` and album/playlist wrappers; reuse their admission and generation protections rather than directly swapping audio state. `PlaybackDestination.ts` and `state/playback.ts` are the UI integration points.
- The daemon alone resolves server credentials and streams. Keep `server_id` portable, rather than using UI selection or runtime provider IDs. Use snake_case in Rust/storage and camelCase on the JSON wire. Never put authenticated URLs or credentials into the UI.
- Treat source unavailability separately from an empty eligible set; show a safe error before mutating the current session. Settings save is independent of start. Cancellation/new request must invalidate any asynchronous retrieval and admission result.
- Define explicit track-count and candidate retrieval bounds for Playback, separate from sync byte/capacity limits. Selection must not load the entire library into a playback queue or hold an unbounded cache. Selection memory, if needed here, is per request and independent of device sync history.
- Use the existing `PipelineInput.seed` for equivalence tests. The current upstream `StdRng` documentation does not guarantee portability of sequences across versions/platforms; test against the repository's selected implementation and explicit fixtures, and do not silently upgrade random dependencies while implementing this story. [Rust rand documentation](https://docs.rs/rand/latest/rand/trait.SeedableRng.html). Serde supports explicit field defaults for backward-compatible config decoding; validate semantics after deserialization. [Serde field attributes](https://serde.rs/field-attrs.html).

### Project Structure Notes

Likely updates: `hifimule-daemon/src/auto_fill/{pipeline,fetch,mod}.rs`, `hifimule-daemon/src/playback/{config,session,commands,model}.rs`, `hifimule-daemon/src/rpc.rs`, `hifimule-ui/src/components/PlaybackDestination.ts`, `hifimule-ui/src/state/playback.ts`, and UI RPC/i18n modules used by those components. Add a focused settings component only if the destination would otherwise become unwieldy. Inspect current implementations before edits; preserve output controls, queue, preview, album and sync behavior.

### Dependencies and Previous Work

- Story 15.17 is currently `in-progress` in sprint tracking. Verify its packaged manual-playback foundation before treating this story as release-ready.
- Story 15.17 owns packaging validation, not Radio. Stories 15.4, 15.8, 15.11, 15.13 and 15.14 established main Play, album/preview behavior, queue and floating controls; preserve their session semantics.
- There is no previous Story 16 file. Epic 16 context is the immediate continuity source.

### References

- `_bmad-output/planning-artifacts/epics.md` — Epic 16, Story 16.1 acceptance criteria and implementation gate.
- `_bmad-output/planning-artifacts/architecture.md` — Playback Radio Selection, Playback UI and Session Control, implementation patterns.
- `_bmad-output/implementation-artifacts/epic-16-context.md` — scope, sequencing, boundaries and release gate.
- `_bmad-output/planning-artifacts/prd.md` — Desktop Playback requirements and success measures.

## Dev Agent Record

### Implementation Plan

- Playback settings use a separate versioned local `playback-selection.json` file so output preference saves and migrations remain intact. The wire carries `schemaVersion: 1`, bounded source entries with portable `serverId`, source kind and optional `ref`, a constrained `AutoFillPipeline` selector, and an explicit `seed: u64`. Defaults are no sources, track unit, no device budget, no memory, and a fixed seed of zero. Invalid versions, IDs, references, unsupported stages, or limits are rejected at the daemon boundary; no device defaults are imported.
- The pure selector input is one bounded, merged candidate pool assembled from typed source pools. A request-scoped map carries each candidate's portable `(serverId, trackId)` identity through normalization and converts the ordered selector result back to those identities. Sync's existing single-server input retains its existing ordering and output. Playback supplies empty per-request history and its own track-count eligibility, without device capacity or durable taste state. The seed is passed to `PipelineInput.seed` unchanged.
- Playback retrieval is capped at 100 genre tracks per page, 4 genre pages, 32 artist albums, 400 candidates per source, 8 sources, 3,200 candidates across sources, and a 15-second fetch deadline plus a separate 15-second preparation deadline. Playlist and album provider APIs return a complete source response; Playback truncates their candidate cache to 400, while the request deadline bounds their retrieval time. The cache is request-scoped and dropped after selection; the Playback queue receives only one selected track. A request generation fences every asynchronous fetch and preparation before main-session admission. The Play command is the sole commit path. New requests and cancellation revoke older generations; failures preserve the main session.
- Before Play admission, Playback now verifies a supported representation and opens the actual authenticated media response within the preparation deadline. The response is handed to the existing audio start, so a successful preparation makes only one media request. The owner checks the selection ticket under the same gate used by new requests and cancellation immediately before committing Play; superseded queued commands leave the current session intact. HTTP/source failures before admission leave the session intact. Failures after Play admission remain ordinary playback pipeline failures reported through the main session.
- The settings component retains the active request when cancellation fails, allowing its eventual result to restore the controls. A successful cancellation invalidates the pending UI result. Native buttons and labeled selects provide keyboard operation; the narrow layout uses one column with no horizontal overflow at 375 px.

### Agent Model Used

GPT-6 Codex (story preparation and implementation)

### Debug Log References

- Windows: `scripts/build-daemon.mjs test -p hifimule-daemon` through the controlled native-runtime wrapper — 1,179 passed, 6 ignored, 5 Audiobookshelf contract tests passed. The first direct `cargo test` attempt could not find FFmpeg; the documented wrapper supplied it.
- Windows: focused Playback selector, RPC failure-preservation, removed/empty genre, and bounded Subsonic genre-page tests passed.
- Windows: TypeScript `tsc --noEmit`, Vite production build, 7 localization tests, `cargo fmt --check`, and ordinary daemon Clippy passed. `clippy -D warnings` is blocked by existing warnings elsewhere in the daemon; no new warning was reported in the touched modules.
- The Build workflow checks the story on Windows x64, Linux x64, macOS x64 and macOS arm64. Story 15.17 packaged playback verification remains in progress.
- macOS (2026-09-28): full daemon suite passed (1,193 unit test cases including 6 ignored, plus 5 Audiobookshelf contract tests), 253 Node script tests passed with 1 existing skip, TypeScript/Vite build passed, seven localization tests passed, `cargo fmt --check` passed, and ordinary daemon Clippy passed with existing warnings. Focused tests cover HTTP preparation failure and single-response handoff, owner-commit cancellation, and UI cancellation failure/success.
- Linux (2026-09-28): three Playback settings UI tests passed in the local Node 24 Bookworm container with a read-only workspace and networking disabled. The complete Node suite could not use the macOS-installed Rollup native package in that container; its Linux package was absent. `actionlint` accepted the four-platform Build workflow.
- Build CI run [36396989733](https://github.com/HifiMule/HifiMule/actions/runs/36396989733) for commit `b61a5c392784072c52e8da85ec06b4630bc97ab1` completed successfully on 2026-09-28. Playback evidence jobs passed on Windows x64, Linux x64, macOS x64 and macOS arm64, covering the updated patch. The local macOS rerun at that commit passed the daemon suite (1,193 unit test cases, 6 ignored, plus 5 Audiobookshelf contract tests), Node script suite (253 passed, 1 skipped), Vite production build and `cargo fmt --check`.
- macOS browser component fixture (2026-09-28): checked the real component at desktop and 375 px widths with two server sources; no horizontal overflow at 375 px. Keyboard Enter activated Start, showed visible focus, and announced the localized result in its live status. This was a temporary component fixture with mocked RPCs, not an installed-app playback test. Story 15.17 packaged playback verification remains in progress.

### Completion Notes List

- Comprehensive story context prepared before implementation.
- Added an isolated versioned Playback selection file and RPCs for saved settings, server-scoped options, start and cancel. Output preference, device manifests, sync defaults and taste history use their existing storage.
- Added a typed portable `(serverId, trackId)` boundary around the shared pure selector. Playback combines at most eight source pools, preserves colliding provider-local IDs, and admits only one resolved track through the main-session Play path.
- Added Playback destination controls, status feedback, focus styles and English/French/Spanish/German strings. Added Windows-focused regressions and cross-platform CI commands.
- Added pre-admission HTTP/representation preparation and owner-commit fencing. A failed cancellation no longer strands the settings UI in its busy state. Narrow controls, numbered Remove names, and forced-color focus styles were verified in the component browser fixture.
- Cross-platform Build CI passed for the updated patch on Windows x64, Linux x64, macOS x64 and macOS arm64. Story 16.1 is ready for review; Story 15.17 still owns packaged-app playback verification.

### File List

- `_bmad-output/implementation-artifacts/16-1-configure-playback-selection-and-start-its-first-selected-track.md`
- `_bmad-output/implementation-artifacts/sprint-status.yaml`
- `.github/workflows/build.yml`
- `hifimule-daemon/src/playback/mod.rs`
- `hifimule-daemon/src/playback/audio.rs`
- `hifimule-daemon/src/playback/selection.rs`
- `hifimule-daemon/src/playback/session.rs`
- `hifimule-daemon/src/providers/mod.rs`
- `hifimule-daemon/src/providers/subsonic.rs`
- `hifimule-daemon/src/rpc.rs`
- `hifimule-daemon/src/rpc/playback_selection.rs`
- `hifimule-i18n/catalog.json`
- `hifimule-ui/src/components/PlaybackDestination.ts`
- `hifimule-ui/src/components/PlaybackSelectionSettings.ts`
- `hifimule-ui/src/rpc.ts`
- `hifimule-ui/src/styles.css`
- `scripts/tests/playback-selection-ui.test.mjs`

### Change Log

- 2026-09-27: Added Playback-owned finite selection, settings persistence, source-scoped RPCs, explicit start/cancel, localized UI and platform CI verification commands. Windows tests and builds pass; story remains in progress pending the recorded gates.
- 2026-09-28: Preflighted the audio response before Play, fenced selection at owner commit, corrected failed-cancellation UI recovery, and checked keyboard/narrow layout. Added the new regressions to the four-platform CI matrix. Updated-patch CI passed on all four targets; advanced the story to review.
