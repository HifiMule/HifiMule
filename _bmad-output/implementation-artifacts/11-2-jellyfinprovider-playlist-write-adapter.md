---
baseline_commit: 9aaf386
---

# Story 11.2: JellyfinProvider Playlist Write Adapter

Status: done

## Story

As a System Admin (Alexis),
I want Jellyfin playlist create/add/remove/delete to work correctly,
so that my Jellyfin server playlists reflect my HifiMule selections.

## Acceptance Criteria

1. **Given** a Jellyfin provider is connected **When** `create_playlist(name, track_ids)` is called **Then** `POST /Playlists` is issued with `MediaType "Audio"`, the name, and the track IDs in the request body **And** the `Id` field from the response is returned as the playlist ID.

2. **Given** a Jellyfin provider is connected **When** `add_to_playlist(playlist_id, track_ids)` is called **Then** `POST /Playlists/{id}/Items?Ids={comma-separated IDs}` is issued.

3. **Given** a Jellyfin provider is connected **When** `remove_from_playlist(playlist_id, track_ids)` is called **Then** `GET /Playlists/{id}/Items` is called first to resolve the Jellyfin `PlaylistItemId` entries matching the given track IDs **And** `DELETE /Playlists/{id}/Items?EntryIds={comma-separated PlaylistItemIds}` removes them.

4. **Given** a Jellyfin provider is connected **When** `delete_playlist(playlist_id)` is called **Then** `DELETE /Items/{id}` is issued (Jellyfin deletes playlists as generic items).

5. **Given** `supports_playlist_write` is queried for a Jellyfin provider **Then** it is `true`.

## Tasks / Subtasks

- [x] Task 1: Add `playlist_item_id` field to `JellyfinItem` (AC: 3)
  - [x] In `hifimule-daemon/src/api.rs`, add to the `JellyfinItem` struct (after `date_created`, before the closing `}` of the struct):
    ```rust
    #[serde(default)]
    pub playlist_item_id: Option<String>,
    ```
  - No other changes needed — existing construction sites are unaffected because the field has `#[serde(default)]` and `JellyfinItem` is constructed from deserialization, not struct literals.

- [x] Task 2: Add five new HTTP methods to `JellyfinClient` in `api.rs` (AC: 1–4)
  - [x] Add `create_playlist` — issues `POST /Playlists` with a JSON body:
    ```rust
    pub async fn create_playlist(
        &self,
        url: &str,
        token: &str,
        user_id: &str,
        name: &str,
        track_ids: &[String],
    ) -> Result<String> {
        CredentialManager::validate_url(url)?;
        CredentialManager::validate_token(token)?;

        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Emby-Token",
            HeaderValue::from_str(token).map_err(|_| anyhow!("Invalid token format"))?,
        );

        let endpoint = format!("{}/Playlists", url.trim_end_matches('/'));
        let body = serde_json::json!({
            "Name": name,
            "MediaType": "Audio",
            "Ids": track_ids,
            "UserId": user_id,
        });

        let response = self
            .client
            .post(&endpoint)
            .headers(headers)
            .json(&body)
            .send()
            .await?;

        let status = response.status();
        let text = response.text().await?;
        if !status.is_success() {
            return Err(anyhow!("Server returned status: {}", status));
        }

        let value: serde_json::Value = serde_json::from_str(&text)?;
        value["Id"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow!("Playlist create response missing Id field"))
    }
    ```
  - [x] Add `add_tracks_to_playlist` — issues `POST /Playlists/{id}/Items?Ids=…`:
    ```rust
    pub async fn add_tracks_to_playlist(
        &self,
        url: &str,
        token: &str,
        user_id: &str,
        playlist_id: &str,
        track_ids: &[String],
    ) -> Result<()> {
        CredentialManager::validate_url(url)?;
        CredentialManager::validate_token(token)?;

        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Emby-Token",
            HeaderValue::from_str(token).map_err(|_| anyhow!("Invalid token format"))?,
        );

        let ids_param = track_ids.join(",");
        let endpoint = format!(
            "{}/Playlists/{}/Items?Ids={}&userId={}",
            url.trim_end_matches('/'),
            playlist_id,
            ids_param,
            user_id
        );

        let response = self.client.post(&endpoint).headers(headers).send().await?;
        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(anyhow!("Server returned status: {} — {}", status, text));
        }
        Ok(())
    }
    ```
  - [x] Add `get_playlist_items` — issues `GET /Playlists/{id}/Items?userId=…`:
    ```rust
    pub async fn get_playlist_items(
        &self,
        url: &str,
        token: &str,
        user_id: &str,
        playlist_id: &str,
    ) -> Result<Vec<JellyfinItem>> {
        CredentialManager::validate_url(url)?;
        CredentialManager::validate_token(token)?;

        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Emby-Token",
            HeaderValue::from_str(token).map_err(|_| anyhow!("Invalid token format"))?,
        );

        let endpoint = format!(
            "{}/Playlists/{}/Items?userId={}",
            url.trim_end_matches('/'),
            playlist_id,
            user_id
        );

        let response = self.client.get(&endpoint).headers(headers).send().await?;
        let status = response.status();
        let text = response.text().await?;
        if !status.is_success() {
            return Err(anyhow!("Server returned status: {}", status));
        }

        let items_response = serde_json::from_str::<JellyfinItemsResponse>(&text)?;
        Ok(items_response.items)
    }
    ```
  - [x] Add `delete_playlist_items` — issues `DELETE /Playlists/{id}/Items?EntryIds=…`:
    ```rust
    pub async fn delete_playlist_items(
        &self,
        url: &str,
        token: &str,
        playlist_id: &str,
        entry_ids: &[String],
    ) -> Result<()> {
        CredentialManager::validate_url(url)?;
        CredentialManager::validate_token(token)?;

        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Emby-Token",
            HeaderValue::from_str(token).map_err(|_| anyhow!("Invalid token format"))?,
        );

        let entry_ids_param = entry_ids.join(",");
        let endpoint = format!(
            "{}/Playlists/{}/Items?EntryIds={}",
            url.trim_end_matches('/'),
            playlist_id,
            entry_ids_param
        );

        let response = self.client.delete(&endpoint).headers(headers).send().await?;
        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(anyhow!("Server returned status: {} — {}", status, text));
        }
        Ok(())
    }
    ```
  - [x] Add `delete_item` — issues `DELETE /Items/{id}`:
    ```rust
    pub async fn delete_item(
        &self,
        url: &str,
        token: &str,
        item_id: &str,
    ) -> Result<()> {
        CredentialManager::validate_url(url)?;
        CredentialManager::validate_token(token)?;

        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Emby-Token",
            HeaderValue::from_str(token).map_err(|_| anyhow!("Invalid token format"))?,
        );

        let endpoint = format!("{}/Items/{}", url.trim_end_matches('/'), item_id);

        let response = self.client.delete(&endpoint).headers(headers).send().await?;
        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(anyhow!("Server returned status: {} — {}", status, text));
        }
        Ok(())
    }
    ```

- [x] Task 3: Flip `supports_playlist_write` to `true` in `jellyfin.rs` (AC: 5)
  - [x] In `hifimule-daemon/src/providers/jellyfin.rs` at line 372–373: Remove the "Gated false" comment and change the value:
    ```rust
    supports_playlist_write: true,
    ```
  - [x] In the test at line 863 (inside `provider_exposes_capabilities`): Change the assertion value:
    ```rust
    supports_playlist_write: true,
    ```

- [x] Task 4: Implement the four `MediaProvider` write methods in `jellyfin.rs` (AC: 1–4)
  - [x] Add immediately after the existing `get_playlist` method (around line 272) — all four methods call the new api.rs helpers via `self.client.*` and map errors with `Self::map_error`:
    ```rust
    async fn create_playlist(
        &self,
        name: &str,
        track_ids: &[String],
    ) -> Result<String, ProviderError> {
        self.client
            .create_playlist(self.url(), self.token(), self.user_id(), name, track_ids)
            .await
            .map_err(Self::map_error)
    }

    async fn add_to_playlist(
        &self,
        playlist_id: &str,
        track_ids: &[String],
    ) -> Result<(), ProviderError> {
        if track_ids.is_empty() {
            return Ok(());
        }
        self.client
            .add_tracks_to_playlist(
                self.url(),
                self.token(),
                self.user_id(),
                playlist_id,
                track_ids,
            )
            .await
            .map_err(Self::map_error)
    }

    async fn remove_from_playlist(
        &self,
        playlist_id: &str,
        track_ids: &[String],
    ) -> Result<(), ProviderError> {
        if track_ids.is_empty() {
            return Ok(());
        }

        let items = self
            .client
            .get_playlist_items(self.url(), self.token(), self.user_id(), playlist_id)
            .await
            .map_err(Self::map_error)?;

        let entry_ids: Vec<String> = items
            .into_iter()
            .filter(|item| track_ids.contains(&item.id))
            .filter_map(|item| item.playlist_item_id)
            .collect();

        if entry_ids.is_empty() {
            return Ok(());
        }

        self.client
            .delete_playlist_items(self.url(), self.token(), playlist_id, &entry_ids)
            .await
            .map_err(Self::map_error)
    }

    async fn delete_playlist(&self, playlist_id: &str) -> Result<(), ProviderError> {
        self.client
            .delete_item(self.url(), self.token(), playlist_id)
            .await
            .map_err(Self::map_error)
    }
    ```

- [x] Task 5: Add tests in `jellyfin.rs` (AC: 1–5)
  - [x] Add after the existing `provider_lists_and_gets_playlist_tracks` test (around line 1023). All four operations plus an edge-case test for `remove_from_playlist` with no match:
    ```rust
    #[tokio::test]
    async fn provider_creates_playlist_returns_server_id() {
        let mut server = Server::new_async().await;
        let url = server.url();
        let _mock = server
            .mock("POST", "/Playlists")
            .match_header("X-Emby-Token", TOKEN)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"Id":"playlist99","Name":"Road Trip"}"#)
            .create_async()
            .await;

        let provider = JellyfinProvider::new(JellyfinClient::new(), url, TOKEN, USER_ID);
        let id = provider
            .create_playlist("Road Trip", &["song1".to_string(), "song2".to_string()])
            .await
            .expect("create_playlist");

        assert_eq!(id, "playlist99");
    }

    #[tokio::test]
    async fn provider_add_to_playlist_posts_ids() {
        let mut server = Server::new_async().await;
        let url = server.url();
        let _mock = server
            .mock("POST", "/Playlists/playlist99/Items")
            .match_query(Matcher::AllOf(vec![
                Matcher::UrlEncoded("Ids".into(), "song1,song2".into()),
            ]))
            .match_header("X-Emby-Token", TOKEN)
            .with_status(204)
            .create_async()
            .await;

        let provider = JellyfinProvider::new(JellyfinClient::new(), url, TOKEN, USER_ID);
        provider
            .add_to_playlist("playlist99", &["song1".to_string(), "song2".to_string()])
            .await
            .expect("add_to_playlist");
    }

    #[tokio::test]
    async fn provider_remove_from_playlist_resolves_entry_ids_then_deletes() {
        let mut server = Server::new_async().await;
        let url = server.url();
        let _get = server
            .mock("GET", "/Playlists/playlist99/Items")
            .match_query(Matcher::UrlEncoded("userId".into(), USER_ID.into()))
            .match_header("X-Emby-Token", TOKEN)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"Items":[
                    {"Id":"song1","Name":"Track 1","Type":"Audio","PlaylistItemId":"entry-a"},
                    {"Id":"song2","Name":"Track 2","Type":"Audio","PlaylistItemId":"entry-b"},
                    {"Id":"song3","Name":"Track 3","Type":"Audio","PlaylistItemId":"entry-c"}
                ],"TotalRecordCount":3,"StartIndex":0}"#,
            )
            .create_async()
            .await;
        let _delete = server
            .mock("DELETE", "/Playlists/playlist99/Items")
            .match_query(Matcher::UrlEncoded("EntryIds".into(), "entry-a,entry-b".into()))
            .match_header("X-Emby-Token", TOKEN)
            .with_status(204)
            .create_async()
            .await;

        let provider = JellyfinProvider::new(JellyfinClient::new(), url, TOKEN, USER_ID);
        provider
            .remove_from_playlist(
                "playlist99",
                &["song1".to_string(), "song2".to_string()],
            )
            .await
            .expect("remove_from_playlist");
    }

    #[tokio::test]
    async fn provider_remove_from_playlist_skips_delete_when_no_entries_match() {
        let mut server = Server::new_async().await;
        let url = server.url();
        let _get = server
            .mock("GET", "/Playlists/playlist99/Items")
            .match_query(Matcher::UrlEncoded("userId".into(), USER_ID.into()))
            .match_header("X-Emby-Token", TOKEN)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"Items":[{"Id":"song3","Name":"Track 3","Type":"Audio","PlaylistItemId":"entry-c"}],"TotalRecordCount":1,"StartIndex":0}"#)
            .create_async()
            .await;
        // No DELETE mock — if DELETE is issued the test would fail (unexpected request)

        let provider = JellyfinProvider::new(JellyfinClient::new(), url, TOKEN, USER_ID);
        // Removing tracks that are NOT in the playlist → should silently succeed
        provider
            .remove_from_playlist("playlist99", &["song1".to_string()])
            .await
            .expect("remove_from_playlist when no match should succeed");
    }

    #[tokio::test]
    async fn provider_delete_playlist_issues_delete_items_endpoint() {
        let mut server = Server::new_async().await;
        let url = server.url();
        let _mock = server
            .mock("DELETE", "/Items/playlist99")
            .match_header("X-Emby-Token", TOKEN)
            .with_status(204)
            .create_async()
            .await;

        let provider = JellyfinProvider::new(JellyfinClient::new(), url, TOKEN, USER_ID);
        provider
            .delete_playlist("playlist99")
            .await
            .expect("delete_playlist");
    }
    ```

- [x] Task 6: Verify compilation and tests (AC: all)
  - [x] Run `rtk cargo check` — zero errors.
  - [x] Run `rtk cargo test` — all existing tests pass; all new tests pass.

### Review Findings

- [x] [Review][Patch] Duplicate track removal must respect requested counts [hifimule-daemon/src/providers/jellyfin.rs:320]
- [x] [Review][Patch] Missing PlaylistItemId is reported as successful removal [hifimule-daemon/src/providers/jellyfin.rs:320]
- [x] [Review][Patch] Playlist write endpoints build paths and query strings without URL encoding [hifimule-daemon/src/api.rs:1393]
- [x] [Review][Patch] Tests do not assert the create/add request contract fully [hifimule-daemon/src/providers/jellyfin.rs:1098]

## Dev Notes

### Critical: flip `supports_playlist_write` from `false` → `true`

Story 11.1's review resolved a "capability lie" by gating `supports_playlist_write` to `false` in both `jellyfin.rs` and `subsonic.rs` with the explicit comment `// Gated false until the playlist-write adapter lands (Story 11.2)`. **This story is that landing.** Two sites must be updated in `jellyfin.rs`:
- `capabilities()` impl at line 372–373 (runtime capability flag)
- `provider_exposes_capabilities` test at line 863 (expected value in assertion)

### `reqwest` DELETE calls

`reqwest::Client` supports `.delete(&url)` directly — same pattern as `.get(&url)` and `.post(&url)`. No additional imports required; `reqwest` is already a dependency.

### `PlaylistItemId` vs `Id` in Jellyfin playlist items

Jellyfin's `GET /Playlists/{id}/Items` response includes a `PlaylistItemId` per item — this is the **per-playlist entry identifier**, not the track's library `Id`. The DELETE endpoint (`/Playlists/{id}/Items?EntryIds=…`) requires the `PlaylistItemId` values, **not** the track IDs. The new `playlist_item_id: Option<String>` field on `JellyfinItem` captures this; it is `None` for all other response types because of `#[serde(default)]`.

### `JellyfinItem` uses `serde(rename_all = "PascalCase")` + `#[serde(default)]`

All fields in `JellyfinItem` are `PascalCase` in JSON (Jellyfin's convention) and the struct uses `#[serde(rename_all = "PascalCase")]`. The new field `playlist_item_id` will be deserialized from the `PlaylistItemId` JSON key automatically. Add `#[serde(default)]` so it deserializes as `None` for items in non-playlist contexts.

### Empty-slice short-circuit

Both `add_to_playlist` and `remove_from_playlist` must short-circuit with `Ok(())` when `track_ids` is empty — issuing requests with an empty Ids/EntryIds parameter would result in a server error. The deferred edge-case note from Story 11.1's review explicitly assigned this handling to 11.2.

### Error mapping: use `Self::map_error`

All new api.rs methods return `Result<..., anyhow::Error>`. In `jellyfin.rs`, map them with `.map_err(Self::map_error)` exactly as every other provider method does. `map_error` maps 401/403 to `Auth`, 404 to `NotFound`, parse failures to `Deserialization`, and everything else to `Http` or `Other`.

### Method insertion location in `jellyfin.rs`

Insert the four new trait methods immediately after `get_playlist` (currently ending around line 272). This keeps all playlist-related methods grouped together.

### New api.rs methods insertion location

Append all five new methods after the last existing public method in `JellyfinClient`'s `impl` block (around line 1290+, before the test `#[cfg(test)]` section). Follow the exact same structure as `report_item_played_at` (validate inputs, build headers, build endpoint, send, check status, return).

### Test mock ordering for `remove_from_playlist`

The `remove_from_playlist` test sets up two mocks (GET then DELETE). `mockito` matches requests in registration order. The GET mock must be registered first, the DELETE second. Both mocks use `create_async()`. The `_get` and `_delete` prefix-underscored variables keep the mocks alive for the test duration — if they're dropped early, mockito unregisters them.

### `provider_remove_from_playlist_skips_delete_when_no_entries_match` test design

This test registers a GET mock but **no DELETE mock**. In mockito, an unregistered request fails the test (unexpected request). This makes the "no DELETE issued" behavior verifiable without any explicit assertion — if the implementation incorrectly issues DELETE, mockito raises an error.

### Exact file locations

| File | Location | Change |
|------|----------|--------|
| `hifimule-daemon/src/api.rs` | `JellyfinItem` struct (~line 150, end of struct) | Add `playlist_item_id: Option<String>` |
| `hifimule-daemon/src/api.rs` | After last public `JellyfinClient` method | Add 5 new public methods |
| `hifimule-daemon/src/providers/jellyfin.rs` | Line 372–373 | Remove comment; `supports_playlist_write: true` |
| `hifimule-daemon/src/providers/jellyfin.rs` | After `get_playlist` (~line 272) | Add 4 trait methods |
| `hifimule-daemon/src/providers/jellyfin.rs` | Line 863 (`provider_exposes_capabilities` test) | `supports_playlist_write: true` |
| `hifimule-daemon/src/providers/jellyfin.rs` | After `provider_lists_and_gets_playlist_tracks` (~line 1023) | Add 5 new tests |

### What this story does NOT change

- `subsonic.rs` — Story 11.3 handles Subsonic; leave it untouched (`supports_playlist_write: false` stays).
- `rpc.rs` — No RPC wiring in this story; that is Story 11.4.
- UI files — No UI changes.
- `Cargo.toml` — No new dependencies. `reqwest`, `serde_json`, `mockito`, and `anyhow` are all present.
- `ProviderError` variants — Use existing variants only. Do NOT add `NotSupported`; the correct variant for unsupported capabilities is `UnsupportedCapability(String)` (Story 11.1 note still applies).

### References

- Epics Story 11.2: `_bmad-output/planning-artifacts/epics.md:2098–2132`
- Architecture Epic 11 section: `_bmad-output/planning-artifacts/architecture.md:523–593`
- Story 11.1 (done): `_bmad-output/implementation-artifacts/11-1-mediaprovider-playlist-write-trait-amendment.md`
- `JellyfinItem` struct: `hifimule-daemon/src/api.rs:104–150`
- `JellyfinClient.http_client()`: `hifimule-daemon/src/api.rs:207`
- `JellyfinProvider.capabilities()`: `hifimule-daemon/src/providers/jellyfin.rs:367–387`
- `JellyfinProvider.get_playlist()`: `hifimule-daemon/src/providers/jellyfin.rs:253–272`
- `map_error` / `map_not_found`: `hifimule-daemon/src/providers/jellyfin.rs:59–103`
- POST pattern (report_playback_session): `hifimule-daemon/src/api.rs:807–872`
- Mockito test pattern: `hifimule-daemon/src/providers/jellyfin.rs:973–1022`

## Dev Agent Record

### Agent Model Used

claude-sonnet-4-6

### Debug Log References

### Completion Notes List

- Added `playlist_item_id: Option<String>` to `JellyfinItem` struct with `#[serde(default)]`. Also patched 6 test-code struct literal construction sites in `auto_fill.rs`, `providers/jellyfin.rs`, and `sync.rs` that used struct syntax rather than deserialization (contrary to story note, these did exist).
- Added 5 new public async methods to `JellyfinClient` in `api.rs`: `create_playlist`, `add_tracks_to_playlist`, `get_playlist_items`, `delete_playlist_items`, `delete_item`.
- Flipped `supports_playlist_write: false → true` in `capabilities()` and in the `provider_exposes_capabilities` test assertion.
- Added 4 `MediaProvider` trait implementations in `jellyfin.rs` after `get_playlist`: `create_playlist`, `add_to_playlist`, `remove_from_playlist`, `delete_playlist`. Both `add_to_playlist` and `remove_from_playlist` short-circuit with `Ok(())` on empty `track_ids`.
- Added 5 new mockito tests: `provider_creates_playlist_returns_server_id`, `provider_add_to_playlist_posts_ids`, `provider_remove_from_playlist_resolves_entry_ids_then_deletes`, `provider_remove_from_playlist_skips_delete_when_no_entries_match`, `provider_delete_playlist_issues_delete_items_endpoint`.
- Review patches applied: duplicate removal now respects requested counts; matching playlist items missing `PlaylistItemId` now return a deserialization error instead of false success; playlist-write URLs now encode path/query components and reject comma-bearing comma-joined IDs; create/add tests assert request body/query contracts.
- All 399 tests pass; zero compilation errors.

### File List

- `hifimule-daemon/src/api.rs` — added `playlist_item_id` field to `JellyfinItem`; added 5 new `JellyfinClient` methods
- `hifimule-daemon/src/providers/jellyfin.rs` — flipped `supports_playlist_write` to `true` (capabilities + test); added 4 `MediaProvider` trait methods; added 5 new tests
- `hifimule-daemon/src/auto_fill.rs` — added `playlist_item_id: None` to test helper struct literal
- `hifimule-daemon/src/sync.rs` — added `playlist_item_id: None` to 3 test helper struct literals

## Change Log

- 2026-06-05: Story 11.2 implemented — JellyfinProvider playlist write adapter complete. Added `playlist_item_id` to `JellyfinItem`, 5 HTTP client methods, 4 `MediaProvider` trait implementations, `supports_playlist_write: true`, and 5 new tests (393 total, all passing).
- 2026-06-05: Code review patches complete — fixed duplicate-entry removal semantics, missing `PlaylistItemId` false success, playlist-write URL encoding, and request-contract test coverage (399 total, all passing).

## Status

done
