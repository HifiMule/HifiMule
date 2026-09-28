---
baseline_commit: 0421475fefc7f92576777256757c8fd8b0097460
---
# Story 16.4: Avoid duplicate Radio recordings across configured servers

Status: review

## Story

As a HifiMule user,
I want Radio to recognize confident copies of the same recording across my servers,
so that duplicate library copies do not repeat unnecessarily while distinct performances remain available.

## Acceptance Criteria

1. **Confident copies count once.** Given eligible candidates from configured servers with accepted same-recording evidence, automatic Radio treats them as one recording for cycle heard eligibility and upcoming selection. A second source copy cannot consume another automatic slot. Source-local track IDs alone never establish recording identity.
2. **Uncertainty stays separate.** Matching artist/title, close duration, shared album/work/release identifiers, absent or malformed recording metadata, and conflicting evidence do not create a confident match. Live, cover, remix, edit and other materially distinct performances remain eligible separately. The resolver's positive and veto evidence are explicit and testable.
3. **One chosen source per automatic occurrence.** Among eligible copies, select a source by a deterministic availability and verified-quality ranking with stable ties. Preserve its portable `(server_id, track_id)` in the occurrence; keep recording ID, occurrence ID and source ID separate. No silent mid-track source swap follows from deduplication.
4. **Correct state lifetime.** A heard Radio recording, including the initial current track, blocks confident copies for the current cycle. Explicit skip of a Radio track or removal of an automatic suggestion excludes confident copies for the logical Radio session, including after restart and cycle renewal. A new Radio resets those session exclusions. Technical failures do not become skips or exclude every copy.
5. **Manual intent survives.** Deliberately repeated manual occurrences retain their independent IDs, positions and source references. Neither matching nor reconciliation collapses or reorders them, and accepted queue entries keep their selected source.
6. **Conservative reconciliation.** Changed, missing or conflicting metadata never rewrites an existing occurrence source, played history or exclusion silently. Persist and version enough evidence/association state to restore decisions safely; when a formerly confident group becomes uncertain, prefer separate future eligibility while retaining explicit historical exclusions and a recoverable state.
7. **Partial provider availability.** An unavailable source or one without recording metadata does not block eligible available sources. Unknown availability is not proof of global exhaustion. Missing evidence never becomes a confident match, and preparation failure may try another eligible copy under existing bounded failure rules.
8. **Source routing survives browse and restart.** Queued and restored occurrences retain their selected source independently of the browsed server. Recording grouping does not alter playback credentials, stream routing, later reporting or export source ownership.
9. **Bounded and private.** Matching works on the existing bounded candidate windows and indexed durable state. No full-library audio fingerprint downloads, third-party metadata lookup, unbounded in-memory identity history or new authenticated data in UI/logs.
10. **Verification and honest evidence.** Deterministic multi-server fixtures cover confident matches, colliding local IDs, uncertain and distinct performances, competing copies, manual repeats, cross-copy heard/skip/removal, changed/conflicting metadata, unavailable sources, stale refill races, migration and restart. Run the affected Rust/UI and four-platform source-build matrix; report actual real-library/provider metadata coverage and installed scenarios separately from synthetic correctness.

## Tasks / Subtasks

- [x] Define the recording contract before modifying selection (AC: 1–3, 6–7).
  - [x] Inventory actual recording-level fields in supported Jellyfin and OpenSubsonic responses and the configured live servers. Document absent/malformed/multiple/conflicting values and version/provider coverage. Audiobookshelf or any source without verified music recording evidence stays unmatched. Check whether Jellyfin list/search requests include the needed provider fields before relying on them.
  - [x] Implement a typed, provenance-bearing recording-evidence value behind `MediaProvider` normalization. As the conservative baseline, accept an unambiguous, valid MusicBrainz **recording** UUID only; do not confuse it with release, release-group, work, artist or MusicBrainz track IDs. An ISRC-only association may be added only with separately documented corroboration and conflict tests; it must never be treated as automatically conclusive. Reject contradictory recording IDs and explicit performance/version conflicts rather than unioning groups transitively. Never infer identity from title/artist/duration alone.
  - [x] Define a stable, versioned recording key and a source-copy ranking: confirmed playable/available first, then verified sustainable representation quality where comparable, then original configured source order, then `(server_id, track_id)`; unknown quality ties by configured order. Do not equate nominal bitrate across incompatible codecs or prepare every copy merely to rank it. Document how a failed first copy can fall back without treating the recording as rejected.
- [x] Extend bounded Radio selection and owner admission (AC: 1–5, 7–9).
  - [x] Carry evidence through `Song`/provider DTOs into Playback candidate pools without exposing private provider metadata or credentials on the public wire. Group only confident copies in bounded pages, including copies encountered in later windows; keep the shared `auto_fill::pipeline::run_pipeline` ordering and Playback's original settings snapshot. Preserve same-artist priority and explainable 16.3 transitions.
  - [x] Apply recording-level heard, excluded and pending-automatic checks in worker filtering, owner recheck and the **same SQLite transaction** that admits new automatic occurrences. Keep source-qualified checks too. Refills must remain fenced by logical session, generation, revision, refill identity, center and cycle; duplicate/stale completion cannot append a second copy.
  - [x] Preserve five automatic upcoming slots, below-two trigger, eight configured sources, 400 candidates/source/window, 15-second retrieval/preparation deadlines and five preparation failures unless measured evidence justifies a tested change. Maintain cursor advancement through partial pages; do not declare exhaustion from a short page or failed source.
  - [x] Keep manual insert/reorder/remove, Preview, Back/replay, Pause/Stop, album continuity, output-loss inhibition, native controls and sync isolated from automatic deduplication. A deliberate manual repeat does not acquire an automatic-origin exclusion merely by sharing a recording key.
- [x] Persist and reconcile identity safely (AC: 4–6, 8).
  - [x] Migrate Playback SQLite schema 9 transactionally to a new version. Preserve the public playback snapshot schema unless a necessary wire field is added and all clients are updated. Store occurrence source unchanged, a versioned resolved recording association/evidence for accepted automatic occurrences, and indexed recording-level heard/excluded membership with cycle/session lifetime. Bound reads and keep paged history.
  - [x] Define migration from source-qualified 16.2/16.3 membership: retain old rows as authoritative; enrich only when fresh, non-conflicting evidence proves an association. Never retroactively merge old exclusions by title or discard them. For older paused sessions with no trustworthy evidence, continue safely with source-qualified state until evidence can be resolved; unknown identity version fails recoverably.
  - [x] On metadata changes, leave historical occurrences and source-specific effects immutable. Define whether the old confident key remains a tombstone for an accepted exclusion and how future candidates are re-evaluated; do not let key churn silently resurrect a skipped copy or erase an exclusion. Cycle renewal clears only heard membership; logical-session exclusions remain. New Radio uses fresh evidence and a new exclusion scope.
- [x] Verify provider and integration behavior (AC: 1–10).
  - [x] Add adapter fixtures for exact recording ID, wrong MusicBrainz entity, malformed/multiple IDs, ISRC-only, absent fields and contradictory/live metadata. Verify server/version-specific response fields before enabling a provider path.
  - [x] Add resolver, bounded-window, owner, SQLite migration/restart and source-routing tests. Assert no accidental manual-queue collapse, no second automatic slot for a confident copy, no cross-copy exclusion after technical failure, and no stale commit after queue edit/new Radio/Preview.
  - [x] Run focused and full affected Rust tests, `cargo fmt --check`, Clippy, UI TypeScript/build, i18n parity if public status changes, and the four shipping-platform CI fixture/build jobs. Record real-library identity coverage separately; a fixture or source build does not prove installed playback.

## Dev Notes

### Current code and preservation map

| UPDATE boundary | Current behavior | Story 16.4 change and guard |
| --- | --- | --- |
| `hifimule-daemon/src/domain/models.rs` | `Song` carries source-local metadata, second-resolution duration and bitrate; private `ProviderItemMetadata` holds artist IDs and audiobook identity but no recording evidence. | Add narrow optional recording evidence with provenance. Keep existing JSON behavior and `#[serde(skip)]` privacy boundary unless a justified wire contract is explicitly versioned. |
| `hifimule-daemon/src/api.rs` | `JellyfinItem` has no `ProviderIds` field and is also round-tripped by item rename; optional fields use `skip_serializing_if` to avoid writing nulls back. | Add a read-only provider-ID field if needed; ensure rename does not send newly parsed IDs or nulls back. Verify exact Jellyfin key/entity semantics with fixtures. |
| `hifimule-daemon/src/providers/{jellyfin,subsonic}.rs` | Adapters normalize tracks but currently discard recording identifiers; OpenSubsonic `SongDto` has no `musicBrainzId`/`mediaType`/`isrc` fields, and Jellyfin mapping does not retain recording `ProviderIds`. | Parse only documented, verified fields from configured servers; distinguish MusicBrainz entity types, reject malformed/conflicting IDs and preserve existing source-local artist/stream behavior. Keep HTTP/authentication in adapters. |
| `hifimule-daemon/src/playback/{selection,radio}.rs` | Versioned Playback settings and a source-aware pure selector use bounded cursor windows; Radio policy owns center, cycle, status and transitions. | Add conservative recording grouping and deterministic copy choice without copying the selector or changing original-settings, artist-transition or cursor semantics. |
| `hifimule-daemon/src/rpc/playback_selection.rs` | A coalesced worker fetches source windows and filters source-qualified membership/active occurrences before selection. | Evaluate recording-level eligibility across configured sources and later windows, while preserving deadlines, lease checks and `unknown` availability. |
| `hifimule-daemon/src/playback/session.rs` | Single mutation owner rechecks source membership, queue revision and center/cycle before append. | Recheck recording groups at admission; keep accepted queue order, manual repetitions, preview and transport state. |
| `hifimule-daemon/src/playback/persistence.rs` | `PERSISTENCE_VERSION = 9`; `playback_radio_membership` keys `(session_id,server_id,track_id,kind)`; append transaction checks source membership and active occurrences; cycle renewal deletes only `heard`. | Add indexed recording identity/membership and migration. Enforce same-recording uniqueness at final transaction, atomic with automatic origin/queue append. Preserve old source rows, Album/Manual restore and paged attempts. |
| `hifimule-daemon/src/playback/model.rs` | Public `SCHEMA_VERSION = 1`; `Occurrence` has occurrence ID and `TrackSource`, no recording field. | Keep selected source authoritative. Add a public recording field only if needed for a user-visible contract; if added, version RPC/TS types and restore together. Private durable association is sufficient for this story. |

**Identity policy detail.** A MusicBrainz recording ID refers to recorded audio, not a composition or release; live and remix performances may be separate recordings. Validate UUID syntax, field meaning and per-track consistency. Treat conflicting recording IDs as uncertain even if title/artist/duration match. A single provider's stale or ambiguous tags must not cause transitive merges through another copy. Keep an explicit version on persisted resolver results so policy changes cannot silently reinterpret old exclusions. ISRC is optional provider data, not guaranteed across servers and not an automatic fallback. [Source: `_bmad-output/planning-artifacts/epics.md` §Story 16.4; `_bmad-output/planning-artifacts/architecture.md` §§Playback State and Ownership, Playback Radio Selection; [MusicBrainz Recording](https://musicbrainz.org/doc/Recording)]

**Provider API check, 2026-09-28.** OpenSubsonic's current `Child` schema lists optional `musicBrainzId`, `mediaType` (required when that ID is supported) and `isrc[]`; availability is per server and the field's entity must be verified. Jellyfin's `BaseItemDto` exposes `ProviderIds` as an optional map, but the API shape alone does not prove a populated recording ID. Use repository-locked libraries and actual adapter fixtures instead of upgrading dependencies for this story. [OpenSubsonic Child](https://opensubsonic.netlify.app/docs/responses/child/), [Jellyfin BaseItemDto](https://typescript-sdk.jellyfin.org/interfaces/generated-client.BaseItemDto.html)

**Previous story and Git intelligence.** 16.3 added an immutable original-settings snapshot, centerless fresh fallback, bounded relation scans, cycle renewal and durable transition provenance. Preserve its verified shared-credit fallback when optional Subsonic artist-info fails and its UI handling of a transition followed by a short refill. Recent commits are `fb762b6 Review 16.3`, `6722df2 CI evidence`, `2c32fb2 Dev 16.3`, `7bebcb6 Story 16.3`, `34695bf Review 16.2`. Four-platform deterministic CI passed for 16.3; installed or real-library end-to-end Radio was not established. [Source: `_bmad-output/implementation-artifacts/16-3-continue-radio-through-meaningful-artist-connections.md` §§Dev Agent Record, Review Findings; Git history]

### Project Structure Notes

Keep policy in `playback/radio.rs` or a small adjacent recording-identity module, provider parsing in `providers/`, owner mutation in `playback/session.rs`, durable state in `playback/persistence.rs`, and source-window orchestration in `rpc/playback_selection.rs`. Reuse `auto_fill::pipeline::run_pipeline` and existing `TrackSource`/`Occurrence` contracts. Rust/SQL use snake_case; JSON uses camelCase. No external metadata service, new sync setting, server playlist write or cross-session taste store belongs in this story. [Source: `_bmad-output/planning-artifacts/architecture.md` §§Playback Provider Integration, Playback Radio Selection, Playback Project Structure; `_bmad-output/implementation-artifacts/epic-16-context.md`]

### References

- `_bmad-output/planning-artifacts/epics.md` — Epic 16 and Story 16.4, especially acceptance criteria and implementation gate.
- `_bmad-output/planning-artifacts/prd.md` — FR66–70, FR78–79, P-NFR2/4/5.
- `_bmad-output/planning-artifacts/architecture.md` — Playback State and Ownership, Provider Integration, Radio Selection and Session Control.
- `_bmad-output/planning-artifacts/ux-design-specification.md` — playback refinement; source badges and browse independence are specified by P-UX-DR14 in `epics.md`.
- `_bmad-output/implementation-artifacts/epic-16-context.md` — sequence, privacy and release evidence.
- `_bmad-output/implementation-artifacts/16-3-continue-radio-through-meaningful-artist-connections.md` — previous-story behavior and review corrections.

## Dev Agent Record

### Agent Model Used

GPT-6 Codex (story preparation)

### Debug Log References

- 2026-09-28 provider contract check: [OpenSubsonic Child](https://opensubsonic.netlify.app/docs/responses/child/) documents optional `musicBrainzId`, `mediaType` and `isrc`; `mediaType=song` is required before treating the ID as a recording. [Jellyfin BaseItemDto](https://typescript-sdk.jellyfin.org/interfaces/generated-client.BaseItemDto.html) exposes optional `ProviderIds`; Jellyfin's [track/recording distinction](https://github.com/jellyfin/jellyfin/issues/11020) rules out `MusicBrainzTrack` as a recording key. The adapter requests `ProviderIds` on music item/list/search paths and only accepts `MusicBrainzRecording` on Audio items. Audiobookshelf remains unmatched.
- Configured local server inventory (server type/version only; no addresses or credentials read into logs): Jellyfin 12.1.0 ×1, OpenSubsonic 1.16.1 ×1, Audiobookshelf with no recorded version ×3. Authenticated track-response coverage is reported below, separately from adapter fixture correctness.
- Red phase: the new resolver tests failed to compile before `RecordingEvidence` existed. Green phase: recording and Radio tests pass. The first full daemon run failed only on one Jellyfin mock request that still expected the old `Fields` list; it was updated. The final unsandboxed loopback fixture suite passed (1,242 daemon unit tests and 5 contract tests; 6 ignored). Local UI TypeScript/build, `cargo fmt --check`, and `git diff --check` passed. Clippy completed with existing warnings and no errors.
- [Build #110](https://github.com/HifiMule/HifiMule/actions/runs/36445479156) passed on source commit `9bc4f0d9f01eda561507f9f47805546f095d7054` for windows-x64, linux-x64, macos-x64, and macos-arm64. All four playback-evidence jobs and their artifacts are shown in the run summary. The separate local macOS arm64 source fixture at `/tmp/hifimule-16-4-macos-evidence.json` passed 9/9 isolated commands on the same clean commit. These results establish source-build and fixture behavior; installed playback has not been exercised.
- 2026-09-28 read-only configured-library sample, using each provider's authenticated `list_tracks` path and the first 100 Audio/song candidates: OpenSubsonic 1.16.1 yielded 100/100 song-entity-eligible rows and 89/100 accepted recording keys; 11/100 remained uncertain. Jellyfin 12.1.0 yielded 0/100 exact `MusicBrainzRecording`-eligible rows and 0/100 accepted keys; 100/100 remained uncertain. The normalized adapter output does not preserve raw rejection causes, so this sample does not distinguish absent, malformed, multiple or conflicting raw values. The fixture matrix covers those cases, but no actual cross-server confident match or installed playback was observed. Three configured Audiobookshelf sources remain unmatched by design. The temporary probe used a read-only SQLite connection and 20-second bounded provider requests, printed aggregate counts only, and was removed after sampling.
- Final completion-gate rerun after removing the probe and updating evidence: `npm run build:daemon -- test -p hifimule-daemon` passed with normal loopback access (1,242 daemon unit tests and 5 contract tests; 6 ignored). No probe code remains in the source tree.

### Implementation Plan

- Keep provider recording evidence private in `Song.provider_metadata`. Use one validated MusicBrainz recording UUID with provider provenance and explicit performance-version vetoes; neither local IDs nor ISRC, release, work, artist or title/duration alone create a recording key.
- Preserve `run_pipeline` ordering, then collapse only confident keys in bounded pools. Choose one fetched copy using comparable same-codec bitrate and stable configured-source ties; try another bounded copy if preparation fails. Keep the artist anchor separate from the selected portable playback source.
- Persist immutable, versioned automatic-occurrence associations and indexed heard/excluded membership in schema 10. Source-qualified schema 9 rows remain authoritative. Recheck both forms of membership in the owner and the append transaction; renew only heard membership, and clear the exclusion scope on a new Radio.

### Completion Notes List

- Ultimate context engine analysis completed - comprehensive developer guide created.
- Implemented private, versioned MusicBrainz recording evidence in Jellyfin and OpenSubsonic adapters. Radio groups only confident copies, ranks fetched sources deterministically, and retries another eligible copy after preparation failure while preserving portable source routing and manual queue intent.
- Migrated Playback SQLite schema 9 to 10 with immutable automatic-occurrence recording associations and indexed heard/excluded membership. Worker, owner, and append transaction enforce recording-level eligibility; source-qualified historical state remains authoritative.
- Verified the full daemon suite, adapter/resolver/owner/migration fixtures, UI TypeScript/build, formatting, Clippy, and four-platform Build #110. The real-library sample found 89/100 accepted OpenSubsonic keys and 0/100 accepted Jellyfin keys; installed playback and an actual cross-server match remain untested.

### File List

- `.github/workflows/build.yml`
- `_bmad-output/implementation-artifacts/16-4-avoid-duplicate-radio-recordings-across-configured-servers.md`
- `_bmad-output/implementation-artifacts/sprint-status.yaml`
- `hifimule-daemon/src/api.rs`
- `hifimule-daemon/src/auto_fill/mod.rs`
- `hifimule-daemon/src/domain/models.rs`
- `hifimule-daemon/src/playback/mod.rs`
- `hifimule-daemon/src/playback/model.rs`
- `hifimule-daemon/src/playback/persistence.rs`
- `hifimule-daemon/src/playback/radio.rs`
- `hifimule-daemon/src/playback/recording.rs`
- `hifimule-daemon/src/playback/selection.rs`
- `hifimule-daemon/src/playback/session.rs`
- `hifimule-daemon/src/providers/audiobookshelf.rs`
- `hifimule-daemon/src/providers/jellyfin.rs`
- `hifimule-daemon/src/providers/subsonic.rs`
- `hifimule-daemon/src/rpc.rs`
- `hifimule-daemon/src/rpc/playback_selection.rs`
- `hifimule-daemon/src/sync.rs`

### Change Log

- 2026-09-28: Implemented conservative cross-server Radio recording deduplication, schema 10 persistence, provider parsing, and deterministic multi-server regression fixtures. Recorded four-platform CI and read-only real-library metadata coverage separately from installed playback evidence.
