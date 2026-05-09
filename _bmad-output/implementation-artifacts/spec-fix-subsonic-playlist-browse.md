---
title: 'Fix Subsonic Playlist Browse'
type: 'bugfix'
created: '2026-05-09'
status: 'done'
baseline_commit: 'b0bbc7f0446a378008e54aa0ee926ea60899bf6d'
context:
  - '{project-root}/_bmad-output/planning-artifacts/project-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/8-3-subsonicprovider-adapter.md'
---

<frozen-after-approval reason="human-owned intent - do not modify unless human renegotiates">

## Intent

**Problem:** When HifiMule is connected to a Subsonic/OpenSubsonic server, playlists are not reachable in the media browser even though `SubsonicProvider` already implements `list_playlists()` and `get_playlist()`. Jellyfin works because it can expose playlist collections through its normal views/items browse path.

**Approach:** Add a Subsonic browse entry point for playlists through the existing provider-to-Jellyfin-legacy RPC adapter, preserving the UI's existing `jellyfin_get_views` / `jellyfin_get_items` contract. The fix should surface playlists as selectable container cards and allow opening a playlist to retrieve its tracks.

## Boundaries & Constraints

**Always:** Keep all Subsonic HTTP calls inside `hifimule-daemon/src/providers/subsonic.rs` through the `MediaProvider` trait. Preserve Jellyfin browse behavior and the existing UI RPC method names. Use the existing legacy JSON shape (`Id`, `Name`, `Type`, `CollectionType`, `Items`, `TotalRecordCount`) so the TypeScript browser can render without a broad UI rewrite.

**Ask First:** Ask before changing visible UX copy, replacing the legacy Jellyfin-shaped RPC contract, adding new server APIs, or changing sync playlist generation semantics beyond making selected Subsonic playlists browseable/selectable.

**Never:** Do not call Subsonic REST endpoints directly from UI or generic RPC code. Do not remove the synthetic Subsonic `"all"` music library. Do not change Jellyfin collection filtering or Jellyfin item fetching.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Subsonic library root | Active provider is Subsonic/OpenSubsonic and UI calls `jellyfin_get_views` | Response includes the existing `"all"` music library and a playlists entry whose `CollectionType` passes the UI's `music/playlists` filter | Provider errors map through existing RPC error handling |
| Subsonic playlist collection | UI calls `jellyfin_get_items` with the playlists entry ID | Response lists provider playlists as legacy `Type: "Playlist"` items with counts/duration/artwork where available | Empty server playlist list returns `Items: []` and `TotalRecordCount: 0` |
| Subsonic playlist open | UI calls `jellyfin_get_items` with a playlist ID | Response lists the playlist tracks as legacy audio items | Missing playlist returns existing provider item-not-found RPC error |
| Jellyfin browse | Active provider is Jellyfin or no non-Jellyfin provider is active | Existing Jellyfin view and item handlers behave as before | Existing Jellyfin errors are unchanged |

</frozen-after-approval>

## Code Map

- `hifimule-daemon/src/providers/subsonic.rs` -- `SubsonicProvider::list_libraries()` currently returns only synthetic `"all"`; provider playlist methods already work and have tests.
- `hifimule-daemon/src/rpc.rs` -- Converts active non-Jellyfin provider data into Jellyfin-shaped browse responses; root/provider browse currently lists artists only.
- `hifimule-ui/src/library.ts` -- UI already accepts views with `CollectionType` of `music` or `playlists` and navigates containers by `Type`, so it should need little or no change.

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-daemon/src/providers/subsonic.rs` -- Add a synthetic playlists library entry while preserving the existing `"all"` entry -- gives the UI a playlist collection card for Subsonic.
- [x] `hifimule-daemon/src/rpc.rs` -- Teach provider browse to return `provider.list_playlists()` when the synthetic playlists library is selected -- retrieves playlist cards through the existing RPC contract.
- [x] `hifimule-daemon/src/rpc.rs` -- Add focused regression tests for Subsonic views, playlist collection browse, and opening a playlist -- locks the bug fix at the RPC/UI contract boundary.
- [x] `hifimule-ui/src/library.ts` -- Inspect after backend changes and only adjust if required by the legacy shape -- avoid unnecessary UI churn.

**Acceptance Criteria:**
- Given an active Subsonic/OpenSubsonic provider, when `jellyfin_get_views` is called, then the returned views include a playlists collection that the current UI filter keeps.
- Given an active Subsonic/OpenSubsonic provider and at least one server playlist, when `jellyfin_get_items` is called for the playlists collection ID, then it returns legacy playlist items.
- Given a returned Subsonic playlist item, when `jellyfin_get_items` is called with that playlist ID, then it returns that playlist's tracks as legacy audio items.
- Given the active provider is Jellyfin, when the same browse RPCs are called, then existing Jellyfin behavior and tests remain unchanged.

## Spec Change Log

## Design Notes

Use a reserved synthetic provider library ID such as `"playlists"` for the collection entry. `provider_items_response()` should handle that ID before the generic artist/album/playlist probing path, because `"playlists"` is not a real provider item ID.

## Verification

**Commands:**
- `rtk cargo test -p hifimule-daemon subsonic_playlist --no-fail-fast` -- expected: focused regression tests pass.
- `rtk cargo test -p hifimule-daemon providers::subsonic --no-fail-fast` -- expected: Subsonic provider tests still pass.
- `rtk cargo test -p hifimule-daemon` -- expected: daemon suite passes.

## Suggested Review Order

**Shared constant — single source of truth for the synthetic ID**

- Defines `SUBSONIC_PLAYLISTS_LIBRARY_ID`; all three changed files import this.
  [`mod.rs:14`](../../hifimule-daemon/src/providers/mod.rs#L14)

**Provider — synthetic library declaration**

- `list_libraries()` now returns two entries; "playlists" is the new synthetic one.
  [`subsonic.rs:151`](../../hifimule-daemon/src/providers/subsonic.rs#L151)

**RPC layer — CollectionType dispatch for the playlists view card**

- ID-based branch emits `CollectionType: "playlists"` so the UI filter passes the card.
  [`rpc.rs:829`](../../hifimule-daemon/src/rpc.rs#L829)

**RPC layer — items routing for the playlists collection and individual playlists**

- Guard for `"playlists"` fires before the generic fallthrough; calls `list_playlists()`.
  [`rpc.rs:1197`](../../hifimule-daemon/src/rpc.rs#L1197)

**Tests — regression coverage at the RPC contract boundary**

- Four new tests: views shape, playlists listing, track listing, Jellyfin regression.
  [`rpc.rs:5399`](../../hifimule-daemon/src/rpc.rs#L5399)

- Updated existing library test to assert both synthetic entries exist.
  [`subsonic.rs:1414`](../../hifimule-daemon/src/providers/subsonic.rs#L1414)
