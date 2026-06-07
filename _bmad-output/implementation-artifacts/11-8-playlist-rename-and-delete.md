---
baseline_commit: 4b15ea1
---

# Story 11.8: Playlist Rename and Delete — Curation View Header

Status: backlog

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

### Task 1: Extend MediaProvider trait with `rename_playlist`

In `hifimule-daemon/src/providers/mod.rs`, add to the `MediaProvider` trait alongside the existing write methods:

```rust
async fn rename_playlist(&self, playlist_id: &str, new_name: &str) -> Result<(), ProviderError>;
```

Add a default `NotSupported` implementation consistent with the existing pattern for providers that don't support playlist write.

### Task 2: JellyfinProvider — implement `rename_playlist`

In `hifimule-daemon/src/providers/jellyfin.rs`:

2-step operation:
1. `GET /Users/{user_id}/Items/{playlist_id}` — fetch the current item JSON
2. Deserialize into the item DTO, update the `Name` field to `new_name`, then `POST /Items/{playlist_id}` with the full updated body

```rust
async fn rename_playlist(&self, playlist_id: &str, new_name: &str) -> Result<(), ProviderError> {
    let item = self.get_item(playlist_id).await?;
    let updated = ItemUpdateDto { name: new_name.to_string(), ..item };
    self.client
        .post(format!("{}/Items/{}", self.base_url, playlist_id))
        .json(&updated)
        .send()
        .await?
        .error_for_status()?;
    Ok(())
}
```

### Task 3: SubsonicProvider — implement `rename_playlist`

In `hifimule-daemon/src/providers/subsonic.rs`:

Single-step using the existing `updatePlaylist.view` endpoint:

```rust
async fn rename_playlist(&self, playlist_id: &str, new_name: &str) -> Result<(), ProviderError> {
    self.get(
        "updatePlaylist",
        &[("playlistId", playlist_id), ("name", new_name)],
    )
    .await?;
    Ok(())
}
```

Apply `sanitize_subsonic_url()` for logging (same rule as other playlist write methods in this provider).

### Task 4: Daemon RPC — `playlist.rename`

In the playlist RPC handler (alongside existing `playlist.create`, `playlist.addTracks`, `playlist.removeTracks`, `playlist.delete`):

```typescript
// Command: playlist.rename
// Payload: { playlistId: string, name: string }
// Returns: void
```

Rust side: parse payload, call `provider.rename_playlist(&playlist_id, &name).await`, return `Ok(())` or propagate `ProviderError`.

### Task 5: i18n keys

In `hifimule-i18n/catalog.json`, add to the `"en"`, `"fr"`, and `"es"` blocks (after existing `playlist.curation.*` keys):

```json
"playlist.curation.rename_save": "Save name",
"playlist.curation.rename_cancel": "Cancel rename",
"playlist.curation.delete_title": "Delete playlist",
"playlist.curation.delete_body": "Delete \"{{name}}\"? This cannot be undone.",
"playlist.curation.delete_confirm": "Delete",
"playlist.curation.delete_cancel_btn": "Cancel"
```

6 keys × 3 languages = 18 additions.

### Task 6: Rename — inline name editing in `PlaylistCurationView.ts`

Add state:
```typescript
private isRenamingPlaylist = false;
```

In `render()`, replace the static playlist name title with:
```typescript
${this.isRenamingPlaylist
    ? `<sl-input
           id="playlist-rename-input"
           value="${escapeAttr(this.playlistName)}"
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
           style="cursor: pointer; border-bottom: 1px dashed var(--sl-color-neutral-400);"
           title="Click to rename"
       >${escapeHtml(this.playlistName)}</span>`
}
```

In `bindEvents()`, add:
```typescript
this.container.querySelector('.playlist-name-title')?.addEventListener('click', () => {
    this.isRenamingPlaylist = true;
    this.render();
    (this.container.querySelector('#playlist-rename-input') as any)?.focus();
});

this.container.querySelector('.playlist-rename-save')?.addEventListener('click', async () => {
    const input = this.container.querySelector('#playlist-rename-input') as any;
    const newName = input?.value?.trim();
    if (newName && newName !== this.playlistName) {
        await invoke('playlist.rename', { playlistId: this.playlistId, name: newName });
        this.playlistName = newName;
    }
    this.isRenamingPlaylist = false;
    this.render();
});

this.container.querySelector('.playlist-rename-cancel')?.addEventListener('click', () => {
    this.isRenamingPlaylist = false;
    this.render();
});

this.container.querySelector('#playlist-rename-input')?.addEventListener('keydown', (e) => {
    if ((e as KeyboardEvent).key === 'Escape') {
        this.isRenamingPlaylist = false;
        this.render();
    }
});
```

### Task 7: Delete — confirmation dialog and navigation in `PlaylistCurationView.ts`

Add delete dialog markup to `render()` (alongside existing dialogs):
```typescript
<sl-dialog id="playlist-delete-dialog" label="${t('playlist.curation.delete_title')}">
    <p>${t('playlist.curation.delete_body').replace('{{name}}', escapeHtml(this.playlistName))}</p>
    <sl-button slot="footer" class="playlist-delete-cancel" variant="default">
        ${t('playlist.curation.delete_cancel_btn')}
    </sl-button>
    <sl-button slot="footer" class="playlist-delete-confirm" variant="danger">
        ${t('playlist.curation.delete_confirm')}
    </sl-button>
</sl-dialog>
```

Add delete icon-button to header (visible only when `this.supportsPlaylistWrite`):
```typescript
${this.supportsPlaylistWrite
    ? `<sl-icon-button
           class="playlist-delete-btn"
           name="trash"
           label="${t('playlist.curation.delete_title')}"
           style="color: var(--sl-color-danger-600);"
       ></sl-icon-button>`
    : ''
}
```

In `bindEvents()`:
```typescript
this.container.querySelector('.playlist-delete-btn')?.addEventListener('click', () => {
    (this.container.querySelector('#playlist-delete-dialog') as any)?.show();
});

this.container.querySelector('.playlist-delete-cancel')?.addEventListener('click', () => {
    (this.container.querySelector('#playlist-delete-dialog') as any)?.hide();
});

this.container.querySelector('.playlist-delete-confirm')?.addEventListener('click', async () => {
    await invoke('playlist.delete', { playlistId: this.playlistId });
    this.navigateBack(); // existing back-navigation used by the ← button
});
```

## Key Notes

- `doRemove`, `fetchPlaylist`, and `navigateBack` are existing methods — no changes needed.
- The Subsonic `updatePlaylist.view` endpoint has been used for track add/remove (Story 11.3); `name` is an additional optional param on the same endpoint — no new auth or URL patterns.
- The Jellyfin 2-step pattern (GET then POST `/Items/{id}`) mirrors the existing 2-step `remove_from_playlist` (GET then DELETE) from Story 11.2.
- TypeScript must compile with zero errors (`rtk tsc`) before marking done.
