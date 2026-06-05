---
baseline_commit: 44f8ebe
---

# Story 11.3: SubsonicProvider Playlist Write Adapter

Status: ready-for-dev

## Story

As a System Admin (Alexis),
I want Subsonic playlist create/add/remove/delete to work correctly,
so that my Navidrome/Subsonic server playlists reflect my HifiMule selections regardless of provider.

## Acceptance Criteria

1. **Given** a Subsonic provider is connected **When** `create_playlist(name, track_ids)` is called **Then** `GET /rest/createPlaylist.view?name={name}&songId={id1}&songId={id2}…` is issued **And** the `id` field from the response `playlist` object is returned as the playlist ID.

2. **Given** a Subsonic provider is connected **When** `add_to_playlist(playlist_id, track_ids)` is called **Then** `GET /rest/updatePlaylist.view?playlistId={id}&songIdToAdd={id1}&songIdToAdd={id2}…` is issued.

3. **Given** a Subsonic provider is connected **When** `remove_from_playlist(playlist_id, track_ids)` is called **Then** `GET /rest/getPlaylist.view?id={id}` fetches the current track list to resolve 0-based song index positions for the requested IDs **And** `GET /rest/updatePlaylist.view?playlistId={id}&songIndexToRemove={idx1}&songIndexToRemove={idx2}…` removes them.

4. **Given** a Subsonic provider is connected **When** `delete_playlist(playlist_id)` is called **Then** `GET /rest/deletePlaylist.view?id={id}` is issued.

5. **Given** any playlist write URL **When** it appears in debug logs **Then** auth params (`u`, `t`, `s`, `password`, `p`) are stripped via the existing `sanitize_subsonic_url()` — this is already guaranteed by `signed_url()` which calls `sanitize_subsonic_url` before logging.

6. **Given** `supports_playlist_write` is queried for a Subsonic or OpenSubsonic provider **Then** it is `true`.

## Tasks / Subtasks

- [ ] Task 1: Add four new methods to `SubsonicClient` (AC: 1–4)
  - [ ] In `hifimule-daemon/src/providers/subsonic.rs`, after `get_playlist` at line 785 (before `search3` at line 788), add:

    ```rust
    async fn create_playlist(
        &self,
        name: &str,
        track_ids: &[String],
    ) -> Result<String, ProviderError> {
        let mut params: Vec<(&str, &str)> = vec![("name", name)];
        for id in track_ids {
            params.push(("songId", id.as_str()));
        }
        let body: PlaylistWithSongsBody = self.get("createPlaylist", &params).await?;
        Ok(body.playlist.id)
    }

    async fn update_playlist_add(
        &self,
        playlist_id: &str,
        track_ids: &[String],
    ) -> Result<(), ProviderError> {
        let mut params: Vec<(&str, &str)> = vec![("playlistId", playlist_id)];
        for id in track_ids {
            params.push(("songIdToAdd", id.as_str()));
        }
        let _: NoBody = self.get("updatePlaylist", &params).await?;
        Ok(())
    }

    async fn update_playlist_remove_by_indices(
        &self,
        playlist_id: &str,
        indices: &[usize],
    ) -> Result<(), ProviderError> {
        let index_strings: Vec<String> = indices.iter().map(|i| i.to_string()).collect();
        let mut params: Vec<(&str, &str)> = vec![("playlistId", playlist_id)];
        for s in &index_strings {
            params.push(("songIndexToRemove", s.as_str()));
        }
        let _: NoBody = self.get("updatePlaylist", &params).await?;
        Ok(())
    }

    async fn delete_playlist(&self, playlist_id: &str) -> Result<(), ProviderError> {
        let _: NoBody = self.get("deletePlaylist", &[("id", playlist_id)]).await?;
        Ok(())
    }
    ```

  - **Key notes:**
    - All calls go through the existing `self.get(endpoint, &params)` method, which internally calls `signed_url()` for authentication and proper URL percent-encoding. No raw URL string construction.
    - `signed_url()` already calls `tracing::debug!(url = %sanitize_subsonic_url(&url), "Subsonic request")` — AC 5 is satisfied automatically.
    - Multi-value params (`songId`, `songIdToAdd`, `songIndexToRemove`) use repeated `(&str, &str)` pairs with the same key built into a `Vec` — `query_pairs_mut().append_pair()` emits one `key=value` token per entry, resulting in `?key=val1&key=val2` URL form.
    - `PlaylistWithSongsBody` is already defined at line ~1445. The `createPlaylist` response body has the same shape as `getPlaylist` (a `playlist` object with `id`, `name`, etc.).
    - `NoBody` (line 1323) is the existing empty-body sentinel — used for `updatePlaylist` and `deletePlaylist` which return no payload beyond the status envelope.

- [ ] Task 2: Flip `supports_playlist_write` to `true` in `SubsonicProvider.capabilities()` (AC: 6)
  - [ ] In `hifimule-daemon/src/providers/subsonic.rs` at line 477: remove the gate comment and change the value:
    ```rust
    supports_playlist_write: true,
    ```
  - [ ] In the test `connect_pings_once_and_caches_capabilities` at line 1715: change the assertion value:
    ```rust
    supports_playlist_write: true,
    ```

- [ ] Task 3: Implement the four `MediaProvider` write methods in `SubsonicProvider` (AC: 1–4)
  - [ ] Add immediately after `get_playlist` (ending at line ~359, before `search` at line 361):

    ```rust
    async fn create_playlist(
        &self,
        name: &str,
        track_ids: &[String],
    ) -> Result<String, ProviderError> {
        self.client.create_playlist(name, track_ids).await
    }

    async fn add_to_playlist(
        &self,
        playlist_id: &str,
        track_ids: &[String],
    ) -> Result<(), ProviderError> {
        if track_ids.is_empty() {
            return Ok(());
        }
        self.client.update_playlist_add(playlist_id, track_ids).await
    }

    async fn remove_from_playlist(
        &self,
        playlist_id: &str,
        track_ids: &[String],
    ) -> Result<(), ProviderError> {
        if track_ids.is_empty() {
            return Ok(());
        }

        let playlist = self.client.get_playlist(playlist_id).await?;
        let track_id_set: std::collections::HashSet<&str> =
            track_ids.iter().map(String::as_str).collect();

        let indices: Vec<usize> = playlist
            .playlist
            .entry
            .iter()
            .enumerate()
            .filter(|(_, song)| track_id_set.contains(song.id.as_str()))
            .map(|(idx, _)| idx)
            .collect();

        if indices.is_empty() {
            return Ok(());
        }

        self.client
            .update_playlist_remove_by_indices(playlist_id, &indices)
            .await
    }

    async fn delete_playlist(&self, playlist_id: &str) -> Result<(), ProviderError> {
        self.client.delete_playlist(playlist_id).await
    }
    ```

  - **Key notes:**
    - `add_to_playlist` and `remove_from_playlist` short-circuit with `Ok(())` when `track_ids` is empty — issuing a request with zero `songId`/`songIdToAdd`/`songIndexToRemove` would result in a server error or a no-op that wastes a round trip.
    - `remove_from_playlist` reuses the existing `self.client.get_playlist()` method (line 784). No new client method needed for the read step.
    - Index resolution: iterate `playlist.playlist.entry` (the `Vec<SongDto>`), collecting the 0-based positions where `song.id` is in the requested `track_ids` set. Preserves order, handles duplicates naturally (each occurrence is an independent index).
    - If a requested track ID appears zero times in the current playlist, it is silently skipped (no error). If `indices` is empty, short-circuit before calling `updatePlaylist`.
    - The `SubsonicProvider` does NOT have `map_error` like `JellyfinProvider`; errors propagate directly since `SubsonicClient.get()` already maps to `ProviderError`.

- [ ] Task 4: Add tests (AC: 1–6)
  - [ ] Add after the last test `list_albums_letter_filter_matches_alpha_and_hash_quick_nav` (before the closing `}` of the `mod tests` block at line 3133):

    ```rust
    #[tokio::test]
    async fn provider_creates_playlist_returns_server_id() {
        let mut server = Server::new_async().await;
        let _mock = server
            .mock("GET", "/rest/createPlaylist.view")
            .match_query(Matcher::AllOf({
                let mut matchers = auth_matchers();
                matchers.push(Matcher::UrlEncoded("name".into(), "Road Trip".into()));
                matchers.push(Matcher::UrlEncoded("songId".into(), "song1".into()));
                matchers.push(Matcher::UrlEncoded("songId".into(), "song2".into()));
                matchers
            }))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(&ok(
                r#""playlist":{"id":"playlist99","name":"Road Trip","songCount":2,"duration":0}"#,
            ))
            .create_async()
            .await;
        let provider = provider(&server).await;

        let id = provider
            .create_playlist("Road Trip", &["song1".to_string(), "song2".to_string()])
            .await
            .expect("create_playlist");

        assert_eq!(id, "playlist99");
    }

    #[tokio::test]
    async fn provider_add_to_playlist_posts_song_id_to_add() {
        let mut server = Server::new_async().await;
        let _mock = server
            .mock("GET", "/rest/updatePlaylist.view")
            .match_query(Matcher::AllOf({
                let mut matchers = auth_matchers();
                matchers.push(Matcher::UrlEncoded("playlistId".into(), "playlist99".into()));
                matchers.push(Matcher::UrlEncoded("songIdToAdd".into(), "song1".into()));
                matchers.push(Matcher::UrlEncoded("songIdToAdd".into(), "song2".into()));
                matchers
            }))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(&ok(""))
            .create_async()
            .await;
        let provider = provider(&server).await;

        provider
            .add_to_playlist("playlist99", &["song1".to_string(), "song2".to_string()])
            .await
            .expect("add_to_playlist");
    }

    #[tokio::test]
    async fn provider_remove_from_playlist_resolves_indices_then_updates() {
        let mut server = Server::new_async().await;
        let _get = server
            .mock("GET", "/rest/getPlaylist.view")
            .match_query(Matcher::AllOf({
                let mut matchers = auth_matchers();
                matchers.push(Matcher::UrlEncoded("id".into(), "playlist99".into()));
                matchers
            }))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(&ok(r#""playlist":{"id":"playlist99","name":"Road Trip","songCount":3,"duration":0,"entry":[
                {"id":"song1","title":"Track 1"},
                {"id":"song2","title":"Track 2"},
                {"id":"song3","title":"Track 3"}
            ]}"#))
            .create_async()
            .await;
        let _update = server
            .mock("GET", "/rest/updatePlaylist.view")
            .match_query(Matcher::AllOf({
                let mut matchers = auth_matchers();
                matchers.push(Matcher::UrlEncoded("playlistId".into(), "playlist99".into()));
                matchers.push(Matcher::UrlEncoded("songIndexToRemove".into(), "0".into()));
                matchers.push(Matcher::UrlEncoded("songIndexToRemove".into(), "2".into()));
                matchers
            }))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(&ok(""))
            .create_async()
            .await;
        let provider = provider(&server).await;

        provider
            .remove_from_playlist(
                "playlist99",
                &["song1".to_string(), "song3".to_string()],
            )
            .await
            .expect("remove_from_playlist");
    }

    #[tokio::test]
    async fn provider_remove_from_playlist_skips_update_when_no_entries_match() {
        let mut server = Server::new_async().await;
        let _get = server
            .mock("GET", "/rest/getPlaylist.view")
            .match_query(Matcher::AllOf({
                let mut matchers = auth_matchers();
                matchers.push(Matcher::UrlEncoded("id".into(), "playlist99".into()));
                matchers
            }))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(&ok(r#""playlist":{"id":"playlist99","name":"Road Trip","songCount":1,"duration":0,"entry":[
                {"id":"song3","title":"Track 3"}
            ]}"#))
            .create_async()
            .await;
        // No updatePlaylist mock — if it is called, mockito will fail the test (unexpected request).
        let provider = provider(&server).await;

        provider
            .remove_from_playlist("playlist99", &["song1".to_string()])
            .await
            .expect("remove_from_playlist when no match should succeed");
    }

    #[tokio::test]
    async fn provider_delete_playlist_calls_delete_playlist_endpoint() {
        let mut server = Server::new_async().await;
        let _mock = server
            .mock("GET", "/rest/deletePlaylist.view")
            .match_query(Matcher::AllOf({
                let mut matchers = auth_matchers();
                matchers.push(Matcher::UrlEncoded("id".into(), "playlist99".into()));
                matchers
            }))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(&ok(""))
            .create_async()
            .await;
        let provider = provider(&server).await;

        provider
            .delete_playlist("playlist99")
            .await
            .expect("delete_playlist");
    }
    ```

  - **Key notes:**
    - `Matcher::AllOf` with two `Matcher::UrlEncoded("songId", "song1")` and `Matcher::UrlEncoded("songId", "song2")` matches a URL containing both `songId=song1` and `songId=song2` as separate query pairs. This is the correct mockito pattern for repeated params.
    - The `ok("")` helper (line 1592) wraps the body in `{"subsonic-response":{"status":"ok","version":"1.16.1","openSubsonic":true,...}}`. For `updatePlaylist` and `deletePlaylist` which have empty bodies, pass `""` — the existing `NoBody` struct (`{}`) deserializes from any JSON object, including one that has only the envelope fields.
    - For the `remove_from_playlist` test: playlist is [song1, song2, song3]. Removing song1 (index 0) and song3 (index 2) → `songIndexToRemove=0` and `songIndexToRemove=2`.
    - The "no match" test registers only a GET mock, no UPDATE mock. In mockito, an unmatched request causes the test to fail — so the absence of an UPDATE mock is the assertion.
    - All tests use `provider(&server).await` (defined at line 1598) which creates an OpenSubsonic provider.

- [ ] Task 5: Verify compilation and tests (AC: all)
  - [ ] Run `rtk cargo check` — zero errors.
  - [ ] Run `rtk cargo test` — all existing tests pass; all five new tests pass.

## Dev Notes

### Critical: `supports_playlist_write` gate — flip `false` → `true` in two places

Story 11.1's review resolved a "capability lie" by gating `supports_playlist_write` to `false` in `subsonic.rs` with the comment `// Gated false until the playlist-write adapter lands (Story 11.3)`. **This story is that landing.** Two sites must be updated:
- `capabilities()` impl at line 477 (runtime capability flag)
- `connect_pings_once_and_caches_capabilities` test at line 1715 (expected value in assertion)

### Subsonic HTTP pattern: all requests are GET with query params

Unlike Jellyfin (which uses POST + JSON body for writes), Subsonic uses GET requests for all operations — including mutating ones like `createPlaylist`, `updatePlaylist`, and `deletePlaylist`. This is per the Subsonic API spec. The `SubsonicClient.get<T>()` method (line 875) handles all these uniformly.

### Multi-value query params via `Vec<(&str, &str)>`

The `signed_url()` method (line 954) takes `&[(&str, &str)]`. For multi-value params (repeated `songId`, `songIdToAdd`, `songIndexToRemove`), build a `Vec<(&str, &str)>` locally and push one entry per value:

```rust
let mut params: Vec<(&str, &str)> = vec![("name", name)];
for id in track_ids {
    params.push(("songId", id.as_str()));
}
self.get("createPlaylist", &params).await
```

`query_pairs_mut().append_pair(key, value)` emits one `key=value` per call — so three `songId` entries produce `?songId=id1&songId=id2&songId=id3`. URL percent-encoding is handled automatically by `reqwest::Url::query_pairs_mut()`.

### Auth sanitization is automatic

`signed_url()` already calls `tracing::debug!(url = %sanitize_subsonic_url(&url), "Subsonic request")` before returning. All new methods use `self.get()` → `signed_url()`, so AC 5 is satisfied at zero additional code cost.

### `SubsonicProvider` has no `map_error` — errors propagate directly

`JellyfinProvider` has a `map_error` method that maps `anyhow::Error` to `ProviderError`. **`SubsonicProvider` does not.** The `SubsonicClient.get<T>()` method already maps HTTP errors, Subsonic API error codes, and deserialization failures to `ProviderError` variants (at lines 903–949). Errors from `self.client.create_playlist()` etc. propagate directly to the caller without any additional mapping.

### `remove_from_playlist` index resolution

The Subsonic `updatePlaylist.view` endpoint removes tracks by 0-based index into the current playlist order. The resolution algorithm:

1. Call `self.client.get_playlist(playlist_id)` — reuses the existing client method at line 784.
2. Use a `HashSet<&str>` over `track_ids` for O(1) lookup.
3. Iterate `playlist.playlist.entry` (the `Vec<SongDto>`) with `.enumerate()`, collecting indices where `song.id` is in the set.
4. If `indices` is empty → short-circuit with `Ok(())` (no `updatePlaylist` call).
5. Otherwise call `self.client.update_playlist_remove_by_indices(playlist_id, &indices)`.

**Note:** `HashSet` is already imported (`use std::collections::{HashMap, HashSet}` at line 19). No new imports needed.

### Empty-slice short-circuit on `add_to_playlist` and `remove_from_playlist`

Both methods must check `if track_ids.is_empty() { return Ok(()); }` before making any HTTP call. Sending `updatePlaylist` with zero `songIdToAdd` or zero `songIndexToRemove` entries may cause a server error or unexpected behavior. This matches the Jellyfin pattern established in Story 11.2.

### `createPlaylist` response reuses `PlaylistWithSongsBody`

The `POST /rest/createPlaylist.view` response has the same shape as `GET /rest/getPlaylist.view`:
```json
{"subsonic-response":{"status":"ok","version":"1.16.1","playlist":{"id":"...","name":"...","entry":[...]}}}
```
The existing `PlaylistWithSongsBody` struct (line 1445) deserializes this correctly. Extract `body.playlist.id` to return the server-assigned playlist ID.

### `ok("")` helper in tests for empty response bodies

The test helper `ok(body: &str)` at line 1592 wraps body text inside:
```
{"subsonic-response":{"status":"ok","version":"1.16.1","openSubsonic":true,{body}}}
```
For `updatePlaylist` and `deletePlaylist` which return no additional fields, pass `""` (empty string). The trailing comma when body is `""` produces `{"subsonic-response":{"status":"ok","version":"1.16.1","openSubsonic":true,}}` — this is what the existing `NoBody` struct handles (it deserializes from any JSON object). **Verify** this works, or use `ok("\"ignored\":null")` as a safe fallback if the trailing comma is rejected.

Actually: looking at the `ok()` helper: `format!(r#"{{"subsonic-response":{{"status":"ok","version":"1.16.1","openSubsonic":true,{body}}}}}"#)`. With `body=""` this produces `...true,}}` — a trailing comma inside the object, which is invalid JSON. Use `"\"_\":null"` or omit body by constructing the string directly. The safest approach:

```rust
.with_body(r#"{"subsonic-response":{"status":"ok","version":"1.16.1"}}"#)
```

Use a raw JSON string for `updatePlaylist` and `deletePlaylist` mocks instead of the `ok()` helper to avoid the trailing comma issue.

### Exact file locations

| File | Location | Change |
|------|----------|--------|
| `hifimule-daemon/src/providers/subsonic.rs` | Line 477 (`capabilities()`) | Remove comment; `supports_playlist_write: true` |
| `hifimule-daemon/src/providers/subsonic.rs` | After `get_playlist` client method (~line 785) | Add 4 new `SubsonicClient` methods |
| `hifimule-daemon/src/providers/subsonic.rs` | After `get_playlist` provider method (~line 359) | Add 4 `MediaProvider` trait methods |
| `hifimule-daemon/src/providers/subsonic.rs` | Line 1715 (`connect_pings_once_and_caches_capabilities` test) | `supports_playlist_write: true` |
| `hifimule-daemon/src/providers/subsonic.rs` | Before closing `}` of `mod tests` (~line 3133) | Add 5 new tests |

### What this story does NOT change

- `jellyfin.rs` — already done in Story 11.2; leave untouched.
- `rpc.rs` — No RPC wiring in this story; that is Story 11.4.
- UI files — No UI changes.
- `Cargo.toml` — No new dependencies. `reqwest`, `mockito`, and all required types are already present.
- `ProviderError` variants — Use existing variants only. Do NOT add `NotSupported`; the correct variant is `UnsupportedCapability(String)`.
- `api.rs` — Subsonic has no separate API file; everything is in `subsonic.rs`.

### Learnings from Story 11.2 (Jellyfin adapter) — avoid these pitfalls

- **URL encoding**: Story 11.2's review flagged manual URL string construction as missing percent-encoding. Subsonic avoids this entirely by using `self.get()` / `signed_url()` — URL encoding is never a concern.
- **Empty-slice short-circuit**: Story 11.2 review also flagged this as deferred from 11.1. Apply it in 11.3 from the start.
- **Test contract coverage**: Story 11.2 review flagged incomplete request contract assertions. In 11.3's tests, assert both the endpoint path AND the query params (including repeated keys). Use `Matcher::AllOf` with explicit `Matcher::UrlEncoded` for every param.
- **Capability lie**: Story 11.1 review forced the `supports_playlist_write: false` gate. Flip both the impl and the test assertion — missing either one breaks the test suite.

### References

- Epics Story 11.3: `_bmad-output/planning-artifacts/epics.md` (Epic 11 section)
- Architecture Epic 11: `_bmad-output/planning-artifacts/architecture.md:523–593`
- Story 11.1 (done): `_bmad-output/implementation-artifacts/11-1-mediaprovider-playlist-write-trait-amendment.md`
- Story 11.2 (done): `_bmad-output/implementation-artifacts/11-2-jellyfinprovider-playlist-write-adapter.md`
- `SubsonicClient` struct: `hifimule-daemon/src/providers/subsonic.rs:673`
- `SubsonicClient.get()` method: `hifimule-daemon/src/providers/subsonic.rs:875`
- `SubsonicClient.signed_url()`: `hifimule-daemon/src/providers/subsonic.rs:954`
- `SubsonicClient.get_playlist()`: `hifimule-daemon/src/providers/subsonic.rs:784`
- `SubsonicProvider.get_playlist()`: `hifimule-daemon/src/providers/subsonic.rs:345`
- `SubsonicProvider.capabilities()`: `hifimule-daemon/src/providers/subsonic.rs:456`
- `PlaylistWithSongsBody` / `PlaylistWithSongsDto`: `hifimule-daemon/src/providers/subsonic.rs:1445`
- `NoBody` struct: `hifimule-daemon/src/providers/subsonic.rs:1323`
- `sanitize_subsonic_url()`: `hifimule-daemon/src/providers/subsonic.rs:1011`
- Test helpers (`ok()`, `provider()`, `auth_matchers()`): `hifimule-daemon/src/providers/subsonic.rs:1592–1610`
- `HashSet` import: `hifimule-daemon/src/providers/subsonic.rs:19`

## Dev Agent Record

### Agent Model Used

claude-sonnet-4-6

### Debug Log References

### Completion Notes List

### File List

## Change Log

- 2026-06-05: Story 11.3 created — SubsonicProvider playlist write adapter ready for dev.

## Status

ready-for-dev
