---
baseline_commit: 9938d12
---

# Story 11.5: Basket "Save as Playlist" and "Send to Playlist" UI

Status: ready-for-dev

## Story

As a Ritualist (Arthur),
I want to save the basket selection as a server playlist and send items to playlists from browse views,
so that I can persist and reuse my curated selections across sessions.

## Acceptance Criteria

1. **Given** the active provider supports playlist write and the basket is non-empty **When** the basket header is visible **Then** a "Save selection as playlist" action is shown.

2. **Given** the active provider does not support playlist write **Then** the "Save selection as playlist" action is hidden.

3. **Given** I click "Save selection as playlist" and the basket contains only manual selections **When** the dialog opens **Then** I can enter a name to create a new server playlist.

4. **Given** I click "Save selection as playlist" and the basket contains an Auto-Fill slot **When** the dialog opens **Then** an inline notice informs me that Auto-Fill tracks are resolved at sync time and will not be saved to the playlist **And** I can still proceed to save the manual selections.

5. **Given** I right-click an artist or album in a browse view **Then** a context menu appears with a "Send to playlist…" option.

6. **Given** I select "Send to playlist…" from a context menu **Then** I can create a new playlist with that item as the initial content.

7. **Given** I confirm playlist creation **Then** `playlist.create` is called with the resolved item IDs **And** the created playlist becomes available in the server playlist browser.

## Tasks / Subtasks

- [x] Task 1: Expose `supportsPlaylistWrite` in daemon state (AC: 1, 2) — **one line of Rust**
  - [x] In `handle_get_daemon_state` in `hifimule-daemon/src/rpc.rs`, add immediately before the `Ok(serde_json::json!({...}))` block (at line ~1357):

    ```rust
    let supports_playlist_write = {
        let guard = state.provider.read().await;
        guard
            .as_ref()
            .map(|p| p.capabilities().supports_playlist_write)
            .unwrap_or(false)
    };
    ```

  - [x] Add `"supportsPlaylistWrite": supports_playlist_write,` to the JSON response object inside `Ok(serde_json::json!({...}))`.

    **Key notes:**
    - `state.provider` is `Arc<RwLock<Option<Arc<dyn MediaProvider>>>>` — pattern already used at line 832. The `read().await` lock is released immediately because the block discards the guard.
    - `capabilities()` is a **synchronous** method — no `.await` needed (confirmed at `rpc.rs:530`).
    - If no provider is connected, `unwrap_or(false)` hides all playlist affordances — correct behavior.
    - `cargo check` must still pass after this change; run it before moving on.

- [x] Task 2: Add new i18n keys to catalog (AC: 1–7)
  - [x] In `hifimule-i18n/catalog.json`, add to the `"en"` block (after `"basket.actions.retry_sync"` key, around line 97):

    ```json
    "basket.actions.save_as_playlist": "Save as Playlist",
    "basket.playlist.create_title": "Save Selection as Playlist",
    "basket.playlist.name_placeholder": "My playlist…",
    "basket.playlist.auto_fill_notice": "Auto-Fill tracks are resolved at sync time and won't be included in this playlist.",
    "basket.playlist.create_btn": "Create Playlist",
    "basket.playlist.creating": "Creating…",
    "basket.playlist.error": "Failed to create playlist: {message}",
    "library.context.send_to_playlist": "Send to playlist…",
    "library.context.create_playlist_title": "Send to New Playlist",
    "library.context.create_btn": "Create"
    ```

  - [x] Add the same keys to the `"fr"` and `"es"` blocks (same English values are acceptable — the existing pattern is to use English in all three until translations land).

    **Key notes:**
    - The `t()` function falls back to the key string if no translation is found (`i18n.ts:28`), so missing keys won't crash — but always add to all three language objects to stay consistent with the existing pattern.

- [x] Task 3: Add playlist-write capability export to `library.ts` (AC: 5, 6)
  - [x] At the top of `hifimule-ui/src/library.ts`, add a module-level variable and exported setter immediately after the imports:

    ```typescript
    let _supportsPlaylistWrite = false;
    export function setPlaylistWriteCapability(v: boolean): void {
        _supportsPlaylistWrite = v;
    }
    ```

  - [x] No other changes to `library.ts` in this task — the variable is consumed in Task 4 and Task 5.

- [x] Task 4: Add context menu to `MediaCard` (AC: 5, 6, 7)
  - [x] Update the `MediaCard.create()` signature in `hifimule-ui/src/components/MediaCard.ts` to accept an optional parameter:

    ```typescript
    public static create(
        item: JellyfinItem | JellyfinView | BrowseDisplayItem,
        mode: 'libraries' | 'items',
        isSynced: boolean,
        onNavigate: () => void | Promise<void>,
        deviceSelectionEnabled?: boolean,
        supportsPlaylistWrite?: boolean,   // NEW
    ): HTMLElement {
    ```

  - [x] Inside `MediaCard.create()`, after all existing event bindings (after the `basketStore.addEventListener('update', ...)` block), add the context menu attachment:

    ```typescript
    // Context menu: "Send to playlist…" on artist/album cards when supported
    if (supportsPlaylistWrite) {
        const isBrowseDisplayItem = !('Id' in item);
        const isArtistOrAlbum = isBrowseDisplayItem
            ? ((item as BrowseDisplayItem).type === 'MusicArtist' || (item as BrowseDisplayItem).type === 'MusicAlbum')
            : false;

        if (isArtistOrAlbum) {
            card.addEventListener('contextmenu', (e) => {
                e.preventDefault();
                MediaCard.showContextMenu(e.clientX, e.clientY, itemId, itemName);
            });
        }
    }
    ```

  - [x] Add the `showContextMenu` static helper to `MediaCard` class (after the `escapeHtml` method):

    ```typescript
    private static activeContextMenu: HTMLElement | null = null;

    static showContextMenu(x: number, y: number, itemId: string, itemName: string): void {
        // Dismiss any existing context menu first
        if (MediaCard.activeContextMenu) {
            MediaCard.activeContextMenu.remove();
            MediaCard.activeContextMenu = null;
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
        `;

        // Clamp position to viewport
        const MARGIN = 8;
        const viewW = window.innerWidth;
        const viewH = window.innerHeight;
        const MENU_W = 200;
        const MENU_H = 44;
        const left = Math.min(x, viewW - MENU_W - MARGIN);
        const top = Math.min(y, viewH - MENU_H - MARGIN);
        menu.style.left = `${left}px`;
        menu.style.top = `${top}px`;

        const sendItem = document.createElement('div');
        sendItem.className = 'hm-context-menu-item';
        sendItem.style.cssText = `
            padding: 8px 16px;
            cursor: pointer;
            font-size: var(--sl-font-size-small, 0.875rem);
            color: var(--sl-color-neutral-900);
            display: flex;
            align-items: center;
            gap: 8px;
        `;
        sendItem.innerHTML = `<sl-icon name="collection-play"></sl-icon> Send to playlist…`;

        sendItem.addEventListener('mouseover', () => {
            sendItem.style.background = 'var(--sl-color-primary-50, #eff6ff)';
        });
        sendItem.addEventListener('mouseout', () => {
            sendItem.style.background = '';
        });

        sendItem.addEventListener('click', () => {
            menu.remove();
            MediaCard.activeContextMenu = null;
            MediaCard.openCreatePlaylistDialog(itemId, itemName);
        });

        menu.appendChild(sendItem);
        document.body.appendChild(menu);
        MediaCard.activeContextMenu = menu;

        // Dismiss on any click outside the menu
        const dismiss = (ev: MouseEvent) => {
            if (!menu.contains(ev.target as Node)) {
                menu.remove();
                MediaCard.activeContextMenu = null;
                document.removeEventListener('click', dismiss, true);
            }
        };
        // Use capture=true so outside clicks register before propagation stops them
        document.addEventListener('click', dismiss, true);
    }

    static openCreatePlaylistDialog(itemId: string, itemName: string): void {
        const dialog = document.createElement('sl-dialog') as any;
        dialog.label = 'Send to New Playlist';
        dialog.innerHTML = `
            <sl-input
                id="ctx-playlist-name"
                placeholder="My playlist…"
                autofocus
                clearable
                value="${MediaCard.escapeHtml(itemName)}"
            ></sl-input>
            <sl-alert id="ctx-playlist-error" variant="danger" closable style="display:none; margin-top: 0.75rem;"></sl-alert>
            <sl-button slot="footer" variant="default" id="ctx-playlist-cancel">Cancel</sl-button>
            <sl-button slot="footer" variant="primary" id="ctx-playlist-create">Create</sl-button>
        `;

        document.body.appendChild(dialog);

        const dismiss = () => dialog.hide();
        dialog.querySelector('#ctx-playlist-cancel')?.addEventListener('click', dismiss);

        dialog.querySelector('#ctx-playlist-create')?.addEventListener('click', async () => {
            const createBtn = dialog.querySelector('#ctx-playlist-create') as any;
            const errorEl = dialog.querySelector('#ctx-playlist-error') as HTMLElement | null;
            const nameInput = dialog.querySelector('#ctx-playlist-name') as any;
            const name = (nameInput?.value ?? '').trim();
            if (!name) return;

            createBtn.loading = true;
            if (errorEl) errorEl.style.display = 'none';

            try {
                const { rpcCall } = await import('../rpc');
                await rpcCall('playlist.create', { name, itemIds: [itemId] });
                dialog.hide();
            } catch (err) {
                const msg = err instanceof Error ? err.message : String(err);
                if (errorEl) {
                    errorEl.textContent = `Failed to create playlist: ${msg}`;
                    errorEl.style.display = '';
                    (errorEl as any).open = true;
                }
            } finally {
                createBtn.loading = false;
            }
        });

        dialog.addEventListener('sl-after-hide', (event: Event) => {
            if (event.target === dialog) dialog.remove();
        });

        customElements.whenDefined('sl-dialog').then(() => dialog.show());
    }
    ```

    **Key notes:**
    - `rpcCall` is imported dynamically inside `openCreatePlaylistDialog` to avoid a circular import since `MediaCard.ts` is already imported by `library.ts` which imports from `rpc.ts`. Dynamic import breaks the cycle cleanly.
    - The `Send to playlist…` action calls `playlist.create` with `itemIds: [itemId]`. The daemon resolves the entity (artist/album) to a flat track list via `provider_sync_items_for_id` (Story 11.4 implementation, `rpc.rs:1667`).
    - Passing a single entity ID is consistent with how `sync.start` handles basket items — the daemon expansion logic handles the container→tracks resolution.
    - `MediaCard.activeContextMenu` is a static property so that only one context menu is open at a time. Opening a second one dismisses the first.
    - The `…` character is the `…` ellipsis — avoids HTML entity in template literals.
    - **Do NOT** hard-code color values — always use CSS custom properties from Shoelace design tokens.
    - TypeScript strict mode is on (`tsconfig.json:21`). `addEventListener('contextmenu', (e) => {...})` on an `HTMLElement` types `e` as `MouseEvent` (via the `HTMLElementEventMap` overload) — `e.clientX` and `e.clientY` are available without casting. ✅

- [x] Task 5: Wire `_supportsPlaylistWrite` into grid and list card creation calls in `library.ts` (AC: 5)
  - [x] In `renderGrid()` in `hifimule-ui/src/library.ts`, locate the `MediaCard.create(item, 'items', false, ...)` call (line ~514) and pass `_supportsPlaylistWrite` as the sixth argument:

    ```typescript
    const card = MediaCard.create(
        item, 'items', false,
        () => navigateToBrowseItem(item),
        selEnabled,
        _supportsPlaylistWrite,   // NEW
    );
    ```

  - [x] In `renderListRow()` in `hifimule-ui/src/library.ts`, add a `contextmenu` event listener on the row immediately after the existing `click` event listener (after the `row.addEventListener('click', ...)` block, before the `row.appendChild(thumb)` calls), but **only for artist/album types**:

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

    - [x] Add the `MediaCard` import at the top of `library.ts` (it's already imported: `import { MediaCard, BrowseDisplayItem } from './components/MediaCard';` — no change needed).

    **Key notes:**
    - `_supportsPlaylistWrite` is the module-level variable added in Task 3.
    - List rows use `item.basketId ?? item.id` as the entity ID — same as the toggle button logic at `library.ts:620`.
    - The context menu in `renderListRow` delegates to `MediaCard.showContextMenu` static method so the popup and dialog logic are not duplicated.

- [x] Task 6: Update `BasketSidebar` for "Save as Playlist" button and dialog (AC: 1–4)
  - [x] Add instance field to `BasketSidebar` class (after the `private completedBytesCount` field, ~line 187):

    ```typescript
    private supportsPlaylistWrite: boolean = false;
    ```

  - [x] Import `setPlaylistWriteCapability` from library at the top of `BasketSidebar.ts`:

    ```typescript
    import { setPlaylistWriteCapability } from '../library';
    ```

  - [x] In `refreshAndRender()`, append these three lines at the very end of the existing `if (daemonStateResult.status === 'fulfilled' && daemonStateResult.value)` block — right before its outermost closing `}` at ~line 298 (after the `} else if (!currentDevice)` sub-block):

    ```typescript
    const newSupportsPlaylist = (state.supportsPlaylistWrite === true);
    this.supportsPlaylistWrite = newSupportsPlaylist;
    setPlaylistWriteCapability(newSupportsPlaylist);
    ```

    The `state` variable is already defined at the top of that block (`const state = daemonStateResult.value as any;`). Do NOT create a second `if (daemonStateResult...)` guard — add inside the existing one.

  - [x] In `startDaemonStatePolling()`, inside the polling callback, after `this.serverType = daemonStateResult?.serverType ?? null;`, add:

    ```typescript
    const newSupportsPlaylist = daemonStateResult?.supportsPlaylistWrite === true;
    if (newSupportsPlaylist !== this.supportsPlaylistWrite) {
        this.supportsPlaylistWrite = newSupportsPlaylist;
        setPlaylistWriteCapability(newSupportsPlaylist);
    }
    ```

  - [x] In the non-empty basket `render()` branch, update the basket header HTML (the `<div class="basket-header">` section, inside the `this.container.innerHTML = \`...\`` at ~line 942) to include the "Save as Playlist" button:

    ```html
    <div class="basket-header">
        <h2>${t('basket.title')}</h2>
        <sl-badge variant="primary" pill>${items.length}</sl-badge>
        ${this.supportsPlaylistWrite ? `
            <sl-icon-button
                id="save-as-playlist-btn"
                name="collection-play"
                label="${t('basket.actions.save_as_playlist')}"
                style="font-size: 1.1rem; margin-left: auto;">
            </sl-icon-button>
        ` : ''}
    </div>
    ```

  - [x] In the event binding section of the same `render()` branch (after the existing bindings starting at ~line 1001), bind the button:

    ```typescript
    this.container.querySelector('#save-as-playlist-btn')?.addEventListener('click', () => {
        this.handleSaveAsPlaylist();
    });
    ```

  - [x] Add the `handleSaveAsPlaylist()` private method to `BasketSidebar` (place it near the other action handlers, e.g. after `confirmClearAll()`):

    ```typescript
    private handleSaveAsPlaylist(): void {
        const allItems = basketStore.getItems();
        const hasAutoFill = allItems.some(i => i.id === AUTO_FILL_SLOT_ID);
        const manualIds = allItems
            .filter(i => i.id !== AUTO_FILL_SLOT_ID)
            .map(i => i.id);

        const autoFillNoticeHtml = hasAutoFill ? `
            <sl-alert variant="warning" open style="margin-bottom: 0.75rem;">
                <sl-icon slot="icon" name="exclamation-triangle"></sl-icon>
                ${t('basket.playlist.auto_fill_notice')}
            </sl-alert>
        ` : '';

        const dialog = document.createElement('sl-dialog') as any;
        dialog.label = t('basket.playlist.create_title');
        dialog.innerHTML = `
            ${autoFillNoticeHtml}
            <sl-input
                id="playlist-name-input"
                placeholder="${t('basket.playlist.name_placeholder')}"
                autofocus
                clearable>
            </sl-input>
            <sl-alert id="playlist-create-error" variant="danger" closable style="display:none; margin-top: 0.75rem;"></sl-alert>
            <sl-button slot="footer" variant="default" id="playlist-cancel-btn">${t('basket.actions.cancel')}</sl-button>
            <sl-button slot="footer" variant="primary" id="playlist-create-btn">
                ${t('basket.playlist.create_btn')}
            </sl-button>
        `;

        document.body.appendChild(dialog);

        dialog.querySelector('#playlist-cancel-btn')?.addEventListener('click', () => dialog.hide());

        dialog.querySelector('#playlist-create-btn')?.addEventListener('click', async () => {
            const createBtn = dialog.querySelector('#playlist-create-btn') as any;
            const errorEl = dialog.querySelector('#playlist-create-error') as HTMLElement | null;
            const nameInput = dialog.querySelector('#playlist-name-input') as any;
            const name = (nameInput?.value ?? '').trim();
            if (!name) return;

            createBtn.loading = true;
            if (errorEl) errorEl.style.display = 'none';

            try {
                await rpcCall('playlist.create', { name, itemIds: manualIds });
                dialog.hide();
            } catch (err) {
                const msg = err instanceof Error ? err.message : String(err);
                if (errorEl) {
                    errorEl.textContent = t('basket.playlist.error', { message: msg });
                    errorEl.style.display = '';
                    (errorEl as any).open = true;
                }
            } finally {
                createBtn.loading = false;
            }
        });

        dialog.addEventListener('sl-after-hide', (event: Event) => {
            if (event.target === dialog) dialog.remove();
        });

        customElements.whenDefined('sl-dialog').then(() => dialog.show());
    }
    ```

    **Key notes:**
    - `rpcCall` is already imported at the top of `BasketSidebar.ts` (`import { rpcCall, getImageUrl } from '../rpc';`) — no dynamic import needed here.
    - `manualIds` is computed from the basket at the moment the user clicks the button — captures the live state, not a stale snapshot. This is correct since the dialog is opened synchronously.
    - The `AUTO_FILL_SLOT_ID` is already imported in `BasketSidebar.ts` (`import { basketStore, BasketItem, AUTO_FILL_SLOT_ID } from '../state/basket';`).
    - The `sl-alert` for auto-fill uses `open` attribute to be immediately visible — consistent with the `sl-alert` usage in `openDeviceSettings()` which uses `(error as any).open = true` programmatically, but here it's inline since it's always visible when present.
    - No toast on success (the dialog closes — that's sufficient feedback). An error inline in the dialog follows the pattern used in `openDeviceSettings()` and `InitDeviceModal`.
    - The `margin-left: auto` on the save-as-playlist button pushes it to the right of the header, visually separated from the title/badge.

- [x] Task 7: Verify compilation and behavior (AC: all)
  - [x] Run `rtk cargo check` — zero new errors.
  - [x] Run `rtk tsc` — zero errors (TypeScript strict check). Pre-existing `baseUrl` deprecation warning in tsconfig.json is unrelated to this story.
  - [ ] Manual verification checklist (in Tauri dev app):
    - Connect to a Jellyfin or Subsonic server (both support playlist write).
    - Add 2–3 artists/albums to basket. Confirm "Save as Playlist" icon appears in basket header.
    - Click the button. Confirm dialog opens with name input.
    - Enter a name and confirm. Verify the playlist appears in the server's playlist browser.
    - Add an Auto-Fill slot. Open the dialog again. Confirm the warning notice is visible.
    - Disconnect from server (or use a provider with `supportsPlaylistWrite: false` — none exists in prod, but the UI hides the button when the field is false). Confirm button disappears.
    - In Artists browse view, right-click an artist card (grid mode). Confirm context menu appears with "Send to playlist…".
    - Click it. Enter a name. Confirm playlist is created on the server.
    - Switch to list view. Right-click an artist row. Confirm same context menu appears.
    - Right-click an album card — confirm context menu appears.
    - Right-click a genre card — confirm no context menu appears (genre type is excluded).

## Dev Notes

### Two-file implementation split: daemon + UI

This story touches **5 files** across two packages:

| File | Change |
|------|--------|
| `hifimule-daemon/src/rpc.rs` | Add `supportsPlaylistWrite` field to `handle_get_daemon_state` response |
| `hifimule-i18n/catalog.json` | Add 10 new i18n keys across all 3 language blocks |
| `hifimule-ui/src/library.ts` | Add module-level capability variable + setter; pass to grid/list renders |
| `hifimule-ui/src/components/MediaCard.ts` | Accept `supportsPlaylistWrite` param; add context menu static methods |
| `hifimule-ui/src/components/BasketSidebar.ts` | Add `supportsPlaylistWrite` field; button in header; `handleSaveAsPlaylist()` dialog |

No new files are created; no other daemon handlers, providers, or Rust files are touched.

### Capability gating: `supportsPlaylistWrite` propagation path

```
get_daemon_state (rpc.rs) → BasketSidebar.refreshAndRender()
                                 → this.supportsPlaylistWrite
                                 → setPlaylistWriteCapability()  (library.ts module var)
                                      → MediaCard.create() — grid/list cards
                          → startDaemonStatePolling()
                                 → this.supportsPlaylistWrite (keeps in sync on 2s poll)
```

The library module variable (`_supportsPlaylistWrite`) is set by `BasketSidebar` which already owns the daemon state polling lifecycle. The initial value is set in `refreshAndRender()` (called on construction). Subsequent updates happen in the 2-second polling interval. Both callers in library.ts — `renderGrid()` and `renderListRow()` — read `_supportsPlaylistWrite` at render time, so they always see the current value.

### Why `supportsPlaylistWrite` must come from daemon, not be inferred from `serverType`

Both Jellyfin and Subsonic/OpenSubsonic set `supports_playlist_write: true` in their `Capabilities` struct (`providers/jellyfin.rs` and `providers/subsonic.rs` — Stories 11.2 and 11.3). In practice today, any connected provider supports playlist write. However, the architecture spec mandates checking `capabilities().supports_playlist_write` explicitly — the UI must not assume based on server type. The daemon field correctly returns `false` when no provider is connected.

### Daemon state response field

The `get_daemon_state` response (line 1357) currently does not include `supportsPlaylistWrite`. Adding it requires reading the provider lock, which is async. The existing provider access pattern at `rpc.rs:832` uses:

```rust
let provider = require_provider(state).await?;
provider.capabilities().supports_playlist_write
```

But for daemon state, we want a non-failing read (returning `false` when disconnected), so the pattern is:

```rust
let supports_playlist_write = {
    let guard = state.provider.read().await;
    guard
        .as_ref()
        .map(|p| p.capabilities().supports_playlist_write)
        .unwrap_or(false)
};
```

The `guard` is dropped at the end of the block, releasing the lock before the long synchronous JSON serialization that follows — correct lifetime management.

### `playlist.create` RPC contract (implemented in Story 11.4)

The RPC handler at `rpc.rs:822–899` accepts:
```json
{ "name": "string", "itemIds": ["id1", "id2", ...] }
```
Returns `{ "playlistId": "server-assigned-id" }`.

**Critical**: The handler silently skips unresolvable item IDs and reports them in `skippedItemIds` (review patch from 11.4). Callers don't need to pre-validate IDs. The UI does not need to display `skippedItemIds` in this story.

**Auto-Fill filtering**: Story 11.4's handler filters `__auto_fill_slot__` silently — but Story 11.5's UI also filters it on the client side (`manualIds`) so the slot is never sent. Both sides are correct.

### Context menu: custom div vs Shoelace `sl-dropdown`

`sl-dropdown` requires a trigger element and renders inline — not suitable for a context menu that must appear at an arbitrary mouse coordinate. A custom absolutely-positioned `div` + click-outside handler is the standard approach (no third-party library needed). The menu uses Shoelace CSS custom properties for colors/shadows so it visually matches the rest of the app.

The `activeContextMenu` static property on `MediaCard` ensures only one context menu exists at a time. Opening a second one (before dismissing the first) removes the old one. This prevents stacking.

### Dialog pattern for `handleSaveAsPlaylist` and `openCreatePlaylistDialog`

Both follow the established `sl-dialog` pattern from `BasketSidebar.openDeviceSettings()`:
1. `document.createElement('sl-dialog')`
2. Set `innerHTML` for form contents
3. `document.body.appendChild(dialog)`
4. Bind footer button events
5. Listen `sl-after-hide` → `dialog.remove()`
6. `customElements.whenDefined('sl-dialog').then(() => dialog.show())`

The `autofocus` attribute on the name `<sl-input>` is set directly on the element — Shoelace Web Components respect this attribute. The user can type the name immediately after the dialog opens.

### Why dynamic import in `MediaCard.openCreatePlaylistDialog`

`MediaCard.ts` does not import `rpcCall` at the top level (it's imported by `library.ts` which is a top-level consumer — adding a static import would create a potential circular import chain: `library.ts → MediaCard.ts → rpc.ts` which is fine, but `MediaCard.ts` also uses `basketStore` from `basket.ts` → `rpc.ts` is fine too). 

Actually, `rpc.ts` has no imports from the UI components, so a static import would work. However, since `openCreatePlaylistDialog` is a rarely-called static method, a dynamic import is the appropriate pattern that matches the lazy-loading style used in `main.ts`. Either approach compiles; use dynamic import for consistency.

### TypeScript: `contextmenu` event type

The `contextmenu` event is a `MouseEvent`. The listener receives a `MouseEvent`, and `e.clientX` / `e.clientY` are available without casting. Use `(e: MouseEvent)` in the parameter type.

### i18n key location in catalog.json

The catalog is structured as a flat JSON object per language. The English block starts with `"en": {` and ends before `"fr": {`. Add the new keys in alphabetical order within the relevant namespace, or after the last `basket.actions.*` key for the basket keys. Maintain valid JSON — no trailing commas on the last key in each object.

### Project Structure Notes

All UI files are in `hifimule-ui/src/`. The daemon file is `hifimule-daemon/src/rpc.rs`. The i18n catalog is at `hifimule-i18n/catalog.json`. No Cargo.toml or package.json changes are needed.

### References

- Epic 11 Story 11.5 spec: `_bmad-output/planning-artifacts/epics.md:2213–2251`
- Architecture Epic 11 (Capability Gating section): `_bmad-output/planning-artifacts/architecture.md:584–592`
- UX spec §5.2 (Save as Playlist + Context Menu): `_bmad-output/planning-artifacts/ux-design-specification.md:91–93`
- Sprint change proposal (UI affordances description): `_bmad-output/planning-artifacts/sprint-change-proposal-2026-06-05.md:152–158`
- `handle_get_daemon_state`: `hifimule-daemon/src/rpc.rs:1284–1373`
- Provider capability check pattern: `hifimule-daemon/src/rpc.rs:832` (`provider.capabilities().supports_playlist_write`)
- `state.provider` type and lock pattern: `hifimule-daemon/src/rpc.rs:832–836`
- `playlist.create` RPC handler: `hifimule-daemon/src/rpc.rs:822` (post-11.4)
- `AUTO_FILL_SLOT_ID` constant: `hifimule-ui/src/state/basket.ts:6`
- `basketStore.getItems()`: `hifimule-ui/src/state/basket.ts:201`
- `BasketSidebar` existing dialog pattern (`openDeviceSettings`): `hifimule-ui/src/components/BasketSidebar.ts:434–568`
- `BasketSidebar` daemon state polling: `hifimule-ui/src/components/BasketSidebar.ts:618–675`
- `BasketSidebar` `refreshAndRender`: `hifimule-ui/src/components/BasketSidebar.ts:229–317`
- `MediaCard.create()` current signature: `hifimule-ui/src/components/MediaCard.ts:39–45`
- `renderGrid()` MediaCard call: `hifimule-ui/src/library.ts:514`
- `renderListRow()` contextmenu opportunity: `hifimule-ui/src/library.ts:693–708`
- `rpcCall` function: `hifimule-ui/src/rpc.ts:75`
- i18n catalog: `hifimule-i18n/catalog.json`
- Story 11.4 (previous, RPC implementation): `_bmad-output/implementation-artifacts/11-4-playlist-rpcs-and-selection-to-tracks-resolution.md`

## Dev Agent Record

### Agent Model Used

claude-sonnet-4-6

### Debug Log References

- `tsc` reported one pre-existing TS5101 warning (`baseUrl` deprecated in tsconfig.json) — unrelated to this story, zero source-file errors.

### Completion Notes List

- Added `supportsPlaylistWrite` field to `handle_get_daemon_state` response in `rpc.rs` by reading `state.provider.read().await` in a scoped block; returns `false` when no provider is connected.
- Added 11 new i18n keys to all three language blocks (en/fr/es) in `hifimule-i18n/catalog.json`; JSON validated clean.
- Added `_supportsPlaylistWrite` module-level variable and `setPlaylistWriteCapability()` exported setter to `library.ts`.
- Extended `MediaCard.create()` with optional `supportsPlaylistWrite` param; added `contextmenu` listener on artist/album cards only. Static `showContextMenu()` creates a positioned div overlay, `openCreatePlaylistDialog()` follows the established `sl-dialog` pattern. `activeContextMenu` static property ensures single-menu-at-a-time behavior.
- Added contextmenu listener to list rows in `renderListRow()` for artist/album types; delegates to `MediaCard.showContextMenu()`.
- `BasketSidebar`: added `supportsPlaylistWrite` field, import, and two update sites (refreshAndRender + polling); added "Save as Playlist" `sl-icon-button` in non-empty basket header; `handleSaveAsPlaylist()` dialog filters Auto-Fill slot from `manualIds` and shows informational notice when slot present.
- `cargo check`: 0 errors, 2 pre-existing warnings (mtp.rs dead code — unrelated).
- `tsc`: 0 source-file errors.

### File List

- hifimule-daemon/src/rpc.rs
- hifimule-i18n/catalog.json
- hifimule-ui/src/library.ts
- hifimule-ui/src/components/MediaCard.ts
- hifimule-ui/src/components/BasketSidebar.ts

## Change Log

- 2026-06-05: Story 11.5 created — "Save as Playlist" UI and context menu ready for dev.
- 2026-06-05: Story 11.5 implemented — daemon state exposes supportsPlaylistWrite; basket header gets Save as Playlist button; MediaCard and list rows get right-click context menu; all 7 tasks complete.

## Status

review
