---
title: 'Library search across media providers'
type: 'feature'
created: '2026-09-26'
status: 'done'
baseline_commit: 'ed2669494b33cd4bd81343d2b3d566860fa627db'
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Audiobookshelf audiobook and podcast searches fail on matching results. Jellyfin and Subsonic audio libraries have no visible way to search for artists, albums, or tracks.

**Approach:** Accept Audiobookshelf's actual search response shapes, and add one dedicated Search tab to music libraries with grouped artist, album, and track results. Reuse the existing browse search RPC and provider abstraction.

## Boundaries & Constraints

**Always:** Preserve audiobook and podcast search in their existing views. Show music search only for Jellyfin and Subsonic-compatible audio libraries. Keep searches bounded, distinguish empty results from errors, and make results navigate or play through existing library actions. Avoid logging sensitive response bodies.

**Ask First:** Any change that requires new server permissions, a breaking RPC contract, or a different search experience from the dedicated tab.

**Never:** Add a search box to each music tab, replace the media provider abstraction, or perform an unbounded whole-library client scan.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Audiobook match | Audiobookshelf search returns a matching book | Book appears in results | Valid wrapped or direct search hits decode |
| Podcast match | Search returns matching shows or episodes | Matches appear in the podcast view | Valid nonempty search hits decode |
| Music match | Jellyfin or Subsonic search query | Artists, albums, and tracks appear in grouped sections | Preserve navigation and playback actions |
| Empty query or no matches | Empty input or valid search with no hits | No unbounded request; clear empty state | No generic library load error |
| Provider failure | Search request fails | Existing localized error state appears | Keep the library usable for another query |

</frozen-after-approval>

## Code Map

- `hifimule-daemon/src/providers/audiobookshelf.rs` — search DTOs, mapping, and provider tests.
- `hifimule-daemon/src/providers/jellyfin.rs` and `hifimule-daemon/src/api.rs` — Jellyfin search implementation and API request.
- `hifimule-daemon/src/providers/subsonic.rs` — existing search3 implementation to reuse and verify.
- `hifimule-daemon/src/rpc.rs` — browse.search response currently omits artists.
- `hifimule-ui/src/rpc.ts` — browse modes and search response typing.
- `hifimule-ui/src/library.ts` and `hifimule-ui/src/styles.css` — mode bar, search view, result actions and layout.
- `hifimule-i18n/catalog.json` — localized search labels and states.

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-daemon/src/providers/audiobookshelf.rs` — decode representative nonempty book, podcast, and episode search hits; cover response variants with regression tests.
- [x] `hifimule-daemon/src/providers/jellyfin.rs`, `hifimule-daemon/src/api.rs` — return matching artists, albums, and tracks through provider search with bounded requests; test mapping.
- [x] `hifimule-daemon/src/rpc.rs`, `hifimule-ui/src/rpc.ts` — include artists in browse.search while retaining current book and podcast flows.
- [x] `hifimule-ui/src/library.ts`, `hifimule-ui/src/styles.css`, `hifimule-i18n/catalog.json` — add music Search tab, grouped result rendering, empty/error states, and existing item actions.

**Acceptance Criteria:**
- Given an Audiobookshelf library with matching books, when a user searches, then matching books display without a deserialization error.
- Given a podcast library with matching shows or episodes, when a user searches, then matching items display without the unavailable state.
- Given a Jellyfin or Subsonic audio library, when a user opens Search and submits a query, then matching artists, albums, and tracks are shown in one tab and existing result actions work.
- Given a search with no matches or an empty query, when the user searches or clears it, then the UI remains usable and does not issue an unbounded request.

## Spec Change Log

## Verification

**Commands:**
- `rtk cargo test -p hifimule-daemon` — expected: affected provider and RPC tests pass.
- `rtk npm run build --prefix hifimule-ui` — expected: UI type checking and build pass.

## Suggested Review Order

**Music search flow**

- The dedicated tab groups results and keeps the search form available.
  [`library.ts:1565`](../../hifimule-ui/src/library.ts#L1565)

- Request identity prevents stale search results from replacing another library view.
  [`library.ts:1640`](../../hifimule-ui/src/library.ts#L1640)

- The RPC passes artists, albums, and tracks through one provider boundary.
  [`rpc.rs:2401`](../../hifimule-daemon/src/rpc.rs#L2401)

**Provider search**

- Jellyfin searches each category separately so tracks cannot crowd out other matches.
  [`jellyfin.rs:492`](../../hifimule-daemon/src/providers/jellyfin.rs#L492)

- Audiobookshelf accepts wrapped book hits from its search endpoint.
  [`audiobookshelf.rs:167`](../../hifimule-daemon/src/providers/audiobookshelf.rs#L167)

- Subsonic bounds each search category and flags possibly truncated results.
  [`subsonic.rs:476`](../../hifimule-daemon/src/providers/subsonic.rs#L476)

**Supporting UI**

- Music providers alone receive the Search tab.
  [`library.ts:3093`](../../hifimule-ui/src/library.ts#L3093)

- Four locales supply labels, prompts, and error text.
  [`catalog.json:216`](../../hifimule-i18n/catalog.json#L216)
