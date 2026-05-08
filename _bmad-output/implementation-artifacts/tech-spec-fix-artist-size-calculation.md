---
title: 'Fix Artist Size Calculation'
slug: 'fix-artist-size-calculation'
created: '2026-03-01'
status: 'Completed'
stepsCompleted: [1, 2, 3, 4]
tech_stack: ['Rust (daemon)', 'TypeScript (UI)', 'reqwest', 'mockito', 'serde_json', 'tokio']
files_to_modify: ['hifimule-daemon/src/api.rs']
code_patterns: ['CONTAINER_TYPES constant dispatch', 'async/await futures::future::join_all', 'mockito server mocking for tests']
test_patterns: ['#[tokio::test] async', 'mockito::Server::new_async()', 'server.mock(...).with_body(...).create_async()']
---

# Tech-Spec: Fix Artist Size Calculation

**Created:** 2026-03-01

## Overview

### Problem Statement

When a user selects a `MusicArtist` item and adds it to the basket, the displayed size is 0 bytes instead of the cumulative file size of all tracks under that artist. Playlist selection correctly shows the sum of all track sizes.

### Solution

Add `"MusicArtist"` to the `CONTAINER_TYPES` constant in `hifimule-daemon/src/api.rs`. The existing container branch in `get_single_item_size()` already calls `get_child_items_with_sizes()` with `Recursive=true`, which traverses Artist → Albums → Tracks in a single Jellyfin API call. No structural changes are needed beyond adding the type string.

### Scope

**In Scope:**
- Size calculation for `MusicArtist` items in `hifimule-daemon/src/api.rs`
- New unit test covering the artist-as-container scenario

**Out of Scope:**
- Track count calculation (works correctly via `recursiveItemCount`)
- UI / display changes
- Playlist or album size handling (already correct)

## Context for Development

### Codebase Patterns

- **Container dispatch via `CONTAINER_TYPES` constant** ([api.rs:8](hifimule-daemon/src/api.rs#L8)): A `&[&str]` slice checked with `.contains()` determines whether an item is fetched recursively (container) or read directly (leaf). This is the single authoritative branching point for size calculation.
- **`get_child_items_with_sizes()` already uses `Recursive=true`** ([api.rs:378](hifimule-daemon/src/api.rs#L378)): The Jellyfin API call is `GET /Items?userId={uid}&ParentId={id}&IncludeItemTypes=Audio,MusicVideo&Fields=MediaSources&Recursive=true`. The `Recursive=true` parameter makes Jellyfin flatten the Artist → Albums → Tracks hierarchy automatically, returning all leaf audio items. No extra recursion is needed in Rust.
- **Tests use `mockito` with `Server::new_async()`**: Container tests mock two endpoints — the item-details call (`/Items/{id}?userId={uid}&Fields=MediaSources`) and the children call (`/Items?userId={uid}&ParentId=...&Recursive=true`). See `test_get_item_sizes_album_container` ([api.rs:1093](hifimule-daemon/src/api.rs#L1093)) as the direct template.
- **Async parallelism via `futures::future::join_all`**: `get_item_sizes()` fires all size lookups concurrently; each lookup is self-contained.

### Files to Reference

| File | Purpose |
| ---- | ------- |
| [hifimule-daemon/src/api.rs](hifimule-daemon/src/api.rs) | **Primary change target.** `CONTAINER_TYPES` (line 8), `get_single_item_size()` (line 429), `get_child_items_with_sizes()` (line 361), existing tests (line 1067+) |
| [hifimule-daemon/src/rpc.rs](hifimule-daemon/src/rpc.rs) | RPC handler `handle_jellyfin_get_item_sizes()` (line 561) — read-only reference, no changes needed |
| [hifimule-ui/src/state/basket.ts](hifimule-ui/src/state/basket.ts) | `getTotalSizeBytes()` — read-only reference, no changes needed |

### Technical Decisions

- **Add `"MusicArtist"` to `CONTAINER_TYPES`** — not a special-case branch. The existing container logic + `Recursive=true` already handles Artist → Albums → Tracks traversal. One string addition is the entire backend change.
- **No UI changes required** — the frontend sums `sizeBytes` from whatever the RPC returns; once the daemon returns the correct value, the UI is correct automatically.

## Implementation Plan

### Tasks

- [x] Task 1: Add `"MusicArtist"` to `CONTAINER_TYPES`
  - File: `hifimule-daemon/src/api.rs`
  - Action: Change line 8 from:
    ```rust
    const CONTAINER_TYPES: &[&str] = &["MusicAlbum", "Playlist"];
    ```
    to:
    ```rust
    const CONTAINER_TYPES: &[&str] = &["MusicAlbum", "Playlist", "MusicArtist"];
    ```
  - Notes: This is the entire production code change. No other files require modification.

- [x] Task 2: Add unit test `test_get_item_sizes_artist_container`
  - File: `hifimule-daemon/src/api.rs` (inside `#[cfg(test)] mod tests`, after `test_get_item_sizes_album_container`)
  - Action: Add a new `#[tokio::test]` following the exact pattern of `test_get_item_sizes_album_container` (line 1093). Mock two endpoints:
    1. `GET /Items/artist1?userId=user1&Fields=MediaSources` → responds with `{"Id": "artist1", "Name": "Test Artist", "Type": "MusicArtist"}` (no `MediaSources`)
    2. `GET /Items?userId=user1&ParentId=artist1&IncludeItemTypes=Audio,MusicVideo&Fields=MediaSources&Recursive=true` → responds with two Audio tracks with sizes (e.g., 3 000 000 and 4 000 000 bytes)
  - Assert: `results[0].1 == 7_000_000`
  - Notes: The mock URL for the children endpoint must match exactly — including the `Recursive=true` query param — because mockito does exact path+query matching.

### Acceptance Criteria

- [x] AC 1: Given a `MusicArtist` item is in the basket, when the basket sidebar renders, then the displayed size equals the sum of `MediaSources[0].Size` for all `Audio` and `MusicVideo` tracks under that artist.

- [x] AC 2: Given a `MusicArtist` item with no tracks (empty artist), when the basket sidebar renders, then the displayed size is 0 bytes (graceful zero-sum, no crash).

- [x] AC 3: Given a `MusicAlbum` or `Playlist` item is in the basket, when the basket sidebar renders, then the displayed size is unchanged from current behavior (regression guard).

- [x] AC 4: Given the `test_get_item_sizes_artist_container` test is run, when `cargo test` executes, then the test passes, asserting the summed size equals the total of all mocked child track sizes.

## Additional Context

### Dependencies

- No new external dependencies.
- Jellyfin server must support `Recursive=true` on `/Items?userId={uid}` (standard Jellyfin API — already used by existing album/playlist paths).

### Testing Strategy

- **Unit tests (automated):** Add `test_get_item_sizes_artist_container` in `api.rs` using the existing `mockito` pattern. Run with `rtk cargo test`.
- **Regression guard:** Existing tests `test_get_item_sizes_album_container` and `test_get_item_sizes_individual_track` cover the unchanged paths and will catch any unintended breakage.
- **Manual verification:** Add an artist to the basket in the UI; confirm the size field shows a non-zero byte count matching the expected sum of the artist's tracks.

### Notes

- **Pre-mortem risk:** The only risk is the children endpoint mock URL not matching exactly in the new test. The URL is `?ParentId={id}&IncludeItemTypes=Audio,MusicVideo&Fields=MediaSources&Recursive=true` — query param order must match what `get_child_items_with_sizes()` constructs (line 378). Copy the mock URL from `test_get_item_sizes_album_container` and substitute `artist1` for `album1`.
- **Future consideration (out of scope):** If `MusicVideo` items at artist level should be excluded from sync, a separate filter in the basket logic would handle that — not here.

## Review Notes

- Adversarial review completed
- Findings: 12 total, 6 fixed, 6 skipped (noise/pre-existing)
- Resolution approach: auto-fix
- Fixed: F9 (.expect(1) on mocks), F4 (numeric literal style), F5 (distinct test data), F11 (doc comment), F1 (empty artist test), F2 (children error test)
- Skipped as noise: F3, F7, F10; skipped as pre-existing: F6, F8, F12
