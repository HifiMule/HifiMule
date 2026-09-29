# Story 16.8: Save supported Like and Dislike preferences on the source server

Status: ready-for-dev

## Story

As a HifiMule user,
I want explicit Like or Dislike actions to update my source server's track preference,
so that the preference is available outside HifiMule rather than becoming a separate local taste profile.

## Acceptance Criteria

1. For the displayed main or preview occurrence, show only actions with a verified, user-specific provider equivalent. Read and show the authoritative server preference when available. Distinguish unknown or unavailable state from explicit neutral, Like, and Dislike.
2. A favorites-only provider may expose Like only after the favorite mapping is verified. Dislike stays unavailable and the daemon rejects a direct unsupported request. Removing a favorite is not Dislike; a low numeric rating is not automatically a Dislike equivalent.
3. An accepted action freezes the occurrence ID, logical session ID, portable source server ID, server-local track ID, and requested value. Use that server's existing credentials regardless of the browsed server or a matching recording on another server. Never propagate feedback across sources.
4. Show pending, server-confirmed, failed, and unresolved/ambiguous states accurately. Persist operational intent before sending. Retry or reconcile only under verified provider semantics; do not blindly repeat an ambiguous toggle or present local intent as server confirmation.
5. Serialize or version opposing intents for the same source track. The latest accepted intent wins through an explicit reconciliation policy; out-of-order responses cannot overwrite it. Changing the displayed occurrence prevents stale results from changing the new track's controls.
6. Explicit Dislike marks that occurrence rejected in the recoverable current logical session for Story 16.9, whether or not the remote write succeeds. A later explicit Like on that same occurrence clears this local rejection; feedback on another occurrence of the same track does not. This session disposition is separate from server preference and does not create a cross-session taste profile.
7. Feedback during main playback or Preview does not issue Play, Skip, Stop, Return, queue mutation, scrobble reversal, or automatic Radio exclusion. It does not undo a server-counted listen. Radio's existing skip exclusions remain unchanged.
8. After reconnect, refresh from the source server and reconcile displayed preference and pending operations. Authoritative remote state, including changes made by another client, supersedes stale cached display state. An unresolved write remains visibly unresolved until a safe read/reconciliation establishes the value.
9. Deterministic provider, journal, RPC, owner, and UI tests cover capability gates, missing/neutral/negative values, network and crash ambiguity, rapid opposite intents, source changes, repeated tracks, Preview, local rejection/reversal, and keyboard/status accessibility. Configured-server probes must verify actual preference effects for each enabled server/version. Run affected Windows, macOS, and Linux build and interaction checks; record any installed-platform gap without claiming it passed.

## Tasks / Subtasks

- [ ] Complete the provider contract gate before enabling a write (AC: 1–5, 8–9).
  - [ ] For each proposed server/version, document a versioned table: read endpoint and absent-value semantics; Like/Dislike/clear endpoint and exact request; user scope; repeat-write and opposing-write behavior; response versus persisted effect; read-after-write consistency; ambiguity/reconciliation; enabled/disabled decision. Capture sanitized request/effect fixtures from configured test accounts.
  - [ ] Verify Jellyfin 12.1.0 `POST /UserItems/{itemId}/Rating?likes=true|false`, DELETE on that route for neutral, and `UserItems/{itemId}/UserData` readback separately from favorite endpoints. The v12.1 controller writes nullable `Likes`; `false` is negative, not clear. Confirm actual track-level user state and behavior on the configured server before enabling either action.
  - [ ] Investigate Navidrome 0.64.2 `star`/`unstar`, `getSong`/`getStarred2` state, and `setRating`. Enable Like only if starring genuinely expresses the user preference and can be read back. Keep Dislike disabled unless the tested server provides a genuine negative preference; `rating=1` is a numeric rating, not proof of Dislike.
  - [ ] Treat other Subsonic/OpenSubsonic implementations and Audiobookshelf as unsupported until separately verified. Capability detection must be per configured server and user, independent of reporting capability.
- [ ] Add source-scoped provider feedback and daemon RPC contracts (AC: 1–5, 8).
  - [ ] Extend `MediaProvider` with explicit feedback capabilities, authoritative read, and set-value operations; default to unsupported. Keep provider URLs, credentials, request details and responses within adapters.
  - [ ] Add schema-versioned read/write RPCs with exact occurrence and expected session identity, bounded input validation, typed capability/state/status/diagnostic responses, and mutation classification. Reject stale or mismatched occurrence and unsupported action before network send.
  - [ ] Define a per-source-track intent sequence. Journal each accepted value and operation ID before send; serialize sends for one source track, fence stale completions, and reconcile ambiguous operations by an authoritative read where the provider contract permits. Never retry a toggle without proof. Bound request timeouts, queued work, retention, and shutdown recovery.
- [ ] Record session rejection without changing transport (AC: 6–7).
  - [ ] Persist a versioned, bounded per-session/per-occurrence disposition separate from terminal playback outcome, Radio membership, and server feedback journal. Record explicit Dislike immediately even if provider persistence later fails. Explicit Like on the same occurrence clears it; session replacement/expiry follows existing session-history lifecycle.
  - [ ] Expose a read path for Story 16.9 so snapshot creation can omit rejected occurrences, including a disliked Preview, without inferring rejection from remote status or track identity. Confirm paused restore retains the current logical session's disposition.
- [ ] Add accessible feedback controls and verification (AC: 1–9).
  - [ ] Reuse the floating playback controls' current-occurrence source badge, report-status pattern, request fencing, and localized status surface. Show capability-specific controls for the current main/preview occurrence; unknown, loading, pending, confirmed, failed and ambiguous states need distinct labels and live announcements without stealing focus.
  - [ ] Test two servers sharing a track ID, duplicate recordings, track/preview switches during requests, opposite rapid actions, server-side changes after reconnect, journal failures, and no transport/reporting side effects. Update API/data-model docs and all four locale strings. Run focused daemon/UI/i18n checks plus the affected cross-platform matrix and record configured-server version/effect evidence.

## Dev Notes

### Implementation gate and provider semantics

Jellyfin v12.1 controller source has `POST /UserItems/{itemId}/Rating` with nullable `likes`: `true` is positive, `false` is negative, and DELETE clears to null. Its favorite endpoints change the separate `IsFavorite` field. A configured Jellyfin 12.1.0 probe must still verify observable track/user effects and readback before enabling the mapping. OpenSubsonic documents `star`, `unstar`, and `setRating(0..5)`; `0` clears a numeric rating. The optional per-user `starred` timestamp and `userRating` fields are distinct; `averageRating` is not a user's preference. Absent fields in an authoritative response can indicate unstarred/unrated, but a missing field in HifiMule's current mapper is **unknown**, not neutral. Probe each proposed mapping with controlled track/account state, repeated and opposed requests, reconnect reads, and another-client changes before advertising it. Keep unverified mappings disabled. Story 16.7's Navidrome reporting probe and disabled Jellyfin reporting route establish **no** feedback semantics. [Source: `epics.md` §Story 16.8 Implementation gate; `architecture.md` §Playback Provider Integration; [Jellyfin v12.1 controller](https://github.com/jellyfin/jellyfin/blob/v12.1/Jellyfin.Api/Controllers/UserLibraryController.cs), [Jellyfin UserDataApi](https://kotlin-sdk.jellyfin.org/dokka/jellyfin-api/org.jellyfin.sdk.api.operations/-user-data-api/index.html); [OpenSubsonic Child](https://opensubsonic.netlify.app/docs/responses/child/), [star](https://opensubsonic.netlify.app/docs/endpoints/star/), [unstar](https://opensubsonic.netlify.app/docs/endpoints/unstar/), [setRating](https://opensubsonic.netlify.app/docs/endpoints/setrating/)]

Use set-value semantics where the server truly supports them. For an ambiguous write, a read can establish the current server value but cannot prove whether this operation or another client produced it; label the operation accordingly. For opposing accepted actions, send the newer intent only after the earlier send is terminal or safely reconciled. If the earlier result is ambiguous and the contract cannot safely order a newer set, expose a conflict requiring refresh/user action. Do not let an old acknowledgement mark a newer intent confirmed. Define a bounded operational journal independent of the completed-listen journal; an occurrence's local rejection survives remote failure and is never inferred from a cached server preference. [Source: `epics.md` §Story 16.8 ACs 4–8; `hifimule-daemon/src/playback/reporting.rs`; `docs/data-models-hifimule-daemon.md` §Live listening report journal]

### Existing files to update and behavior to preserve

| File | Current state | Story change and preservation rule |
| --- | --- | --- |
| `hifimule-daemon/src/providers/mod.rs` | `MediaProvider` has read-only favorites and live-report methods; unsupported defaults. | Add capability/read/set feedback methods without changing browse, scrobble or other provider defaults. |
| `hifimule-daemon/src/providers/jellyfin.rs`, `subsonic.rs` | Jellyfin maps `UserData.IsFavorite`; Subsonic `getStarred2` lists favorites, while ordinary song mapping leaves `is_favorite: None`. No preference write exists. | Implement only verified version/user mappings and authoritative track read. Preserve `None` as unknown until an actual read; do not treat missing `starred` as neutral. |
| `hifimule-daemon/src/playback/model.rs`, `session.rs` | `SessionSnapshot.current` identifies the visible main/preview occurrence with frozen `TrackSource`; owner fences generations and owns transport. | Capture exact occurrence before an async action; add session disposition without transport effects or a queue revision change. Preserve Preview/Return and source identity. |
| `hifimule-daemon/src/playback/persistence.rs` | Playback schema v12 stores occurrences, Radio membership, audition outcomes and live report journal. | Add a migration and bounded feedback/disposition state. Preserve existing outcome constraints, skipped Radio exclusions, paused restore, and previous migrations. |
| `hifimule-daemon/src/playback/reporting.rs` | Durable live-report operations, bounded worker, source lookup, ambiguous-send handling. | Reuse its operational patterns, not its completed-listen eligibility or status rows. Feedback and reporting must not trigger each other. |
| `hifimule-daemon/src/rpc.rs`, `hifimule-ui/src/rpc.ts` | Schema v1 playback RPC and typed UI wrappers; mutating methods are classified separately. | Add typed feedback RPCs and guard mutation routing; keep credentials and provider payloads off the wire. |
| `hifimule-ui/src/components/PlaybackControls.ts`, `hifimule-i18n/catalog.json` | Current-occurrence controls, source badge, live-report status polling and stale-result request fencing. | Add capability-gated controls and localized state announcements without affecting transport, narrow layouts, or focus. |
| `docs/api-contracts-hifimule-daemon.md`, `docs/data-models-hifimule-daemon.md` | Describe playback RPC and v12 reporting state. | Document exact feedback wire contract, operation state, local rejection/reversal, bounds and provider limitations. |

`Song.is_favorite: Option<bool>` is browse metadata, not a complete feedback state: it cannot by itself represent pending/ambiguous operations or a genuine negative preference. Reuse the frozen `Occurrence.source` (`serverId`, `trackId`) and `SessionSnapshot.current`, including Preview; never use selected browse server or recording deduplication identity for writes. Keep Rust/SQL snake_case and JSON camelCase. The workspace uses Rust edition 2024, Tokio ~1.49, rusqlite ~0.38, reqwest ~0.12 and pinned UI dependencies; no upgrade is required for this story. [Source: `hifimule-daemon/src/domain/models.rs`; `hifimule-daemon/src/playback/model.rs`; `hifimule-daemon/src/providers/subsonic.rs`; `Cargo.toml`]

### Previous story and Git intelligence

Story 16.7 added the v12 live-report journal, source-frozen operation rows, bounded delivery and inspectable `pending/confirmed/failed/ambiguous/unsupported` status. Its review fixed completion-first processing, pre-send deferral, journal-failure visibility, output-loss/shutdown evidence and UI occurrence scoping. Reuse these patterns for feedback where applicable. Its configured-server evidence covered Navidrome 0.64.2 reporting and Jellyfin 12.1.0 counting only; installed Windows/macOS/Linux server-effect checks were tracked separately. Recent commits `cd93a0c` (story), `9a781c7` (implementation), and `ec9a562` (review) are the current baseline. [Source: `16-7-report-listening-accurately-to-the-source-server.md` §§Provider Contract Gate, Review Findings, File List; Git history]

### Testing and completion evidence

Provider fixtures must assert request method/path/body, the exact user and track scope, authoritative readback, repeat/opposite values and ambiguous outcomes. Persistence tests must cover crash before send, crash during send, bounded queue/retention, restore and session rejection. RPC tests must reject unsupported/stale source requests; UI tests must verify current-occurrence fencing, four-locale labels, keyboard focus and status announcements. Run formatting, focused Rust tests, Clippy, frontend build and playback UI/i18n tests using repository scripts; extend to the affected Windows/macOS/Linux installed interaction matrix as available. Record server version, platform, tested account scope and observed state changes. A 2xx or success envelope alone does not establish the provider preference effect. [Source: `epics.md` §Story 16.8 final AC; `prd.md` P-NFR4–6; `scripts/tests/playback-ui.test.mjs`]

### Project Structure Notes

The old `project-context.md` says greenfield; current code and approved Epic 16 architecture are the implementation baseline. Feedback belongs in daemon provider adapters and playback owner, with UI only presenting typed RPC state. No local-only durable taste fallback, cross-server propagation, third-party metadata service, or device manifest change is in scope. Story 16.9 consumes the per-occurrence rejected disposition; playlist and basket export follow in 16.10–16.11. [Source: `epic-16-context.md`; `architecture.md` §§Playback State and Ownership, Playback Provider Integration; `epics.md` §§Stories 16.8–16.11]

### References

- `_bmad-output/planning-artifacts/epics.md` — Epic 16, Story 16.8, P-UX-DR8/13/14.
- `_bmad-output/planning-artifacts/prd.md` — FR77–78 and P-NFR4–6.
- `_bmad-output/planning-artifacts/architecture.md` — Playback State and Ownership; Playback Provider Integration.
- `_bmad-output/implementation-artifacts/epic-16-context.md` — source/occurrence and release boundaries.
- `_bmad-output/implementation-artifacts/16-7-report-listening-accurately-to-the-source-server.md` — reporting journal lessons and verification limits.

## Dev Agent Record

### Agent Model Used

GPT-6 (Codex)

### Debug Log References

### Completion Notes List

- Ultimate context engine analysis completed - comprehensive developer guide created.

### File List

- `_bmad-output/implementation-artifacts/16-8-save-supported-like-and-dislike-preferences-on-the-source-server.md`
- `_bmad-output/implementation-artifacts/sprint-status.yaml`
