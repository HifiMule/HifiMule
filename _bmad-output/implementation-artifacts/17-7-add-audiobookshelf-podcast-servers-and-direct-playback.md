# Story 17.7: Add Audiobookshelf podcast servers and direct playback

Status: ready-for-dev

## Story

As a listener,
I want to select a Podcasts library and browse and play shows and episodes,
so that podcasts work without being forced into audiobook or album semantics.

## Acceptance Criteria

1. **Independent server scope.** A selected Audiobookshelf Podcasts library remains a separate HifiMule server with its own persisted library ID, immutable podcast role, stable server identity, and budget. Multiple libraries at one endpoint coexist. Podcast operations never read a Books library and book operations never read a Podcasts library, including after restart, upsert, removal, and a changed upstream library role.
2. **Distinct catalog.** The provider maps a podcast library item to a show and each episode to an episode, with explicit role/type in provider-neutral daemon and RPC contracts. Stable identity is `(library ID, library-item ID, media ID, episode ID)`. Never use title, ordinal, path, cover URL, or playback URL as identity. Episode metadata, duration, publication date and show artwork appear when available. Removed or replaced items are rejected as stale. Shows and episodes do not enter album, book-part, artist, playlist, music selection, or sync paths through an alias.
3. **Browse and search.** A podcast server exposes show listing, show detail/episodes, and show/episode search through the existing browse/search RPC and capability routing, with bounded pagination and truthful truncation. Podcast search follows the verified v2.36.1 behavior: `page` is ignored and `limit` caps categories. Empty, loading, stale, unavailable, and permission states are accessible and localized. The UI uses Show and Episode labels and episode-appropriate metadata, keyboard/focus behavior, and play actions; it never presents Book/Part or Album/Track labels for podcasts.
4. **Direct episode playback.** An episode starts through the existing serialized playback owner and source-server routing. The provider uses the episode-scoped play endpoint and proves the returned session belongs to the selected library, show, media, and episode before admitting audio. Reuse the book direct-play guards for supported server version, direct MP3/AAC representation, authenticated range read, redirect/origin/path restrictions, bounded response, one-time credential refresh, session close, generation fencing, seek/output safety, and recoverable errors. Authenticated URLs, tokens, raw IDs and upstream session IDs stay daemon-side. Incompatible or unavailable episodes do not start a different episode or fall through to a book/music source.
5. **Regression and scope.** Existing audiobook mapping, player-owned whole-book progress, Jellyfin/Subsonic browse and playback, and music queue behavior remain intact. This story does not add podcast progress read/write, device transfer, sync reconciliation, Autofill retention, series/collection playlists, or remote collection editing. Offline provider, RPC, UI and playback tests cover role isolation, pagination/search limits, stable episode identity, playable and incompatible media, cleanup, race conditions and redaction. Distinguish offline evidence from installed-app or controlled-server evidence.

## Tasks / Subtasks

- [ ] **Add a distinct podcast domain and browse contract** (AC: 1–3)
  - [ ] Define show and episode DTOs and typed browse/search capabilities in `domain/models.rs`, `providers/mod.rs`, and the browse RPC without reusing `Album`, `Song`, or book-oriented UI terminology as a public podcast model. Preserve existing provider defaults and wire compatibility.
  - [ ] Route selected podcast servers through role-scoped listing, show detail and episode lookup in `providers/audiobookshelf.rs`. Validate library, item, media and episode IDs on every detail request; use opaque public IDs and daemon-private upstream metadata. Handle empty and removed episodes and deterministic display order without treating order as identity.
  - [ ] Add bounded podcast show and episode search. Preserve the upstream `possiblyTruncated` signal when a category reaches the request limit; never page the search endpoint by changing `page`.
- [ ] **Build podcast presentation** (AC: 2–3)
  - [ ] Extend `hifimule-ui/src/rpc.ts`, `library.ts` and relevant browse components with Show/Episode rows, show detail, accessible states, and direct play controls. Use localization in `hifimule-i18n/catalog.json`. Keep podcast results out of album/book search, favorites, basket and device sync until their own contracts exist.
- [ ] **Admit direct episode playback safely** (AC: 4–5)
  - [ ] Extend `MediaProvider` and the existing `playback.playTrack`/serialized session path only as needed for typed episode admission. Use `POST /api/items/{item}/play/{episode}` and verify session scope and requested episode before handing a byte stream to the decoder.
  - [ ] Reuse the established Audiobookshelf direct media verification, private auth refresh, session cleanup, and failure classification. Explicitly reject HLS/transcode and unverified formats; do not call book timing/progress for podcast occurrences.
- [ ] **Verify isolation and regressions** (AC: 1–5)
  - [ ] Add synthetic fixture-backed provider tests for list/detail/search, malformed or cross-role IDs, changed media/episode, 401/403/404/429/5xx, truncation and sanitized errors; playback tests for valid episode sessions, mismatched response, media-read 404, cleanup, replacement and stop races.
  - [ ] Add RPC/UI tests for show and episode presentation, accessible states and isolation from Books/Music. Run relevant daemon/UI tests, type-check/build, formatting/lint and diff checks; report environment blockers separately.

## Dev Notes

### Binding implementation context

- Story 17.2 already persists independent Audiobookshelf library/server role and budget. `rpc.rs` setup selects `ProviderLibraryRole::Podcast`; do not add a second server creation path or change the immutable role rules.
- `hifimule-daemon/src/providers/audiobookshelf.rs` currently rejects podcast access in `audiobook_library_id()`. `catalogue_page`, `catalogue_book`, `search_books`, `get_song`, `resolve_playback`, `fetch_cover_art`, and `audiobookshelf_browse_capabilities` are book-specific. Add a parallel, role-checked podcast branch. Preserve their book behavior and the private protected GET/POST, bounded JSON, opaque ID, version, URL/range, and cleanup helpers.
- `hifimule-daemon/src/domain/models.rs` currently has `Album`, `Song`, and `AlbumWithTracks`, but no Show/Episode model. `MediaProvider` in `providers/mod.rs` exposes album/song browse and `resolve_playback(song_id)`; `BrowseMode` has no podcast mode. Introduce only the provider-neutral domain and RPC changes required to express podcasts faithfully. Audit browse routing and player admission before changing signatures; other providers must retain their default behavior.
- `hifimule-ui/src/library.ts` branches on `isBookLibrary` within album and track views, and `hifimule-ui/src/rpc.ts` types only album/track browse. Add a role-specific podcast view and typed RPC, preserving Book/Part and music behavior. Existing playback controls and queue state remain the player surface.
- The pinned controlled v2.36.1 contract records podcast library pagination, `podcast` and `episodes` search categories, show detail with `episodes`, an episode-scoped play session, and episode progress. It does not prove every episode format is directly decodable. Gate each episode with the current decoder/range checks. The upstream API reference documents `POST /api/items/{ID}/play/{EpisodeID}`, but warns that its documentation is out of date; the repository's controlled v2.36.1 contract is the binding version evidence.
- Story 17.6 introduced whole-book progress on the playback owner. Podcast admission must bypass book timing and reporting. Its current story file records a fast replacement reporting caveat; preserve the existing fence/cleanup behavior and do not couple podcast playback to that unresolved book edge case.

### Previous story and repository intelligence

- 17.6 persisted a player occurrence binding, added daemon-private book progress, and kept raw provider IDs out of UI/RPC/logs. Its offline checks passed, while installed-app playback was not exercised. Recent commits `3b862ac` and `3ad613a` are the review and implementation for that story; inspect their playback seams before editing.
- 17.5 established Audiobookshelf direct MP3/AAC byte-stream verification and cleanup. Transcoded HLS is unsupported by the current decoder. Reuse those checks for episodes instead of treating a successful upstream play response as proof of compatible delivery.

### References

- [Source: `_bmad-output/planning-artifacts/epics.md` — Epic 17, Story 17.7]
- [Source: `_bmad-output/planning-artifacts/prd.md` — FR82, FR86]
- [Source: `_bmad-output/planning-artifacts/architecture.md` — Epic 17 amendment]
- [Source: `_bmad-output/planning-artifacts/ux-design-specification.md` — §8]
- [Source: `docs/audiobookshelf-integration-contract.md` — Record 1 and Record 2]
- [Source: `_bmad-output/implementation-artifacts/17-6-preserve-audiobookshelf-listening-continuity-safely.md` — prior story]
- [Source: `hifimule-daemon/src/providers/audiobookshelf.rs`, `hifimule-daemon/src/providers/mod.rs`, `hifimule-daemon/src/domain/models.rs`, `hifimule-ui/src/library.ts`, `hifimule-ui/src/rpc.ts` — current implementation]
- [Source: `https://api.audiobookshelf.org/` — upstream API reference, marked outdated by its maintainers]

## Dev Agent Record

### Agent Model Used

GPT-6 Codex

### Completion Notes List

- Ultimate context engine analysis completed - comprehensive developer guide created.

### File List

