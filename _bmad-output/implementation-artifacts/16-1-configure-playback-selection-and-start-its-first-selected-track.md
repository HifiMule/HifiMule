# Story 16.1: Configure Playback selection and start its first selected track

Status: ready-for-dev

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

- [ ] Define the implementation contract before editing behavior (AC: 2, 3, 5, 7, 8).
  - [ ] Record pure selector input/output, portable source identity, per-consumer eligibility, deterministic seed, defaults, invalid-setting handling, and candidate/page/cache bounds.
  - [ ] Specify a versioned Playback-only settings schema and wire shape; preserve existing output preference and migration behavior.
- [ ] Extend the shared selection boundary for Playback (AC: 3, 5, 7, 8).
  - [ ] Carry source server identity through candidate and result types without altering the ordering or sync path for existing inputs.
  - [ ] Reuse `run_pipeline` and provider candidate retrieval; adapt device-shaped budget/history inputs into Playback-owned eligibility and track-count limits.
  - [ ] Route each source through its own provider, with bounded retrieval and no dependency on currently browsed server.
- [ ] Add Playback settings persistence and RPCs (AC: 1, 2, 6, 8).
  - [ ] Validate servers, capabilities, input references, bounds and schema version at the daemon boundary; return distinguishable setup, source and empty errors.
  - [ ] Make saved settings available in authoritative reconnect state without touching `autoFill.setPipeline`, device manifests or sync defaults.
- [ ] Add settings UI and explicit first-track action (AC: 1, 4, 6, 7, 9).
  - [ ] Reuse appropriate selection controls and localization patterns, omitting device-only fields.
  - [ ] Fence/cancel pending requests and only commit a prepared result through the main-session Play command. Preserve current session on every failure.
  - [ ] Provide keyboard operation, visible focus, accessible names and status/error feedback at responsive sizes.
- [ ] Verify behavior (AC: 2–9).
  - [ ] Test stable ordering equivalence, identity collisions, settings isolation/round trip, stale completion, cancellation, empty/unavailable sources, current-session preservation and simultaneous sync selection.
  - [ ] Run relevant Rust/UI tests and cross-platform checks; document any platform not actually verified.

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

### Agent Model Used

GPT-6 Codex (story preparation)

### Debug Log References

### Completion Notes List

- Comprehensive story context prepared; implementation remains pending.

### File List

- `_bmad-output/implementation-artifacts/16-1-configure-playback-selection-and-start-its-first-selected-track.md`
