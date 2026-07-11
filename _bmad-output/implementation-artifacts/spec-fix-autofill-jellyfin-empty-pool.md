---
title: 'Fix AutoFill empty library pool on Jellyfin (list_all_songs_page)'
type: 'bugfix'
created: '2026-06-15'
status: 'done'
context: []
baseline_commit: '03e2c1dec039ac5fc08b040e191eec82b8bd78ec'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** On a Jellyfin server, AutoFill preview (and the real sync bulk-fill pass) logs `[AutoFill] list_all_songs_page: UnsupportedCapability, empty pool` and the Library source contributes zero tracks. `JellyfinProvider` never overrides `list_all_songs_page`, so it falls through to the default trait impl that returns `UnsupportedCapability` — even though Jellyfin can enumerate all audio via the same `/Items` endpoint its `list_tracks` already uses.

**Approach:** Implement `list_all_songs_page` on `JellyfinProvider`, mirroring the unfiltered branch of its existing `list_tracks`: call `client.get_items` with `IncludeItemTypes=Audio` (the client already sets `Recursive=true`), pass through `offset`/`limit`, sort by `Name`, and return the real `total_record_count`. No call-site changes needed — both consumers already invoke the trait method.

## Boundaries & Constraints

**Always:** Reuse the existing `get_items` client method and `song_from_item` mapper. Return Jellyfin's `total_record_count` as the total. Honor `library_id` as `parent_id` when provided (pass `None` through unchanged). Treat `limit == 0` as "no Limit param" exactly like `list_tracks` does.

**Ask First:** If implementing this requires changing the `MediaProvider` trait signature or any shared call site (`fetch.rs`, `auto_fill/mod.rs`) — it should not; HALT if it appears to.

**Never:** Do not touch the Subsonic impl, the default trait impl, or the AutoFill pipeline/consumers. Do not add a new HTTP client method — `get_items` already covers this. Do not restrict to a hardcoded "music" library; rely on `IncludeItemTypes=Audio` like `list_tracks`.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Happy path | `list_all_songs_page(None, 0, 200)`, library has audio | `(songs, total)` with songs from `/Items?IncludeItemTypes=Audio&StartIndex=0&Limit=200`, total = `total_record_count` | N/A |
| Pagination | `offset=200, limit=200` | Page 2 returned via `StartIndex=200` | N/A |
| Library-scoped | `library_id=Some("lib1")` | `ParentId=lib1` passed through | N/A |
| Empty library | provider returns 0 items | `(vec![], 0)` — no error, AutoFill pool simply empty | N/A |
| Transport/HTTP error | `/Items` request fails | Propagate via `Self::map_error` (AutoFill logs it non-fatal) | Map to `ProviderError` |

</frozen-after-approval>

## Code Map

- `hifimule-daemon/src/providers/jellyfin.rs:787` -- existing `list_tracks` unfiltered branch — golden pattern for the new method; add `list_all_songs_page` in the same `impl MediaProvider` block.
- `hifimule-daemon/src/providers/mod.rs:194` -- default `list_all_songs_page` returning `UnsupportedCapability` — the fallback being overridden (reference only).
- `hifimule-daemon/src/api.rs:347` -- `JellyfinClient::get_items` — already sets `Recursive=true`; the HTTP call to reuse.
- `hifimule-daemon/src/auto_fill/fetch.rs:514` -- `fetch_library` preview consumer (no change; verifies fix).
- `hifimule-daemon/src/auto_fill/mod.rs:488` -- real sync bulk-fill consumer (no change; also benefits).
- `hifimule-daemon/src/providers/jellyfin.rs:1230` -- existing mockito `/Items` test — pattern for the new unit test.

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-daemon/src/providers/jellyfin.rs` -- Implement `async fn list_all_songs_page(&self, library_id, offset, limit)` in the `impl MediaProvider for JellyfinProvider` block: `limit_param = (limit > 0).then_some(limit)`; call `self.client.get_items(self.url(), self.token(), self.user_id(), library_id, Some("Audio"), Some(offset), limit_param, None, None, None, None, Some("Name"))`, `.map_err(Self::map_error)?`; map items via `song_from_item`; return `(songs, response.total_record_count)`. -- Makes Jellyfin's Library pool real instead of empty.
- [x] `hifimule-daemon/src/providers/jellyfin.rs` (tests) -- Add a unit test mirroring the `/Items` mockito test: mock `GET /Items` returning audio items with a `TotalRecordCount`, assert `list_all_songs_page(None, 0, 200)` returns the mapped songs and the total, and (recommended) assert the request carries `IncludeItemTypes=Audio` and `StartIndex`/`Limit`. -- Locks the contract.

**Acceptance Criteria:**
- Given a Jellyfin-backed server with audio, when AutoFill preview runs, then the Library source contributes tracks and the `list_all_songs_page: UnsupportedCapability, empty pool` log no longer appears.
- Given the same provider, when `list_all_songs_page` is called with an `offset` beyond the library size, then it returns an empty page without error.

## Spec Change Log

## Design Notes

The new method is the no-filter case of `list_tracks` ([jellyfin.rs:787](hifimule-daemon/src/providers/jellyfin.rs#L787)) with a different return shape. Golden shape:

```rust
async fn list_all_songs_page(&self, library_id: Option<&str>, offset: u32, limit: u32)
    -> Result<(Vec<Song>, u32), ProviderError> {
    let limit_param = (limit > 0).then_some(limit);
    let response = self.client.get_items(
        self.url(), self.token(), self.user_id(),
        library_id, Some("Audio"), Some(offset), limit_param,
        None, None, None, None, Some("Name"),
    ).await.map_err(Self::map_error)?;
    let songs = response.items.into_iter().map(song_from_item).collect();
    Ok((songs, response.total_record_count))
}
```

Returning the real `total_record_count` is safe: both consumers loop on `songs.len() < PAGE_SIZE` for exhaustion, not on total.

## Verification

**Commands:**
- `rtk cargo test -p hifimule-daemon jellyfin` -- expected: new + existing Jellyfin tests pass.
- `rtk cargo clippy -p hifimule-daemon` -- expected: no new warnings.

## Suggested Review Order

- The fix: Jellyfin now overrides `list_all_songs_page`, mirroring the unfiltered `list_tracks` branch — `IncludeItemTypes=Audio`, real `total_record_count`.
  [`jellyfin.rs:828`](../../hifimule-daemon/src/providers/jellyfin.rs#L828)

- Contract lock: mockito `/Items` test asserts query params and the (songs, total) shape.
  [`jellyfin.rs:1291`](../../hifimule-daemon/src/providers/jellyfin.rs#L1291)
