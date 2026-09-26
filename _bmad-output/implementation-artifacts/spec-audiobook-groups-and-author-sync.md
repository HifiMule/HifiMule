---
title: 'Browse audiobook series, collections, and authors separately and sync by author'
type: 'feature'
created: '2026-09-26'
status: 'done'
baseline_commit: 'cf8bff05003b69190a946767693635ab63de0b41'
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** An Audiobookshelf Books library puts series and collections together under Playlists. There is no Authors view, so a user cannot select every audiobook credited to an author for device sync.

**Approach:** Give Books libraries separate Series, Collections, and Authors browse tabs. Authors list their books and can be added to the basket; sync resolves the author to all eligible books in the selected Audiobookshelf library.

## Boundaries & Constraints

**Always:** Scope these tabs to Audiobookshelf audiobook libraries. Keep series and collections read only and preserve their existing browse and basket behavior. Include books where the selected author is any credited author, including coauthors; deduplicate books and tracks, preserve stable opaque IDs and library scope, and fail clearly rather than silently syncing a partial catalogue. Keep music provider Artists and Playlists behavior unchanged. Keep English and French UI strings in sync.

**Ask First:** A provider limitation that makes complete author membership impossible without a new server side permission or a fundamentally different sync contract.

**Never:** Treat narrators as authors; modify remote collections or series; broaden an author selection to another library or server; silently cap the books selected for sync.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Audiobook tabs | Books server with series, collections, and authors | Three separate tabs; each shows only its group type | Empty lists show the normal empty state |
| Author browse | Book credits include primary and additional authors | Each credited author has a group; opening it shows that author's books once each | Missing author IDs use a stable, scoped fallback only if identity is unambiguous |
| Author sync | Author selected in basket | All eligible parts of every credited book resolve through normal sync, without a playlist file | Incomplete pagination or foreign library membership aborts with a useful error |
| Existing providers | Music or podcast server selected | Existing mode bar and playlist/artist behavior remain intact | Unsupported modes are never advertised |

</frozen-after-approval>

## Code Map

- `hifimule-daemon/src/providers/audiobookshelf.rs` -- audiobook capabilities, series/collection groups, book credits, and provider lookup.
- `hifimule-daemon/src/providers/mod.rs` -- provider-neutral browse mode contract.
- `hifimule-daemon/src/rpc.rs` -- browse RPCs and basket ID to sync media resolution.
- `hifimule-ui/src/library.ts` -- mode bar, loaders, group navigation, and selection.
- `hifimule-ui/src/rpc.ts` -- browse mode and payload types.
- `hifimule-ui/src/components/MediaCard.ts` -- group card selection affordance.
- `hifimule-i18n/catalog.json` -- English and French labels.

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-daemon/src/providers/mod.rs` -- add separate Series and Collections browse capabilities without changing existing provider modes.
- [x] `hifimule-daemon/src/providers/audiobookshelf.rs` -- expose separately typed series and collection lists and complete author list/detail with scoped IDs, coauthor membership, and guarded pagination; implement author resolution through the provider's artist contract or an explicit equivalent.
- [x] `hifimule-daemon/src/rpc.rs` -- expose group browse data and ensure an author basket ID resolves all member book tracks for sync, with no playlist artifact; reject incomplete or cross-library results.
- [x] `hifimule-ui/src/rpc.ts` and `hifimule-ui/src/library.ts` -- add audiobook-only tabs, route lists and details, and allow author group basket selection while retaining existing music behavior.
- [x] `hifimule-ui/src/components/MediaCard.ts` and `hifimule-i18n/catalog.json` -- present Authors, Series, and Collections with correct labels and selection affordances in English and French.
- [x] `hifimule-daemon/src/providers/audiobookshelf.rs`, `hifimule-daemon/src/rpc.rs`, and `hifimule-ui/tests/audiobookshelfBrowse.test.mjs` -- add focused tests for coauthors, duplicate credits/books, paginated catalogues, empty groups, foreign library data, author basket sync, and music/podcast mode isolation.

**Acceptance Criteria:**
- Given an Audiobookshelf Books library, when the library opens, then Series, Collections, and Authors appear as distinct tabs and Playlists is absent.
- Given an author credited on multiple books, when their group is opened, then each book appears once and can be opened normally.
- Given an author group in the basket, when device sync plans the selection, then every eligible part from every credited book is included once and no playlist file is created for the author.
- Given a music or podcast server, when the library opens, then its prior browse modes and selection behavior are unchanged.

## Spec Change Log

## Design Notes

The existing Audiobookshelf provider maps series and collections to read only playlist IDs. Preserve those IDs and `get_playlist` lookup so saved basket selections remain valid. The current product requirements list author as the primary artist equivalent but did not require author group sync; this request extends that behavior.

## Verification

**Commands:**
- `rtk npm run build:daemon -- test -p hifimule-daemon audiobookshelf` -- provider group and author cases pass through the required verified audio runtime.
- `rtk npm run build:daemon -- test -p hifimule-daemon rpc` -- author selection resolves fully and safely.
- `rtk node --test hifimule-ui/tests/audiobookshelfBrowse.test.mjs` -- UI behavior tests pass.
- `rtk npm run build --prefix hifimule-ui` -- TypeScript and frontend build pass.

## Suggested Review Order

**Browse contract**

- Books servers advertise distinct author, series, and collection modes.
  [audiobookshelf.rs:2526](../../hifimule-daemon/src/providers/audiobookshelf.rs#L2526)

- Group endpoints retain existing IDs while splitting the two lists.
  [audiobookshelf.rs:3362](../../hifimule-daemon/src/providers/audiobookshelf.rs#L3362)

- The RPC boundary exposes only supported group modes.
  [rpc.rs:2193](../../hifimule-daemon/src/rpc.rs#L2193)

**Author membership and sync**

- Complete catalogue paging rejects missing or changing pages before author resolution.
  [audiobookshelf.rs:1561](../../hifimule-daemon/src/providers/audiobookshelf.rs#L1561)

- Coauthors receive scoped groups; ambiguous name-only credits fail safely.
  [audiobookshelf.rs:1607](../../hifimule-daemon/src/providers/audiobookshelf.rs#L1607)

- Basket IDs expand to unique parts without creating playlist files.
  [rpc.rs:4926](../../hifimule-daemon/src/rpc.rs#L4926)

- Author basket estimates count and size the same unique parts.
  [rpc.rs:4285](../../hifimule-daemon/src/rpc.rs#L4285)

**Desktop presentation**

- The mode bar routes each Books tab to its own browse data.
  [library.ts:1757](../../hifimule-ui/src/library.ts#L1757)

- Author cards carry a scoped basket type and track count.
  [library.ts:223](../../hifimule-ui/src/library.ts#L223)

**Verification**

- The sync test checks unique parts, basket estimates, and no playlist artifact.
  [rpc.rs:16214](../../hifimule-daemon/src/rpc.rs#L16214)

- The UI contract test checks tabs and translated labels.
  [audiobookshelfBrowse.test.mjs:73](../../hifimule-ui/tests/audiobookshelfBrowse.test.mjs#L73)
