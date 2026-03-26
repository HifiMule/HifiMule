---
title: 'Server-Sorted Auto-Fill with Early Pagination Stop'
slug: 'server-sorted-autofill-early-stop'
created: '2026-03-25'
status: 'Completed'
stepsCompleted: [1, 2, 3, 4]
tech_stack: ['rust', 'reqwest', 'serde_json', 'anyhow', 'jellyfin-items-api']
files_to_modify:
  - 'jellyfinsync-daemon/src/api.rs'
  - 'jellyfinsync-daemon/src/auto_fill.rs'
code_patterns:
  - 'reqwest with manual HeaderMap injection (X-Emby-Token)'
  - 'format! URL string construction with trim_end_matches'
  - 'paginate with StartIndex+Limit, capture total_record_count from first page only'
  - 'CredentialManager::validate_url + validate_token at top of API methods'
test_patterns:
  - 'pure sync unit tests via rank_and_truncate helper'
  - 'make_track() factory used in all tests'
---

# Tech-Spec: Server-Sorted Auto-Fill with Early Pagination Stop

**Created:** 2026-03-25

## Overview

### Problem Statement

The current auto-fill flow always fetches the entire Jellyfin library (all pages, up to 200 × 500 = 100,000 tracks), then sorts and truncates in memory — even when only a small fraction of tracks is needed to fill device capacity. This wastes network bandwidth and memory.

### Solution

Merge `get_audio_tracks_for_autofill` (api.rs) and `rank_and_truncate` (auto_fill.rs) into a single async flow in `run_auto_fill` that:
- Passes `SortBy=IsFavoriteOrLiked,PlayCount,DateCreated&SortOrder=Descending,Descending,Descending` so tracks arrive pre-sorted from the server
- Passes `ExcludeItemIds` to the server to skip manually-selected tracks before they reach Rust
- Stops paginating as soon as `max_fill_bytes` is filled (`break`, not `continue`)
- Removes the client-side `sort_by` step entirely

### Scope

**In Scope:**
- Rewrite `run_auto_fill` in `auto_fill.rs` to inline the fetch+fill loop with server-side sorting and exclusion
- Delete `get_audio_tracks_for_autofill` from `api.rs`
- Simplify `rank_and_truncate` in `auto_fill.rs`: remove exclude filter, remove sort, change `continue` → `break`
- Delete `date_sort_key` helper
- Update unit tests: delete 5 tests, keep 5, add 1

**Out of Scope:**
- `AutoFillItem` or `AutoFillParams` struct changes
- Any other API methods in `api.rs`
- UI or frontend changes
- `run_auto_fill` public signature change (callers in `rpc.rs` and `main.rs` must not change)

## Context for Development

### Codebase Patterns

- API calls use `reqwest` with manual `HeaderMap` — header built as `HeaderValue::from_str(token).map_err(|_| anyhow!("Invalid token format"))?`
- URL built with `format!` and `url.trim_end_matches('/')` — no URL builder library
- Pagination pattern (from `get_audio_tracks_for_autofill`): `StartIndex` + `Limit`; `total_record_count` captured from first page only via `get_or_insert`; exit condition: `fetched < PAGE_SIZE || start_index + fetched >= total || page_num >= MAX_PAGES`
- `CredentialManager::validate_url` and `validate_token` called at start of API methods — must be added to `run_auto_fill` (currently missing)
- `rank_and_truncate` is a pure sync function — existing tests call it directly; keep it for capacity/size tests after removing sort and exclude logic

### Callers (must not break)

| Caller | File | Call |
| ------ | ---- | ---- |
| `handle_basket_auto_fill` | `rpc.rs:1410` | `run_auto_fill(&state.jellyfin_client, fill_params)` |
| auto-sync trigger | `main.rs:516` | `run_auto_fill(&jellyfin_client, fill_params)` |

### Files to Modify

| File | Change |
| ---- | ------ |
| `jellyfinsync-daemon/src/api.rs` | Delete `get_audio_tracks_for_autofill` method (lines 300–364) |
| `jellyfinsync-daemon/src/auto_fill.rs` | Rewrite `run_auto_fill`; simplify `rank_and_truncate`; delete `date_sort_key`; add imports; update tests |

### Technical Decisions

- `run_auto_fill` public signature unchanged: `(client: &JellyfinClient, params: AutoFillParams) -> Result<Vec<AutoFillItem>>`
- Pagination loop moves from `get_audio_tracks_for_autofill` into `run_auto_fill`; `JellyfinItemsResponse` imported in `auto_fill.rs`
- `ExcludeItemIds` param: `params.exclude_item_ids.join(",")` → omit the param entirely if list is empty (avoid `ExcludeItemIds=` with empty value)
- Capacity loop uses `break` — first track that would exceed remaining budget stops accumulation; subsequent smaller tracks in remaining pages are NOT considered (bin-packing intentionally removed)
- `date_sort_key` helper deleted; `sort_by` closure deleted
- Client-side exclude filter removed from `rank_and_truncate`; server handles it

### Tests Delta

| Test | Action | Reason |
| ---- | ------ | ------ |
| `test_favorites_ranked_first` | DELETE | Sort now server-side |
| `test_play_count_secondary_sort` | DELETE | Sort now server-side |
| `test_date_created_tertiary_sort` | DELETE | Sort now server-side |
| `test_capacity_skip_large_includes_smaller` | DELETE | `continue` → `break`; bin-packing removed |
| `test_exclude_item_ids` | DELETE | Exclusion now server-side; `rank_and_truncate` no longer filters |
| `test_capacity_truncation` | KEEP | Capacity truncation still applies |
| `test_empty_library` | KEEP | Edge case still valid |
| `test_negative_size_tracks_skipped` | KEEP | Guard still applies |
| `test_zero_size_tracks_skipped` | KEEP | Guard still applies |
| `test_zero_capacity_returns_empty` | KEEP | Edge case still valid |
| `test_stops_after_first_oversized` | ADD | Verifies `break` not `continue` |

## Implementation Plan

### Tasks

- [x] Task 1: Delete `get_audio_tracks_for_autofill` from `api.rs`
  - File: `jellyfinsync-daemon/src/api.rs`
  - Action: Delete lines 300–364 — the doc comment block (`///`) and the full `pub async fn get_audio_tracks_for_autofill` method including its closing `}`
  - Notes: No other callers outside `auto_fill.rs`. After this task, the project will not compile until Task 3 is complete.

- [x] Task 2: Simplify `rank_and_truncate` in `auto_fill.rs`
  - File: `jellyfinsync-daemon/src/auto_fill.rs`
  - Action (a): Delete `date_sort_key` helper (lines 57–59 — `fn date_sort_key` and its body)
  - Action (b): In `rank_and_truncate`, delete the `exclude_set` construction and `tracks.retain(...)` call (currently lines 63–67)
  - Action (c): In `rank_and_truncate`, delete the entire `tracks.sort_by(...)` block (currently lines 70–90)
  - Action (d): In `rank_and_truncate`, change `continue` → `break` in the capacity-overflow guard (currently line 114: `if cumulative_bytes + size_bytes > params.max_fill_bytes { continue; }`)
  - Action (e): Update the doc comment on `rank_and_truncate` — remove mention of "ranking" and "excluded items"; describe it as capacity-truncation of a pre-sorted list
  - Notes: `AutoFillParams` still passed (tests use it); `exclude_item_ids` field will be unused in the function body — that's fine for now.

- [x] Task 3: Rewrite `run_auto_fill` in `auto_fill.rs`
  - File: `jellyfinsync-daemon/src/auto_fill.rs`
  - Action: Replace the body of `run_auto_fill` with the inline fetch+fill loop below. Keep the function signature unchanged.
  - New body:
    ```rust
    let (url, token, user_id) =
        CredentialManager::get_credentials().map_err(|e| anyhow::anyhow!("{}", e))?;
    let user_id = user_id.ok_or_else(|| anyhow::anyhow!(
        "No user ID in stored credentials; auto-fill requires an authenticated Jellyfin user"
    ))?;
    CredentialManager::validate_url(&url)?;
    CredentialManager::validate_token(&token)?;

    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        "X-Emby-Token",
        reqwest::header::HeaderValue::from_str(&token)
            .map_err(|_| anyhow::anyhow!("Invalid token format"))?,
    );

    let exclude_param = if params.exclude_item_ids.is_empty() {
        String::new()
    } else {
        format!("&ExcludeItemIds={}", params.exclude_item_ids.join(","))
    };

    const PAGE_SIZE: u32 = 500;
    const MAX_PAGES: u32 = 200;
    let mut result: Vec<AutoFillItem> = Vec::new();
    let mut cumulative_bytes: u64 = 0;
    let mut start_index: u32 = 0;
    let mut total_record_count: Option<u32> = None;
    let mut capacity_reached = false;

    'pages: loop {
        let endpoint = format!(
            "{}/Items?userId={}&IncludeItemTypes=Audio&Recursive=true\
             &Fields=MediaSources,UserData,DateCreated\
             &SortBy=IsFavoriteOrLiked,PlayCount,DateCreated\
             &SortOrder=Descending,Descending,Descending\
             {}&StartIndex={}&Limit={}",
            url.trim_end_matches('/'),
            user_id,
            exclude_param,
            start_index,
            PAGE_SIZE,
        );

        let response = client.http_client()
            .get(&endpoint)
            .headers(headers.clone())
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            let text = response.text().await?;
            return Err(anyhow::anyhow!("Server returned status: {} - {}", status, text));
        }
        let text = response.text().await?;
        let page: crate::api::JellyfinItemsResponse = serde_json::from_str(&text)?;

        let fetched = page.items.len() as u32;
        let total = *total_record_count.get_or_insert(page.total_record_count);

        for track in page.items {
            let size_bytes = track
                .media_sources
                .as_ref()
                .and_then(|ms| ms.first())
                .and_then(|ms| ms.size)
                .and_then(|s| if s > 0 { Some(s as u64) } else { None })
                .unwrap_or(0);

            if size_bytes == 0 {
                continue;
            }

            if cumulative_bytes + size_bytes > params.max_fill_bytes {
                capacity_reached = true;
                break;
            }

            let is_favorite = track.user_data.as_ref().map(|u| u.is_favorite).unwrap_or(false);
            let play_count = track.user_data.as_ref().map(|u| u.play_count).unwrap_or(0);
            let priority_reason = if is_favorite {
                "favorite".to_string()
            } else if play_count > 0 {
                format!("playCount:{}", play_count)
            } else {
                "new".to_string()
            };

            cumulative_bytes += size_bytes;
            result.push(AutoFillItem {
                id: track.id,
                name: track.name,
                album: track.album,
                artist: track.album_artist.or_else(|| {
                    track.artists.and_then(|a| a.into_iter().next())
                }),
                size_bytes,
                priority_reason,
            });
        }

        let page_num = start_index / PAGE_SIZE + 1;
        if capacity_reached
            || fetched < PAGE_SIZE
            || start_index + fetched >= total
            || page_num >= MAX_PAGES
        {
            break 'pages;
        }
        start_index += PAGE_SIZE;
    }

    Ok(result)
    ```
  - Notes: `client.http_client()` exposes the inner `reqwest::Client` — see Task 4. `JellyfinItemsResponse` accessed via `crate::api::JellyfinItemsResponse`. `serde_json` must be in scope — see Task 4. Remove the `println!("DEBUG: ...")` lines from the old body.

- [x] Task 4: Update imports in `auto_fill.rs`
  - File: `jellyfinsync-daemon/src/auto_fill.rs`
  - Action (a): Add `JellyfinItemsResponse` to the `crate::api` import: `use crate::api::{CredentialManager, JellyfinClient, JellyfinItem, JellyfinItemsResponse};`
  - Action (b): Add `use serde_json;` (or use `serde_json::from_str` inline — confirm it's already available as a dependency in `Cargo.toml`)
  - Action (c): Add `pub fn http_client(&self) -> &reqwest::Client` accessor to `JellyfinClient` in `api.rs` so `auto_fill.rs` can call `client.http_client().get(...)` without duplicating the reqwest client setup
  - Notes: Alternatively to (c), move the pagination HTTP call directly using `reqwest::Client::new()` — but reusing the existing client on `JellyfinClient` is preferable. Check if `JellyfinClient.client` is already accessible; if not, add the accessor.

- [x] Task 5: Update unit tests in `auto_fill.rs`
  - File: `jellyfinsync-daemon/src/auto_fill.rs`
  - Action (a): Delete these test functions entirely: `test_favorites_ranked_first`, `test_play_count_secondary_sort`, `test_date_created_tertiary_sort`, `test_capacity_skip_large_includes_smaller`, `test_exclude_item_ids`
  - Action (b): Add new test `test_stops_after_first_oversized`:
    ```rust
    #[test]
    fn test_stops_after_first_oversized() {
        // With break semantics: after the first track that exceeds remaining budget,
        // smaller tracks later in the list are NOT included.
        let tracks = vec![
            make_track("a", false, 0, "2024-01-01", 1_000_000), // 1MB - fits
            make_track("b", false, 0, "2024-01-01", 4_000_000), // 4MB - exceeds remaining 2MB → break
            make_track("c", false, 0, "2024-01-01", 500_000),   // 0.5MB - never reached
        ];
        let result = rank_and_truncate(
            tracks,
            AutoFillParams {
                exclude_item_ids: vec![],
                max_fill_bytes: 3_000_000,
            },
        );
        assert_eq!(result.len(), 1, "only 'a' fits; break stops at 'b', 'c' never considered");
        assert_eq!(result[0].id, "a");
    }
    ```
  - Notes: Existing `use` imports in the test module already cover all needed types.

### Acceptance Criteria

- [ ] AC 1: Given auto-fill runs with a non-empty `exclude_item_ids`, when `run_auto_fill` builds the Jellyfin request URL, then the URL contains `ExcludeItemIds=<comma-separated-ids>` as a query parameter.

- [ ] AC 2: Given `exclude_item_ids` is empty, when `run_auto_fill` builds the Jellyfin request URL, then the URL does NOT contain any `ExcludeItemIds` parameter.

- [ ] AC 3: Given auto-fill runs, when the Jellyfin Items API is called, then the request URL contains `SortBy=IsFavoriteOrLiked,PlayCount,DateCreated` and `SortOrder=Descending,Descending,Descending`.

- [ ] AC 4: Given the server returns tracks in priority order and the cumulative size of accepted tracks reaches `max_fill_bytes`, when the first oversized track is encountered, then `run_auto_fill` stops accumulating and returns immediately without fetching further pages.

- [ ] AC 5: Given a list of tracks where track B would exceed the remaining budget and track C is smaller, when `rank_and_truncate` processes them in order, then only the tracks before B are included (C is not included — `break` not `continue`).

- [ ] AC 6: Given `cargo test` is run, when all tests execute, then all tests pass — the 5 deleted tests are gone, the 5 kept tests pass with the simplified `rank_and_truncate`, and `test_stops_after_first_oversized` passes.

- [ ] AC 7: Given `get_audio_tracks_for_autofill` is removed from `api.rs`, when the project is compiled, then it compiles without errors.

## Additional Context

### Dependencies

- No new crate dependencies required
- `serde_json` is already a dependency (used in `api.rs`)
- `reqwest` already a dependency
- Jellyfin Items API: `SortBy`, `SortOrder`, `ExcludeItemIds` are documented query parameters supported since Jellyfin 10.x

### Testing Strategy

- Unit tests: `rank_and_truncate` pure function tests cover capacity/size logic — run with `cargo test -p jellyfinsync-daemon auto_fill`
- Manual test: run auto-fill against a real Jellyfin instance; verify in debug logs that only 1-2 pages are fetched when device has small free space, and that the result is ordered favorites → play_count → newest
- Compile check: `cargo check -p jellyfinsync-daemon` after Task 1 + before Task 3 should fail (expected); passing after Task 3+ completes is the acceptance signal

### Notes

- **Risk:** `JellyfinClient.client` field is private (`client: reqwest::Client`). Task 4 adds a `pub fn http_client(&self) -> &reqwest::Client` accessor to expose it. This is a one-line addition to `api.rs`.
- **Risk:** Jellyfin's `IsFavoriteOrLiked` sort key sorts favorites to the top (descending). Confirm the sort key name matches the server version in use — some older Jellyfin versions use `IsFavorite` instead.
- **Known limitation:** Zero-size tracks within a page still use `continue` (not `break`) — they are skipped individually but don't stop accumulation. This is intentional and consistent with existing behavior.
- **Future:** If bin-packing is re-introduced, the `break` → `continue` change is the only code change needed, plus restoring the corresponding test.
