---
baseline_commit: 7d5aefbcf0cb0cf7fa8eb00e3babe1bfa5f8220e
---

# Story 17.5: Directly play Audiobookshelf audiobooks through HifiMule

Status: review

## Story

As a listener,
I want to play a selected book or chapter through the existing playback path,
so that Audiobookshelf delivers immediate listening value without device synchronization.

## Acceptance Criteria

1. **Book and part admission.** A selected Books-library book starts the existing album playback flow with the deterministic audio-file order established in 17.3. A selected part starts the existing track flow. A single-file book plays as one track. Chapter markers remain timing metadata; they must not be turned into fictitious file tracks.
2. **Existing playback ownership.** Playback uses the track's portable source-server identity, even after browsing another server. The daemon's existing session, queue, output selection, generation fencing, buffering, skip/seek, and recoverable retry rules apply. Album playback never silently skips an unavailable part or classifies a technical failure as a user skip.
3. **Verified delivery only.** The Audiobookshelf provider resolves a daemon-private, authenticated playback description from the selected library and stable item/part identity. It accepts only representations whose actual method, MIME/container/codec, range/seek behavior, and decoder compatibility have been verified for the supported server version. Direct and playback-session transcode are distinct; a transcode session is not a downloadable conversion. An unexpected fallback or malformed response is rejected or verified before audio is opened.
4. **Session and credential safety.** Access tokens, authenticated URLs, library IDs, raw item IDs, response bodies, and session identifiers never reach UI/RPC payloads or logs. Provider-owned upstream playback sessions are closed on completion, replacement, stop, failure, and cancelled or stale work, without letting late cleanup alter the current session. Auth refresh remains bounded to one retry after 401; authenticated media reads remain within the configured endpoint and selected-library scope.
5. **Truthful failure.** Missing or removed media, stale library scope, denied access, expired credentials, rate limiting, unsupported format, incompatible representation, failed transcode, and network/server interruption map to sanitized, actionable, recoverable states as appropriate. An unavailable book remains available for explicit retry; no false success, silent fallback, or source skip is reported. The untested unavailable/incompatible upstream case must be probed and recorded before a specific response mapping is enabled.
6. **Scoped UI.** Book and part play controls are keyboard accessible, visibly focused, and use Book/Part language and existing loading/error/playback status patterns. Existing Jellyfin/Subsonic controls and music vocabulary remain unchanged. Audiobookshelf basket, download, sync, Autofill, preview, progress write, and remote mutation remain unavailable in this story; podcasts and series/collections stay out of scope.
7. **Regression evidence.** Offline provider/fixture, daemon playback/RPC, and UI tests prove multi-part order, single-file play, part admission, source routing, direct and transcode selection, incompatible/unavailable error, session cleanup and redaction, cancellation, and existing music-provider behavior. Completion claims distinguish offline fixture checks from installed playback evidence.

## Tasks / Subtasks

- [x] **Close the delivery evidence gap** (AC: 3, 5, 7)
  - [x] Probe unavailable/incompatible delivery and any needed multipart/seek behavior on the pinned v2.36.1 controlled deployment. Add redacted, versioned evidence to `docs/audiobookshelf-integration-contract.md` and the fixture manifest; retain `untested` where unobserved. Never infer success from `delivery-unavailable.json`.
  - [x] Confirm actual direct and transcode response fields, content types, playback-session close semantics, and supported decoder path before enabling each representation.
- [x] **Resolve provider playback privately** (AC: 1–5)
  - [x] In `hifimule-daemon/src/providers/audiobookshelf.rs`, implement `MediaProvider::resolve_playback` using the existing opaque song identity and selected Books library; reject Podcast, stale, or cross-library identities before request. Reuse the scoped auth/session HTTP client and error classification.
  - [x] Map verified `audioTracks` to the requested ordered file/part; validate `playMethod` rather than assuming requested direct/transcode mode was honored. Resolve only server-relative, same-origin URLs. Return the existing `PlaybackDescription`/representation types, with honest codec, range, and seeking fields.
  - [x] Add provider-owned playback-session lifetime/cleanup at the narrowest existing daemon boundary needed; close sessions exactly once when retired, including failed preparation and stale asynchronous work. Keep credentials and upstream session identifiers private.
- [x] **Connect existing playback admission and UI** (AC: 1, 2, 6)
  - [x] Reuse `playback.play_album` and `playback.play_track` RPC paths, album order, portable source identity, and session/output handling. Extend shared playback structures only where upstream session cleanup genuinely requires it.
  - [x] In `hifimule-ui/src/library.ts` and the existing card/row play components, replace the 17.4 Book/BookPart play gate with accessible play actions. Keep basket/download/sync/Autofill and other deferred controls gated. Add localized labels through the catalog and regenerate typed i18n output if new strings are introduced.
- [x] **Verify behavior and regressions** (AC: 1–7)
  - [x] Add co-located `mockito` tests for auth, scope, identity, ordering, response validation, direct/transcode, failure classes, close on all lifecycles, and secret redaction; add playback/RPC tests for source routing and non-skipping album failure.
  - [x] Add focused `hifimule-ui/tests/*.test.mjs` tests for Book/Part controls and deferred-action gates. Run focused and daemon suites, UI tests/type-check/build, diff check, and applicable formatting/lint gates. Record any pre-existing formatting drift separately.

## Dev Notes

### Binding implementation context

- **Precedence:** Final Epic 17, PRD FR84, architecture amendment, the pinned v2.36.1 controlled contract, and completed 17.1–17.4 are the implementation basis. Current upstream documentation is orientation; it does not expand verified compatibility. Audiobookshelf v2.36.1 was the latest release when checked on 2026-09-22. [Audiobookshelf releases](https://github.com/advplyr/audiobookshelf/releases)
- **Existing model:** Story 17.3 maps `audioFiles[].ino` to opaque song IDs and sorts files numerically. Story 17.4 exposes Books browse/search but intentionally hides Book/BookPart play and basket actions. Preserve book metadata, author/narrator hierarchy, chapter markers, safe artwork, selected-library validation, and search semantics.
- **Provider UPDATE:** `hifimule-daemon/src/providers/audiobookshelf.rs` currently has authentication, Books mapping, and safe artwork, but no `resolve_playback` override; its `download_url` rejects transfer. Keep those contracts separate. `hifimule-daemon/src/providers/mod.rs` already defines `PlaybackDescription`, `PlaybackRepresentation`, `PlaybackRequest`, selection policy, and an explicit unsupported default. Follow the Jellyfin/Subsonic adapter patterns without copying their endpoint semantics.
- **Playback UPDATE:** `hifimule-daemon/src/rpc.rs` and `hifimule-daemon/src/playback/commands.rs` already resolve through the source provider; `playback/audio.rs` owns decoder/output preparation. Read the exact admission and retirement paths before changing them. An ABS session may need a private cleanup handle because the current playback description contains only stream request data; establish clear ownership across retries, replacement, prefetch, cancellation, and process shutdown without leaking handles into persistence or RPC. Preserve queue revision checks, generation fencing, source routing, output authorization, and album failure pause/retry.
- **UI UPDATE:** `hifimule-ui/src/library.ts` creates Book and BookPart display items and currently gates their actions; `hifimule-ui/src/components/MediaCard.ts` hides generic play for those types. Wire book to the existing album play control and part to the existing track play control via `hifimule-ui/src/rpc.ts`/shared playback state. Do not reuse music basket or preview paths inadvertently. Existing keyboard/focus, virtualization, loading, and error presentation stay intact.
- **Verified wire evidence:** The contract observed `POST /api/items/{id}/play` with `forceDirectPlay: true` returning `playMethod: 0`, a server-relative track, and authenticated range `206` with `audio/mp4`; `forceTranscode: true` returned `playMethod: 2` for playback only. A MIME mismatch could fall back to transcode. `POST /api/session/{id}/close` was observed. The unavailable/incompatible response is explicitly **untested** and requires controlled evidence before coding a claimed exact status mapping. Use `direct-delivery.json`, `transcode-delivery.json`, `delivery-mime-mismatch.json`, and `delivery-unavailable.json` as synthetic shape fixtures, not raw recordings.
- **Security and compatibility:** Reuse `reqwest`, `serde`, Tokio, `async-trait`, `thiserror`, and existing `mockito`; no Audiobookshelf SDK is needed. Deny redirects to another origin for authenticated media. Never return an authenticated content URL or Bearer header through JSON-RPC. A server's `playMethod` and the decoder's actual stream probe, rather than filename alone, determine compatibility and seeking. Do not advertise seeking for an unverified ABS delivery mechanism.
- **Prior work and Git:** Story 17.4's final review fixed hidden navigation, premature basket actions, missing credits/chapters, bounded cover retrieval, and error classification. Its reported checks passed: daemon suite, contract tests, focused UI tests, UI build, and diff check. Its only reported format issue was pre-existing `playback/session.rs` drift. Recent commits `fb0004e`, `0834650`, `5a21a67`, `6025102`, and `fd7fd94` establish the 17.4/17.3 patterns; verify current code rather than relying only on those notes.

### Project Structure Notes

- Keep Audiobookshelf wire DTOs, auth, stream URL construction, and session-close API inside `hifimule-daemon/src/providers/audiobookshelf.rs`. Generic lifetime plumbing, if required, belongs in existing provider/playback modules. Do not create a parallel player, playback RPC, provider cache, or UI audiobook subsystem.
- Add fixture evidence under `hifimule-daemon/tests/fixtures/audiobookshelf/2.36.1/`, provider tests beside the adapter, playback/RPC tests beside their owners, and UI tests under `hifimule-ui/tests/`.

### References

- [Source: `_bmad-output/planning-artifacts/epics.md` — Epic 17 and Story 17.5]
- [Source: `_bmad-output/planning-artifacts/prd.md` — FR71–75, FR84, P-NFR4–6]
- [Source: `_bmad-output/planning-artifacts/architecture.md` — Playback Provider Integration and Epic 17 amendment]
- [Source: `_bmad-output/planning-artifacts/ux-design-specification.md` — Audiobookshelf integration]
- [Source: `docs/audiobookshelf-integration-contract.md` — Record 1, delivery and failure register]
- [Source: `_bmad-output/implementation-artifacts/17-4-browse-and-search-an-audiobookshelf-audiobook-server.md` — prior story and review corrections]
- [Source: `hifimule-daemon/src/providers/audiobookshelf.rs`, `hifimule-daemon/src/providers/mod.rs`, `hifimule-daemon/src/playback/commands.rs`, `hifimule-daemon/src/rpc.rs` — current daemon seams]
- [Source: `hifimule-ui/src/library.ts`, `hifimule-ui/src/components/MediaCard.ts`, `hifimule-ui/src/rpc.ts` — current UI seams]

## Dev Agent Record

### Agent Model Used

GPT-6 Codex

### Debug Log

- Used the existing album and track RPCs. The provider resolves a part through the scoped book detail, requests a playback session, validates v2.36.1, `playMethod`, `ino`, item-scoped URL, codec, MIME, and authenticated byte-range response, then returns only a daemon-private request. HLS session transcode is rejected because this decoder path cannot safely play its playlist and segments.
- A reference-counted request cleanup closes an upstream session after the last preparation or output owner retires. Daemon shutdown drains pending closes. Initial media fetch and range reads refresh once after 401; the cleanup path does the same. No session or credential fields were added to RPC or persistence.
- Reused Book and Part display items and existing play controls. Book actions call album playback; Part actions call track playback. Music basket, preview, and queue controls remain gated for Audiobookshelf.

### Completion Notes List

- Ultimate context engine analysis completed - comprehensive developer guide created.
- Story prepared for implementation; no production playback or upstream failure probe is claimed complete.
- 2026-09-22: Started implementation workflow. Live endpoint access was later provided. Direct AAC/MP4, HLS transcode, missing-item 404, and session close were observed and recorded in redacted contract evidence. No multipart item was found on the first catalog page; a forced-direct request with unsupported MIME still returned direct playback. The incompatible delivery failure remains untested. The current decoder has no HLS path, so transcode playback cannot yet be enabled. No response mapping or playback capability was enabled.
- 2026-09-22: Probed the user-identified ten-part book. Direct session returned ten tracks; its first indexed file matched a direct MP3 track and still returned authenticated range 206. Forced transcode returned one HLS track without first-file identity. The unavailable-media failure remains unobserved, and the first evidence task remains incomplete.
- 2026-09-22: Reproduced the actual missing-file race after detail and direct session capture. A refreshed token read of the first-track URL returned 404; session close returned 200. The earlier 401 was caused by token expiry during coordination and was not classified as delivery failure. Unavailable behavior is now verified; incompatible-format failure remains untested.
- Evidence task completed: direct MP3/AAC byte ranges and session close are observed; HLS transcode is deliberately not enabled; incompatible-format failure remains explicitly untested. Five offline contract tests pass through the verified FFmpeg build wrapper.
- Implementation complete for review. Offline checks passed: 1,099 daemon unit tests plus five contract tests, 18 UI tests, 49 playback UI tests, UI TypeScript/Vite build, touched-file rustfmt, `git diff --check`, and daemon Clippy with no new warnings in touched files. The first full UI run exposed a pre-existing missing test-harness stub in `shutdownStatus.test.mjs`; it was corrected and the full UI suite passes.
- The repository-wide `cargo fmt --all -- --check` still reports pre-existing formatting drift only in `hifimule-daemon/src/playback/session.rs` at lines 5385–5392; this file was not modified. Controlled v2.36.1 HTTP probes established the delivery contract, while HifiMule playback in an installed application was not exercised. HLS transcode and the upstream incompatible-format failure remain unsupported or untested as stated in the contract.

### File List

- `_bmad-output/implementation-artifacts/17-5-directly-play-audiobookshelf-audiobooks-through-hifimule.md`
- `_bmad-output/implementation-artifacts/sprint-status.yaml`
- `docs/audiobookshelf-integration-contract.md`
- `hifimule-daemon/tests/fixtures/audiobookshelf/2.36.1/manifest.json`
- `hifimule-daemon/tests/fixtures/audiobookshelf/2.36.1/delivery-incompatible.json`
- `hifimule-daemon/tests/fixtures/audiobookshelf/2.36.1/delivery-unavailable.json`
- `hifimule-daemon/tests/fixtures/audiobookshelf/2.36.1/playback-follow-up-2026-09-22.json`
- `hifimule-daemon/tests/audiobookshelf_contract.rs`
- `hifimule-daemon/src/main.rs`
- `hifimule-daemon/src/playback/audio.rs`
- `hifimule-daemon/src/playback/audio/queue_edit_tests.rs`
- `hifimule-daemon/src/playback/http_source.rs`
- `hifimule-daemon/src/playback/session/album_admission_tests.rs`
- `hifimule-daemon/src/providers/audiobookshelf.rs`
- `hifimule-daemon/src/providers/jellyfin.rs`
- `hifimule-daemon/src/providers/mod.rs`
- `hifimule-daemon/src/providers/subsonic.rs`
- `hifimule-daemon/src/rpc/album_tests.rs`
- `hifimule-i18n/catalog.json`
- `hifimule-ui/src/components/AlbumPlayButton.ts`
- `hifimule-ui/src/components/MediaCard.ts`
- `hifimule-ui/src/library.ts`
- `hifimule-ui/tests/audiobookshelfBrowse.test.mjs`
- `hifimule-ui/tests/shutdownStatus.test.mjs`

### Change Log

- 2026-09-22: Added verified Audiobookshelf direct Book/Part playback, private session cleanup and media validation, scoped UI actions, redacted delivery evidence, and regression coverage. Left unverified HLS transcode and incompatible-format delivery disabled.
