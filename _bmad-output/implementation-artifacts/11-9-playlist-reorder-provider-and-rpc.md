---
baseline_commit: 7a63bb5
---

# Story 11.9: MediaProvider Reorder Contract — Trait, Adapters & RPC

Status: done

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a Ritualist (Arthur),
I want the daemon to reorder tracks in a server playlist,
so that my curated playlist plays in the sequence I intend.

## Acceptance Criteria

1. **Given** the active provider supports playlist write **When** `reorder_playlist(playlist_id, ordered_track_ids)` is called **Then** the playlist's tracks are set to exactly that order — the same track set, only the sequence changes.

2. **Given** a Jellyfin provider **When** `reorder_playlist` is called **Then** the current playlist entries are fetched and `POST /Playlists/{id}/Items/{playlistItemId}/Move/{index}` is issued per out-of-place entry until the playlist matches `ordered_track_ids` **And** no entries are removed or re-created (item identity / `PlaylistItemId` is preserved).

3. **Given** a Subsonic/OpenSubsonic provider **When** `reorder_playlist` is called **Then** `createPlaylist?playlistId={id}&songId=…` is issued with the song IDs in the requested order, replacing the playlist contents in that order.

4. **Given** a provider that does not support playlist write **When** `reorder_playlist` is called **Then** `ProviderError::UnsupportedCapability` is returned (the trait default).

5. **Given** the daemon receives `playlist.reorder({ playlistId, trackIds })` **Then** it verifies `supports_playlist_write`, calls `reorder_playlist`, and returns `{ ok: true }`; a request against a capability-absent provider is rejected with `ERR_UNSUPPORTED_CAPABILITY`.

> **⚠️ Spec correction:** Epic 11.9's AC text and the sprint-change-proposal both say `ProviderError::NotSupported`. **That variant does not exist** in this codebase. The real variant is `ProviderError::UnsupportedCapability(String)` (`providers/mod.rs:385`), which every sibling playlist-write default already returns. Use `UnsupportedCapability` — AC4 above is corrected to match reality.

## Tasks / Subtasks

### Task 1: Add `reorder_playlist` default to the `MediaProvider` trait (AC: #1, #4)

**File:** `hifimule-daemon/src/providers/mod.rs`

Add after the `rename_playlist` default (ends at line 246), alongside the other playlist-write methods, following the **exact** sibling pattern:

```rust
async fn reorder_playlist(
    &self,
    _playlist_id: &str,
    _ordered_track_ids: &[String],
) -> Result<(), ProviderError> {
    Err(ProviderError::UnsupportedCapability(
        "reorder_playlist is not supported by this provider".to_string(),
    ))
}
```

This is identical in shape to `add_to_playlist` (`mod.rs:209`), `remove_from_playlist` (`mod.rs:219`), `delete_playlist` (`mod.rs:229`), and `rename_playlist` (`mod.rs:238`).

**Also add the trait-default unit test** in the `#[cfg(test)]` block, next to the existing `trait_default_*_returns_unsupported` tests (these live around `mod.rs:920-1040` — search for `trait_default_rename_playlist_returns_unsupported` added in Story 11.8 and mirror it):

```rust
#[tokio::test]
async fn trait_default_reorder_playlist_returns_unsupported() {
    let provider = MockProvider::default();
    let result = provider
        .reorder_playlist("playlist-1", &["a".to_string(), "b".to_string()])
        .await;
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("reorder_playlist"), "message should name the method: {msg}");
}
```

(The exact mock type name is whatever the sibling tests use — confirm by reading the block, Story 11.8 used `MockProvider::default()`.)

### Task 2: Add `move_playlist_item` to `JellyfinApiClient` in `api.rs` (AC: #2)

**File:** `hifimule-daemon/src/api.rs`

There is **no** Jellyfin Move method yet — it must be created. Add it after `delete_playlist_items` (ends `api.rs:1490`) or near `delete_item`. It calls `POST /Playlists/{playlistId}/Items/{playlistItemId}/Move/{newIndex}` with no body:

```rust
pub async fn move_playlist_item(
    &self,
    url: &str,
    token: &str,
    playlist_id: &str,
    playlist_item_id: &str,
    new_index: usize,
) -> Result<()> {
    CredentialManager::validate_url(url)?;
    CredentialManager::validate_token(token)?;

    let mut headers = HeaderMap::new();
    headers.insert(
        "X-Emby-Token",
        HeaderValue::from_str(token).map_err(|_| anyhow!("Invalid token format"))?,
    );

    let index_str = new_index.to_string();
    let endpoint = jellyfin_endpoint(
        url,
        &["Playlists", playlist_id, "Items", playlist_item_id, "Move", &index_str],
    )?;

    let response = self.client.post(endpoint).headers(headers).send().await?;
    let status = response.status();
    if !status.is_success() {
        let text = response.text().await.unwrap_or_default();
        return Err(anyhow!("Server returned status: {} — {}", status, text));
    }
    Ok(())
}
```

**Critical notes:**
- `jellyfin_endpoint(url, &[segments…])` percent-encodes each path segment — it is the same helper used by every playlist method (`api.rs:1369`, `1416`, `1447`, `1478`, `1502`). Pass the index as a `&str` segment (bind `index_str` to a `let` first so the borrow lives long enough — note `add_tracks_to_playlist`/`delete_playlist_items` use query pairs, but Move puts the index in the **path**, so it's a path segment).
- `HeaderMap` / `HeaderValue` / `anyhow!` are already imported in `api.rs` — no new `use`.
- Move takes **no request body** — `.post(endpoint).headers(headers).send()` with nothing else, like `add_tracks_to_playlist` (`api.rs:1422`).

### Task 3: Implement `reorder_playlist` in `JellyfinProvider` (AC: #1, #2)

**File:** `hifimule-daemon/src/providers/jellyfin.rs`

Add after `rename_playlist` (`jellyfin.rs:400-405`). This is the **only non-trivial piece** in the story — a selection sort that maps target track IDs to their `PlaylistItemId` entries and moves out-of-place entries into position, maintaining a local mirror of the order so each subsequent move computes against the correct current state.

```rust
async fn reorder_playlist(
    &self,
    playlist_id: &str,
    ordered_track_ids: &[String],
) -> Result<(), ProviderError> {
    // Fetch current entries (each carries track id + PlaylistItemId, in server order)
    let items = self
        .client
        .get_playlist_items(self.url(), self.token(), self.user_id(), playlist_id)
        .await
        .map_err(Self::map_error)?;

    // Local mirror: (track_id, playlist_item_id) in current order.
    // Any entry missing a PlaylistItemId is a malformed response — same guard remove_from_playlist uses.
    let mut current: Vec<(String, String)> = Vec::with_capacity(items.len());
    for item in items {
        match item.playlist_item_id {
            Some(entry_id) => current.push((item.id, entry_id)),
            None => {
                return Err(ProviderError::Deserialization(format!(
                    "Playlist item missing PlaylistItemId: {}",
                    item.id
                )))
            }
        }
    }

    // Selection sort against the requested order. For position i, find — at or after i —
    // the first mirror entry whose track id equals ordered_track_ids[i] (consumes duplicates
    // left-to-right), move it to index i on the server, and mirror the move locally.
    for (target_index, wanted_track_id) in ordered_track_ids.iter().enumerate() {
        if target_index >= current.len() {
            break;
        }
        if &current[target_index].0 == wanted_track_id {
            continue; // already in place
        }
        let Some(found) = (target_index..current.len())
            .find(|&j| &current[j].0 == wanted_track_id)
        else {
            // Requested id not present in the playlist's remaining entries → set mismatch.
            return Err(ProviderError::UnsupportedCapability(format!(
                "reorder_playlist: track {wanted_track_id} is not in playlist {playlist_id}"
            )));
        };

        let (_, entry_id) = current[found].clone();
        self.client
            .move_playlist_item(self.url(), self.token(), playlist_id, &entry_id, target_index)
            .await
            .map_err(Self::map_error)?;

        // Mirror the move: remove from `found`, reinsert at `target_index`.
        let moved = current.remove(found);
        current.insert(target_index, moved);
    }

    Ok(())
}
```

**Critical notes:**
- `JellyfinItem` has `id: String` (the track/song id, used as `item.id` in `remove_from_playlist` at `jellyfin.rs:362`) and `playlist_item_id: Option<String>` (the per-entry id, `api.rs:173`). The Move endpoint addresses the **entry** (`PlaylistItemId`), the order list is **track ids** — you must map between them, exactly as `remove_from_playlist` does (`jellyfin.rs:348-381`).
- Reuse `get_playlist_items` (`api.rs:1431`) — do **not** invent a new fetch. It returns `Vec<JellyfinItem>` in playlist order.
- `self.url()`, `self.token()`, `self.user_id()`, and `Self::map_error` (`jellyfin.rs:60`) are the standard accessors/error mapper used by every method in this file.
- **Why a local mirror:** Jellyfin's `Move/{index}` reorders the live playlist. After moving an entry to index `i`, the positions of the entries you have not yet placed shift. Mirroring the same `remove`/`insert` locally keeps your `current` model in sync so the next iteration's "find at or after `i`" and the next move index are correct. Do **not** recompute against the original fetch.
- The decision to treat a missing track id as an error (rather than silently skipping) honors AC1 ("same track set, only the sequence changes"). The frontend (11.10) always sends the full current id list, so a mismatch indicates a real desync and should surface, not corrupt order.
- Empty `ordered_track_ids` → the loop body never runs → `Ok(())`. A single-track or already-ordered playlist issues zero Move calls. Good.

### Task 4: Implement `reorder_playlist` in `SubsonicProvider` + client method (AC: #1, #3)

**File:** `hifimule-daemon/src/providers/subsonic.rs`

**4a — Provider impl.** Add after `rename_playlist` (`subsonic.rs:427-431`):

```rust
async fn reorder_playlist(
    &self,
    playlist_id: &str,
    ordered_track_ids: &[String],
) -> Result<(), ProviderError> {
    self.client
        .set_playlist_order(playlist_id, ordered_track_ids)
        .await
}
```

**4b — Client method.** Add to `SubsonicApiClient` after `update_playlist_rename` (`subsonic.rs:909-918`). Subsonic's `createPlaylist` replaces a playlist's contents when given an existing `playlistId` plus ordered `songId` params — this is the one-call reorder:

```rust
async fn set_playlist_order(
    &self,
    playlist_id: &str,
    ordered_track_ids: &[String],
) -> Result<(), ProviderError> {
    let mut params: Vec<(&str, &str)> = vec![("playlistId", playlist_id)];
    for id in ordered_track_ids {
        params.push(("songId", id.as_str()));
    }
    // createPlaylist with an existing playlistId replaces the song list in the given order.
    // It echoes the playlist body; deserialize-and-discard with the same type create_playlist uses.
    let _: PlaylistWithSongsBody = self.get("createPlaylist", &params).await?;
    Ok(())
}
```

**Critical notes:**
- Model the param building on the existing `create_playlist` client method (`subsonic.rs:859-870`) — same `("songId", id)` loop. The **only** difference: pass `("playlistId", playlist_id)` instead of `("name", name)`.
- `createPlaylist` returns a playlist body, so deserialize as `PlaylistWithSongsBody` (the type `create_playlist` already uses at `subsonic.rs:868`) and discard — **not** `NoBody`. `NoBody` is for the `updatePlaylist`/`deletePlaylist` endpoints that return an empty envelope; `createPlaylist` returns the playlist. (If a real server is found to return an empty `createPlaylist` body, fall back to `NoBody` — but default to `PlaylistWithSongsBody` to match the sibling.)
- `self.get(...)` already runs every request through `sanitize_subsonic_url()` for logging (`subsonic.rs` — same path all client methods use). No extra sanitization needed.
- Note: `createPlaylist` with `playlistId` keeps the same playlist id and name; it only rewrites the song list. This is the documented Subsonic/OpenSubsonic behavior and is what Navidrome implements.

### Task 5: Add the `playlist.reorder` RPC handler (AC: #5)

**File:** `hifimule-daemon/src/rpc.rs`

**5a — Dispatch.** Add to the match table after `"playlist.rename"` (`rpc.rs:373`):

```rust
"playlist.reorder" => handle_playlist_reorder(&state, payload.params).await,
```

**5b — Handler.** Add after `handle_playlist_rename` (ends `rpc.rs:1163`). This is structurally `handle_playlist_add_tracks`/`handle_playlist_remove_tracks` (the array-param handlers, `rpc.rs:1003-1082`):

```rust
async fn handle_playlist_reorder(
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
        .reorder_playlist(&playlist_id, &track_ids)
        .await
        .map_err(provider_error_to_rpc)?;
    Ok(serde_json::json!({ "ok": true }))
}
```

**Critical notes:**
- `require_provider` (`rpc.rs:471`), the `supports_playlist_write` guard, `ERR_INVALID_PARAMS`/`ERR_UNSUPPORTED_CAPABILITY` constants (`rpc.rs:26,36`), and `provider_error_to_rpc` (`rpc.rs:479`) are all existing — reuse, don't redefine.
- `provider_error_to_rpc` already maps `ProviderError::UnsupportedCapability → ERR_UNSUPPORTED_CAPABILITY` (`rpc.rs:486-490`), so the trait-default path returns the right RPC error automatically.
- `params["trackIds"].as_array()` + `filter_map(as_str)` is the verbatim pattern from `handle_playlist_add_tracks` (`rpc.rs:1028-1036`). The frontend sends the full ordered id list as `trackIds`.

### Task 6: Add tests (AC: #2, #3, #5)

- **Trait default** (Task 1) — already covered above.
- **Jellyfin adapter test** (`providers/jellyfin.rs` `#[cfg(test)]`): mockito-based, modeling the existing playlist tests around `jellyfin.rs:1220-1330` (they mock `Playlists/{id}/Items` GET with `PlaylistItemId`s). Mock the `get_playlist_items` GET to return a known out-of-order set, then mock the `Move` POST endpoint(s); assert the provider issues the expected Move calls to achieve the target order, and that a reorder equal to the current order issues **zero** Moves.
- **Subsonic adapter test** (`providers/subsonic.rs` `#[cfg(test)]`): mockito, modeling the existing `createPlaylist` mock at `subsonic.rs:3271`. Assert `createPlaylist` is called with `playlistId` + ordered `songId` params in the requested order.
- **RPC capability gate** (`rpc.rs` `#[cfg(test)]`): there is a fake provider with `supports_playlist_write: true` (`rpc.rs:8779`) and `FakeBrowseProvider` with `false` (`rpc.rs:9007`, used by the existing create/add/remove/delete `ERR_UNSUPPORTED_CAPABILITY` assertions at `rpc.rs:9020-9035`). Add a `playlist.reorder` case to that capability-gate test asserting `ERR_UNSUPPORTED_CAPABILITY` on the incapable provider.

### Task 7: Verify compilation and tests (AC: all)

- `rtk cargo check` — zero new errors. Changed Rust files: `providers/mod.rs`, `api.rs`, `providers/jellyfin.rs`, `providers/subsonic.rs`, `rpc.rs`.
- `rtk cargo test` — all pass (Story 11.8 baseline was 411 passing after review). New tests from Task 6 included.
- No `rtk tsc` needed — **this is a backend-only story.** No TypeScript, no i18n, no UI. (Frontend is Story 11.10.)

## Dev Notes

### Scope boundary — backend only

Story 11.9 is the **backend half** of the playlist-reorder feature: one trait method, both provider adapters, one new Jellyfin api.rs call, and one RPC. **No frontend, no i18n, no capability flag.** The `PlaylistCurationView` UI, `#N` order numbers, ↑/↓ controls, and the `playlist.reorder` call site are **Story 11.10** and must not be touched here. Reuses the existing `supports_playlist_write` capability — **do not add a new capability flag** (sprint-change-proposal §2 Technical Impact).

### Files to change

| File | Change |
|------|--------|
| `hifimule-daemon/src/providers/mod.rs` | Add `reorder_playlist` trait default (`UnsupportedCapability`) + trait-default unit test |
| `hifimule-daemon/src/api.rs` | Add `move_playlist_item` to `JellyfinApiClient` (new `Items/{id}/Move/{index}` call) |
| `hifimule-daemon/src/providers/jellyfin.rs` | Implement `reorder_playlist` (selection-sort via Move, local mirror) + test |
| `hifimule-daemon/src/providers/subsonic.rs` | Implement `reorder_playlist` + `set_playlist_order` client method (`createPlaylist` w/ `playlistId`) + test |
| `hifimule-daemon/src/rpc.rs` | Add `"playlist.reorder"` dispatch + `handle_playlist_reorder` + capability-gate test |

No new files. No `Cargo.toml` changes.

### Provider contract (from sprint-change-proposal §2)

```rust
async fn reorder_playlist(&self, playlist_id: &str, ordered_track_ids: &[String]) -> Result<(), ProviderError>
```

One set-order abstraction; each adapter satisfies it differently:
- **Subsonic/OpenSubsonic/Navidrome:** `createPlaylist?playlistId={id}&songId=…` in order — replaces contents in the given order, one native call.
- **Jellyfin:** selection-sort via `POST /Playlists/{id}/Items/{playlistItemId}/Move/{index}` — no removal, preserves entry identity, O(n) move calls (acceptable for DAP-sized playlists).

### Available RPCs (post-story)

| RPC | Params | Returns | Status |
|-----|--------|---------|--------|
| `playlist.reorder` | `{ playlistId: string, trackIds: string[] }` | `{ ok: true }` | **NEW — Task 5** |
| `playlist.rename` | `{ playlistId, name }` | `{ ok: true }` | Existing — Story 11.8 |
| `playlist.delete` | `{ playlistId }` | `{ ok: true }` | Existing — Story 11.4 |
| `playlist.addTracks` | `{ playlistId, trackIds }` | `{ ok: true }` | Existing — Story 11.4 |
| `playlist.removeTracks` | `{ playlistId, trackIds }` | `{ ok: true }` | Existing — Story 11.4 |

### Existing code being modified — current state & what must be preserved

**`MediaProvider` trait (`providers/mod.rs:199-246`):** Five playlist-write defaults (`create_playlist`, `add_to_playlist`, `remove_from_playlist`, `delete_playlist`, `rename_playlist`), every one returning `UnsupportedCapability("<method> is not supported by this provider")`. `reorder_playlist` must be the sixth, identical in shape. **Preserve:** do not change the other defaults; just append.

**Jellyfin `remove_from_playlist` (`jellyfin.rs:339-391`):** This is the closest existing template for the track-id→`PlaylistItemId` mapping and the malformed-entry guard. It fetches via `get_playlist_items`, uses a per-track count map for duplicates, and returns `ProviderError::Deserialization` when a `PlaylistItemId` is missing. Reorder reuses the same fetch and the same missing-`PlaylistItemId` guard. **Preserve:** entry identity — reorder must never delete/recreate entries (AC2); only Move.

**Subsonic `create_playlist` (`subsonic.rs:859-870`) and `update_playlist_*` (`872-918`):** `create_playlist` builds `("songId", id)` params and deserializes `PlaylistWithSongsBody`. `update_playlist_rename` (Story 11.8) shows the `NoBody` + `self.get("updatePlaylist", …)` pattern. Reorder's `set_playlist_order` is `create_playlist`'s param shape with `playlistId` swapped in for `name`. **Preserve:** the `descending-index` correctness comment on `update_playlist_remove_by_indices` (`subsonic.rs:885-902`) is unrelated — don't touch it.

**RPC dispatch (`rpc.rs:368-373`) and handlers (`856-1163`):** Six `playlist.*` handlers, all gated on `supports_playlist_write` before doing work, all returning `{ ok: true }`. `handle_playlist_add_tracks`/`handle_playlist_remove_tracks` are the array-param templates. **Preserve:** the guard-before-work ordering (capability check first, then param extraction) and the `provider_error_to_rpc` propagation.

### Jellyfin selection-sort — the one tricky bit

The order list is **track ids**; the Move endpoint addresses **entry ids** (`PlaylistItemId`). You must keep a local `(track_id, playlist_item_id)` mirror and apply each Move to it so subsequent index math stays correct (Jellyfin reorders the live list under you). See Task 3 for the full algorithm. Edge cases the algorithm already handles: empty list (0 moves), already-sorted (0 moves), duplicate track ids (consumed left-to-right by the "find at or after i" scan). A requested id absent from the playlist surfaces an error rather than silently corrupting order — the 11.10 frontend always sends the full live id list, so a mismatch means a real desync.

### Testing standards

- Rust tests live in `#[cfg(test)]` modules in the same file. Provider adapter tests use **mockito** to stub the HTTP server (see `jellyfin.rs:1220+` and `subsonic.rs:3271`). RPC tests use fake providers (`rpc.rs:8779`, `9007`).
- Per `spec-avoid-keychain-in-tests.md`, do not touch the OS keychain in tests — the existing provider/RPC test harnesses already avoid it; follow their construction.
- Baseline: Story 11.8 left 411 Rust tests passing. New story must keep all green plus add the four new tests (trait default, Jellyfin, Subsonic, RPC gate).

### Previous story intelligence (Story 11.8 — rename & delete)

Story 11.8 added `rename_playlist` across the exact same five files (minus api.rs's new method) and is the **direct precedent** for 11.9's backend shape. Learnings that carry over:
- The trait-default + sibling-test pattern is mechanical — mirror `trait_default_rename_playlist_returns_unsupported`.
- `JellyfinItem` is `#[serde(rename_all = "PascalCase")]` with `skip_serializing_if = "Option::is_none"` on its `Option` fields (hardened in 11.8 code review) — relevant only if you ever serialize it; the Move call sends **no body**, so not a concern here.
- 11.8's review hardened error handling and added daemon-side validation (empty-name reject). For reorder, the analogous validation is the "track not in playlist" guard in the Jellyfin path; the Subsonic path delegates validation to the server.
- Code-review for 11.x runs Blind Hunter + Edge Case Hunter + Acceptance Auditor — write the adapter tests to pre-empt edge-case findings (empty list, single track, duplicates, already-ordered).

### Git intelligence

Recent commits (`7a63bb5 Review 11.8`, `a8289d8 Dev 11.8`, `53565aa Story 11.8`) confirm the per-story rhythm: Story → Dev → Review on the `playlist-edit` branch. The 11.8 diff touched `providers/mod.rs`, `api.rs`, `providers/jellyfin.rs`, `providers/subsonic.rs`, `rpc.rs` — the same backend set 11.9 touches (plus 11.8's UI files, which 11.9 does **not**). Baseline commit for this story: `7a63bb5`.

### Project Structure Notes

- Daemon crate: `hifimule-daemon/src/` — `providers/mod.rs` (trait + `ProviderError`), `providers/jellyfin.rs`, `providers/subsonic.rs`, `api.rs` (`JellyfinApiClient` + `SubsonicApiClient` HTTP), `rpc.rs` (JSON-RPC dispatch). This story stays entirely within these.
- The provider abstraction is the single seam: the RPC layer never special-cases Jellyfin vs Subsonic — it calls `reorder_playlist` and each adapter does the right thing. Keep that invariant.
- No conflicts with the unified structure; the change is purely additive (sprint-change-proposal §3: "no existing code reverted").

### References

- Sprint change proposal (source of 11.9/11.10): [sprint-change-proposal-2026-06-07-playlist-reorder.md](_bmad-output/planning-artifacts/sprint-change-proposal-2026-06-07-playlist-reorder.md)
- Epic 11.9 definition: [epics.md:2414](_bmad-output/planning-artifacts/epics.md:2414)
- FR40 (new reorder FR): [epics.md:116](_bmad-output/planning-artifacts/epics.md:116) and prd.md FR40
- Previous story (rename & delete, the backend precedent): [11-8-playlist-rename-and-delete.md](_bmad-output/implementation-artifacts/11-8-playlist-rename-and-delete.md)
- `MediaProvider` playlist-write defaults: `hifimule-daemon/src/providers/mod.rs:199-246`
- `ProviderError` enum (note: `UnsupportedCapability`, **no** `NotSupported`): `hifimule-daemon/src/providers/mod.rs:371-388`
- Jellyfin `remove_from_playlist` (track-id→PlaylistItemId mapping template): `hifimule-daemon/src/providers/jellyfin.rs:339-391`
- Jellyfin `delete_playlist`/`rename_playlist` (delegation template): `hifimule-daemon/src/providers/jellyfin.rs:393-405`
- Jellyfin `map_error`: `hifimule-daemon/src/providers/jellyfin.rs:60`
- `JellyfinItem.id` / `.playlist_item_id`: `hifimule-daemon/src/api.rs:173` (and `item.id` usage at `jellyfin.rs:362`)
- api.rs playlist methods (templates for `move_playlist_item`): `get_playlist_items` `api.rs:1431`, `delete_playlist_items` `api.rs:1461`, `add_tracks_to_playlist` `api.rs:1398`, `jellyfin_endpoint` usage `api.rs:1369/1416/1447/1478`
- Subsonic `create_playlist` (param-build template) `subsonic.rs:859`; `update_playlist_rename` (Story 11.8) `subsonic.rs:909`; `update_playlist_add` `subsonic.rs:872`
- RPC dispatch table: `hifimule-daemon/src/rpc.rs:368-373`
- RPC handler templates: `handle_playlist_add_tracks` `rpc.rs:1003`, `handle_playlist_remove_tracks` `rpc.rs:1044`, `handle_playlist_rename` `rpc.rs:1117`
- RPC helpers: `require_provider` `rpc.rs:471`, `provider_error_to_rpc` `rpc.rs:479-490`, error-code consts `rpc.rs:26,36`
- RPC capability-gate test (add reorder case here): `rpc.rs:9007-9035`; fake provider w/ write=true `rpc.rs:8779`
- Existing Jellyfin playlist-items mock tests: `jellyfin.rs:1220-1330`; Subsonic `createPlaylist` mock: `subsonic.rs:3271`
- Test keychain guard: [spec-avoid-keychain-in-tests.md](_bmad-output/implementation-artifacts/spec-avoid-keychain-in-tests.md)

## Dev Agent Record

### Agent Model Used

claude-sonnet-4-6

### Debug Log References

### Completion Notes List

- Task 1: Added `reorder_playlist` trait default to `MediaProvider` (after `rename_playlist` at line 246) + `trait_default_reorder_playlist_returns_unsupported` test using `MinimalProvider` (not `MockProvider` — story note corrected).
- Task 2: Added `move_playlist_item` to `JellyfinApiClient` in `api.rs` after `delete_playlist_items`. Uses `jellyfin_endpoint` path-segment approach with `let index_str` borrow-lifetime trick for the index.
- Task 3: Implemented `reorder_playlist` in `JellyfinProvider` (selection-sort via local `(track_id, playlist_item_id)` mirror; mirrors each `remove`/`insert` locally to keep index math correct across moves).
- Task 4: Implemented `reorder_playlist` in `SubsonicProvider` delegating to `set_playlist_order` client method, which calls `createPlaylist` with `playlistId` + ordered `songId` params deserializing as `PlaylistWithSongsBody`.
- Task 5: Added `"playlist.reorder"` dispatch in RPC match table and `handle_playlist_reorder` handler (capability-guard first, then param extraction, then provider call).
- Task 6: Added 4 tests — trait-default (mod.rs), Jellyfin out-of-order + already-sorted (jellyfin.rs), Subsonic createPlaylist call (subsonic.rs), plus RPC capability-gate extended (rpc.rs).
- Task 7: `rtk cargo check` — 0 errors. `rtk cargo test` — 419 passing (baseline was 411 after 11.8 review commit `7a63bb5`).

### File List

- hifimule-daemon/src/providers/mod.rs
- hifimule-daemon/src/api.rs
- hifimule-daemon/src/providers/jellyfin.rs
- hifimule-daemon/src/providers/subsonic.rs
- hifimule-daemon/src/rpc.rs

## Change Log

- 2026-06-07: Story 11.9 created — `reorder_playlist` trait method + Jellyfin (selection-sort via Items/Move) & Subsonic (`createPlaylist` set-order) adapters + `playlist.reorder` RPC. Backend-only; reuses `supports_playlist_write`. Status → ready-for-dev.
- 2026-06-07: Story 11.9 implemented — all 5 backend files modified, 4 new tests added (trait-default, Jellyfin x2, Subsonic, RPC gate extended). 419/419 tests passing. Status → review.

## Review Findings

_Code review 2026-06-08 — Blind Hunter + Edge Case Hunter + Acceptance Auditor. All 5 ACs verified satisfied; all 4 Task-6 tests present; scope boundary (backend-only, no new capability flag) respected._

- [x] [Review][Patch] Enforce strict, non-destructive reorder validation on the Subsonic path (resolved from decision, option 2) — **APPLIED 2026-06-08**: `set_playlist_order` now fetches the current playlist via `get_playlist` and rejects any request whose track set isn't a permutation of the current set (`UnsupportedCapability`), and the RPC handler rejects non-string `trackIds` elements (`ERR_INVALID_PARAMS`) instead of silently dropping them. Tests added (`provider_reorder_playlist_rejects_mismatched_track_set_without_replacing`; existing Subsonic test updated with a `getPlaylist` mock). — `set_playlist_order` issues `createPlaylist?playlistId=…&songId=…`, which **replaces the entire track set**, not just the order, so the same `playlist.reorder` RPC is strict/reorder-only on Jellyfin but add/drop/wipe-capable on Subsonic (empty `trackIds` wipes the playlist; a subset drops tracks; a foreign id is added; non-string elements are silently filtered). **Fix:** before calling `set_playlist_order`, fetch the current playlist and assert `ordered_track_ids` is a permutation of the current track set (same elements, same multiplicity) — reject empty / subset / foreign-id / mismatched requests with an appropriate `ProviderError`, matching Jellyfin's strict contract and AC1's "same track set, only sequence changes." Also tighten the RPC handler so non-string `trackIds` elements are rejected rather than silently dropped. Add a test. Sources: blind+edge+auditor. [`hifimule-daemon/src/providers/subsonic.rs` `set_playlist_order` / `hifimule-daemon/src/rpc.rs` `handle_playlist_reorder`]
- [x] [Review][Patch] Jellyfin reorder lacks a multi-element (≥3, multi-move) test — **APPLIED 2026-06-08**: added `provider_reorder_playlist_multi_move_keeps_local_mirror_in_sync` (3 entries, 2 moves) asserting the exact `Move/{index}` call sequence (`entry-c→0` then `entry-b→1`), exercising the local-mirror index math across successive moves. [`hifimule-daemon/src/providers/jellyfin.rs` `#[cfg(test)]`]
- [x] [Review][Defer] Jellyfin reorder is non-atomic — a mid-sequence `move_playlist_item` failure leaves the playlist partially reordered with no rollback; the RPC returns `Err` while the first N moves are already persisted server-side. [`hifimule-daemon/src/providers/jellyfin.rs` reorder loop] — deferred, inherent to the per-entry Move design (no Jellyfin batch/transaction API); acceptable for DAP-sized playlists.
- [x] [Review][Defer] Jellyfin set-mismatch surfaces as `ERR_UNSUPPORTED_CAPABILITY` — a "track not in playlist" desync returns `ProviderError::UnsupportedCapability`, which to an RPC client implies the provider lacks the capability rather than signalling a bad request. [`hifimule-daemon/src/providers/jellyfin.rs` reorder loop] — deferred, spec-prescribed code (Task 3); revisit if a dedicated desync error variant is introduced.

_Dismissed as noise (2): duplicate track-id ordering (two entries with the same track id are content-interchangeable — the final track sequence still matches the request); `get_playlist_items` order assumption (the call preserves playlist order, same guarantee `remove_from_playlist` already relies on)._
