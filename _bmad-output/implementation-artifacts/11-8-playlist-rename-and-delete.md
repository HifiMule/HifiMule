---
baseline_commit: d712ade
---

# Story 11.8: Playlist Rename and Delete — Curation View Header

Status: done

## Story

As a Ritualist (Arthur),
I want to rename and delete a playlist directly from the curation view,
so that I can manage my library's playlist catalogue without leaving the edit context.

## Acceptance Criteria

1. **Given** the curation view is open for a playlist **When** I click the playlist name in the header **Then** the name becomes an inline `<sl-input>` pre-filled with the current name **And** Save and Cancel affordances appear alongside the input.

2. **Given** the inline name input is open **When** I edit the name and click Save **Then** `playlist.rename({ playlistId, name: newName })` is called **And** the header title updates to the new name **And** the input is dismissed.

3. **Given** the inline name input is open **When** I press Escape or click Cancel **Then** the input is dismissed with no RPC call.

4. **Given** the active provider supports playlist write **When** the curation view renders **Then** a delete icon-button (trash) is visible in the header.

5. **Given** I click the delete icon-button **Then** an `<sl-dialog>` opens showing the playlist name and asking for confirmation.

6. **Given** the confirmation dialog is open and I confirm **Then** `playlist.delete({ playlistId })` is called **And** the UI navigates back to the playlist browser.

7. **Given** the confirmation dialog is open and I cancel **Then** the dialog closes with no RPC call.

8. **Given** the active provider does not support playlist write **Then** the delete icon-button is hidden.

## Tasks

### Task 1: Extend `MediaProvider` trait with `rename_playlist` [x]

**File:** `hifimule-daemon/src/providers/mod.rs`

Add after the existing `delete_playlist` default (line ~229), alongside all other playlist write methods:

```rust
async fn rename_playlist(
    &self,
    _playlist_id: &str,
    _new_name: &str,
) -> Result<(), ProviderError> {
    Err(ProviderError::UnsupportedCapability(
        "rename_playlist is not supported by this provider".to_string(),
    ))
}
```

**Pattern:** Identical to `delete_playlist`'s default — `UnsupportedCapability` with a string naming the method.

Also add a unit test in the `#[cfg(test)]` block (same pattern as the four existing `trait_default_*_returns_unsupported` tests, lines ~981–1017):

```rust
#[tokio::test]
async fn trait_default_rename_playlist_returns_unsupported() {
    let provider = MockProvider::default();
    let result = provider.rename_playlist("playlist-1", "New Name").await;
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("rename_playlist"), "message should name the method: {msg}");
}
```

### Task 2: Add `rename_item` to `JellyfinApiClient` in `api.rs` [x]

**File:** `hifimule-daemon/src/api.rs`

Add a new method after `delete_item` (line ~1508). Performs a 2-step rename:
1. GET `/Items/{id}` to fetch the full item JSON (reuses `get_item_details`)
2. POST `/Items/{id}` with the full body with `Name` updated

`JellyfinItem` is `#[serde(rename_all = "PascalCase")]` (confirmed line 122) so it round-trips correctly as the POST body.

```rust
pub async fn rename_item(
    &self,
    url: &str,
    token: &str,
    user_id: &str,
    item_id: &str,
    new_name: &str,
) -> Result<()> {
    CredentialManager::validate_url(url)?;
    CredentialManager::validate_token(token)?;

    // Step 1: fetch current item JSON
    let mut item = self
        .get_item_details(url, token, user_id, item_id)
        .await?;

    // Step 2: mutate name and POST back
    item.name = new_name.to_string();

    let mut headers = HeaderMap::new();
    headers.insert(
        "X-Emby-Token",
        HeaderValue::from_str(token).map_err(|_| anyhow!("Invalid token format"))?,
    );

    let endpoint = jellyfin_endpoint(url, &["Items", item_id])?;

    let response = self
        .client
        .post(endpoint)
        .headers(headers)
        .json(&item)
        .send()
        .await?;

    let status = response.status();
    if !status.is_success() {
        let text = response.text().await.unwrap_or_default();
        return Err(anyhow!("Server returned status: {} — {}", status, text));
    }
    Ok(())
}
```

**Critical notes:**
- `jellyfin_endpoint(url, &["Items", item_id])` is the same helper used by `delete_item` (line 1499) — use it.
- `HeaderMap` and `HeaderValue` are already imported in api.rs — no new `use` needed.
- `get_item_details` has a `println!("DEBUG: ...")` leftover (line 376) — don't remove it, it's pre-existing.
- `JellyfinItem` has `#[serde(rename_all = "PascalCase")]` so `item.name` serializes as `"Name"` — correct for Jellyfin.

### Task 3: Implement `rename_playlist` in `JellyfinProvider` [x]

**File:** `hifimule-daemon/src/providers/jellyfin.rs`

Add after `delete_playlist` (line ~393):

```rust
async fn rename_playlist(&self, playlist_id: &str, new_name: &str) -> Result<(), ProviderError> {
    self.client
        .rename_item(self.url(), self.token(), self.user_id(), playlist_id, new_name)
        .await
        .map_err(Self::map_error)
}
```

**Pattern:** Identical to `delete_playlist` (line 393-398) — delegate to `self.client`, map error with `Self::map_error`.

### Task 4: Implement `rename_playlist` in `SubsonicProvider` [x]

**File:** `hifimule-daemon/src/providers/subsonic.rs`

Add after `delete_playlist` (line ~423):

```rust
async fn rename_playlist(&self, playlist_id: &str, new_name: &str) -> Result<(), ProviderError> {
    self.client
        .update_playlist_rename(playlist_id, new_name)
        .await
}
```

Add the corresponding client method in `SubsonicApiClient` (after `delete_playlist` at line ~898):

```rust
async fn update_playlist_rename(
    &self,
    playlist_id: &str,
    new_name: &str,
) -> Result<(), ProviderError> {
    let _: NoBody = self
        .get("updatePlaylist", &[("playlistId", playlist_id), ("name", new_name)])
        .await?;
    Ok(())
}
```

**Critical notes:**
- `sanitize_subsonic_url()` is applied inside `self.get()` already (line 1091 — all requests log through the same sanitizer). No extra `sanitize_subsonic_url()` call needed here.
- `NoBody` and `self.get()` are the exact pattern used by `update_playlist_add` (line 875) and `update_playlist_remove_by_indices` (line 894).
- `updatePlaylist` endpoint is idempotent for name changes — same `playlistId` param, only `name` changes.

### Task 5: Add `playlist.rename` RPC handler [x]

**File:** `hifimule-daemon/src/rpc.rs`

**Step 5a:** Add to the match dispatch table (after `"playlist.delete"` at line 372):

```rust
"playlist.rename" => handle_playlist_rename(&state, payload.params).await,
```

**Step 5b:** Add handler function after `handle_playlist_delete` (after line ~1114):

```rust
async fn handle_playlist_rename(
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
    let name = params["name"]
        .as_str()
        .ok_or(JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: "Missing name".to_string(),
            data: None,
        })?
        .to_owned();
    provider
        .rename_playlist(&playlist_id, &name)
        .await
        .map_err(provider_error_to_rpc)?;
    Ok(serde_json::json!({ "ok": true }))
}
```

**Pattern:** `handle_playlist_delete` (line 1084) is the exact template — same `require_provider`, same `supports_playlist_write` guard, same `ERR_INVALID_PARAMS` extraction, same `provider_error_to_rpc` propagation.

### Task 6: Add i18n keys [x]

**File:** `hifimule-i18n/catalog.json`

Add to the `"en"` block after the existing `"playlist.curation.no_search_results"` entry (line ~192), and to the `"fr"` and `"es"` blocks in the same relative position:

```json
"playlist.curation.rename_save": "Save name",
"playlist.curation.rename_cancel": "Cancel rename",
"playlist.curation.delete_title": "Delete playlist",
"playlist.curation.delete_body": "Delete \"{name}\"? This cannot be undone.",
"playlist.curation.delete_confirm": "Delete",
"playlist.curation.delete_cancel_btn": "Cancel"
```

6 keys × 3 languages = 18 additions.

**Critical:** No trailing comma on the last key of each language object — validate JSON is well-formed after editing.

**Note on `delete_body`:** Use `{name}` as the placeholder (not `{{name}}`). The `t()` function in `i18n.ts` uses `{key}` interpolation — check by searching `catalog.json` for existing body strings with placeholders (e.g. `add_tracks_error` uses `{message}`).

### Task 7: Update `PlaylistCurationView` — add `supportsPlaylistWrite` field [x]

**File:** `hifimule-ui/src/components/PlaylistCurationView.ts`

**Step 7a:** Add `private supportsPlaylistWrite: boolean = false;` field after `private isAddingTracks = false;` (line ~31).

**Step 7b:** Update constructor signature to accept `supportsPlaylistWrite`:

```typescript
constructor(
    container: HTMLElement,
    playlistId: string,
    playlistName: string,
    onClose: () => void,
    supportsPlaylistWrite = false,
) {
    this.container = container;
    this.playlistId = playlistId;
    this.playlistName = playlistName;
    this.onClose = onClose;
    this.supportsPlaylistWrite = supportsPlaylistWrite;
}
```

Default value `= false` keeps the constructor backwards-compatible (no forced update elsewhere).

**Step 7c:** Update call site in `library.ts` (line ~1101):

```typescript
const view = new PlaylistCurationView(
    container,
    playlistId,
    playlistName,
    () => {
        invalidatePlaylistsCache();
        loadPlaylists();
    },
    _supportsPlaylistWrite,
);
```

### Task 8: Add rename — inline name editing in `PlaylistCurationView.ts` [x]

**File:** `hifimule-ui/src/components/PlaylistCurationView.ts`

**Step 8a:** Add state field after `private supportsPlaylistWrite` (Task 7):

```typescript
private isRenamingPlaylist = false;
```

**Step 8b:** In `render()`, replace the static playlist name `<span>` in the header (line ~157) with:

```typescript
${this.isRenamingPlaylist
    ? `<sl-input
           id="playlist-rename-input"
           value="${this.escapeAttr(this.playlistName)}"
           size="small"
           style="flex: 1; max-width: 300px;"
       ></sl-input>
       <sl-icon-button
           class="playlist-rename-save"
           name="check"
           label="${t('playlist.curation.rename_save')}"
       ></sl-icon-button>
       <sl-icon-button
           class="playlist-rename-cancel"
           name="x"
           label="${t('playlist.curation.rename_cancel')}"
       ></sl-icon-button>`
    : `<span
           class="playlist-name-title"
           style="font-weight: var(--sl-font-weight-semibold); font-size: var(--sl-font-size-medium); cursor: pointer; border-bottom: 1px dashed var(--sl-color-neutral-400);"
           title="${t('playlist.curation.rename_save')}"
       >${this.escapeHtml(this.playlistName)}</span>`
}
```

**Step 8c:** In `bindEvents()`, add after the existing `#curation-close-btn` listener (line ~279):

```typescript
// Rename: click title → enter edit mode
this.container.querySelector('.playlist-name-title')?.addEventListener('click', () => {
    this.isRenamingPlaylist = true;
    this.render();
    const input = this.container.querySelector('#playlist-rename-input') as any;
    if (input) input.focus();
});

// Rename: save
this.container.querySelector('.playlist-rename-save')?.addEventListener('click', async () => {
    const input = this.container.querySelector('#playlist-rename-input') as any;
    const newName = input?.value?.trim();
    if (newName && newName !== this.playlistName) {
        await rpcCall('playlist.rename', { playlistId: this.playlistId, name: newName });
        this.playlistName = newName;
    }
    this.isRenamingPlaylist = false;
    this.render();
});

// Rename: cancel
this.container.querySelector('.playlist-rename-cancel')?.addEventListener('click', () => {
    this.isRenamingPlaylist = false;
    this.render();
});

// Rename: Escape key
this.container.querySelector('#playlist-rename-input')?.addEventListener('keydown', (e) => {
    if ((e as KeyboardEvent).key === 'Escape') {
        this.isRenamingPlaylist = false;
        this.render();
    }
});
```

**Critical notes:**
- Use `escapeAttr()` for the `value` attribute of `<sl-input>` (attribute context). Use `escapeHtml()` for the `<span>` text content. Both methods exist on `PlaylistCurationView` at line ~566-578.
- Do NOT name any lambda parameter `t` — it shadows the imported `t()` i18n function. This was a learnt guard from Story 11.7.
- `rpcCall` is already imported at the top of the file (line 1): `import { fetchBrowsePlaylist, fetchBrowseSearch, BrowseTrack, rpcCall } from '../rpc';`
- No `e.stopPropagation()` needed here (title `<span>` and icon-buttons are not nested inside rows that have separate click handlers).
- On save: update `this.playlistName` before `this.render()` so the header shows the new name immediately.

### Task 9: Add delete — confirmation dialog in `PlaylistCurationView.ts` [x]

**File:** `hifimule-ui/src/components/PlaylistCurationView.ts`

**Step 9a:** In `render()`, add delete icon-button to the header (after the rename region, before the closing `</div>` of `.curation-header`):

```typescript
${this.supportsPlaylistWrite
    ? `<sl-icon-button
           class="playlist-delete-btn"
           name="trash"
           label="${t('playlist.curation.delete_title')}"
           style="color: var(--sl-color-danger-600); margin-left: auto;"
       ></sl-icon-button>`
    : ''
}
```

**Note:** `margin-left: auto` pushes the trash button to the far right of the header.

**Step 9b:** Add delete confirmation dialog markup inside the main container HTML (e.g., at the end of the `curation-view` div, as a sibling to `.curation-panels`):

```typescript
<sl-dialog id="playlist-delete-dialog" label="${t('playlist.curation.delete_title')}">
    <p>${t('playlist.curation.delete_body').replace('{name}', this.escapeHtml(this.playlistName))}</p>
    <sl-button slot="footer" class="playlist-delete-cancel" variant="default">
        ${t('playlist.curation.delete_cancel_btn')}
    </sl-button>
    <sl-button slot="footer" class="playlist-delete-confirm" variant="danger">
        ${t('playlist.curation.delete_confirm')}
    </sl-button>
</sl-dialog>
```

**Step 9c:** In `bindEvents()`, add:

```typescript
// Delete: open dialog
this.container.querySelector('.playlist-delete-btn')?.addEventListener('click', () => {
    (this.container.querySelector('#playlist-delete-dialog') as any)?.show();
});

// Delete: cancel
this.container.querySelector('.playlist-delete-cancel')?.addEventListener('click', () => {
    (this.container.querySelector('#playlist-delete-dialog') as any)?.hide();
});

// Delete: confirm
this.container.querySelector('.playlist-delete-confirm')?.addEventListener('click', async () => {
    await rpcCall('playlist.delete', { playlistId: this.playlistId });
    this.onClose();
});
```

**Critical notes:**
- `this.onClose()` is the existing back-navigation method — it invalidates the playlists cache and restores the list view (wired in `library.ts` at line 1101). Use it, not any non-existent `navigateBack()` method. `navigateBack()` does NOT exist on `PlaylistCurationView`.
- `playlist.delete` RPC exists since Story 11.4 — no backend changes needed.
- `sl-dialog` pattern follows the existing pattern in Story 11.5's `openCreatePlaylistDialog` (in `MediaCard.ts`): use `.show()` / `.hide()` on the Shoelace dialog element.
- The dialog is inside `this.container.innerHTML` so it's re-created on every `render()` call — the `show()` call is safe because it's invoked after `render()` has already placed the dialog in the DOM.

### Task 10: Verify compilation [x]

- `rtk cargo check` — zero new Rust errors (changes: `providers/mod.rs`, `api.rs`, `jellyfin.rs`, `subsonic.rs`, `rpc.rs`)
- `rtk tsc` — zero TypeScript errors (changes: `PlaylistCurationView.ts`, `library.ts`)

## Key Notes

- `this.onClose()` is the back-navigation — it exists, it works, use it.
- `navigateBack()` does NOT exist on `PlaylistCurationView` — don't create it or reference it.
- `JellyfinItem` round-trips correctly for POST because it's `#[serde(rename_all = "PascalCase")]`.
- `rename_item` is a NEW api.rs method — it does not exist yet; it must be added (Task 2).
- The `supportsPlaylistWrite` constructor parameter is new — pass `_supportsPlaylistWrite` from `library.ts` (Task 7c).
- `escapeAttr()` for attribute values (e.g., `<sl-input value="...">`), `escapeHtml()` for HTML text content.
- `t()` interpolation uses `{key}` format (not `{{key}}`).
- `playlist.delete` RPC from Story 11.4 is reused unchanged — no backend work for delete.

## Dev Notes

### Files to change

| File | Change |
|------|--------|
| `hifimule-daemon/src/providers/mod.rs` | Add `rename_playlist` default + unit test |
| `hifimule-daemon/src/api.rs` | Add `rename_item` method to `JellyfinApiClient` |
| `hifimule-daemon/src/providers/jellyfin.rs` | Implement `rename_playlist` via `client.rename_item` |
| `hifimule-daemon/src/providers/subsonic.rs` | Implement `rename_playlist` + `update_playlist_rename` client method |
| `hifimule-daemon/src/rpc.rs` | Add `"playlist.rename"` dispatch + `handle_playlist_rename` handler |
| `hifimule-ui/src/components/PlaylistCurationView.ts` | `supportsPlaylistWrite` field + constructor param + `isRenamingPlaylist` field + inline rename UI + delete button + delete dialog |
| `hifimule-ui/src/library.ts` | Pass `_supportsPlaylistWrite` to `PlaylistCurationView` constructor |
| `hifimule-i18n/catalog.json` | 6 new keys × 3 language blocks |

No new files. No Cargo.toml or package.json changes.

### Available RPCs (post-story)

| RPC | Params | Returns | Status |
|-----|--------|---------|--------|
| `playlist.rename` | `{ playlistId: string, name: string }` | `{ ok: true }` | **NEW — Task 5** |
| `playlist.delete` | `{ playlistId: string }` | `{ ok: true }` | Existing — Story 11.4 |

### `PlaylistCurationView` constructor — before vs after

**Before (Story 11.7):**
```typescript
constructor(
    container: HTMLElement,
    playlistId: string,
    playlistName: string,
    onClose: () => void,
)
```

**After (Story 11.8):**
```typescript
constructor(
    container: HTMLElement,
    playlistId: string,
    playlistName: string,
    onClose: () => void,
    supportsPlaylistWrite = false,  // ← new, default false
)
```

The default `= false` means no other callsite breaks. Only `library.ts` (line ~1101) needs updating.

### Shoelace patterns in this component

From Story 11.7 learnings (confirmed in existing code):
- `sl-input` fires `sl-input` custom event (not native `input`) — use `addEventListener('sl-input', ...)`
- `sl-checkbox` fires `sl-change` (not native click) — but no checkboxes in this story
- `sl-dialog` uses `.show()` / `.hide()` methods (not native open attribute)
- `sl-icon-button` click events do NOT need `e.stopPropagation()` unless nested inside a parent that also handles click

### `delete_body` interpolation

`t('playlist.curation.delete_body').replace('{name}', this.escapeHtml(this.playlistName))`

The `t()` function signature returns a string. The `.replace()` is applied after translation. Confirm by checking `add_tracks_error` in the existing component — same pattern: `t('playlist.curation.add_tracks_error', { message: msg })`. Actually, `t()` accepts an optional second arg for replacements — check the `i18n.ts` `t()` signature to decide which form to use.

If `t()` supports `t(key, { name: value })` interpolation (likely, see `add_tracks_error` usage), use:
```typescript
t('playlist.curation.delete_body', { name: this.escapeHtml(this.playlistName) })
```
Otherwise fall back to `.replace('{name}', ...)`.

### Learnings from Story 11.7 applied

1. **`escapeAttr()` vs `escapeHtml()`**: attribute values use `escapeAttr`, HTML text content uses `escapeHtml`. Both methods exist on `PlaylistCurationView` (lines 566-578).
2. **No `t` lambda params**: lambdas in this component must not name any param `t` — it shadows the imported `t()` i18n function.
3. **`sl-input` event**: Shoelace fires `sl-input`, not native `input`. In the rename save handler, read `input.value` directly (it's already the current value after the event fired).
4. **Pessimistic vs optimistic**: Rename is pessimistic — wait for `playlist.rename` RPC to succeed before updating `this.playlistName` and re-rendering.
5. **`render()` is stateful**: After `this.isRenamingPlaylist = true; this.render()`, the container is fully re-rendered with the `<sl-input>` in place — safe to `.focus()` immediately after.
6. **`rpcCall` is top-level imported**: Already imported at line 1. No lazy import needed here (unlike `MediaCard.ts`).

### References

- Story 11.7 (previous — add tracks): `_bmad-output/implementation-artifacts/11-7-add-tracks-to-playlist-browse-and-curation.md`
- Story 11.6 (dual-panel curation view): `_bmad-output/implementation-artifacts/11-6-dual-panel-playlist-curation-view-and-stats.md`
- Story 11.4 (`playlist.delete` RPC): `_bmad-output/implementation-artifacts/11-4-playlist-rpcs-and-selection-to-tracks-resolution.md`
- Sprint change proposal: `_bmad-output/planning-artifacts/sprint-change-proposal-2026-06-07-playlist-rename-and-delete.md`
- `MediaProvider` trait playlist write methods: `hifimule-daemon/src/providers/mod.rs:199–236`
- `delete_playlist` default (template for `rename_playlist` default): `hifimule-daemon/src/providers/mod.rs:229`
- `JellyfinItem` struct (PascalCase derive): `hifimule-daemon/src/api.rs:121–171`
- `get_item_details` in api.rs: `hifimule-daemon/src/api.rs:350`
- `delete_item` in api.rs (template for `rename_item`): `hifimule-daemon/src/api.rs:1489`
- `jellyfin_endpoint` helper usage: `hifimule-daemon/src/api.rs:1499`
- Jellyfin `delete_playlist` impl (template for `rename_playlist` impl): `hifimule-daemon/src/providers/jellyfin.rs:393`
- Subsonic `update_playlist_add` (template for `update_playlist_rename`): `hifimule-daemon/src/providers/subsonic.rs:866`
- Subsonic `delete_playlist` client method (template): `hifimule-daemon/src/providers/subsonic.rs:898`
- `handle_playlist_delete` in rpc.rs (template for `handle_playlist_rename`): `hifimule-daemon/src/rpc.rs:1084`
- RPC dispatch table: `hifimule-daemon/src/rpc.rs:368–372`
- `PlaylistCurationView` constructor: `hifimule-ui/src/components/PlaylistCurationView.ts:33`
- `PlaylistCurationView` render header (line with playlist name span): `hifimule-ui/src/components/PlaylistCurationView.ts:157`
- `PlaylistCurationView` bindEvents: `hifimule-ui/src/components/PlaylistCurationView.ts:279`
- `escapeHtml` / `escapeAttr`: `hifimule-ui/src/components/PlaylistCurationView.ts:566–578`
- `openCurationView` call site in library.ts: `hifimule-ui/src/library.ts:1101`
- `_supportsPlaylistWrite` module variable: `hifimule-ui/src/library.ts:30`
- Existing `playlist.curation.*` i18n keys: `hifimule-i18n/catalog.json:173–192`

## Dev Agent Record

### Agent Model Used

claude-sonnet-4-6

### Debug Log References

_None — implementation was straightforward with no blockers._

### Completion Notes List

- Task 1: Added `rename_playlist` default to `MediaProvider` trait (`providers/mod.rs`) returning `UnsupportedCapability`. Unit test `trait_default_rename_playlist_returns_unsupported` added alongside existing four trait-default tests — all pass.
- Task 2: Added `rename_item` to `JellyfinApiClient` (`api.rs`). 2-step approach: GET `/Items/{id}` via `get_item_details`, mutate `item.name`, POST back. Uses `jellyfin_endpoint` helper and existing `HeaderMap`/`HeaderValue` imports.
- Task 3: Added `rename_playlist` impl to `JellyfinProvider` (`jellyfin.rs`) — delegates to `client.rename_item` with `map_error`, identical pattern to `delete_playlist`.
- Task 4: Added `rename_playlist` impl to `SubsonicProvider` (`subsonic.rs`) + `update_playlist_rename` client method using `updatePlaylist` endpoint with `playlistId`/`name` params. `NoBody` pattern matches `update_playlist_add`.
- Task 5: Added `"playlist.rename"` dispatch and `handle_playlist_rename` handler to `rpc.rs`. Exact same structure as `handle_playlist_delete`: `require_provider`, `supports_playlist_write` guard, `ERR_INVALID_PARAMS` extraction for `playlistId` and `name`, `provider_error_to_rpc` propagation.
- Task 6: Added 6 i18n keys × 3 language blocks (en/fr/es) = 18 additions to `catalog.json`. JSON validated well-formed.
- Task 7: Added `supportsPlaylistWrite` and `isRenamingPlaylist` fields to `PlaylistCurationView`. Updated constructor with `supportsPlaylistWrite = false` default. Updated call site in `library.ts` to pass `_supportsPlaylistWrite`.
- Task 8: Replaced static name `<span>` in header with conditional rename UI: click title → `<sl-input>` + check/x icon-buttons. Save handler calls `playlist.rename` RPC, updates `this.playlistName`, then re-renders. Escape key / cancel button dismiss without RPC. Used `escapeAttr()` for input value attribute, `escapeHtml()` for span text.
- Task 9: Added trash icon-button (gated on `supportsPlaylistWrite`) to header. Delete confirmation `<sl-dialog>` placed as sibling to `.curation-panels`. Confirm handler calls `playlist.delete` and invokes `this.onClose()`. Cancel hides the dialog. Used `t()` with `{ name: ... }` interpolation for dialog body.
- Task 10: `rtk cargo check` — 0 errors (2 pre-existing dead_code warnings in mtp.rs). `cargo test` — 415 passed. TypeScript: pre-existing `TS5101 baseUrl deprecated` warning only, no new errors.

### File List

- `hifimule-daemon/src/providers/mod.rs`
- `hifimule-daemon/src/api.rs`
- `hifimule-daemon/src/providers/jellyfin.rs`
- `hifimule-daemon/src/providers/subsonic.rs`
- `hifimule-daemon/src/rpc.rs`
- `hifimule-ui/src/components/PlaylistCurationView.ts`
- `hifimule-ui/src/library.ts`
- `hifimule-i18n/catalog.json`

## Change Log

- 2026-06-07: Story 11.8 created — playlist rename (new backend) and delete UI in curation view header ready for dev.
- 2026-06-07: Story 11.8 implemented — `rename_playlist` trait + Jellyfin/Subsonic impls, `playlist.rename` RPC, inline rename UI and delete confirmation dialog in `PlaylistCurationView`. 415 Rust tests pass. Status → review.
- 2026-06-07: Code review — 1 decision-needed + 3 patches applied (Jellyfin `skip_serializing_if` hardening, rename/delete error handling + re-entrancy guards, daemon empty-name validation, tooltip i18n key), 2 low-priority items deferred, 6 dismissed. `cargo check` 0 errors, 411 tests pass, `tsc` no new errors, catalog.json valid. Status → done.

## Review Findings

_Code review 2026-06-07 — Blind Hunter + Edge Case Hunter + Acceptance Auditor. All 8 ACs met as written; findings below are robustness/UX/data-integrity gaps. 6 findings dismissed as noise (escapeAttr XSS — escapeAttr escapes `&"<>`; "Escape listener dies after render" — `render()` re-runs `bindEvents()`; Subsonic body discard — envelope `status` is checked in `get()`; hardcoded English RPC error strings — consistent with existing `rpc.rs` handlers; stale list name — `onClose()` invalidates cache + reloads list; rename path not capability-gated — spec AC1 intent + view only opens when write supported + backend guards it)._

- [x] [Review][Patch] (was Decision — resolved: harden) Jellyfin `rename_item` partial GET-then-POST can clobber playlist metadata — **FIXED**: added `#[serde(skip_serializing_if = "Option::is_none")]` to all `Option` fields of `JellyfinItem` so unset fields are no longer POSTed as explicit `null`. [`hifimule-daemon/src/api.rs`:128-170]
- [x] [Review][Patch] Rename Save + Delete Confirm lack error handling and re-entrancy guards — **FIXED**: both handlers now use a `try/catch/finally` with `isSavingRename` / `isDeleting` guard flags and surface failures via `showToast(..., 'danger')`; rename stays in edit mode and delete keeps the dialog open on failure so the user can retry. [`hifimule-ui/src/components/PlaylistCurationView.ts`:335-359, 376-389]
- [x] [Review][Patch] Daemon `handle_playlist_rename` does not validate empty/whitespace name — **FIXED**: handler now trims `name` and rejects empty with `ERR_INVALID_PARAMS`. [`hifimule-daemon/src/rpc.rs` `handle_playlist_rename`]
- [x] [Review][Patch] Click-to-rename title tooltip uses the wrong i18n key — **FIXED**: title span now uses new `playlist.curation.rename_hint` ("Rename playlist") key, added across en/fr/es. [`hifimule-ui/src/components/PlaylistCurationView.ts`:181]
- [x] [Review][Defer] No Enter-to-save / blur-to-commit on rename input — only Escape + explicit Save/Cancel resolve the edit; Enter does nothing, click-away leaves uncommitted text. UX enhancement, not required by AC1–AC3. [`hifimule-ui/src/components/PlaylistCurationView.ts`:354] — deferred, UX enhancement out of AC scope
- [x] [Review][Defer] Re-render race during rename — a concurrent `render()` (e.g. a track-removal completing in another panel) rebuilds `innerHTML` and re-reads `this.playlistName`, wiping unsaved rename text and re-creating an open delete dialog. Low likelihood (requires editing the name while another async op completes). [`hifimule-ui/src/components/PlaylistCurationView.ts`:127] — deferred, edge timing low likelihood
