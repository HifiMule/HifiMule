---
baseline_commit: cd93a0c76ff96a44da2678ba7aac86dfa16a86e5
---
# Story 16.7: Report listening accurately to the source server

Status: done

## Story

As a HifiMule user,
I want supported listening activity recorded on the server that supplied the track,
so that its listening history reflects what I heard without HifiMule creating its own taste profile.

## Acceptance Criteria

1. When audible playback starts or its relevant state changes, send supported now-playing status to that occurrence's source server. Prefetch, queue insertion, native metadata updates, and paused-session restoration must not submit a completed listen.
2. Submit a completed-listen report only after actual listening meets the verified policy for that provider. Requesting or downloading a full stream is not listening evidence.
3. An early skip, stop, replacement, or technical failure must not manufacture a completed listen where the provider permits. Never promise reversal of a play already counted by a server; a technical failure is not a dislike.
4. Seek forward, replay, pause, buffering, and restart must have explicit, tested effects on eligibility. Wall-clock time, skipped-over duration, and duplicate progress events must not manufacture completion.
5. A fully heard audition counts where supported; an interrupted audition follows the same provider rules. Returning to the preserved main occurrence does not count it again merely because it resumed.
6. A qualifying occurrence uses its frozen server ID, server-local track ID, and existing credentials. Mixed-source queues, browsing changes, and repeated track IDs cannot reroute reports. A deliberate repeat occurrence is distinct from a retry of one report.
7. Persist pending, confirmed, failed, and ambiguous outcomes as needed. Retry only after verified provider idempotency or reliable reconciliation; never blindly repeat an ambiguous non-idempotent write or claim guaranteed exactly-once delivery.
8. Unsupported or unreconcilable reporting is inspectable and never interrupts audio. Neither persisted evidence nor logs expose credentials or authenticated stream URLs.
9. Bound the outage queue, retry/backoff, retention, and memory work under a documented policy. Keep reporting and persistence off the audio callback. Do not create a cross-session recommendation or taste model.
10. Provider request fixtures and deterministic local tests cover completion, early skip, seeking, audition, return, duplicate events, repeated occurrences, restart, and ambiguous responses. Configured-server API probes on macOS arm64 verify the observed now-playing and play-count effects for Jellyfin 12.1.0 and Navidrome 0.64.2. Unverified or unsupported semantics remain disabled or explicitly limited. Installed HifiMule playback and server-effect checks on Windows, macOS, and Linux are tracked separately and are not claimed as evidence for this story.

## Tasks / Subtasks

- [x] Establish the provider contract before enabling writes (AC: 1–4, 7, 10).
  - [x] For each supported Jellyfin and Subsonic/OpenSubsonic server/version, record now-playing, start/progress/stop, automatic play-count, seek/resume, completed submission, error, and idempotency behavior with request fixtures and configured-server observations. Check Navidrome behavior independently. Treat Audiobookshelf music scrobbling as unsupported unless separately verified.
  - [x] Define a provider-specific eligibility table with exact thresholds, duration/unknown-duration behavior, seeks/replays, pauses/buffering, previews, natural end, interrupted end, and server-side counting effects. Do not substitute one universal threshold. Disable a behavior if its effect cannot be verified.
  - [x] Decide the safe contract for Jellyfin session start/progress/stop versus explicit PlayedItems and for legacy Subsonic `submission=false/true` versus advertised OpenSubsonic `playbackReport`; prevent two mechanisms from counting the same occurrence. Document any server/version limitation and split provider implementations into ordered follow-up stories if the gate cannot be completed in one implementation session.
- [x] Capture authoritative audible evidence in the playback owner (AC: 1–6).
  - [x] Key reporting to the existing logical session, occurrence, playback attempt/generation, source server, and provider track identities. Reuse owner events and consumed-frame progress; ignore prefetch, queued, stale-generation, and UI-interpolated position events.
  - [x] Track only the evidence the verified eligibility policies need (for example, monotonic listened media intervals or validated contiguous coverage). Keep it bounded, distinguish seeking from heard time, and freeze terminal outcomes for skip, stop, replacement, natural completion, audition return, output loss, and shutdown.
  - [x] Preserve main occurrence identity across Preview/Return and paused restore. Deliberate replay/repeated queue occurrences remain separate; duplicate owner events and callback samples do not create duplicate operations.
- [x] Add source-routed provider reporting and durable operation processing (AC: 1–3, 6–9).
  - [x] Extend the existing `MediaProvider` boundary and adapters with separate capability-gated status and completed-listen operations. Keep credentials and server-specific endpoints inside the daemon/providers; do not dispatch by the currently browsed server or route provider API calls from the UI.
  - [x] Add versioned SQLite state and migration for operation identity, frozen source/track, kind, eligibility evidence, status, attempt count, timestamps, and safe diagnostic category. Commit intent before a non-idempotent send; classify definite rejection versus ambiguous transport/timeout/crash. Reconcile where reliable, otherwise surface unresolved state without blind retry.
  - [x] Use bounded worker concurrency, backoff, retention, and shutdown behavior. Keep audio nonblocking and preserve the Rockbox/device-scrobble history and Audiobookshelf progress paths.
- [x] Expose truthful reporting status and verification (AC: 8–10).
  - [x] Add a read-only daemon RPC/status surface and accessible, localized UI explanation for pending, confirmed, failed, ambiguous, and unsupported outcomes; avoid claiming server confirmation from local eligibility alone. Keep status scoped to source/occurrence and avoid exposing secrets.
  - [x] Add deterministic owner/persistence/provider tests, including crash boundaries, duplicate messages, two servers sharing a track ID, manual/album/Radio/Preview, output loss, seek gaps, and resumed paused state. Run focused Rust/UI/i18n checks and configured-server API probes; record server version, platform architecture, observed server effects, and installed-app gaps separately.

### Review Findings

- [x] [Review][Patch] Narrow AC10 to verified provider probes — User chose the limited verified scope. AC10 now names the macOS arm64 configured-server probes and explicitly excludes unrun installed-app checks; the Windows, macOS, and Linux matrix is tracked in `deferred-work.md`.
- [x] [Review][Patch] Bound provider requests and prioritize completion delivery [hifimule-daemon/src/playback/reporting.rs:255] — Completion claims now precede status work; provider resolution, preflight, status, and send have deadlines.
- [x] [Review][Patch] Defer failures before the completion send [hifimule-daemon/src/providers/subsonic.rs:750] — Navidrome verification is a separate pre-send operation, so transient failure safely defers and a timed-out submission remains ambiguous.
- [x] [Review][Patch] Surface live-report journal write failures [hifimule-daemon/src/playback/session.rs:5570] — A bounded volatile `failed` result with `journalUnavailable` is inspectable through the read-only RPC without sending.
- [x] [Review][Patch] Persist output-loss and shutdown outcomes [hifimule-daemon/src/playback/session.rs:1579] — Owner terminal paths now journal ineligible evidence; a later eligible resume can promote the same unsent occurrence.
- [x] [Review][Patch] Scope UI report status to the displayed occurrence [hifimule-ui/src/components/PlaybackControls.ts:290] — The controls filter report rows by current occurrence and frozen server.
- [x] [Review][Patch] Classify unsupported providers before applying Navidrome eligibility [hifimule-daemon/src/playback/reporting.rs:135] — Jellyfin and Audiobookshelf journal rows report `unsupported` even when below Navidrome's threshold.

## Dev Notes

### Implementation gate and contract decisions

The epic requires verified provider semantics **before coding reporting behavior**. Produce a short decision table in the implementation record: provider/server version; status endpoint and side effects; completion endpoint and threshold; seek/restart rule; idempotency or reconciliation proof; enabled/disabled result. A 2xx response is evidence of request acceptance, not proof of a play-count effect. If an endpoint automatically counts a listen, the owner must not also send an explicit completed submission for that same occurrence. This gate is especially important for Jellyfin session events and the newer OpenSubsonic `playbackReport` extension. [Source: `_bmad-output/planning-artifacts/epics.md` §Story 16.7 Implementation gate; `_bmad-output/planning-artifacts/architecture.md` §Playback Provider Integration]

The current provider `ScrobbleRequest` has `Playing` and `Played`, but Jellyfin and Subsonic adapters reject `Playing`; `Played` maps to Jellyfin `UserPlayedItems` and Subsonic `scrobble(..., submission=true)`. That path supports the older Rockbox/device scrobbler and has no verified live playback eligibility, durable delivery, or now-playing contract. Reuse its adapter and credential patterns while keeping live occurrence operation IDs separate from `scrobble_history(device_id, artist, album, title, timestamp_unix)`. Audiobookshelf's `scrobble` is unsupported and its own book/podcast continuity logic must remain intact. [Source: `hifimule-daemon/src/providers/mod.rs` §ScrobbleRequest; `hifimule-daemon/src/providers/jellyfin.rs` §scrobble; `hifimule-daemon/src/providers/subsonic.rs` §scrobble; `hifimule-daemon/src/scrobbler.rs`; `hifimule-daemon/src/playback/book_progress.rs`]

The device scrobbler explicitly documents a duplicate-count window when the server accepts a write but local `record_scrobble` fails. Its `report_playback_session` helper sends synthetic Jellyfin start/stop events for device history. Neither its dedup key nor that helper is a safe recovery model for live playback. `ScrobbleRequest.position_seconds` and `played_at_unix_seconds` are present but ignored by today's Jellyfin/Subsonic adapters. [Source: `hifimule-daemon/src/scrobbler.rs` §§submission, database recording; `hifimule-daemon/src/api.rs` §report_playback_session; provider adapters]

### Existing code to update and preserve

| Boundary | Current state | Change and preservation |
| --- | --- | --- |
| `hifimule-daemon/src/playback/session.rs`, `model.rs`, `audio.rs` | One daemon owner holds current/main/audition occurrence, attempt and generation. Audio emits active/buffering/completed events and consumed-frame progress; completion follows output drain. | Derive heard evidence and terminal reason here, outside the callback. Preserve generation fences, five-upcoming Radio bound, manual/album/Preview behavior, native controls, output-loss pause, and atomic session replacement. |
| `hifimule-daemon/src/playback/persistence.rs`, `hifimule-daemon/src/db.rs` | Session attempts persist occurrence/source and terminal disposition/position. Preview state tracks contiguous heard/seek uncertainty; main attempts do not yet prove heard coverage. SQLite also holds device scrobble history. | Add a migration and durable live-report operation journal. Reuse safe SQLite transaction patterns without treating cursor position or legacy device history as heard coverage. Keep restoration paused and preserve corrupt-state rejection. |
| `hifimule-daemon/src/providers/mod.rs`, `jellyfin.rs`, `subsonic.rs`, `audiobookshelf.rs`, `hifimule-daemon/src/api.rs` | Provider scrobble supports completed submissions only in Jellyfin/Subsonic. Jellyfin client has explicit played and synthetic session start/stop helpers. Audiobookshelf scrobble is unsupported. | Add capability-driven reporting semantics in adapters after verification. Preserve source identity and existing device scrobbles; avoid duplicate Jellyfin counting and unadvertised OpenSubsonic extension use. |
| `hifimule-daemon/src/playback/mod.rs`, `hifimule-daemon/src/main.rs` | The module registers playback services; daemon startup owns background worker lifetimes and shutdown. | Register the new reporting coordinator and a bounded recovery worker. Respect clean Quit and never hold the Tao/menu loop or playback owner on network submission. |
| `hifimule-daemon/src/rpc.rs`, UI playback state/components, `hifimule-i18n/catalog.json` | RPC exposes authoritative playback state and device scrobbler result; UI consumes snapshots. Four locales contain playback strings. | Expose inspectable live-report status without blocking transport. Keep status/error wording accessible and localized, source-scoped, request/occurrence-stable, and separate from device scrobbler results. |

The architecture names `hifimule-daemon/src/playback/reporting.rs` as the intended new coordinator. Place migration and durable queries with the repository's existing DB/persistence pattern; provider-specific HTTP stays in adapters. Do not put network, SQLite, allocation-heavy bookkeeping, or sync-held locks in the CPAL callback. No new external service, credential store, or UI-side API client is needed. [Source: `_bmad-output/planning-artifacts/architecture.md` §§Playback Audio/Streaming, Playback Provider Integration, Playback Implementation Contracts; `Cargo.toml`]

### Latest provider documentation checked 2026-09-28

- OpenSubsonic `scrobble` distinguishes a now-playing notification (`submission=false`) from a submission (`submission=true`); `time` is optional milliseconds since epoch. Existing HifiMule client sends only `id` and `submission`, so use of historical timestamps needs an explicit adapter change and fixture. [OpenSubsonic scrobble](https://opensubsonic.netlify.app/docs/endpoints/scrobble/)
- OpenSubsonic extension `playbackReport` v1 is advertised by `getOpenSubsonicExtensions`; its `reportPlayback` endpoint accepts `mediaId`, `mediaType`, `positionMs`, and `starting|playing|paused|stopped`. The specification says servers can infer position between updates and `ignoreScrobble=true` prevents play-count effects. Detect support per server and verify implementation; do not assume every Subsonic/Navidrome server implements it. [Playback Report extension](https://opensubsonic.netlify.app/docs/extensions/playbackreport/), [reportPlayback endpoint](https://opensubsonic.netlify.app/docs/endpoints/reportplayback/)
- Jellyfin exposes distinct `POST /Sessions/Playing`, `/Sessions/Playing/Progress`, and `/Sessions/Playing/Stopped` operations. Its explicit PlayedItems endpoint is separate. Verify whether session events already count a play for the supported server and ensure JSON fields and tick units match the actual contract. [Jellyfin PlaystateController](https://github.com/jellyfin/jellyfin/blob/master/Jellyfin.Api/Controllers/PlaystateController.cs)

### Previous story and Git intelligence

Story 16.6 established one daemon-owned, UI-closed Radio start path and strengthened accepted-command ordering, generation fencing, post-admission failure feedback, and source-bound session replacement. Reporting must follow the accepted owner state, not an obsolete prepared start. The sprint ledger marks 16.6 done, while its story file still says in-progress and its configured installed Radio/menu/bar matrix remains unverified; do not inherit that evidence as complete. Recent commits `643df64` (Story 16.6), `5b77e72` (implementation), and `1015b7d` (review) touched the owner, RPC, native menu, UI and status contracts. [Source: `_bmad-output/implementation-artifacts/16-6-start-radio-with-play-something-without-opening-the-main-window.md` §§Review Findings, Dev Agent Record; Git history; `sprint-status.yaml`]

### Project Structure Notes

Use Rust edition 2024 and the repository's pinned dependency versions; no upgrade is needed for this story. Keep snake_case in Rust/SQL and camelCase on JSON wires. The older `project-context.md` calls the project greenfield, but the current code and approved playback architecture are the operative baseline. Maintain daemon ownership, portable source identity, and existing privacy rules. [Source: `Cargo.toml`; `_bmad-output/implementation-artifacts/epic-16-context.md`; `_bmad-output/planning-artifacts/architecture.md`]

### References

- `_bmad-output/planning-artifacts/epics.md` — Epic 16 and Story 16.7 (canonical ACs and implementation gate).
- `_bmad-output/planning-artifacts/prd.md` — FR76, FR81, P-NFR4–5 and playback success/counter-evidence.
- `_bmad-output/planning-artifacts/architecture.md` — Playback Provider Integration and Implementation Contracts.
- `_bmad-output/implementation-artifacts/epic-16-context.md` — occurrence/source boundaries and story dependency order.
- `docs/api-contracts-hifimule-daemon.md`, `docs/data-models-hifimule-daemon.md` — update reporting RPC and persisted/status data contracts when implemented.

## Dev Agent Record

### Agent Model Used

GPT-6 (Codex)

### Implementation Plan

1. Complete the provider contract gate with versioned, configured-server observations and request fixtures before enabling any live reporting write.
2. Then implement bounded audible-evidence tracking, durable source-scoped operations, provider adapters, and read-only status in the task order above. Keep session events and explicit completed submissions mutually exclusive where the server can count both.

### Provider Contract Gate (2026-09-29; verified Navidrome route enabled)

| Provider / observed local version | Published status contract and possible counting effect | Completion / eligibility | Seek, restart, idempotency or reconciliation | Decision |
| --- | --- | --- | --- | --- |
| Jellyfin 12.1.0 (configured test account) | `/Sessions/Playing` returned 204 and raised a 38-second track's play count 0→1 immediately. A stop after one second returned 204 and left it at 1. Status events would count an early skip. | `/UserPlayedItems` returned 200 and moved another track 0→1; repeating that call left it at 1, so it did not represent a deliberate repeat. No live completion route yet satisfies both early-skip and repeat semantics. | Seek/resume and crash reconciliation are unverified; start-side counting already disqualifies the session path. | Live status and completion **disabled** for Jellyfin 12.1.0. Keep device scrobbling unchanged. |
| Navidrome 0.64.2 (OpenSubsonic protocol 1.16.1; configured test account) | Advertises `playbackReport` v1. `scrobble(submission=false)` appeared in now-playing with no play-count change. `reportPlayback(ignoreScrobble=true)` left counts unchanged through starting/playing/paused, a position jump, restart, and stopped. | `scrobble(submission=true)` moved counts 0→1 and a second call 1→2. Explicit submissions are non-idempotent; the endpoint does not enforce heard-time eligibility. | Daemon-owned consumed audio establishes eligibility at known duration and `min(ceil(50% of duration), 240 s)` heard; skipped duration and unknown duration do not qualify. Pauses/buffering add no heard time, backward replay adds only newly consumed audio, and restore alone adds none. Status uses only `ignoreScrobble=true`; completion uses only `submission=true`. Intent is committed before send; ambiguous completion is never retried. | Enabled for this exact tested implementation/version after local evidence tracking, journal, request fixtures, and worker checks. Other Subsonic-compatible servers remain unsupported pending their own contract. |
| Other Subsonic/OpenSubsonic versions | No configured observation. | Unknown. | Unknown. | Disabled. |
| Audiobookshelf | Music scrobbling unsupported by the existing adapter. Its book/podcast progress is a separate contract. | Unsupported. | Not applicable. | Keep live music reporting unsupported. |

Request and effect fixture: `docs/playback-evidence/reporting-contract-2026-09-29.json` (macOS arm64, anonymized track IDs, no credentials or server URLs). Sources checked: [OpenSubsonic scrobble](https://opensubsonic.netlify.app/docs/endpoints/scrobble/), [playbackReport extension](https://opensubsonic.netlify.app/docs/extensions/playbackreport/), [reportPlayback endpoint](https://opensubsonic.netlify.app/docs/endpoints/reportplayback/), [Jellyfin PlaystateController](https://github.com/jellyfin/jellyfin/blob/master/Jellyfin.Api/Controllers/PlaystateController.cs), and [open Jellyfin music-counting proposal](https://github.com/jellyfin/jellyfin/pull/16969). Published request acceptance is distinct from observed play-count effects. The local SQLite discovery read only provider type, version, and row counts; it did not read URLs, usernames, or credentials. Configured-server probes used the supplied test accounts and emitted no tokens or authenticated URLs.

### Debug Log References

- 2026-09-29: Captured baseline commit `cd93a0c76ff96a44da2678ba7aac86dfa16a86e5` and set story/sprint to in-progress.
- 2026-09-29: Existing adapters reject `ScrobbleSubmission::Playing`; `Played` maps to Jellyfin `PlayedItems` and Subsonic `submission=true`. No live occurrence contract or request fixtures exist in the repository.
- 2026-09-29: Provider gate remains open. Need controlled access to configured test servers and authorization to alter test listening history; cross-platform installed checks also remain outstanding. No reporting writes or implementation changes were made.
- 2026-09-29: User supplied Jellyfin 12.1.0 and Navidrome 0.64.2 test accounts. Controlled probes found Jellyfin session start increments play count before any heard duration; Navidrome status-only operations did not increment, while duplicate completed scrobbles incremented twice. Evidence fixture saved without secrets. Windows/Linux installed checks remain outstanding.
- 2026-09-29: Full daemon regression suite passed under the supported `npm run build:daemon` wrapper with loopback sockets enabled. The restricted sandbox's first run failed because mock HTTP servers could not bind; the elevated rerun passed.
- 2026-09-29: Added generation-fenced heard-time evidence, durable operation journal, one-at-a-time delivery worker, Navidrome 0.64.2 provider gating, bounded live-status owner events, read-only RPC, and four-locale UI explanations. Local daemon regression, provider fixtures, UI build/tests, i18n tests, formatting, and Clippy ran successfully during implementation; final rerun follows remaining edits.
- 2026-09-29: Installed configured-server checks remain unavailable on macOS, Windows, and Linux. The supplied LAN servers were probed directly from macOS arm64; those provider observations do not constitute installed HifiMule playback evidence on any platform.
- 2026-09-29: Review fixes passed the final loopback-enabled daemon suite (1282 unit tests passed, 6 ignored, 5 integration tests passed), playback UI suite (56 passed), frontend production build, formatting, Clippy, and `git diff --check`. Clippy completed with repository warnings. The user narrowed AC10 to verified configured-server API probes; installed Windows/macOS/Linux checks remain separately tracked without a completion claim.

### Completion Notes List

- Ultimate context engine analysis completed - comprehensive developer guide created.
- Provider gate complete for the tested versions. Jellyfin 12.1.0 and non-Navidrome Subsonic variants remain disabled for live reporting. Navidrome 0.64.2 status-only and explicit completion contracts are backed by anonymized configured-server request/effect fixtures. Full daemon regression passed.
- Live completion is based on frozen source identity and consumed audio; a known-duration occurrence must reach `min(ceil(duration/2), 240000 ms)`. An early skip, unknown duration, technical failure, output loss, or shutdown does not submit. Journal recovery leaves uncertain writes ambiguous and never blindly retries them. Source lookup failures before a send receive bounded backoff. The UI labels local eligibility separately from server acceptance.
- Code review applied six fixes: deadline-bound completion-first reporting, safe pre-send verification deferral, inspectable journal failures, output-loss/shutdown terminal evidence with resume promotion, occurrence-scoped UI status, and unsupported-provider classification. All findings are closed under the revised AC10 scope.

### File List

- `_bmad-output/implementation-artifacts/16-7-report-listening-accurately-to-the-source-server.md`
- `_bmad-output/implementation-artifacts/sprint-status.yaml`
- `docs/playback-evidence/reporting-contract-2026-09-29.json`
- `docs/api-contracts-hifimule-daemon.md`
- `docs/data-models-hifimule-daemon.md`
- `hifimule-daemon/src/playback/mod.rs`
- `hifimule-daemon/src/playback/persistence.rs`
- `hifimule-daemon/src/playback/reporting.rs`
- `hifimule-daemon/src/playback/session.rs`
- `hifimule-daemon/src/providers/mod.rs`
- `hifimule-daemon/src/providers/subsonic.rs`
- `hifimule-daemon/src/rpc.rs`
- `hifimule-i18n/catalog.json`
- `hifimule-ui/src/components/PlaybackControls.ts`
- `hifimule-ui/src/rpc.ts`
- `scripts/tests/playback-ui.test.mjs`

## Change Log

- 2026-09-29: Established versioned reporting contracts and captured configured-server request/effect evidence; disabled unverified or unsafe provider behavior.
- 2026-09-29: Implemented owner evidence, durable source-routed Navidrome reporting, inspectable RPC/UI outcomes, and bounded non-retrying recovery; local verification complete, cross-platform configured checks outstanding.
- 2026-09-29: Review decision narrowed AC10 to configured-server API probes on macOS arm64. Installed Windows/macOS/Linux playback and server-effect verification remains a separate follow-up.
- 2026-09-29: Code review fixes applied and verified; Story 16.7 marked done under the revised scope.
