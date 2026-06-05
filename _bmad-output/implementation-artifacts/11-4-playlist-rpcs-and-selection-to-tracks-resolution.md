---
baseline_commit: 7416354
---

# Story 11.4: Playlist RPCs — playlist.create / addTracks / removeTracks / delete

Status: ready-for-dev

## Story

As a System Admin (Alexis),
I want the daemon to expose playlist management RPCs,
so that the UI can create and edit server playlists from the device selection basket.

## Acceptance Criteria

1. **Given** the active provider supports playlist write **When** `playlist.create({ name, itemIds })` is called **Then** the daemon resolves all basket entities (albums, artists, genres, individual tracks) in `itemIds` to a concrete flat track list using the existing container-expansion logic **And** Auto-Fill virtual slots (`id: '__auto_fill_slot__'`) are silently excluded from the resolved list **And** `provider.create_playlist(name, resolved_track_ids)` is called **And** the response returns `{ playlistId: string }` with the server-assigned ID.

2. **Given** a playlist exists **When** `playlist.addTracks({ playlistId, trackIds })` is called **Then** `provider.add_to_playlist(playlistId, trackIds)` is called with the provided track IDs directly (no entity resolution) **And** `{ ok: true }` is returned.

3. **Given** a playlist exists **When** `playlist.removeTracks({ playlistId, trackIds })` is called **Then** `provider.remove_from_playlist(playlistId, trackIds)` is called **And** `{ ok: true }` is returned.

4. **Given** a playlist exists **When** `playlist.delete({ playlistId })` is called **Then** `provider.delete_playlist(playlistId)` is called **And** `{ ok: true }` is returned.

5. **Given** the active provider does not support playlist write **When** any playlist write RPC is called **Then** an RPC error with code `ERR_UNSUPPORTED_CAPABILITY` (-5) is returned.

## Tasks / Subtasks

- [ ] Task 1: Add four playlist handler functions to `rpc.rs` (AC: 1–5)
  - [ ] Add the following four functions immediately before `async fn handle_server_connect` (line 823 in the baseline). Insert them as a block at line ~822:

    ```rust
    async fn handle_playlist_create(
        state: &AppState,
        params: Option<Value>,
    ) -> Result<Value, JsonRpcError> {
        let provider = require_provider(state).await?;
        if !provider.capabilities().supports_playlist_write {
            return Err(JsonRpcError {
                code: ERR_UNSUPPORTED_CAPABILITY,
                message: "Connected provider does not support playlist write".to_string(),
                data: None,
            });
        }
        let params = params.ok_or(JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: "Missing params".to_string(),
            data: None,
        })?;
        let name = params["name"]
            .as_str()
            .ok_or(JsonRpcError {
                code: ERR_INVALID_PARAMS,
                message: "Missing name".to_string(),
                data: None,
            })?
            .to_owned();
        let raw_ids = params["itemIds"].as_array().ok_or(JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: "Missing or invalid itemIds array".to_string(),
            data: None,
        })?;
        let item_ids: Vec<String> = raw_ids
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .filter(|id| id != "__auto_fill_slot__")
            .collect();

        let mut track_ids: Vec<String> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        for item_id in &item_ids {
            let (tracks, _playlist) =
                provider_sync_items_for_id(provider.clone(), item_id).await?;
            for track in tracks {
                if seen.insert(track.jellyfin_id.clone()) {
                    track_ids.push(track.jellyfin_id);
                }
            }
        }

        let playlist_id = provider
            .create_playlist(&name, &track_ids)
            .await
            .map_err(provider_error_to_rpc)?;
        Ok(serde_json::json!({ "playlistId": playlist_id }))
    }

    async fn handle_playlist_add_tracks(
        state: &AppState,
        params: Option<Value>,
    ) -> Result<Value, JsonRpcError> {
        let provider = require_provider(state).await?;
        if !provider.capabilities().supports_playlist_write {
            return Err(JsonRpcError {
                code: ERR_UNSUPPORTED_CAPABILITY,
                message: "Connected provider does not support playlist write".to_string(),
                data: None,
            });
        }
        let params = params.ok_or(JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: "Missing params".to_string(),
            data: None,
        })?;
        let playlist_id = params["playlistId"]
            .as_str()
            .ok_or(JsonRpcError {
                code: ERR_INVALID_PARAMS,
                message: "Missing playlistId".to_string(),
                data: None,
            })?
            .to_owned();
        let raw_ids = params["trackIds"].as_array().ok_or(JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: "Missing or invalid trackIds array".to_string(),
            data: None,
        })?;
        let track_ids: Vec<String> = raw_ids
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect();
        provider
            .add_to_playlist(&playlist_id, &track_ids)
            .await
            .map_err(provider_error_to_rpc)?;
        Ok(serde_json::json!({ "ok": true }))
    }

    async fn handle_playlist_remove_tracks(
        state: &AppState,
        params: Option<Value>,
    ) -> Result<Value, JsonRpcError> {
        let provider = require_provider(state).await?;
        if !provider.capabilities().supports_playlist_write {
            return Err(JsonRpcError {
                code: ERR_UNSUPPORTED_CAPABILITY,
                message: "Connected provider does not support playlist write".to_string(),
                data: None,
            });
        }
        let params = params.ok_or(JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: "Missing params".to_string(),
            data: None,
        })?;
        let playlist_id = params["playlistId"]
            .as_str()
            .ok_or(JsonRpcError {
                code: ERR_INVALID_PARAMS,
                message: "Missing playlistId".to_string(),
                data: None,
            })?
            .to_owned();
        let raw_ids = params["trackIds"].as_array().ok_or(JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: "Missing or invalid trackIds array".to_string(),
            data: None,
        })?;
        let track_ids: Vec<String> = raw_ids
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect();
        provider
            .remove_from_playlist(&playlist_id, &track_ids)
            .await
            .map_err(provider_error_to_rpc)?;
        Ok(serde_json::json!({ "ok": true }))
    }

    async fn handle_playlist_delete(
        state: &AppState,
        params: Option<Value>,
    ) -> Result<Value, JsonRpcError> {
        let provider = require_provider(state).await?;
        if !provider.capabilities().supports_playlist_write {
            return Err(JsonRpcError {
                code: ERR_UNSUPPORTED_CAPABILITY,
                message: "Connected provider does not support playlist write".to_string(),
                data: None,
            });
        }
        let params = params.ok_or(JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: "Missing params".to_string(),
            data: None,
        })?;
        let playlist_id = params["playlistId"]
            .as_str()
            .ok_or(JsonRpcError {
                code: ERR_INVALID_PARAMS,
                message: "Missing playlistId".to_string(),
                data: None,
            })?
            .to_owned();
        provider
            .delete_playlist(&playlist_id)
            .await
            .map_err(provider_error_to_rpc)?;
        Ok(serde_json::json!({ "ok": true }))
    }
    ```

  - **Key notes:**
    - `capabilities()` is a synchronous method on `MediaProvider` — call it as `provider.capabilities().supports_playlist_write` (no `.await`). See existing usage at `rpc.rs:530`.
    - `HashSet` is already imported at the top of `rpc.rs` (`use std::collections::HashSet`). No new import needed.
    - `provider_sync_items_for_id` is a private `async fn` in the same file (line 1667). It resolves an ID by trying `get_album`, `get_playlist`, `get_artist`, `get_song`, then `get_genre_tracks` in that order. The `_playlist` in the destructure is intentionally ignored — for `playlist.create` we only need the track list, not the `PlaylistSyncItem` wrapper.
    - The `__auto_fill_slot__` filter string matches the constant `AUTO_FILL_SLOT_ID = '__auto_fill_slot__'` defined in the UI at `hifimule-ui/src/state/basket.ts:6`. There is no daemon-side constant; the string is compared inline.
    - Duplicate tracks that appear across different container entities are deduplicated using the `seen: HashSet<String>` set. This matches the dedup pattern already used in `provider_calculate_delta` (lines 1754–1784).
    - `playlist.addTracks` and `playlist.removeTracks` pass `trackIds` directly to the provider with **no entity resolution**. Callers supply concrete track IDs.
    - All four handlers check capability before parsing params — matching the pattern used by browse handlers.

- [ ] Task 2: Wire the four new RPCs into the dispatch match in `handler()` (AC: 1–5)
  - [ ] In `handler()` at `rpc.rs:292`, add four new arms immediately before the `_ =>` fallback (currently at line 367). Place them after the last `"browse.listFavoriteItems"` arm (lines 364–366):

    ```rust
    "playlist.create" => handle_playlist_create(&state, payload.params).await,
    "playlist.addTracks" => handle_playlist_add_tracks(&state, payload.params).await,
    "playlist.removeTracks" => handle_playlist_remove_tracks(&state, payload.params).await,
    "playlist.delete" => handle_playlist_delete(&state, payload.params).await,
    ```

- [ ] Task 3: Add tests (AC: 1–5)
  - [ ] Add at the end of the `mod tests` block (before the closing `}` at line 8300). First define `FakePlaylistProvider`, then add six tests.

    ```rust
    // --- FakePlaylistProvider for playlist RPC tests ---

    struct FakePlaylistProvider {
        songs: HashMap<String, crate::domain::models::Song>,
        playlist_return_id: String,
        create_calls: Mutex<Vec<(String, Vec<String>)>>,
        add_calls: Mutex<Vec<(String, Vec<String>)>>,
        remove_calls: Mutex<Vec<(String, Vec<String>)>>,
        delete_calls: Mutex<Vec<String>>,
    }

    impl FakePlaylistProvider {
        fn new(playlist_return_id: &str) -> Arc<Self> {
            Arc::new(Self {
                songs: HashMap::new(),
                playlist_return_id: playlist_return_id.to_string(),
                create_calls: Mutex::new(vec![]),
                add_calls: Mutex::new(vec![]),
                remove_calls: Mutex::new(vec![]),
                delete_calls: Mutex::new(vec![]),
            })
        }

        fn with_song(playlist_return_id: &str, song: crate::domain::models::Song) -> Arc<Self> {
            let mut songs = HashMap::new();
            songs.insert(song.id.clone(), song);
            Arc::new(Self {
                songs,
                playlist_return_id: playlist_return_id.to_string(),
                create_calls: Mutex::new(vec![]),
                add_calls: Mutex::new(vec![]),
                remove_calls: Mutex::new(vec![]),
                delete_calls: Mutex::new(vec![]),
            })
        }
    }

    #[async_trait::async_trait]
    impl MediaProvider for FakePlaylistProvider {
        async fn list_libraries(&self) -> Result<Vec<crate::domain::models::Library>, ProviderError> { unimplemented!() }
        async fn list_artists(&self, _: Option<&str>, _: Option<&str>, _: u32, _: u32) -> Result<(Vec<crate::domain::models::Artist>, u32), ProviderError> { unimplemented!() }
        async fn get_artist(&self, _: &str) -> Result<crate::domain::models::ArtistWithAlbums, ProviderError> {
            Err(ProviderError::UnsupportedCapability("no artists".to_string()))
        }
        async fn list_albums(&self, _: Option<&str>, _: Option<&str>, _: u32, _: u32) -> Result<(Vec<crate::domain::models::Album>, u32), ProviderError> { unimplemented!() }
        async fn get_album(&self, _: &str) -> Result<crate::domain::models::AlbumWithTracks, ProviderError> {
            Err(ProviderError::UnsupportedCapability("no albums".to_string()))
        }
        async fn get_song(&self, song_id: &str) -> Result<crate::domain::models::Song, ProviderError> {
            self.songs.get(song_id).cloned().ok_or(ProviderError::NotFound {
                item_type: "Song".to_string(),
                id: song_id.to_string(),
            })
        }
        async fn list_playlists(&self) -> Result<Vec<crate::domain::models::Playlist>, ProviderError> { unimplemented!() }
        async fn get_playlist(&self, _: &str) -> Result<crate::domain::models::PlaylistWithTracks, ProviderError> {
            Err(ProviderError::UnsupportedCapability("no playlists".to_string()))
        }
        async fn search(&self, _: &str) -> Result<crate::domain::models::SearchResult, ProviderError> { unimplemented!() }
        async fn download_url(&self, _: &str, _: Option<&crate::providers::TranscodeProfile>) -> Result<String, ProviderError> { unimplemented!() }
        async fn cover_art_url(&self, _: &str) -> Result<String, ProviderError> { unimplemented!() }
        async fn changes_since_with_context(&self, _: Option<&str>, _: &crate::providers::ProviderChangeContext) -> Result<Vec<crate::domain::models::ChangeEvent>, ProviderError> { unimplemented!() }
        async fn scrobble(&self, _: crate::providers::ScrobbleRequest) -> Result<(), ProviderError> { unimplemented!() }
        async fn list_genres(&self, _: Option<&str>, _: u32, _: u32) -> Result<(Vec<crate::domain::models::Genre>, u64), ProviderError> {
            Ok((vec![], 0))
        }
        async fn get_genre_tracks(&self, _: &str, _: u32, _: u32) -> Result<(Vec<crate::domain::models::Song>, u32), ProviderError> {
            Ok((vec![], 0))
        }
        fn server_type(&self) -> crate::providers::ServerType { crate::providers::ServerType::Subsonic }
        fn capabilities(&self) -> crate::providers::Capabilities {
            crate::providers::Capabilities {
                open_subsonic: false,
                supports_changes_since: false,
                supports_server_transcoding: false,
                supports_playlist_write: true,
                browse: crate::providers::BrowseCapabilities { list_modes: vec![] },
            }
        }
        async fn create_playlist(&self, name: &str, track_ids: &[String]) -> Result<String, ProviderError> {
            self.create_calls.lock().unwrap().push((name.to_string(), track_ids.to_vec()));
            Ok(self.playlist_return_id.clone())
        }
        async fn add_to_playlist(&self, playlist_id: &str, track_ids: &[String]) -> Result<(), ProviderError> {
            self.add_calls.lock().unwrap().push((playlist_id.to_string(), track_ids.to_vec()));
            Ok(())
        }
        async fn remove_from_playlist(&self, playlist_id: &str, track_ids: &[String]) -> Result<(), ProviderError> {
            self.remove_calls.lock().unwrap().push((playlist_id.to_string(), track_ids.to_vec()));
            Ok(())
        }
        async fn delete_playlist(&self, playlist_id: &str) -> Result<(), ProviderError> {
            self.delete_calls.lock().unwrap().push(playlist_id.to_string());
            Ok(())
        }
    }

    #[tokio::test]
    async fn playlist_create_resolves_song_and_returns_server_id() {
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let state = make_test_state(db);
        let song = crate::domain::models::Song {
            id: "song1".to_string(),
            title: "Track 1".to_string(),
            artist_id: None,
            artist_name: Some("Artist".to_string()),
            album_id: Some("album-1".to_string()),
            album_title: Some("Album".to_string()),
            duration_seconds: 180,
            bitrate_kbps: Some(320),
            track_number: Some(1),
            disc_number: Some(1),
            cover_art_id: None,
            date_added: None,
            last_played_at: None,
            play_count: None,
            is_favorite: None,
            content_type: Some("audio/mpeg".to_string()),
            suffix: Some("mp3".to_string()),
        };
        let provider = FakePlaylistProvider::with_song("playlist-42", song);
        *state.provider.write().await = Some(provider.clone() as Arc<dyn MediaProvider>);

        let result = handle_playlist_create(
            &state,
            Some(serde_json::json!({ "name": "My Playlist", "itemIds": ["song1"] })),
        )
        .await
        .expect("playlist.create");

        assert_eq!(result["playlistId"], "playlist-42");
        let calls = provider.create_calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "My Playlist");
        assert_eq!(calls[0].1, vec!["song1"]);
    }

    #[tokio::test]
    async fn playlist_create_excludes_auto_fill_slot() {
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let state = make_test_state(db);
        let provider = FakePlaylistProvider::new("playlist-99");
        *state.provider.write().await = Some(provider.clone() as Arc<dyn MediaProvider>);

        // Only item is the auto-fill slot — should be filtered; create_playlist called with empty list.
        let result = handle_playlist_create(
            &state,
            Some(serde_json::json!({ "name": "Auto", "itemIds": ["__auto_fill_slot__"] })),
        )
        .await
        .expect("playlist.create with only auto-fill slot");

        assert_eq!(result["playlistId"], "playlist-99");
        let calls = provider.create_calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert!(calls[0].1.is_empty(), "auto-fill slot must be excluded");
    }

    #[tokio::test]
    async fn playlist_add_tracks_passes_ids_directly_without_resolution() {
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let state = make_test_state(db);
        let provider = FakePlaylistProvider::new("ignored");
        *state.provider.write().await = Some(provider.clone() as Arc<dyn MediaProvider>);

        let result = handle_playlist_add_tracks(
            &state,
            Some(serde_json::json!({ "playlistId": "p1", "trackIds": ["t1", "t2"] })),
        )
        .await
        .expect("playlist.addTracks");

        assert_eq!(result["ok"], true);
        let calls = provider.add_calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "p1");
        assert_eq!(calls[0].1, vec!["t1", "t2"]);
    }

    #[tokio::test]
    async fn playlist_remove_tracks_passes_ids_directly_without_resolution() {
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let state = make_test_state(db);
        let provider = FakePlaylistProvider::new("ignored");
        *state.provider.write().await = Some(provider.clone() as Arc<dyn MediaProvider>);

        let result = handle_playlist_remove_tracks(
            &state,
            Some(serde_json::json!({ "playlistId": "p2", "trackIds": ["t3"] })),
        )
        .await
        .expect("playlist.removeTracks");

        assert_eq!(result["ok"], true);
        let calls = provider.remove_calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "p2");
        assert_eq!(calls[0].1, vec!["t3"]);
    }

    #[tokio::test]
    async fn playlist_delete_passes_playlist_id() {
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let state = make_test_state(db);
        let provider = FakePlaylistProvider::new("ignored");
        *state.provider.write().await = Some(provider.clone() as Arc<dyn MediaProvider>);

        let result = handle_playlist_delete(
            &state,
            Some(serde_json::json!({ "playlistId": "p3" })),
        )
        .await
        .expect("playlist.delete");

        assert_eq!(result["ok"], true);
        let calls = provider.delete_calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0], "p3");
    }

    #[tokio::test]
    async fn playlist_write_rpcs_return_unsupported_when_capability_false() {
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let state = make_test_state(db);
        // FakeBrowseProvider has supports_playlist_write: false
        let provider = FakeBrowseProvider::new(vec![], vec![]);
        *state.provider.write().await = Some(provider as Arc<dyn MediaProvider>);

        let dummy_create_params =
            Some(serde_json::json!({ "name": "x", "itemIds": [] }));
        let dummy_modify_params =
            Some(serde_json::json!({ "playlistId": "p", "trackIds": [] }));
        let dummy_delete_params = Some(serde_json::json!({ "playlistId": "p" }));

        let create_err = handle_playlist_create(&state, dummy_create_params)
            .await
            .expect_err("create should fail");
        assert_eq!(create_err.code, ERR_UNSUPPORTED_CAPABILITY);

        let add_err = handle_playlist_add_tracks(&state, dummy_modify_params.clone())
            .await
            .expect_err("addTracks should fail");
        assert_eq!(add_err.code, ERR_UNSUPPORTED_CAPABILITY);

        let remove_err = handle_playlist_remove_tracks(&state, dummy_modify_params)
            .await
            .expect_err("removeTracks should fail");
        assert_eq!(remove_err.code, ERR_UNSUPPORTED_CAPABILITY);

        let delete_err = handle_playlist_delete(&state, dummy_delete_params)
            .await
            .expect_err("delete should fail");
        assert_eq!(delete_err.code, ERR_UNSUPPORTED_CAPABILITY);
    }
    ```

  - **Key notes:**
    - `FakePlaylistProvider` uses `Mutex<Vec<...>>` (std, not tokio) for call capture since the `create_playlist` etc. methods take `&self`. Hold an `Arc<FakePlaylistProvider>` in the test alongside the `Arc<dyn MediaProvider>` stored in state — both point to the same allocation, so post-call assertions work.
    - `get_song` returns `ProviderError::NotFound` (not `UnsupportedCapability`) for unknown IDs. `provider_sync_items_for_id` treats `NotFound` the same as `UnsupportedCapability` at the `get_song` step (line 1724: `Err(ProviderError::UnsupportedCapability(_)) | Err(ProviderError::NotFound { .. }) => {}`) and falls through to genre. `get_genre_tracks` returns `(vec![], 0)`, so the genre path also yields nothing. For an unknown ID, `provider_sync_items_for_id` will ultimately return an `Err` because no handler succeeded — **do not pass unknown song IDs to `playlist.create` in tests**.
    - `FakePlaylistProvider` must implement `get_genre_tracks` to return `Ok((vec![], 0))` (not `unimplemented!()`) because `provider_sync_items_for_id` always tries the genre path last when no other handler matched. If `get_genre_tracks` panics, the `playlist_create_excludes_auto_fill_slot` test (which has no items after filtering) won't hit this path — but the `with_song` test might depending on song resolution order. Keep it non-panicking.
    - The `FakePlaylistProvider::get_song` returns `NotFound` for unknowns. In the `playlist_create_excludes_auto_fill_slot` test, `item_ids` is empty after filtering, so `provider_sync_items_for_id` is never called at all — `create_playlist` is called directly with `&[]`.
    - `use std::sync::Mutex;` is already imported in the `rpc.rs` test module or available from std — no new import needed in the test module.
    - `make_test_state(db)` is the existing test helper in `mod tests`. Use it as-is.

- [ ] Task 4: Verify compilation and tests (AC: all)
  - [ ] Run `rtk cargo check` — zero errors.
  - [ ] Run `rtk cargo test` — all existing tests pass; all six new tests pass.

## Dev Notes

### Single file, all changes in `hifimule-daemon/src/rpc.rs`

This story is entirely contained in `hifimule-daemon/src/rpc.rs`. No other files are touched:
- `providers/mod.rs` — already has all four `MediaProvider` methods with default impls (Stories 11.1–11.3).
- `providers/jellyfin.rs` and `providers/subsonic.rs` — already implemented (Stories 11.2 and 11.3).
- UI files — no UI changes in this story (that is Story 11.5).

### Dispatch pattern

The `handler()` function at line 292 is a single `match` block. Every RPC is one match arm: `"method.name" => handle_fn(&state, payload.params).await`. Add the four new arms just before the `_ =>` fallback arm. This matches every existing pattern in the file.

### Capability check: sync call, not async

`capabilities()` on `MediaProvider` is **not async** (defined at `providers/mod.rs:256` as `fn capabilities(&self) -> Capabilities`). Call it as `provider.capabilities().supports_playlist_write` — no `.await`. This is confirmed by existing usage at `rpc.rs:530`.

### `require_provider` error and capability check ordering

Call `require_provider(state)?` first (AC 5 requires returning an error, but only if a provider exists with the wrong capability; `require_provider` handles the "no provider" case). Then check `capabilities().supports_playlist_write`. This ordering matches how all browse handlers work.

### Container expansion for `playlist.create` — reuse `provider_sync_items_for_id`

The architecture spec says: "Container-expansion reuses the existing `rpc.rs:807–866` path." In the current file (post-11.3), this logic lives at lines 1667–1737 as `async fn provider_sync_items_for_id`. It resolves a single ID by trying (in order): `get_album`, `get_playlist`, `get_artist`, `get_song`, genre tracks. Call it once per item ID.

The `_playlist` return value (a `PlaylistSyncItem` for M3U generation) is intentionally unused in `playlist.create` — we only need the `Vec<DesiredItem>` and extract `jellyfin_id` from each.

### Auto-Fill slot filtering

Filter `"__auto_fill_slot__"` from `item_ids` **before** any provider call. The literal string matches `AUTO_FILL_SLOT_ID` defined in `hifimule-ui/src/state/basket.ts:6`. No daemon-side constant exists — compare inline.

If all items are filtered out (basket contains only an auto-fill slot), `track_ids` will be empty and `create_playlist(name, &[])` is called. The provider layer already handles the empty-slice case (both adapters short-circuit for empty `track_ids` in `add_to_playlist`/`remove_from_playlist`; `create_playlist` with empty IDs creates an empty playlist, which is valid server behavior).

### Deduplication in `playlist.create`

When `itemIds` contains overlapping entities (e.g., an artist AND one of their albums), `provider_sync_items_for_id` will return the album tracks from both entries. Deduplicate using a `HashSet<String>` on `jellyfin_id` before passing to `create_playlist`. This matches the dedup pattern in `provider_calculate_delta` at lines 1754–1784.

### `HashSet` import

`HashSet` is already imported at the top of `rpc.rs` (`use std::collections::{HashMap, HashSet}`). No new import needed.

### `playlist.addTracks` / `playlist.removeTracks` — no entity resolution

These two handlers pass `trackIds` directly to the provider. The UI is responsible for providing concrete track IDs (not container IDs). No `provider_sync_items_for_id` call.

### `provider_error_to_rpc` handles `ProviderError::UnsupportedCapability`

Looking at lines 472–495, `provider_error_to_rpc` maps `ProviderError::UnsupportedCapability(msg)` → `ERR_UNSUPPORTED_CAPABILITY (-5)`. So if a provider's `create_playlist` returns `UnsupportedCapability` at runtime (unexpected, since capability was checked), it will surface correctly. The capability pre-check at the handler level catches the expected case before any provider call.

### `ERR_UNSUPPORTED_CAPABILITY` constant

Defined at `rpc.rs:36` as `const ERR_UNSUPPORTED_CAPABILITY: i32 = -5`. Available throughout the file. Use it directly in the capability check error return.

### Test: `FakePlaylistProvider` implements `get_genre_tracks` returning empty, not `unimplemented!()`

`provider_sync_items_for_id` calls `provider_genre_sync_items_for_id` (line 1728) as its last fallback. `provider_genre_sync_items_for_id` calls `provider.get_genre_tracks()`. If that panics, any test that triggers entity resolution for an unknown ID will blow up. For `FakePlaylistProvider`, return `Ok((vec![], 0))` from `get_genre_tracks` so the function path completes gracefully (returning the outer `Err` at line 1732 for truly unknown IDs, or resolving correctly for known songs).

### Test: `Arc<FakePlaylistProvider>` vs `Arc<dyn MediaProvider>`

The test holds `let provider = FakePlaylistProvider::new("...")` as `Arc<FakePlaylistProvider>`. It passes `provider.clone() as Arc<dyn MediaProvider>` to the state. After the handler returns, it locks `provider.create_calls` etc. directly — this works because `Arc::clone` shares the same allocation, so the Mutex state is shared.

### What this story does NOT change

- `providers/mod.rs` — the trait already has default `UnsupportedCapability` impls for the four write methods. Do not modify it.
- `providers/jellyfin.rs` / `providers/subsonic.rs` — already implemented. Leave untouched.
- UI files — no UI in this story.
- `Cargo.toml` — no new dependencies.
- Any other `.rs` files.

### Project Structure Notes

All changes are in `hifimule-daemon/src/rpc.rs`. The file already contains ~8300 lines; new additions:
- ~150 lines of handler functions (four handlers)
- ~4 lines in the dispatch match
- ~180 lines of tests (`FakePlaylistProvider` struct + 6 test functions)

No architectural conflicts. The handlers follow the exact same structure as every browse handler in the same file.

### References

- Epic 11 Story 11.4 spec: `_bmad-output/planning-artifacts/epics.md:2172–2211`
- Architecture Epic 11 (Daemon RPCs section): `_bmad-output/planning-artifacts/architecture.md:566–592`
- `handler()` dispatch match: `hifimule-daemon/src/rpc.rs:292–372`
- `require_provider`: `hifimule-daemon/src/rpc.rs:464–470`
- `provider_error_to_rpc`: `hifimule-daemon/src/rpc.rs:472–495`
- `ERR_UNSUPPORTED_CAPABILITY`: `hifimule-daemon/src/rpc.rs:36`
- `capabilities()` sync call pattern: `hifimule-daemon/src/rpc.rs:530`
- `provider_sync_items_for_id`: `hifimule-daemon/src/rpc.rs:1667–1737`
- `provider_calculate_delta` (dedup pattern): `hifimule-daemon/src/rpc.rs:1754–1784`
- `HashSet` import: `hifimule-daemon/src/rpc.rs` top-level imports
- `MediaProvider` trait playlist write methods (with default `UnsupportedCapability` impls): `hifimule-daemon/src/providers/mod.rs:199–236`
- `Capabilities` struct: `hifimule-daemon/src/providers/mod.rs`
- `BrowseCapabilities` struct: `hifimule-daemon/src/providers/mod.rs`
- `AUTO_FILL_SLOT_ID` constant (UI): `hifimule-ui/src/state/basket.ts:6`
- `FakeBrowseProvider` test struct (for `supports_playlist_write: false` test): `hifimule-daemon/src/rpc.rs:7855`
- `make_test_state` helper: `hifimule-daemon/src/rpc.rs` (in `mod tests`)
- Story 11.3 (previous, for learnings): `_bmad-output/implementation-artifacts/11-3-subsonicprovider-playlist-write-adapter.md`

## Dev Agent Record

### Agent Model Used

{{agent_model_name_version}}

### Debug Log References

### Completion Notes List

### File List

- hifimule-daemon/src/rpc.rs

## Change Log

- 2026-06-05: Story 11.4 created — Playlist RPCs and selection-to-tracks resolution ready for dev.

## Status

ready-for-dev
