---
baseline_commit: 18d5475
---

# Story 11.7: Add Tracks to Playlist — Browse Context Menu & Curation View

Status: ready-for-dev

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

- [ ] Task 1: Add `browse.search` RPC handler to daemon (AC: 6–8)
  - [ ] In `hifimule-daemon/src/rpc.rs`, add the handler function after `handle_browse_list_favorite_items` (around line 365):

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
        let songs: Vec<Value> = result
            .songs
            .into_iter()
            .map(|s| song_to_browse_track_json(&s))
            .collect();
        Ok(serde_json::json!({ "tracks": songs }))
    }
    ```

  - [ ] In the `match payload.method.as_str()` block (around line 367), add after `browse.listFavoriteItems`:

    ```rust
    "browse.search" => handle_browse_search(&state, payload.params).await,
    ```

  - [ ] **Critical**: Verify `song_to_browse_track_json` exists in rpc.rs and produces the same shape as `BrowseTrack`. If not, use the inline `serde_json::json!({...})` pattern already used elsewhere in rpc.rs for Song→BrowseTrack conversion (grep for `"artistName"` in rpc.rs to find the existing pattern).

    **Key notes:**
    - `SearchResult.songs` is `Vec<Song>` (from domain/models.rs:107–112). The `Song` struct maps directly to `BrowseTrack` in the TypeScript layer.
    - `provider.search(&query)` is implemented by `JellyfinProvider` (jellyfin.rs:400) and `SubsonicProvider` (subsonic.rs:427). Both return `Result<SearchResult, ProviderError>`.
    - Only `songs` (tracks) are returned in the RPC response — not albums/artists/playlists. The curation search dialog only needs tracks.
    - No capability guard is needed: `search()` is a read operation and both providers support it unconditionally (no `supports_playlist_write` check required here).

- [ ] Task 2: Add `fetchBrowseSearch()` to rpc.ts (AC: 6–8)
  - [ ] In `hifimule-ui/src/rpc.ts`, add after `fetchBrowseFavoriteItems` (after line 282):

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

- [ ] Task 3: Add i18n keys (AC: 1–9)
  - [ ] In `hifimule-i18n/catalog.json`, add to the `"en"` block (after existing `playlist.curation.*` keys, around line 181):

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

  - [ ] Add the same 9 keys to the `"fr"` and `"es"` blocks (same English values are acceptable — existing pattern).

    **Key notes:**
    - 9 keys × 3 languages = 27 additions total.
    - Maintain valid JSON — no trailing commas on the last key in each language object.
    - Insert after `"playlist.curation.no_tracks": "No tracks for this selection"` (line 181) to keep the namespace grouped.

- [ ] Task 4: Add `showTrackContextMenu()` to MediaCard.ts (AC: 1–5)
  - [ ] In `hifimule-ui/src/components/MediaCard.ts`, add a new static method after `openCreatePlaylistDialog` (after line 438):

    ```typescript
    static showTrackContextMenu(x: number, y: number, trackId: string, trackName: string): void {
        // Dismiss any existing context menu first
        if (MediaCard.dismissActiveMenu) {
            MediaCard.dismissActiveMenu();
        }

        const menu = document.createElement('div');
        menu.className = 'hm-context-menu';
        menu.style.cssText = `
            position: fixed;
            z-index: 9999;
            background: var(--sl-panel-background-color, #fff);
            border: 1px solid var(--sl-color-neutral-200, #e2e8f0);
            border-radius: var(--sl-border-radius-medium, 4px);
            box-shadow: var(--sl-shadow-large);
            padding: 4px 0;
            min-width: 180px;
            visibility: hidden;
        `;

        const addItem = document.createElement('div');
        addItem.className = 'hm-context-menu-item';
        addItem.style.cssText = `
            padding: 8px 16px;
            cursor: pointer;
            font-size: var(--sl-font-size-small, 0.875rem);
            color: var(--sl-color-neutral-900);
            display: flex;
            align-items: center;
            gap: 8px;
        `;
        addItem.innerHTML = `<sl-icon name="collection-play"></sl-icon> ${t('playlist.context.add_to_playlist')}`;
        addItem.addEventListener('mouseover', () => { addItem.style.background = 'var(--sl-color-primary-50, #eff6ff)'; });
        addItem.addEventListener('mouseout', () => { addItem.style.background = ''; });

        menu.appendChild(addItem);
        document.body.appendChild(menu);
        MediaCard.activeContextMenu = menu;

        const MARGIN = 8;
        const rect = menu.getBoundingClientRect();
        const left = Math.max(MARGIN, Math.min(x, window.innerWidth - rect.width - MARGIN));
        const top = Math.max(MARGIN, Math.min(y, window.innerHeight - rect.height - MARGIN));
        menu.style.left = `${left}px`;
        menu.style.top = `${top}px`;
        menu.style.visibility = 'visible';

        const cleanup = () => {
            menu.remove();
            MediaCard.activeContextMenu = null;
            MediaCard.dismissActiveMenu = null;
            document.removeEventListener('click', onDocClick, true);
            window.removeEventListener('scroll', cleanup, true);
            window.removeEventListener('resize', cleanup);
            document.removeEventListener('keydown', onKeyDown, true);
        };
        MediaCard.dismissActiveMenu = cleanup;

        const onDocClick = (ev: MouseEvent) => {
            if (!menu.contains(ev.target as Node)) cleanup();
        };
        const onKeyDown = (ev: KeyboardEvent) => {
            if (ev.key === 'Escape') cleanup();
        };

        addItem.addEventListener('click', () => {
            cleanup();
            MediaCard.openAddToPlaylistDialog(trackId, trackName);
        });

        document.addEventListener('click', onDocClick, true);
        window.addEventListener('scroll', cleanup, true);
        window.addEventListener('resize', cleanup);
        document.addEventListener('keydown', onKeyDown, true);
    }

    static openAddToPlaylistDialog(trackId: string, trackName: string): void {
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
                            await rpcCall('playlist.addTracks', { playlistId: pl.id, trackIds: [trackId] });
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
    - `showTrackContextMenu` reuses the exact same styling/lifecycle/cleanup as the existing `showContextMenu` — same `activeContextMenu`, `dismissActiveMenu`, and `cleanup` pattern. Copy-paste-adapt is correct here; do NOT merge them into one function with a flag parameter (that would add coupling).
    - `openAddToPlaylistDialog`: Loads existing playlists from `browse.listPlaylists` on open. Shows "New playlist…" first (calls existing `openCreatePlaylistDialog`), then all server playlists as buttons.
    - On successful `playlist.addTracks`: closes the dialog only (no inline error shown). The closed dialog is the "success" UX.
    - On error: shows inline `sl-alert` in the dialog (same pattern as `openCreatePlaylistDialog`).
    - `rpcCall` is imported lazily via `await import('../rpc')` — same pattern as `openCreatePlaylistDialog` line 409.
    - `t()` is already imported at the top of MediaCard.ts — do not add a second import.
    - `MediaCard.openCreatePlaylistDialog` already exists (line 375): do NOT duplicate it; call it directly for "New playlist…".
    - `pl.id` and `pl.name` come from `BrowsePlaylist` interface in rpc.ts:114–119.

- [ ] Task 5: Wire track context menu in `renderListRow()` (AC: 1, 5)
  - [ ] In `hifimule-ui/src/library.ts`, in the `renderListRow()` function (around line 711–718), extend the existing context menu block:

    **Current code (around line 711–718):**
    ```typescript
    // Context menu for artist/album rows
    if (_supportsPlaylistWrite && (item.type === 'MusicArtist' || item.type === 'MusicAlbum')) {
        const rowItemId = item.basketId ?? item.id;
        row.addEventListener('contextmenu', (e) => {
            e.preventDefault();
            MediaCard.showContextMenu(e.clientX, e.clientY, rowItemId, item.name);
        });
    }
    ```

    **Replace with:**
    ```typescript
    // Context menu for artist/album/track rows
    if (_supportsPlaylistWrite && (item.type === 'MusicArtist' || item.type === 'MusicAlbum')) {
        const rowItemId = item.basketId ?? item.id;
        row.addEventListener('contextmenu', (e) => {
            e.preventDefault();
            MediaCard.showContextMenu(e.clientX, e.clientY, rowItemId, item.name);
        });
    } else if (_supportsPlaylistWrite && item.type === 'Audio') {
        row.addEventListener('contextmenu', (e) => {
            e.preventDefault();
            MediaCard.showTrackContextMenu(e.clientX, e.clientY, item.id, item.name);
        });
    }
    ```

    **Key notes:**
    - `item.id` (not `item.basketId ?? item.id`) because tracks are identified directly by their server ID for `playlist.addTracks`. Basket IDs are compound keys for containers; individual tracks use their raw `id`.
    - `_supportsPlaylistWrite` is the module-level variable (library.ts:30) — same guard as the existing artist/album block. No extra check needed.
    - Track items have `type: 'Audio'` in both `mapFlatTracks()` (line 292) and `mapAlbumTracks()` (line 306). This covers: album detail views, playlist flat track views, history/frequently-played/recently-played/favorites modes.
    - The context menu does NOT apply to grid mode cards (`renderGrid()`). Grid cards for tracks are in album detail and playlist track views — in those contexts, right-click on cards does nothing (same as before). The spec says "individual track rows in browse views" — list view is the primary context for track rows. Grid cards are for artists/albums/playlists.
    - Do not add `MediaCard.showTrackContextMenu` to `renderGrid()` / `MediaCard.create()` — grid cards for tracks don't have a meaningful right-click UX currently.

- [ ] Task 6: Add "Add tracks" button and search dialog to `PlaylistCurationView.ts` (AC: 6–9)
  - [ ] In `hifimule-ui/src/components/PlaylistCurationView.ts`, update the import line at the top:

    ```typescript
    import { fetchBrowsePlaylist, fetchBrowseSearch, BrowseTrack, rpcCall } from '../rpc';
    ```

  - [ ] Add `private isAddingTracks = false;` field to the class, after the existing `private isRemoving = false;` field.

  - [ ] Replace `renderStats()` method:

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

  - [ ] In `bindEvents()`, add after the track remove button listener block:

    ```typescript
    // "Add tracks" button
    this.container.querySelector('#curation-add-tracks-btn')?.addEventListener('click', () => {
        this.openAddTracksDialog();
    });
    ```

  - [ ] Add `openAddTracksDialog()` method to the class:

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

                const toggleSelect = () => {
                    if (selectedIds.has(track.id)) {
                        selectedIds.delete(track.id);
                    } else {
                        selectedIds.add(track.id);
                    }
                    row.style.background = selectedIds.has(track.id) ? 'var(--sl-color-primary-50)' : 'transparent';
                    row.style.borderColor = selectedIds.has(track.id) ? 'var(--sl-color-primary-300)' : 'transparent';
                    cb.checked = selectedIds.has(track.id);
                    const btn = confirmBtn();
                    if (btn) btn.disabled = selectedIds.size === 0;
                };

                row.addEventListener('click', toggleSelect);
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
    - The search uses `fetchBrowseSearch(query)` (added in Task 2) with a 300ms debounce on `sl-input` events.
    - `isAddingTracks` flag (new field from above) prevents double-submit on the confirm button — mirrors `isRemoving` pattern.
    - After successful `playlist.addTracks`, the dialog hides first, then `fetchBrowsePlaylist` re-fetches fresh data, updates `this.tracks`, and calls `this.render()`. This is the "re-fetches the playlist and re-renders all panels" AC.
    - On cancel (no selection or dialog closed), no RPC is called — `selectedIds` is checked before the `rpcCall`.
    - `fetchBrowseSearch` and `fetchBrowsePlaylist` are already imported at the top of the file after Task 6's import update.
    - `escapeHtml` is already a private method on the class — use `this.escapeHtml(...)`.
    - The `sl-input` event from Shoelace fires on every keystroke — use `sl-input` NOT `input` (Shoelace wraps the native event).
    - `sl-checkbox`: Shoelace checkbox component, already used in the project. Its `.checked` property is set directly.
    - The `confirmBtn.disabled = selectedIds.size === 0` logic keeps the confirm button disabled until at least one track is selected.
    - `basket.actions.cancel` i18n key already exists in catalog.json — reuse it.

- [ ] Task 7: Verify compilation (AC: all)
  - [ ] Run `rtk cargo check` — verify zero new Rust errors. Changes: 1 new handler function + 1 match arm.
  - [ ] Run `rtk tsc` — zero TypeScript errors. Common pitfalls:
    - `fetchBrowseSearch` added to rpc.ts imports in PlaylistCurationView.ts
    - `MediaCard.showTrackContextMenu` is `static` and callable directly
    - `selectedIds: Set<string>` in `openAddTracksDialog` closure — no TypeScript type annotation needed (inferred from `new Set<string>()`)
    - `searchTimeout: ReturnType<typeof setTimeout> | null` — needed to avoid `NodeJS.Timeout` vs `number` conflict in browser/node environments
    - `isAddingTracks` field declared before use in `openAddTracksDialog`

## Dev Notes

### UI-only story with 1 thin Rust change

| File | Change |
|------|--------|
| `hifimule-daemon/src/rpc.rs` | 1 handler function + 1 match arm for `browse.search` |
| `hifimule-ui/src/rpc.ts` | `fetchBrowseSearch()` wrapper |
| `hifimule-i18n/catalog.json` | 9 new i18n keys × 3 language blocks |
| `hifimule-ui/src/components/MediaCard.ts` | New `showTrackContextMenu()` + `openAddToPlaylistDialog()` static methods |
| `hifimule-ui/src/library.ts` | `renderListRow()` — add `Audio` type context menu trigger |
| `hifimule-ui/src/components/PlaylistCurationView.ts` | New `isAddingTracks` field + updated `renderStats()` + `openAddTracksDialog()` method + updated import + `bindEvents()` wiring |

No provider changes. No Cargo.toml or package.json changes.

### CRITICAL: No existing `browse.search` RPC

The sprint change proposal states "Pure frontend change — no new daemon endpoints needed if a track search RPC exists." **No such RPC exists.** The `search()` provider method IS implemented in both `JellyfinProvider` (jellyfin.rs:400) and `SubsonicProvider` (subsonic.rs:427) but has never been exposed as an RPC endpoint. Task 1 adds a minimal wrapper. This is low-risk because the provider implementations are already tested.

Check `song_to_browse_track_json` existence before Task 1: run `grep -n "song_to_browse_track_json" hifimule-daemon/src/rpc.rs`. If it doesn't exist, find the existing `Song`→JSON serialization pattern in the file (grep for `"artistName"` in rpc.rs) and use the same inline approach.

### Available RPCs (no new handlers except `browse.search`)

| RPC | Params | Returns | Implemented |
|-----|--------|---------|-------------|
| `browse.search` | `{ query: string }` | `{ tracks: BrowseTrack[] }` | **NEW — Task 1** |
| `browse.listPlaylists` | none | `{ playlists: BrowsePlaylist[] }` | rpc.rs:350 |
| `playlist.addTracks` | `{ playlistId, trackIds }` | `{ ok: true }` | rpc.rs:368 / Story 11.4 |
| `playlist.create` | `{ name, itemIds }` | `{ playlistId, skippedItemIds }` | rpc.rs:367 / Story 11.4 |
| `browse.getPlaylist` | `{ playlistId }` | `{ playlist, tracks }` | rpc.rs:351 / Story 9.x |

TypeScript wrappers already in rpc.ts:
- `fetchBrowsePlaylists()` → line 193 — used by `openAddToPlaylistDialog` indirectly via `rpcCall('browse.listPlaylists')`
- `fetchBrowsePlaylist(playlistId)` → line 197 — used by curation view reload after add
- `fetchBrowseSearch(query)` → NEW (Task 2)

### MediaCard static method context: existing vs new

The existing `showContextMenu()` (MediaCard.ts:291) is for **artist/album** items → shows "Send to playlist…" → always creates a NEW playlist via `openCreatePlaylistDialog`. 

The new `showTrackContextMenu()` is for **individual track** items → shows "Add to playlist…" → fetches existing playlists AND offers "New playlist…" option. The two methods are structurally parallel but differ in behavior. Do NOT merge them.

`openAddToPlaylistDialog` calls `MediaCard.openCreatePlaylistDialog(trackId, trackName)` for the "New playlist…" case. `openCreatePlaylistDialog` calls `playlist.create` with `{ name, itemIds: [trackId] }` — this is correct because `playlist.create` does entity resolution, and for a single track ID the resolved track list is just that track.

### `renderListRow()` context: where tracks appear

`renderListRow()` is called by `renderList()` (list mode) for any `BrowseDisplayItem`. Track items (`type: 'Audio'`) appear in:
- **Album detail view**: `loadAlbumTracks()` → `renderGrid()` (grid mode, not list). BUT if user switches to list mode, `renderList()` is also used.
- **Playlist flat track view**: `loadPlaylistTracks()` → `renderGrid()`.
- **History/frequency/favorites modes**: `loadX()` → `renderGrid()`.

The context menu is wired only in `renderListRow()` (list mode). Track context menus in grid mode are NOT in scope — grid cards currently have no right-click handler for any item type. This is intentional scope limitation. Do not add right-click to grid track cards.

### `item.id` for tracks (not `item.basketId`)

In `renderListRow()`, the existing artist/album block uses `item.basketId ?? item.id`. For tracks, use `item.id` directly. Reason: `playlist.addTracks` takes `trackIds` (server IDs), and `item.basketId` is only set for certain compound items (FavoriteArtist, FavoriteAlbum). Track items from `mapFlatTracks`/`mapAlbumTracks` never set `basketId`. Using `item.id` is correct and safe.

### `openAddToPlaylistDialog` — lazy import pattern

MediaCard.ts uses dynamic `await import('../rpc')` (line 409) for RPC calls inside the dialog handler. Continue this pattern in `openAddToPlaylistDialog` for `rpcCall('browse.listPlaylists')` and `rpcCall('playlist.addTracks')`. Do NOT add a top-level import of `rpcCall` to MediaCard.ts — this breaks the lazy-loading pattern.

But `fetchBrowseSearch` should NOT be called from MediaCard — it's only used from `PlaylistCurationView.ts`. In `openAddToPlaylistDialog`, use raw `rpcCall('browse.listPlaylists')` and `rpcCall('playlist.addTracks')` with destructured `{ rpcCall }` from the dynamic import — same as the existing `openCreatePlaylistDialog` pattern.

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

### Completion Notes List

### File List

- hifimule-daemon/src/rpc.rs
- hifimule-ui/src/rpc.ts
- hifimule-i18n/catalog.json
- hifimule-ui/src/components/MediaCard.ts
- hifimule-ui/src/library.ts
- hifimule-ui/src/components/PlaylistCurationView.ts

## Change Log

- 2026-06-07: Story 11.7 created — add tracks to playlist via browse context menu and curation view ready for dev.
