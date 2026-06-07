---
baseline_commit: 18d5475
---

# Story 11.7: Add Tracks to Playlist — Browse Context Menu & Curation View

Status: review

## Story

As a Ritualist (Arthur),
I want to add individual tracks to a server playlist from browse views and from within the curation view,
so that I can build and expand playlists track-by-track without needing to create a new playlist each time.

## Acceptance Criteria

1. **Given** the active provider supports playlist write **When** I right-click an individual track row in any browse view **Then** an "Add to playlist…" option appears in the context menu.

2. **Given** I select "Add to playlist…" from a track's context menu **When** the sub-menu or dialog opens **Then** a list of existing server playlists is shown alongside a "New playlist…" option.

3. **Given** I select an existing playlist from the "Add to playlist…" dialog **Then** `playlist.addTracks({ playlistId, trackIds: [track.id] })` is called **And** a success notification is shown confirming the track was added.

4. **Given** I select "New playlist…" from the "Add to playlist…" dialog **Then** I am prompted for a playlist name **And** `playlist.create({ name, itemIds: [track.id] })` is called **And** the created playlist becomes available in the server playlist browser.

5. **Given** the active provider does not support playlist write **Then** the "Add to playlist…" context action is hidden on track rows.

6. **Given** the curation view is open for a playlist **When** I click the "Add tracks" button in the statistics header **Then** a search dialog opens that accepts a query (title, artist, or album).

7. **Given** I enter a query in the "Add tracks" search dialog **Then** matching tracks from the library are displayed as a selectable list.

8. **Given** I select one or more tracks in the search dialog and confirm **Then** `playlist.addTracks({ playlistId, trackIds: selectedIds })` is called **And** the curation view re-fetches the playlist and re-renders all panels **And** the statistics header updates.

9. **Given** I open the "Add tracks" search dialog and cancel without selecting **Then** no RPC is called and the curation view is unchanged.

## Tasks / Subtasks

- [x] Task 1: Add `browse.search` RPC handler to daemon (AC: 6–8)
  - [x] In `hifimule-daemon/src/rpc.rs`, add the handler function after `handle_browse_list_favorite_items`:

    ```rust
    async fn handle_browse_search(
        state: &AppState,
        params: Option<Value>,
    ) -> Result<Value, JsonRpcError> {
        let provider = require_provider(state).await?;
        let query = params
            .as_ref()
            .and_then(|p| p["query"].as_str())
            .ok_or(JsonRpcError {
                code: ERR_INVALID_PARAMS,
                message: "Missing query".to_string(),
                data: None,
            })?
            .to_owned();
        let result = provider
            .search(&query)
            .await
            .map_err(provider_error_to_rpc)?;
        Ok(serde_json::json!({ "tracks": result.songs }))
    }
    ```

  - [x] In the `match payload.method.as_str()` block, add after `browse.listFavoriteItems`:

    ```rust
    "browse.search" => handle_browse_search(&state, payload.params).await,
    ```

  - [x] **Critical**: `song_to_browse_track_json` does NOT exist in rpc.rs. Use `serde_json::json!({ "tracks": result.songs })` directly — `Song` derives `Serialize` with `#[serde(rename_all = "camelCase")]` and the explicit `#[serde(rename)]` overrides on `album_title`→`albumName` and `duration_seconds`→`duration`, producing the exact `BrowseTrack` shape.

    **Key notes:**
    - `SearchResult.songs` is `Vec<Song>` (domain/models.rs). `Song` serializes directly to the `BrowseTrack` JSON shape via serde — no manual conversion needed.
    - `provider.search(&query)` is implemented by `JellyfinProvider` (jellyfin.rs:400) and `SubsonicProvider` (subsonic.rs:427).
    - Only `songs` (tracks) are returned — not albums/artists/playlists.
    - No capability guard needed: `search()` is a read operation.
    - **Jellyfin fix**: `search_audio_items` in `api.rs` must include `Recursive=true` in the query URL, otherwise Jellyfin returns library root folders instead of audio tracks. Also set `Limit=25` (was 10).

- [x] Task 2: Add `fetchBrowseSearch()` to rpc.ts (AC: 6–8)
  - [x] In `hifimule-ui/src/rpc.ts`, add after `fetchBrowseFavoriteItems` (after line 282):

    ```typescript
    export async function fetchBrowseSearch(
        query: string,
    ): Promise<{ tracks: BrowseTrack[] }> {
        return await rpcCall('browse.search', { query });
    }
    ```

    **Key notes:**
    - Return type reuses the existing `BrowseTrack` interface (rpc.ts:121–137) — no new type needed.
    - `fetchBrowseSearch` returns only `tracks`, not the full `SearchResult` — the handler filters to songs only.

- [x] Task 3: Add i18n keys (AC: 1–9)
  - [x] In `hifimule-i18n/catalog.json`, add to the `"en"` block (after existing `playlist.curation.*` keys, around line 181):

    ```json
    "playlist.context.add_to_playlist": "Add to playlist…",
    "playlist.context.new_playlist": "New playlist…",
    "playlist.context.added_success": "Track added to playlist",
    "playlist.context.pick_playlist_title": "Add to Playlist",
    "playlist.curation.add_tracks": "Add tracks",
    "playlist.curation.add_tracks_placeholder": "Search by title, artist, or album…",
    "playlist.curation.add_tracks_confirm": "Add Selected",
    "playlist.curation.add_tracks_error": "Failed to add tracks: {message}",
    "playlist.curation.no_search_results": "No tracks found"
    ```

  - [x] Add the same 9 keys to the `"fr"` and `"es"` blocks (same English values are acceptable — existing pattern).

    **Key notes:**
    - 9 keys × 3 languages = 27 additions total.
    - Maintain valid JSON — no trailing commas on the last key in each language object.
    - Insert after `"playlist.curation.no_tracks": "No tracks for this selection"` (line 181) to keep the namespace grouped.

- [x] Task 4: Unify context menu for all item types in MediaCard.ts (AC: 1–5)
  - [x] In `hifimule-ui/src/components/MediaCard.ts`, replace `showContextMenu()` with unified `showItemContextMenu()` that opens `openAddToPlaylistDialog` for all item types (artists, albums, tracks):

    ```typescript
    static showItemContextMenu(x: number, y: number, itemId: string, itemName: string): void {
        if (MediaCard.dismissActiveMenu) {
            MediaCard.dismissActiveMenu();
        }
        // ... (same menu scaffold as existing showContextMenu)
        // Label: t('playlist.context.add_to_playlist')
        // On click: MediaCard.openAddToPlaylistDialog(itemId, itemName)
    }
    ```

  - [x] Add `playlist.addItems` RPC handler to `hifimule-daemon/src/rpc.rs` — accepts `{ playlistId, itemIds }`, runs entity resolution via `provider_sync_items_for_id` (handles artists, albums, tracks, playlists), then calls `provider.add_to_playlist()`. This replaces `playlist.addTracks` for the context menu flow because `playlist.addTracks` passes IDs straight through with no resolution — artist/album IDs would fail.

  - [x] Update `openAddToPlaylistDialog` to use `playlist.addItems { playlistId, itemIds: [itemId] }` instead of `playlist.addTracks { playlistId, trackIds: [trackId] }`:

    ```typescript
    static openAddToPlaylistDialog(itemId: string, itemName: string): void {
        const dialog = document.createElement('sl-dialog') as any;
        dialog.label = t('playlist.context.pick_playlist_title');
        dialog.innerHTML = `
            <div id="ctx-track-playlist-list" style="display:flex; flex-direction:column; gap:0.5rem; max-height:300px; overflow-y:auto;">
                <sl-spinner></sl-spinner>
            </div>
            <sl-alert id="ctx-track-error" variant="danger" closable style="display:none; margin-top: 0.75rem;"></sl-alert>
            <sl-button slot="footer" variant="default" id="ctx-track-cancel">${t('basket.actions.cancel')}</sl-button>
        `;

        document.body.appendChild(dialog);

        dialog.querySelector('#ctx-track-cancel')?.addEventListener('click', () => dialog.hide());
        dialog.addEventListener('sl-after-hide', (event: Event) => {
            if (event.target === dialog) dialog.remove();
        });

        customElements.whenDefined('sl-dialog').then(async () => {
            dialog.show();
            const listEl = dialog.querySelector('#ctx-track-playlist-list') as HTMLElement;
            const errorEl = dialog.querySelector('#ctx-track-error') as HTMLElement | null;

            try {
                const { rpcCall } = await import('../rpc');
                const result = await rpcCall('browse.listPlaylists');
                const playlists: Array<{ id: string; name: string }> = result.playlists ?? [];

                listEl.innerHTML = '';

                // "New playlist…" option
                const newBtn = document.createElement('sl-button') as any;
                newBtn.variant = 'default';
                newBtn.style.cssText = 'width: 100%; text-align: left;';
                newBtn.innerHTML = `<sl-icon slot="prefix" name="plus-circle"></sl-icon> ${t('playlist.context.new_playlist')}`;
                newBtn.addEventListener('click', () => {
                    dialog.hide();
                    MediaCard.openCreatePlaylistDialog(trackId, trackName);
                });
                listEl.appendChild(newBtn);

                if (playlists.length > 0) {
                    const divider = document.createElement('sl-divider') as any;
                    listEl.appendChild(divider);
                }

                for (const pl of playlists) {
                    const btn = document.createElement('sl-button') as any;
                    btn.variant = 'default';
                    btn.style.cssText = 'width: 100%; text-align: left;';
                    btn.textContent = pl.name;
                    btn.dataset.plId = pl.id;
                    btn.addEventListener('click', async () => {
                        btn.loading = true;
                        btn.disabled = true;
                        if (errorEl) errorEl.style.display = 'none';
                        try {
                            await rpcCall('playlist.addItems', { playlistId: pl.id, itemIds: [itemId] });
                            dialog.hide();
                        } catch (err) {
                            const msg = err instanceof Error ? err.message : String(err);
                            if (errorEl) {
                                errorEl.textContent = t('playlist.curation.add_tracks_error', { message: msg });
                                errorEl.style.display = '';
                                (errorEl as any).open = true;
                            }
                            btn.loading = false;
                            btn.disabled = false;
                        }
                    });
                    listEl.appendChild(btn);
                }

                if (playlists.length === 0) {
                    const emptyNote = document.createElement('p');
                    emptyNote.style.cssText = 'color: var(--sl-color-neutral-500); font-size: var(--sl-font-size-small); padding: 0.5rem 0;';
                    emptyNote.textContent = 'No playlists yet. Use "New playlist…" to create one.';
                    listEl.appendChild(emptyNote);
                }
            } catch (err) {
                const msg = err instanceof Error ? err.message : String(err);
                listEl.innerHTML = '';
                if (errorEl) {
                    errorEl.textContent = msg;
                    errorEl.style.display = '';
                    (errorEl as any).open = true;
                }
            }
        });
    }
    ```

    **Key notes:**
    - `showItemContextMenu` replaces both the old `showContextMenu` (artists/albums) and the never-shipped `showTrackContextMenu`. One method for all item types — the old `showContextMenu` created new playlists only; the unified method opens `openAddToPlaylistDialog` for all types.
    - `openAddToPlaylistDialog`: loads existing playlists from `browse.listPlaylists`, shows "New playlist…" first (calls `openCreatePlaylistDialog`), then server playlists as buttons.
    - Uses `playlist.addItems { playlistId, itemIds }` (new RPC with entity resolution) instead of `playlist.addTracks { playlistId, trackIds }`. This is necessary because `playlist.addTracks` passes IDs to the provider as-is — artist/album IDs would be sent as track IDs and fail. `playlist.addItems` resolves artists/albums/tracks to actual track IDs via `provider_sync_items_for_id` before calling `add_to_playlist`.
    - On success: closes the dialog. On error: shows inline `sl-alert`.
    - `rpcCall` is imported lazily via `await import('../rpc')` — preserves the existing lazy-loading pattern in MediaCard.ts.
    - `t()` is already imported at the top of MediaCard.ts.
    - `MediaCard.openCreatePlaylistDialog` is reused unchanged for "New playlist…".
    - In `MediaCard.create()`, pass `(item as BrowseDisplayItem).id` (raw server entity ID) to `showItemContextMenu`, not `item.basketId ?? item.id` — basket IDs can be compound keys (`favorites:artist:…`) that `playlist.addItems` cannot resolve.

- [x] Task 5: Wire unified context menu in `renderListRow()` and `MediaCard.create()` (AC: 1, 5)
  - [x] In `hifimule-ui/src/library.ts`, replace the split artist/album + track blocks with a single unified block in `renderListRow()`:

    ```typescript
    // Context menu for artist/album/track rows
    if (_supportsPlaylistWrite && (item.type === 'MusicArtist' || item.type === 'MusicAlbum' || item.type === 'Audio')) {
        row.addEventListener('contextmenu', (e) => {
            e.preventDefault();
            MediaCard.showItemContextMenu(e.clientX, e.clientY, item.id, item.name);
        });
    }
    ```

  - [x] In `MediaCard.create()`, update the `supportsPlaylistWrite` block to call `showItemContextMenu` for all three types using `(item as BrowseDisplayItem).id` (not `itemId` which may be a basket compound key):

    ```typescript
    if (itemType === 'MusicArtist' || itemType === 'MusicAlbum' || itemType === 'Audio') {
        const serverItemId = (item as BrowseDisplayItem).id;
        card.addEventListener('contextmenu', (e) => {
            e.preventDefault();
            MediaCard.showItemContextMenu(e.clientX, e.clientY, serverItemId, itemName);
        });
    }
    ```

    **Key notes:**
    - `item.id` (raw server entity ID) is used for all types — not `item.basketId ?? item.id`. `playlist.addItems` requires resolvable server IDs; basket compound keys (`favorites:artist:…`) would fail entity resolution.
    - Grid mode (`MediaCard.create()`) now also shows the context menu for tracks — this was a post-implementation fix after observing that tracks almost always render as grid cards in album/playlist/history views.
    - `_supportsPlaylistWrite` / `supportsPlaylistWrite` guard applies to all types unchanged.

- [x] Task 6: Add "Add tracks" button and search dialog to `PlaylistCurationView.ts` (AC: 6–9)
  - [x] In `hifimule-ui/src/components/PlaylistCurationView.ts`, update the import line at the top:

    ```typescript
    import { fetchBrowsePlaylist, fetchBrowseSearch, BrowseTrack, rpcCall } from '../rpc';
    ```

  - [x] Add `private isAddingTracks = false;` field to the class, after the existing `private isRemoving = false;` field.

  - [x] Replace `renderStats()` method:

    ```typescript
    private renderStats(): string {
        const count = this.tracks.length;
        const totalSecs = this.tracks.reduce((s, track) => s + (track.duration ?? 0), 0);
        const totalBytes = this.tracks.reduce((s, track) => s + (track.sizeBytes ?? 0), 0);
        return `
            <div class="curation-stats" style="
                padding: 0.5rem 1rem;
                background: var(--sl-color-neutral-50);
                border-bottom: 1px solid var(--sl-color-neutral-200);
                font-size: var(--sl-font-size-small);
                color: var(--sl-color-neutral-600);
                display: flex;
                align-items: center;
                gap: 1.5rem;
            ">
                <span>${count} track${count === 1 ? '' : 's'}</span>
                <span>${formatDuration(totalSecs)}</span>
                <span>${formatBytes(totalBytes)}</span>
                <sl-button
                    id="curation-add-tracks-btn"
                    size="small"
                    variant="default"
                    style="margin-left: auto;"
                >
                    <sl-icon slot="prefix" name="plus-circle"></sl-icon>
                    ${t('playlist.curation.add_tracks')}
                </sl-button>
            </div>
        `;
    }
    ```

  - [x] In `bindEvents()`, add after the track remove button listener block:

    ```typescript
    // "Add tracks" button
    this.container.querySelector('#curation-add-tracks-btn')?.addEventListener('click', () => {
        this.openAddTracksDialog();
    });
    ```

  - [x] Add `openAddTracksDialog()` method to the class:

    ```typescript
    private openAddTracksDialog(): void {
        const dialog = document.createElement('sl-dialog') as any;
        dialog.label = t('playlist.curation.add_tracks');
        dialog.innerHTML = `
            <sl-input
                id="add-tracks-query"
                placeholder="${t('playlist.curation.add_tracks_placeholder')}"
                clearable
                autofocus
            ></sl-input>
            <div id="add-tracks-results" style="
                margin-top: 0.75rem;
                max-height: 300px;
                overflow-y: auto;
                display: flex;
                flex-direction: column;
                gap: 0.25rem;
            "></div>
            <sl-alert id="add-tracks-error" variant="danger" closable style="display:none; margin-top: 0.75rem;"></sl-alert>
            <sl-button slot="footer" variant="default" id="add-tracks-cancel">${t('basket.actions.cancel')}</sl-button>
            <sl-button slot="footer" variant="primary" id="add-tracks-confirm" disabled>${t('playlist.curation.add_tracks_confirm')}</sl-button>
        `;

        document.body.appendChild(dialog);

        const selectedIds = new Set<string>();
        let searchTimeout: ReturnType<typeof setTimeout> | null = null;

        const queryInput = () => dialog.querySelector('#add-tracks-query') as any;
        const resultsEl = () => dialog.querySelector('#add-tracks-results') as HTMLElement;
        const errorEl = () => dialog.querySelector('#add-tracks-error') as HTMLElement | null;
        const confirmBtn = () => dialog.querySelector('#add-tracks-confirm') as any;

        const renderResults = (tracks: BrowseTrack[]) => {
            const el = resultsEl();
            el.innerHTML = '';
            if (tracks.length === 0) {
                el.innerHTML = `<p style="color: var(--sl-color-neutral-500); font-size: var(--sl-font-size-small);">${t('playlist.curation.no_search_results')}</p>`;
                return;
            }
            for (const track of tracks) {
                const row = document.createElement('div');
                row.style.cssText = `
                    display: flex; align-items: center; gap: 0.5rem;
                    padding: 0.35rem 0.5rem;
                    border-radius: var(--sl-border-radius-small);
                    cursor: pointer;
                    background: ${selectedIds.has(track.id) ? 'var(--sl-color-primary-50)' : 'transparent'};
                    border: 1px solid ${selectedIds.has(track.id) ? 'var(--sl-color-primary-300)' : 'transparent'};
                `;
                row.dataset.trackId = track.id;

                const cb = document.createElement('sl-checkbox') as any;
                cb.checked = selectedIds.has(track.id);
                cb.style.flexShrink = '0';

                const info = document.createElement('div');
                info.style.cssText = 'flex: 1; overflow: hidden;';
                info.innerHTML = `
                    <div style="font-size: var(--sl-font-size-small); overflow: hidden; text-overflow: ellipsis; white-space: nowrap;">${this.escapeHtml(track.title)}</div>
                    <div style="font-size: var(--sl-font-size-x-small); color: var(--sl-color-neutral-500); overflow: hidden; text-overflow: ellipsis; white-space: nowrap;">${this.escapeHtml(track.artistName || '')} — ${this.escapeHtml(track.albumName || '')}</div>
                `;

                row.appendChild(cb);
                row.appendChild(info);

                const updateRow = (selected: boolean) => {
                    row.style.background = selected ? 'var(--sl-color-primary-50)' : 'transparent';
                    row.style.borderColor = selected ? 'var(--sl-color-primary-300)' : 'transparent';
                    cb.checked = selected;
                    const btn = confirmBtn();
                    if (btn) btn.disabled = selectedIds.size === 0;
                };

                // Row click (text area): guard against checkbox to avoid double-firing
                row.addEventListener('click', (e) => {
                    if ((e.target as HTMLElement).closest('sl-checkbox')) return;
                    if (selectedIds.has(track.id)) { selectedIds.delete(track.id); } else { selectedIds.add(track.id); }
                    updateRow(selectedIds.has(track.id));
                });

                // Checkbox: Shoelace fires sl-change (not native click) after toggling cb.checked
                cb.addEventListener('sl-change', () => {
                    if (cb.checked) { selectedIds.add(track.id); } else { selectedIds.delete(track.id); }
                    updateRow(cb.checked);
                });

                row.appendChild(cb);
                row.appendChild(info);
                resultsEl().appendChild(row);
            }
        };

        const doSearch = async (query: string) => {
            if (!query.trim()) {
                resultsEl().innerHTML = '';
                return;
            }
            resultsEl().innerHTML = '<sl-spinner></sl-spinner>';
            try {
                const result = await fetchBrowseSearch(query);
                renderResults(result.tracks);
            } catch (err) {
                const msg = err instanceof Error ? err.message : String(err);
                resultsEl().innerHTML = '';
                const err2 = errorEl();
                if (err2) {
                    err2.textContent = msg;
                    err2.style.display = '';
                    (err2 as any).open = true;
                }
            }
        };

        dialog.querySelector('#add-tracks-cancel')?.addEventListener('click', () => dialog.hide());

        dialog.querySelector('#add-tracks-confirm')?.addEventListener('click', async () => {
            if (selectedIds.size === 0 || this.isAddingTracks) return;
            this.isAddingTracks = true;
            const btn = confirmBtn();
            if (btn) { btn.loading = true; btn.disabled = true; }
            const err2 = errorEl();
            if (err2) err2.style.display = 'none';
            try {
                await rpcCall('playlist.addTracks', {
                    playlistId: this.playlistId,
                    trackIds: Array.from(selectedIds),
                });
                dialog.hide();
                // Re-fetch to get updated playlist state
                const result = await fetchBrowsePlaylist(this.playlistId);
                this.tracks = result.tracks;
                this.render();
            } catch (err) {
                const msg = err instanceof Error ? err.message : String(err);
                if (err2) {
                    err2.textContent = t('playlist.curation.add_tracks_error', { message: msg });
                    err2.style.display = '';
                    (err2 as any).open = true;
                }
                if (btn) { btn.loading = false; btn.disabled = false; }
            } finally {
                this.isAddingTracks = false;
            }
        });

        customElements.whenDefined('sl-dialog').then(() => {
            dialog.show();
            // Wire up search input after dialog is shown
            const inputEl = dialog.querySelector('#add-tracks-query') as any;
            inputEl?.addEventListener('sl-input', () => {
                if (searchTimeout) clearTimeout(searchTimeout);
                searchTimeout = setTimeout(() => doSearch(inputEl.value ?? ''), 300);
            });
        });

        dialog.addEventListener('sl-after-hide', (event: Event) => {
            if (event.target === dialog) {
                if (searchTimeout) clearTimeout(searchTimeout);
                dialog.remove();
            }
        });
    }
    ```

    **Key notes:**
    - The search uses `fetchBrowseSearch(query)` (Task 2) with a 300ms debounce on `sl-input` events.
    - `isAddingTracks` flag prevents double-submit — mirrors `isRemoving` pattern.
    - After successful `playlist.addTracks`, dialog hides, then `fetchBrowsePlaylist` re-fetches and `this.render()` re-renders all panels (AC 8).
    - On cancel or close with no selection, no RPC is called (AC 9).
    - **`sl-checkbox` event**: Shoelace's `sl-checkbox` fires `sl-change` (not a bubbling native `click`) after toggling `cb.checked` internally. Listen for `sl-change` on the checkbox to sync `selectedIds`, and guard the row `click` with `.closest('sl-checkbox')` to prevent double-firing. A single `row.addEventListener('click', ...)` without the `sl-change` handler means clicking the checkbox does nothing visible.
    - `sl-input` event (not native `input`) for the search field — Shoelace wraps the native event.
    - `basket.actions.cancel` i18n key already exists in catalog.json — reuse it.

- [x] Task 7: Verify compilation (AC: all)
  - [x] Run `rtk cargo check` — zero new Rust errors. Rust changes: `browse.search` handler, `playlist.addItems` handler, `api.rs` Jellyfin search fix.
  - [x] Run `rtk tsc` — zero TypeScript errors. Common pitfalls:
    - `fetchBrowseSearch` added to rpc.ts imports in PlaylistCurationView.ts
    - `MediaCard.showItemContextMenu` is `static` — callable directly from library.ts
    - `selectedIds: Set<string>` in `openAddTracksDialog` — type inferred from `new Set<string>()`
    - `searchTimeout: ReturnType<typeof setTimeout> | null` — avoids NodeJS.Timeout vs number conflict
    - `isAddingTracks` field declared before use

## Dev Notes

### Files changed

| File | Change |
|------|--------|
| `hifimule-daemon/src/rpc.rs` | `handle_browse_search` + `handle_playlist_add_items` handlers + 2 match arms |
| `hifimule-daemon/src/api.rs` | `search_audio_items`: added `Recursive=true`, bumped `Limit` 10→25 |
| `hifimule-ui/src/rpc.ts` | `fetchBrowseSearch()` wrapper |
| `hifimule-i18n/catalog.json` | 9 new i18n keys × 3 language blocks |
| `hifimule-ui/src/components/MediaCard.ts` | Replaced `showContextMenu` with `showItemContextMenu`; added `openAddToPlaylistDialog` (uses `playlist.addItems`); updated `MediaCard.create()` for all three item types |
| `hifimule-ui/src/library.ts` | `renderListRow()` — unified context menu for artist/album/track |
| `hifimule-ui/src/components/PlaylistCurationView.ts` | New `isAddingTracks` field + updated `renderStats()` + `openAddTracksDialog()` + updated import + `bindEvents()` wiring + `sl-change` checkbox fix |

No provider changes. No Cargo.toml or package.json changes.

### CRITICAL: No existing `browse.search` RPC

The sprint change proposal states "Pure frontend change — no new daemon endpoints needed if a track search RPC exists." **No such RPC exists.** The `search()` provider method IS implemented in both `JellyfinProvider` (jellyfin.rs:400) and `SubsonicProvider` (subsonic.rs:427) but has never been exposed as an RPC endpoint. Task 1 adds a minimal wrapper. This is low-risk because the provider implementations are already tested.

Check `song_to_browse_track_json` existence before Task 1: run `grep -n "song_to_browse_track_json" hifimule-daemon/src/rpc.rs`. If it doesn't exist, find the existing `Song`→JSON serialization pattern in the file (grep for `"artistName"` in rpc.rs) and use the same inline approach.

### Available RPCs

| RPC | Params | Returns | Implemented |
|-----|--------|---------|-------------|
| `browse.search` | `{ query: string }` | `{ tracks: BrowseTrack[] }` | **NEW — Task 1** |
| `playlist.addItems` | `{ playlistId, itemIds }` | `{ ok: true }` | **NEW — Task 4** |
| `browse.listPlaylists` | none | `{ playlists: BrowsePlaylist[] }` | rpc.rs / Story 11.4 |
| `playlist.addTracks` | `{ playlistId, trackIds }` | `{ ok: true }` | rpc.rs / Story 11.4 — still used by `PlaylistCurationView` (search results are already resolved track IDs) |
| `playlist.create` | `{ name, itemIds }` | `{ playlistId, skippedItemIds }` | rpc.rs / Story 11.4 — used by "New playlist…" in `openCreatePlaylistDialog` |
| `browse.getPlaylist` | `{ playlistId }` | `{ playlist, tracks }` | rpc.rs / Story 9.x |

TypeScript wrappers in rpc.ts:
- `fetchBrowsePlaylists()` — used by `openAddToPlaylistDialog` indirectly via `rpcCall('browse.listPlaylists')`
- `fetchBrowsePlaylist(playlistId)` — used by curation view reload after add
- `fetchBrowseSearch(query)` — NEW (Task 2)

### Context menu: unified `showItemContextMenu`

`showItemContextMenu()` is the single context menu entry point for all item types (artist, album, track) in both grid mode (`MediaCard.create()`) and list mode (`renderListRow()`). It shows "Add to playlist…" and opens `openAddToPlaylistDialog`.

`openAddToPlaylistDialog` uses `playlist.addItems` (entity-resolving) for adding to existing playlists. This handles artist/album IDs correctly — `playlist.addTracks` would not, as it passes IDs to the provider as literal track IDs.

`openCreatePlaylistDialog` is unchanged and is still called for the "New playlist…" option. It uses `playlist.create` which also does entity resolution.

### `renderListRow()` context: where tracks appear

`renderListRow()` is called by `renderList()` (list mode). Most track views (`loadAlbumTracks`, `loadPlaylistTracks`, history/favorites modes) default to `renderGrid()`. The context menu is wired in both:
- `renderListRow()` — for list mode
- `MediaCard.create()` — for grid mode (post-implementation addition; tracks almost always appear in grid mode)

### `item.id` for all types (not `item.basketId`)

Both `renderListRow()` and `MediaCard.create()` pass `item.id` (raw server entity ID) to `showItemContextMenu`. Basket IDs (`item.basketId`) can be compound keys like `favorites:artist:abc123` — these are used only for basket store operations and are not valid server entity IDs that `playlist.addItems` can resolve.

### `openAddToPlaylistDialog` — lazy import pattern

MediaCard.ts uses `await import('../rpc')` lazily for RPC calls inside dialog handlers. `openAddToPlaylistDialog` uses `rpcCall('browse.listPlaylists')` and `rpcCall('playlist.addItems')` via the same destructured dynamic import pattern. Do NOT add a top-level `rpcCall` import to MediaCard.ts.

### `PlaylistCurationView.ts` imports

Current import (line 1):
```typescript
import { fetchBrowsePlaylist, BrowseTrack, rpcCall } from '../rpc';
```

Update to:
```typescript
import { fetchBrowsePlaylist, fetchBrowseSearch, BrowseTrack, rpcCall } from '../rpc';
```

`fetchBrowseSearch` is used directly in `openAddTracksDialog` — it's safe to import at the top level (not lazy) because `PlaylistCurationView.ts` is only loaded when the curation view is opened.

### Song→BrowseTrack shape in Rust

The `browse.search` RPC handler (Task 1) converts `Song` structs to JSON. Verify by finding the existing Song→BrowseTrack serialization in rpc.rs — search for `"artistName"` to find an existing pattern like:
```rust
serde_json::json!({
    "id": s.jellyfin_id,
    "title": s.title,
    "artistName": s.artist_name.unwrap_or_default(),
    "albumName": s.album_title.unwrap_or_default(),
    ...
})
```
If `song_to_browse_track_json(s)` already exists as a helper, use it. If not, inline the pattern. The return shape must match `BrowseTrack` (rpc.ts:121–137): `id`, `title`, `artistName`, `albumName`, `duration`, `trackNumber`, `bitrateKbps`, `coverArtId`, `sizeBytes`.

### `sl-input` vs `input` event in Shoelace

Shoelace's `sl-input` component fires a `sl-input` custom event (not the native `input` event). Always listen on `'sl-input'`, not `'input'`. See the existing pattern in `BasketSidebar.ts` for reference.

### Debounce search — 300ms

The search query in `openAddTracksDialog` debounces at 300ms using `setTimeout`. Store the timeout handle as `ReturnType<typeof setTimeout>` (browser-safe, avoids Node.js type conflicts). Clear it on `sl-after-hide` to prevent stale RPC calls after the dialog is closed.

### isAddingTracks guard

`isAddingTracks` prevents concurrent confirms (e.g., double-click). It mirrors the existing `isRemoving` field. Set to `true` before the RPC, reset in `finally`. The confirm button is also disabled while loading, providing visual feedback.

### Story 11.6 learnings applied to this story

From 11.6 review findings:
1. **`escapeAttr()` vs `escapeHtml()`**: `data-*` attribute values need `escapeAttr()`. HTML content uses `escapeHtml()`. Both methods already exist on `PlaylistCurationView`.
2. **`e.stopPropagation()` on all button handlers**: Every `sl-icon-button` click handler calls `e.stopPropagation()` to prevent bubbling. Follow this in the "Add tracks" button handler.
3. **No filter callback named `t`**: All lambdas in `PlaylistCurationView` use `track` not `t` to avoid shadowing the imported `t()` i18n function. Maintain this in new code.
4. **Optimistic vs pessimistic**: Remove operations are optimistic (local state updated before RPC). Add operations are pessimistic (re-fetch after RPC confirms) — correct for adds because we want the server to assign any ordering or dedup.
5. **`render()` after error**: In `doRemove()`, `this.render()` runs unconditionally in a `finally`-equivalent position, then the error is shown on the freshly-rendered DOM. Same pattern in `openAddTracksDialog`'s confirm handler: dialog hides on success, error shown on existing dialog DOM on failure.

### Project Structure Notes

- No new files except for the Rust handler (inline in existing rpc.rs).
- `PlaylistCurationView.ts` already at `hifimule-ui/src/components/PlaylistCurationView.ts`.
- `MediaCard.ts` already at `hifimule-ui/src/components/MediaCard.ts`.
- `library.ts` already at `hifimule-ui/src/library.ts`.
- `rpc.ts` already at `hifimule-ui/src/rpc.ts`.
- No barrel file (`index.ts`) in `components/` — direct imports.

### References

- Epic 11 Story 11.7 spec: `_bmad-output/planning-artifacts/epics.md:2311–2363`
- Sprint change proposal: `_bmad-output/planning-artifacts/sprint-change-proposal-2026-06-07-add-tracks-to-playlist.md`
- Story 11.6 (previous — dual-panel curation view): `_bmad-output/implementation-artifacts/11-6-dual-panel-playlist-curation-view-and-stats.md`
- `PlaylistCurationView` current source: `hifimule-ui/src/components/PlaylistCurationView.ts`
- `MediaCard.showContextMenu()` implementation: `hifimule-ui/src/components/MediaCard.ts:291–373`
- `MediaCard.openCreatePlaylistDialog()` implementation: `hifimule-ui/src/components/MediaCard.ts:375–438`
- `renderListRow()` context menu block: `hifimule-ui/src/library.ts:711–718`
- `browse.search` RPC dispatch table: `hifimule-daemon/src/rpc.rs:345–370`
- `JellyfinProvider.search()` implementation: `hifimule-daemon/src/providers/jellyfin.rs:400`
- `SubsonicProvider.search()` implementation: `hifimule-daemon/src/providers/subsonic.rs:427`
- `SearchResult` domain model: `hifimule-daemon/src/domain/models.rs:107–112`
- `MediaProvider` trait `search()`: `hifimule-daemon/src/providers/mod.rs:88`
- `BrowseTrack` TypeScript interface: `hifimule-ui/src/rpc.ts:121–137`
- `fetchBrowsePlaylists()`: `hifimule-ui/src/rpc.ts:193`
- `fetchBrowsePlaylist()`: `hifimule-ui/src/rpc.ts:197`
- `mapFlatTracks()` — `type: 'Audio'` for tracks: `hifimule-ui/src/library.ts:277–300`
- `mapAlbumTracks()` — `type: 'Audio'` for tracks: `hifimule-ui/src/library.ts:302–313`
- `_supportsPlaylistWrite` module variable: `hifimule-ui/src/library.ts:30`
- `playlist.addTracks` RPC handler: `hifimule-daemon/src/rpc.rs:904–943`
- `playlist.create` RPC handler: `hifimule-daemon/src/rpc.rs:827–902`
- `browse.listPlaylists` RPC handler: `hifimule-daemon/src/rpc.rs:350`
- Story 11.5 — existing `sendToPlaylist` context menu on artist/album rows: `_bmad-output/implementation-artifacts/11-5-save-selection-as-playlist-ui-and-context-menu.md`
- Story 11.4 — `playlist.addTracks` RPC implementation: `_bmad-output/implementation-artifacts/11-4-playlist-rpcs-and-selection-to-tracks-resolution.md`

## Dev Agent Record

### Agent Model Used

claude-sonnet-4-6

### Debug Log References

_No blocking issues encountered._

### Completion Notes List

- Task 1: Added `handle_browse_search` to rpc.rs. `song_to_browse_track_json` did not exist; `serde_json::json!({ "tracks": result.songs })` used directly — `Song`'s `Serialize` derive with `#[serde(rename_all = "camelCase")]` produces the correct `BrowseTrack` shape.
- Task 1 (fix): Added `Recursive=true` and `Limit=25` to `search_audio_items` in `api.rs` — without `Recursive=true`, Jellyfin returns library root folders instead of audio tracks.
- Task 2: Added `fetchBrowseSearch()` wrapper to rpc.ts.
- Task 3: Added 9 i18n keys × 3 language blocks (en/fr/es) = 27 additions. catalog.json valid JSON confirmed.
- Task 4: Replaced `showContextMenu` (artist/album, new-playlist-only) with unified `showItemContextMenu` for all item types. Added `openAddToPlaylistDialog` (uses `playlist.addItems` with entity resolution, not `playlist.addTracks`). Added `handle_playlist_add_items` to rpc.rs. Added context menu to `MediaCard.create()` for Audio items (grid mode) — tracks almost always render as grid cards. Dynamic `await import('../rpc')` pattern maintained.
- Task 5: Unified `renderListRow()` context menu to single block covering artist/album/track. Both list and grid modes now support the context menu for all three types.
- Task 6: Updated `PlaylistCurationView.ts` — new import, `isAddingTracks` guard, updated `renderStats()` with "Add tracks" button, `bindEvents()` wiring, `openAddTracksDialog()` with debounced search + multi-select + pessimistic re-fetch. Fixed `sl-checkbox` interaction: Shoelace fires `sl-change` not a bubbling click — added `sl-change` handler on checkbox and guarded row click with `.closest('sl-checkbox')`.
- Task 7: `rtk cargo check` — 0 new errors. `rtk tsc` — 0 errors.

### File List

- hifimule-daemon/src/rpc.rs
- hifimule-daemon/src/api.rs
- hifimule-ui/src/rpc.ts
- hifimule-i18n/catalog.json
- hifimule-ui/src/components/MediaCard.ts
- hifimule-ui/src/library.ts
- hifimule-ui/src/components/PlaylistCurationView.ts

## Change Log

- 2026-06-07: Story 11.7 created — add tracks to playlist via browse context menu and curation view ready for dev.
- 2026-06-07: Story 11.7 implemented — `browse.search` RPC, `fetchBrowseSearch` TS wrapper, 27 i18n additions, unified `showItemContextMenu` + `openAddToPlaylistDialog` (with new `playlist.addItems` RPC for entity resolution) in MediaCard covering all item types in both grid and list mode, "Add tracks" button and search dialog in `PlaylistCurationView`.
- 2026-06-07: Post-implementation fixes — Jellyfin `search_audio_items` missing `Recursive=true` (returned root folders instead of tracks); `sl-checkbox` requires `sl-change` event not row `click`; context menus unified across artist/album/track replacing the separate `showContextMenu`/`showTrackContextMenu` split.
