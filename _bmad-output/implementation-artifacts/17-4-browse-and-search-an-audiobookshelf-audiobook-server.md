---
baseline_commit: 6025102
---

# Story 17.4: Browse and search an Audiobookshelf audiobook server

Status: ready-for-dev

## Story

As a listener,
I want to browse and search my audiobook library,
so that I can choose books without confusing them with music or podcasts.

## Acceptance Criteria

1. **Books-only capability publication.** A configured `AudiobookshelfProvider` with persisted `ProviderLibraryRole::Audiobook` publishes only the existing Albums browse capability after its selected-library scope has been validated. A Podcast-scoped provider keeps this capability unavailable. No UI permits a caller to choose another Audiobookshelf library, and no podcast, series, collection, folder, or playlist view is introduced.
2. **Faithful, accessible book browse.** The existing capability-driven Albums browse flow presents Audiobookshelf albums as **Books**, not music albums. A book shows its title, primary author, ordered additional-author/narrator hierarchy, available artwork, and truthful part/chapter information. Playable units remain the ordered audio files established by Story 17.3; chapter markers are descriptive metadata, never assumed to be one file per chapter. A one-file book is labeled truthfully.
3. **Compatible search contract and presentation.** `browse.search` exposes Audiobookshelf book/album hits through an additive, provider-neutral response shape while preserving the existing Jellyfin/Subsonic tracks-only response behavior. The UI renders Audiobookshelf book results as books, retains author/narrator hierarchy, and does not treat author, narrator, series, tag, podcast, or collection categories as albums. The provider remains one bounded Books search request; a result count at the bound is presented without claiming a complete result set.
4. **Truthful UI states and accessibility.** Browse and search use the existing loading, empty, unavailable, error, keyboard, focus, pagination, cache, and virtualization conventions. They clearly distinguish: no books/no matches; loading; a temporarily unavailable provider; stale/missing configured library; access denied; and retryable rate-limit/server failures. Accessible names and descriptions use Book and Part/Chapter vocabulary and never leak tokens, endpoint details, raw upstream IDs, or provider response bodies.
5. **Safe authenticated artwork.** Audiobookshelf artwork is retrieved only through a daemon-owned authenticated provider request/proxy path. The UI receives a local safe image reference, never an upstream authenticated URL, `Authorization` header, token, library ID, or raw provider identity. Reuse scoped session/refresh-once, size/time limit, error classification, and redaction behavior; retain the current Jellyfin/Subsonic artwork behavior unchanged.
6. **No premature listening or sync behavior.** Audiobookshelf book cards, rows, and context controls do not advertise or invoke direct play, preview, queue, basket-add, download, progress, sync, Autofill, or remote mutation before their owning stories. Existing music-provider controls continue to work unchanged. Story 17.5 owns direct playback; 17.6 progress; 17.7 podcasts; 17.8 sync/Autofill; 17.9 series/collections.
7. **Regression evidence.** Provider, RPC, and UI tests cover the role/capability gate; selected-library scope; album/book and part/chapter labels; author/narrator ordering; empty/loading/error/stale states; bounded search and additive response compatibility; authenticated cover success, refresh-once, errors, and redaction; disabled premature actions; and existing Jellyfin/Subsonic browse, search, and image behavior.

## Tasks / Subtasks

- [ ] **Publish the narrowly scoped browse capability** (AC: 1, 6)
  - [ ] In `hifimule-daemon/src/providers/audiobookshelf.rs`, enable only `BrowseMode::Albums` for the persisted Audiobook role; retain Podcast rejection and selected-library validation before network I/O.
  - [ ] Do not add modes, playlists, changes-since, download, playback, progress, sync, or mutation support.

- [ ] **Add safe cover delivery behind the provider boundary** (AC: 5, 7)
  - [ ] Add the smallest provider-neutral authenticated cover-fetch seam required by the existing daemon image path; do not expose token-bearing URLs or make UI HTTP requests to Audiobookshelf.
  - [ ] Reuse the adapter's existing session mutex, Bearer injection, one-refresh-after-401, response bounds, status mapping, path encoding, and secret redaction.
  - [ ] Keep the legacy Jellyfin/Subsonic route and output behavior compatible; only add routing/identification necessary for a selected Audiobookshelf server.

- [ ] **Extend RPC data without breaking current clients** (AC: 2–4, 7)
  - [ ] Update `hifimule-daemon/src/rpc.rs` so `browse.search` can add albums/book hits while preserving existing `{ tracks }` semantics for current providers and callers.
  - [ ] Carry only safe, public presentation data. Keep `ProviderItemMetadata`, raw identities, library IDs, cover references, headers, and credentials daemon-private.
  - [ ] Map `NotFound`, `StaleConfiguration`, `Forbidden`, rate-limit, and sanitized server errors through the established RPC error convention; do not collapse stale configuration into an empty list.

- [ ] **Make the capability-driven UI domain-appropriate** (AC: 2–4, 6)
  - [ ] Update `hifimule-ui/src/rpc.ts` with additive browse/search DTOs and wrappers.
  - [ ] Update `hifimule-ui/src/library.ts` and the smallest relevant card/row components to render an Audiobook server as Books and ordered playable parts as Part/Chapter labels. Preserve existing music terminology, card types, pagination, selection, focus, and virtualization for every non-Audiobookshelf server.
  - [ ] Render author-first metadata and ordered additional author/narrator credits without fabricating identities or promoting narrators when an author is absent.
  - [ ] Preserve existing status-state components and localized accessible labels. Add catalog keys/types for every new visible string across all locales; regenerate the typed catalog using the project convention.
  - [ ] Gate/hide Audiobookshelf playback, preview, queue, and basket controls. In particular, avoid generic `MediaCard` album-play creation and legacy Jellyfin basket/size calls for ABS items.

- [ ] **Prove the contract and regressions** (AC: 1–7)
  - [ ] Add co-located Audiobookshelf `mockito` tests for capability/role, safe cover fetch/proxy, exact authentication behavior, refresh once, 403/404/429/5xx classification, bounded body handling, and secret redaction.
  - [ ] Add Rust RPC tests alongside the existing album/browse tests for additive album search results and legacy tracks-only compatibility.
  - [ ] Add Node UI tests using the established `hifimule-ui/tests/*.test.mjs` convention for vocabulary/hierarchy, state presentation, focus/accessibility, no-premature-action gating, and generic-provider regressions.
  - [ ] Run `rtk cargo fmt --check`, focused provider/RPC/contract tests, `rtk cargo test -p hifimule-daemon`, applicable UI tests, UI type-check/build, and `rtk git diff --check`. Run strict Clippy where practical and report the existing baseline blocker truthfully if it remains.

## Dev Notes

### Scope and source precedence

- Treat the completed Stories 17.1–17.3, final Epic 17, PRD FR84, architecture amendment, and controlled v2.36.1 fixture contract as authoritative. Upstream `master` documentation is orientation only and cannot widen the supported contract.
- Story 17.3 already implements Books-scoped `list_albums`, `get_album`, and a one-request bounded search. This story must expose and present those seams; do not reimplement catalogue mapping.
- The Books and Podcasts domains remain permanently distinct even for the same endpoint. No series/collection playlist, collection writer, or Audiobookshelf-only filter belongs here.

### Current state, required change, and preservation rules

| File / seam | Current state | Story 17.4 change | Must preserve |
| --- | --- | --- | --- |
| `hifimule-daemon/src/providers/audiobookshelf.rs` | Scoped Books catalogue/search mapper and private identity/credit/chapter metadata exist; browse capabilities are empty; `cover_art_url` is unsupported. | Publish only Albums for Audiobook role and provide safe daemon-owned cover retrieval. | Persisted role/library scope, one refresh, redaction, response bounds, opaque IDs, Podcast rejection, no later capabilities. |
| `hifimule-daemon/src/rpc.rs` | Generic album list/detail exist; `browse.search` serializes only tracks; legacy image proxy fetches a provider URL without ABS auth. | Add provider-neutral albums to search and route authenticated artwork safely. | Existing error vocabulary and Jellyfin/Subsonic search/image wire behavior. |
| `hifimule-ui/src/rpc.ts` | Browse DTOs/wrappers model generic albums/tracks; search expects `{ tracks }`. | Add optional album/book result presentation fields without breaking callers. | No daemon-private values in TypeScript; existing RPC calls remain valid. |
| `hifimule-ui/src/library.ts` + card/row components | Capability-driven album UI uses music semantics, generic `Audio` rows, primary artist subtitle, and generic controls. | Select server-role-aware book/part terminology and credit hierarchy; render existing states. | Music UI, caching, virtualized list, focus, selection, and unavailable-state behavior. |
| image helper + `hifimule-ui/src-tauri/src/lib.rs` | Existing local `/jellyfin/image/` proxy path is hardcoded for legacy artwork. | Add only the compatible safe route/identifier plumbing needed for ABS. | Never emit authenticated URLs/tokens; do not regress legacy images. |

### Binding data and UX rules

- A **Book** is the mapped `Album`. Ordered `audioFiles` are the playable parts/tracks. `ChapterMarker`s are logical timing markers and can cross audio-file boundaries; present them as context, never as fabricated tracks.
- The primary author remains the current primary artist-equivalent. Additional authors and narrators are ordered typed credits in daemon-only metadata today; expose only the minimum safe public presentation data necessary. Missing credits stay absent—never display “Unknown” or invent IDs.
- Cover availability is derived from the validated provider-owned cover reference. The canonical upstream route is authenticated and must be fetched server-side.
- Search returns Books category hits only. It is bounded; a category count at the request limit is potentially truncated. Do not page-loop, merge repeated pages, or assert a total/completeness not supplied by the contract.
- Empty list is not a substitute for a stale configured library. Detail 404 is missing item; selected-library 404 is stale configuration; 403 is inaccessible; 429 preserves valid `Retry-After`; 5xx is sanitized.

### Architecture and security guardrails

- All provider traffic stays behind `MediaProvider`; reuse workspace `reqwest`, `serde`, Tokio, `async-trait`, `thiserror`, and `mockito`. Add no Audiobookshelf SDK or new HTTP test dependency.
- Credentials, Bearer headers, access/refresh tokens, upstream endpoint/library IDs, raw identity tuples, cover references, raw responses, and authenticated URLs never cross RPC/UI/log boundaries.
- Reuse the existing local-server-ID cache, portable-server-ID routing, configured provider reconstruction, and error mapping. Do not create another session, provider cache, or credential persistence path.
- Keep wire changes additive and serde-compatible. Existing Jellyfin/Subsonic serialized shapes and music behavior are regression contracts.

### Previous Story Intelligence

- Story 17.3 review fixes are binding: validate scope before zero-limit return; encode path IDs; bounded streamed responses; numeric part order; truthful cover/count/duration mapping; retained private identity/credits on list and search; only reject nonempty unsupported letter filters; truncated-search detection before malformed-result filtering.
- Story 17.2 supplies two-stage setup, immutable scope/configuration, lazy rebuild, scoped re-auth, and secret-safe error handling. Do not replace its session/cache model.
- The contract fixture history includes an earlier incorrect flattened token shape. Runtime tests must parse validated production DTOs, not generic fixture JSON assumptions.

### Latest technical information

- As checked on 2026-09-22, Audiobookshelf v2.36.1 remains the latest release. HifiMule supports only its pinned observed contract; validate before accepting a later shape. [Audiobookshelf releases](https://github.com/advplyr/audiobookshelf/releases)
- The upstream OpenAPI describes Bearer security and `/api/libraries/{id}/items`, but its current master schema cannot override HifiMule's fixture-backed v2.36.1 behavior. [Audiobookshelf OpenAPI](https://github.com/advplyr/audiobookshelf/blob/master/docs/openapi.json)
- The library model exposes `mediaType` as `book` or `podcast`; use the persisted role gate rather than inspecting browse result shapes. [Library schema](https://github.com/advplyr/audiobookshelf/blob/master/docs/objects/Library.yaml)

### Project Structure Notes

- Keep provider DTOs/auth mapping private to `hifimule-daemon/src/providers/audiobookshelf.rs`; generic additive wire contracts belong in the existing daemon domain/RPC seams.
- Keep fixture/runtime evidence under the existing `hifimule-daemon/tests/fixtures/audiobookshelf/2.36.1/` and co-located adapter tests. Fixture provenance must remain synthetic/redacted and honest.
- No new top-level UI feature/module is necessary. Extend the capability-driven library flow and existing i18n catalog rather than building a parallel audiobook browser.

### References

- [Source: `_bmad-output/planning-artifacts/epics.md` — Epic 17 and Story 17.4]
- [Source: `_bmad-output/planning-artifacts/prd.md` — FR84 and Audiobookshelf non-goals]
- [Source: `_bmad-output/planning-artifacts/architecture.md` — Epic 17 Audiobookshelf Architecture Amendment]
- [Source: `_bmad-output/implementation-artifacts/17-3-map-audiobookshelf-books-faithfully-into-the-hifimule-catalog.md` — completed prerequisite, review fixes, and boundaries]
- [Source: `hifimule-daemon/src/providers/audiobookshelf.rs` — current adapter]
- [Source: `hifimule-daemon/src/providers/mod.rs` — provider and browse capability contracts]
- [Source: `hifimule-daemon/src/domain/models.rs` — album/song/credit/private metadata contracts]
- [Source: `hifimule-daemon/src/rpc.rs` and `hifimule-daemon/src/rpc/album_tests.rs` — browse, search, image, and test contracts]
- [Source: `hifimule-ui/src/rpc.ts`, `hifimule-ui/src/library.ts`, `hifimule-ui/src/components/MediaCard.ts`, and `hifimule-ui/src-tauri/src/lib.rs` — current UI/proxy seams]

## Dev Agent Record

### Agent Model Used

GPT-5 Codex

### Debug Log References

- 2026-09-22: Ultimate context engine analysis completed — full Epic 17/PRD/architecture, Story 17.3 implementation and review learning, current provider/RPC/UI seams, Git history, fixtures, and current Audiobookshelf release/API documentation analyzed.

### Completion Notes List

- Comprehensive developer guide created; scope explicitly gates direct playback, progress, podcasts, sync/Autofill, and series/collection work to their owning stories.

### File List

- `_bmad-output/implementation-artifacts/17-4-browse-and-search-an-audiobookshelf-audiobook-server.md`
