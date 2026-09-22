---
baseline_commit: 2cfe297
---

# Story 17.3: Map Audiobookshelf books faithfully into the HifiMule catalog

Status: ready-for-dev

## Story

As a listener,
I want books represented as ordered albums with accurate credits,
so that I can find and play long-form audio naturally.

## Acceptance Criteria

1. **Books-only scoped catalogue.** A persisted `AudiobookshelfProvider` whose immutable role is `ProviderLibraryRole::Audiobook` can list and fetch books only from its saved upstream library. Each returned book maps to one HifiMule `Album`. Catalogue methods on a Podcast-scoped provider remain `UnsupportedCapability`; callers cannot supply another upstream library ID or escape the persisted scope. No downcast or database lookup is used to determine the role.
2. **Deterministic playable track mapping.** A book's `audioFiles` are the playable `Song` units. Sort them by validated numeric `index`, never lexical order, response order, title, or path. A one-file book maps to exactly one track. Multi-file books map one track per valid audio file. Preserve ordered chapter IDs/boundaries as typed daemon-side provider metadata associated with the book/parts; chapters do not create duplicate playable tracks or assume a false one-chapter-per-file relationship. Mapping the same DTO repeatedly produces byte-for-byte stable public IDs and ordering.
3. **Faithful titles, durations, and sparse data.** Album/track titles, part labels, durations, year, and counts come only from validated v2.36.1 fields. Missing optional metadata degrades to safe `None`/empty values rather than rejecting the book or inventing values. Duration conversion has explicit finite, non-negative, rounding, and overflow handling. Invalid/duplicate part indices are handled deterministically and covered by tests; malformed required identity is a sanitized deserialization error, not a title-derived fallback.
4. **Primary and secondary credits.** Preserve authors in source order. The first author supplies the existing primary artist-equivalent fields; additional authors and all narrators are retained as ordered typed credits. Narrators are secondary credits and never replace the primary author. Narrator names have no observed stable upstream IDs, so the mapper must not fabricate them. Missing authors/narrators produce empty credits without fake “Unknown” identities.
5. **Stable provider identity.** Retain the exact stable identity tuple `(persisted library ID, library-item ID, media ID)` in typed daemon-side provider metadata for every book and part, plus the audio-file ID and ordered chapter markers where applicable. Public album/track IDs are deterministic opaque encodings derived from stable IDs, not display title, path, ordinal, cover path, or a transient/authenticated URL. Upstream library IDs and raw identity components remain daemon-private and are not added to RPC/UI payloads, logs, fixtures containing real data, or persisted authenticated URLs.
6. **Artwork reference without credential leakage.** Preserve cover availability as an opaque provider-owned cover reference derived from stable item identity. The canonical upstream route remains `/api/items/{libraryItem}/cover` with Bearer authentication. This story does not return authenticated artwork URLs, put tokens in query strings, or make the existing unauthenticated image proxy claim Audiobookshelf support; authenticated artwork delivery is enabled only by its owning browse/UI work after a safe provider request abstraction exists.
7. **Catalogue pagination.** `list_albums` performs one scoped `GET /api/libraries/{library}/items?page={page}&limit={limit}` request per requested page, sends Bearer auth daemon-side, honors the response's authoritative `total`, and maps `offset`/`limit` safely to the provider's zero-based page contract. Empty final pages return an empty list with the authoritative total. Define and test `limit == 0`, non-page-aligned offsets, conversion overflow, 401 refresh-once, 403, 404, 429, and sanitized 5xx behavior; never silently skip or duplicate items.
8. **Bounded search semantics.** `search` issues exactly one bounded Books search request and maps book results to `SearchResult.albums`. Audiobookshelf v2.36.1 ignores `page` and applies `limit` independently to categories, so the implementation never page-loops and never claims completeness when the book category reaches the bound. Author/narrator/series/tag categories are not coerced into albums or tracks. User-visible search result shape and state remain owned by Story 17.4; existing Jellyfin/Subsonic `browse.search` wire behavior is unchanged here.
9. **Remote removal and truthful errors.** A detail 404 maps to `ProviderError::NotFound { item_type: "album", id }`, while an inaccessible library remains non-retryable `Forbidden`, a missing configured library remains `StaleConfiguration`, 429 retains valid `Retry-After`, and 5xx stays sanitized. List/search naturally omit removed items. Do not invent an incremental changes endpoint, change token, deletion event, or set `supports_changes_since`; `changes_since_with_context` remains unsupported until an observed provider contract exists.
10. **Capability and sequencing boundary.** Implement provider-side `list_albums`, `get_album`, and bounded `search` mapping, but keep `BrowseCapabilities::list_modes` empty until Story 17.4 supplies the accessible book/chapter UX and RPC contract. Keep direct playback/download, authenticated artwork proxying, progress, device sync, Autofill, podcasts, playlists, series/collections, and remote mutation unsupported for their owning stories.
11. **Fixture-backed runtime evidence and non-regression.** Add additive, synthetic/redacted, full-response v2.36.1 fixtures or equivalent provider-test bodies for catalogue pages and book detail before parsing fields that current summary fixtures omit. Keep contract-fixture validation distinct from runtime adapter tests. Cover duplicate titles, stable identity, 10-part numeric ordering, 24 chapter boundaries across parts, one-file books, multiple/missing authors and narrators, missing artwork, malformed/removed items, empty final pages, bounded search, deterministic remapping, auth refresh, error classification, and redaction. All existing Audiobookshelf connection, Jellyfin/Subsonic provider, domain serialization, RPC, and daemon tests remain green.

## Tasks / Subtasks

- [ ] **Establish the provider-neutral catalogue metadata contract** (AC: 2–6)
  - [ ] Add typed credit and provider-identity/chapter metadata in `hifimule-daemon/src/domain/models.rs` (or an equally provider-neutral domain seam). Use additive `serde(default)` and omit empty wire fields where appropriate so existing Jellyfin/Subsonic payloads remain compatible.
  - [ ] Keep raw Audiobookshelf identity metadata daemon-only with `#[serde(skip, default)]` or an equivalent non-wire carrier. Do not use an untyped `serde_json::Value` map or Audiobookshelf-specific fields directly on generic RPC DTOs.
  - [ ] Preserve the existing primary `artist_id`/`artist_name` contract; add ordered typed secondary/additional credits rather than concatenating authors/narrators into one string.
  - [ ] Define deterministic opaque album/track ID encoding and round-trip parsing helpers with delimiter-safe input handling and fixed vectors. Do not hash away fields that later playback/progress must recover unless the typed metadata retains the exact tuple.

- [ ] **Extend the scoped Audiobookshelf adapter with catalogue DTOs and mapping** (AC: 1–5, 7)
  - [ ] In `hifimule-daemon/src/providers/audiobookshelf.rs`, add private DTOs for the validated catalogue page, book summary/detail, metadata, author, narrator, audio-file, and chapter fields. Reject a non-Audiobook role before network I/O.
  - [ ] Factor the existing authenticated request path so protected GETs share Bearer injection, exactly-one refresh after 401, status classification, response-size/time bounds, and sanitized deserialization errors. Preserve the Story 17.2 session mutex and secret-redaction invariants.
  - [ ] Implement `list_albums` from the selected library only, with checked offset/page conversion and authoritative totals. Do not forward the unvalidated `letter` filter in a way that falsifies paging; reject a non-empty unsupported filter or document a provider-neutral no-filter call path for Story 17.4.
  - [ ] Implement `get_album` through the scoped item-detail endpoint and map `audioFiles` by numeric index. Attach ordered chapter boundaries as metadata without manufacturing a 1:1 chapter/file relation.
  - [ ] Keep `get_song` unsupported unless it can be implemented entirely from the same validated detail contract without enabling playback. `download_url`, `resolve_playback`, and `cover_art_url` remain unsupported.

- [ ] **Implement bounded Books search without prematurely changing UX** (AC: 8, 10)
  - [ ] Issue one request to `/api/libraries/{library}/search` with a fixed documented maximum and map only the `book` category into albums.
  - [ ] Represent “possibly truncated” internally/testably when the category reaches the bound; never loop `page`, merge repeated first pages, or expose a false complete-total claim.
  - [ ] Leave `hifimule-daemon/src/rpc.rs` search and image-proxy response shapes unchanged in this story unless an additive provider-neutral field is strictly required. Story 17.4 owns book-search RPC/UI presentation.

- [ ] **Use normal missing-item conventions without inventing delta support** (AC: 9)
  - [ ] Distinguish item-detail 404 (`NotFound`) from scoped-library 404 (`StaleConfiguration`) and retain 403/429/5xx behavior from Story 17.2.
  - [ ] Keep `supports_changes_since == false` and `changes_since_with_context` unsupported. Do not infer deletions from a partial page or add a polling cache inside the provider.

- [ ] **Create runtime-quality fixtures and regression tests** (AC: 1–11)
  - [ ] Add only synthetic, redacted fixture content grounded in the pinned v2.36.1 observation/source. Do not rewrite a summary fixture to imply fields were observed when they were not; update the manifest/README provenance honestly for any additions.
  - [ ] Add co-located `mockito` adapter tests for exact method/path/query/Bearer shape, library scoping, refresh once, page totals/final page, bounded search, 401/403/404/429/500, and secret/error redaction.
  - [ ] Add pure mapper tests for duplicate-title identity, deterministic IDs, numeric indices `1..10`, 10 files versus 24 chapters, chapter boundaries crossing part offsets, one file/one track, missing credits/art, multiple ordered credits, malformed indices/durations, and repeatability.
  - [ ] Update all affected domain literals/mocks exhaustively and assert legacy serialized Jellyfin/Subsonic shapes remain compatible.
  - [ ] Run `rtk cargo fmt --check`, targeted Audiobookshelf/provider tests, `rtk cargo test -p hifimule-daemon`, `rtk cargo clippy -p hifimule-daemon --all-targets -- -D warnings` where the repository baseline permits, and `rtk git diff --check`. Report any environment-limited check truthfully.

## Dev Notes

### Scope and source precedence

- The final Epic 17 story, PRD FR83, architecture amendment, and completed Stories 17.1–17.2 are authoritative. The approved change proposal is supporting history where its earlier Story 17.2 wording differs.
- The controlled v2.36.1 contract and redacted fixtures outrank current upstream `master` documentation when shapes differ. Upstream docs are useful for orientation, not permission to infer fields or widen supported versions.
- This story creates the faithful provider-side book catalogue. Story 17.4 owns visible browsing/search states and authenticated artwork presentation; 17.5 playback; 17.6 progress; 17.7 podcasts; 17.8 sync/Autofill; 17.9 series/collections.

### Current state, required change, and preservation rules

| File / seam | Current state | Story 17.3 change | Must preserve |
| --- | --- | --- | --- |
| `hifimule-daemon/src/providers/audiobookshelf.rs` | Auth, discovery, immutable scope, refresh, and error mapping exist; every catalogue/media method returns `UnsupportedCapability`; browse modes are empty. | Add Books-scoped catalogue DTOs, authenticated list/detail/search requests, deterministic mapper, and item-level 404 semantics. | Story 17.2 auth/session/redaction, exact selected-library scope, Podcast rejection, and all later capabilities remaining off. |
| `hifimule-daemon/src/domain/models.rs` | `Album`/`Song` have one primary artist and no general secondary credits or provider identity/chapter metadata. | Add the smallest typed provider-neutral credit and daemon-only identity metadata needed for faithful mapping. | Existing wire names, legacy deserialization, equality semantics, and Jellyfin/Subsonic behavior. |
| `hifimule-daemon/src/providers/mod.rs` | `MediaProvider` already exposes list/get/search; `ProviderError::NotFound` and role access exist. | Prefer existing methods; add only provider-neutral shared types if domain ownership requires it. | No Audiobookshelf-specific trait/downcast and no false `supports_changes_since`. |
| `hifimule-daemon/src/rpc.rs` | Album browse handlers exist; `browse.search` returns only songs; image proxy fetches a URL without provider Bearer headers. | Normally no behavior change in 17.3; explicitly defer book-search UI shape and authenticated cover delivery. | Existing Jellyfin/Subsonic search/image contracts and no token-bearing URLs. |
| `hifimule-daemon/tests/audiobookshelf_contract.rs` + fixture corpus | Validates observation summaries and contract invariants, not production DTO parsing. Some summaries omit titles/full response fields. | Add honest full-response evidence/bodies and separate runtime adapter/mapper tests. | Versioned/redacted corpus, status truthfulness, and no real identities/URLs/secrets. |

### Mapping contract

- **Album identity:** exact `(library ID, library-item ID, media ID)` in daemon metadata; public opaque ID must be deterministic and collision-safe.
- **Track granularity:** `audioFiles`, ordered by numeric index. The verified 10-file/24-chapter fixture proves files and chapters are not interchangeable. A single file is one track. Chapters remain ordered logical markers with whole-book start/end offsets for later seek/progress translation.
- **Credits:** first author remains primary artist-equivalent; all authors and narrators remain ordered credits. Never promote narrator to primary artist because the author is absent; use an empty primary field instead.
- **Artwork:** retain an opaque reference to the library item. Never persist `coverPath` as identity or expose an authenticated URL.
- **Search:** v2.36.1 ignores `page`; one bounded request only. A hit count equal to the limit is “possibly truncated,” not a complete total.
- **Removal:** item 404 is `NotFound`; library 404 is stale configuration. A partial/empty page is not proof that an arbitrary item was deleted.

### Architecture and security guardrails

- Reuse workspace `reqwest`, `serde`, Tokio, `async-trait`, `thiserror`, and dev `mockito`; add no Audiobookshelf SDK or HTTP-test dependency.
- All provider traffic stays behind `MediaProvider`. Access/refresh tokens, Bearer headers, raw responses, authenticated URLs, upstream library IDs, and internal identity tuples remain daemon-side and sanitized from errors/logs.
- Reuse the existing local-server-ID provider cache and portable-server-ID routing. Do not create a second client/session/cache or persist access/refresh JWTs.
- Do not broaden the feature by enabling browse modes merely because list/get/search now work. Capability publication is a product/API promise and belongs to Story 17.4.

### Previous Story Intelligence

- Story 17.2 created the adapter, library-scoped identity/configuration, two-stage setup, lazy reconstruction, scoped re-authentication, and role contract. Extend those seams; do not replace them.
- Review fixes that remain binding: exact `/status` detection, concurrent scoped-upsert isolation, no synthetic server version, stored-library 403 versus 404 validation, HTTP-date `Retry-After`, compensating re-auth/cache publication, one-use setup handling, per-state pending maps, explicit shutdown clearing, and late-probe UI race protection.
- The post-17.2 connection investigation found that Story 17.1 fixtures originally mirrored an incorrect token shape. Runtime adapter tests must therefore parse production DTOs from independently grounded evidence, not merely assert generic JSON invariants.
- Story 17.2 finished with 1,062 daemon tests (6 ignored), five contract tests, focused UI tests, UI production build, and daemon check passing. Treat those as regression baselines, not as certification of catalogue behavior.

### Git Intelligence Summary

- `2cfe297` (`Review 17.2`) hardened scoped persistence, library validation, refresh/rate-limit behavior, and UI/setup concurrency. Preserve these fixes while factoring authenticated catalogue requests.
- `c8181c8` (`Dev 17.2`) introduced the production adapter and its existing dependency set; keep catalogue work concentrated in that adapter plus provider-neutral domain additions.
- `a53cb96` (`Review 17.1`) established the evidence rule: parse every fixture, assert complete sequences/invariants, distinguish contract tests from runtime provider tests, and never promote unobserved behavior to verified.

### Latest Technical Information

- As checked on 2026-09-22, the official releases page and container registry still identify Audiobookshelf v2.36.1 as latest. HifiMule nevertheless supports only the pinned observed contract; a future release requires additive validation before broadening claims. [Official releases](https://github.com/advplyr/audiobookshelf/releases)
- The upstream generated OpenAPI declares Bearer security and documents `/api/libraries/{id}/items`, but the repository's controlled fixtures remain authoritative for runtime response semantics and the observed non-pageable search behavior. [Generated OpenAPI](https://github.com/advplyr/audiobookshelf/blob/master/docs/openapi.json)
- Upstream library documentation confirms `mediaType` distinguishes `book` and `podcast`; keep the existing persisted role gate rather than inspecting result shapes. [Library schema](https://github.com/advplyr/audiobookshelf/blob/master/docs/objects/Library.yaml)

### Project Structure Notes

- No new production module is required. Keep Audiobookshelf DTOs/mapping private to `providers/audiobookshelf.rs`; generic catalogue types belong in `domain/models.rs`.
- Prefer additive fixtures under `hifimule-daemon/tests/fixtures/audiobookshelf/2.36.1/` only when their evidence status/provenance is explicit. Co-located provider unit tests may use synthetic mock bodies when they do not assert new upstream facts.
- No UI or i18n change is expected. If implementation needs user-visible book/chapter labels or new RPC search presentation, stop and move that work to Story 17.4 rather than silently expanding this story.

### References

- [Source: `_bmad-output/planning-artifacts/epics.md` — Epic 17 and Story 17.3]
- [Source: `_bmad-output/planning-artifacts/prd.md` — FR83 and Audiobookshelf non-goals]
- [Source: `_bmad-output/planning-artifacts/architecture.md` — Epic 17 Audiobookshelf Architecture Amendment]
- [Source: `_bmad-output/planning-artifacts/ux-design-specification.md` — §8 Audiobookshelf integration]
- [Source: `_bmad-output/planning-artifacts/sprint-change-proposal-2026-09-20-audiobookshelf.md` — Story 17.3, architecture, UX, fixture matrix, sequencing]
- [Source: `_bmad-output/implementation-artifacts/17-2-connect-audiobookshelf-libraries-as-independent-servers.md` — completed prerequisite and review findings]
- [Source: `docs/audiobookshelf-integration-contract.md` — v2.36.1 catalogue/search/identity/failure contract]
- [Source: `hifimule-daemon/src/providers/audiobookshelf.rs` — existing scoped adapter and unsupported catalogue methods]
- [Source: `hifimule-daemon/src/domain/models.rs` — current Album/Song/SearchResult contracts]
- [Source: `hifimule-daemon/src/providers/mod.rs` — MediaProvider, capabilities, role, errors]
- [Source: `hifimule-daemon/tests/audiobookshelf_contract.rs` and `hifimule-daemon/tests/fixtures/audiobookshelf/2.36.1/` — existing evidence and invariant tests]

## Dev Agent Record

### Agent Model Used

GPT-5 Codex

### Debug Log References

### Completion Notes List

- Ultimate context engine analysis completed - comprehensive developer guide created.
- Story preparation analyzed the full Epic 17/PRD/architecture/UX scope, completed Story 17.2 and review fixes, current provider/domain/RPC code, v2.36.1 contract fixtures, recent commits, and current official upstream release/API sources.

### File List

- `_bmad-output/implementation-artifacts/17-3-map-audiobookshelf-books-faithfully-into-the-hifimule-catalog.md`

