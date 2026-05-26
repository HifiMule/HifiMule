# Sprint Change Proposal — Sync Quality-Awareness, Missing File Recovery & Force Sync

**Project:** HifiMule  
**Date:** 2026-05-26  
**Workflow:** bmad-correct-course  
**Prepared for:** Alexis  
**Mode:** Batch

---

## 1. Issue Summary

Three related improvements to the sync engine were requested:

1. **Quality-aware re-sync:** The manifest stores which files have been synced but not *at what quality*. If a file on the server is later re-encoded at a higher bitrate (e.g. FLAC re-ripped at a higher sample rate, or an MP3 upgraded from 192 to 320 kbps), HifiMule has no way to detect the quality change and will leave the lower-quality copy on the device.

2. **Missing file recovery:** If a user manually deletes a synced file directly on the device (without using HifiMule), the manifest still records that file as present. The next sync computes `unchanged` for that item and skips it — leaving the device in a broken state. The `device_file_exists` helper already exists in `sync.rs` and is used post-write, but it is never applied to items the delta engine marks as already-present.

3. **Force sync mode:** There is currently no way to trigger a full overwrite of all files on the device — useful when a profile changes, when files are suspected to be corrupt, or when a manual device operation has left things in an unknown state. The sync button offers no variant for "nuke and re-sync."

These are additive hardening changes to the existing sync engine (Epic 4). No previously completed story needs to be reverted or re-opened. No new epic is required.

---

## 2. Impact Analysis

### Checklist Summary

| Item | Status | Notes |
|------|--------|-------|
| Triggering story | [N/A] | No active triggering story — proactive quality hardening request |
| Core problem defined | [x] | Three clearly scoped gaps in sync fidelity |
| Evidence in code | [x] | `SyncedItem` lacks bitrate/encoding fields; `calculate_delta` has no existence check; no `force` param in RPC |
| Epic impact | [x] | Epic 4 only — no other epics affected |
| Story impact | [x] | Existing Stories 4.1 and 4.5 need amendments; two new micro-stories added (4.11, 4.12) |
| PRD conflict | [x] | FR12 ("differential sync based on manifest") is broadened by quality-awareness — minor PRD amendment |
| Architecture conflict | [x] | Manifest schema change (backward-compatible), RPC API addition (`force` param), UI split-button addition |
| UX conflict | [x] | UX spec update needed for force-sync split button pattern |
| Recommended path | [x] | Direct adjustment — add stories, amend existing ones |

### Epic Impact

Epic 4 ("The Sync Engine & Self-Healing Core") is the only affected epic. The three changes slot cleanly as amendments to:

- **Story 4.1** (Differential Sync Algorithm): quality-awareness and missing-file check both extend the delta calculation contract.
- **Story 4.5** (Start Sync UI): force sync mode requires both an RPC extension and a UI control change.

Two thin stories are proposed to formalize the implementation surface cleanly.

### Story Impact

**Stories requiring amendments:**
- Story 4.1: acceptance criteria must cover quality-upgrade detection and missing-file recovery as part of the delta engine.
- Story 4.5: acceptance criteria must cover the `force` RPC param and the UI split-button control.

**New stories:**
- Story 4.11: Quality-Aware Re-Sync and Missing File Recovery
- Story 4.12: Force Sync Mode

### Artifact Conflicts

**PRD:**
- FR12 currently says "perform a differential sync based on the local manifest." This needs a one-line extension: "…and re-sync existing files when the server reports a quality improvement."

**Architecture:**
- `DeviceManifest.SyncedItem` schema: adds two optional backward-compatible fields.
- `sync_calculate_delta` RPC: unchanged — delta calculation itself does not change. Missing-file recovery is a post-delta augmentation step, not a delta recalculation.
- `sync_execute` RPC: adds optional `force: bool` param.
- `execute_sync` / `execute_provider_sync`: both callers need the manifest cleared (or an equivalent full-add delta) when `force = true`.

**UX Design Specification:**
- Basket sidebar section needs a split-button (or a dropdown trigger on the sync button) documented.

---

## 3. Detailed Change Proposals

### Change A — Story 4.1 Amendment: Quality-Aware Delta Detection

**Affected files:**
- `hifimule-daemon/src/device/mod.rs` — `SyncedItem` struct
- `hifimule-daemon/src/sync.rs` — `DesiredItem`, `SyncAddItem`, `calculate_delta`
- Provider code that builds `DesiredItem` values (API → DesiredItem mapping)

---

#### A1 — `SyncedItem`: Add bitrate and container fields

**OLD (`SyncedItem` in `device/mod.rs` lines 12–33):**
```rust
pub struct SyncedItem {
    #[serde(rename = "providerItemId")]
    pub jellyfin_id: String,
    pub name: String,
    #[serde(default)]
    pub album: Option<String>,
    #[serde(default)]
    pub artist: Option<String>,
    pub local_path: String,
    pub size_bytes: u64,
    pub synced_at: String,
    #[serde(default)]
    pub original_name: Option<String>,
    #[serde(default)]
    pub etag: Option<String>,
    #[serde(default)]
    pub provider_album_id: Option<String>,
    #[serde(default)]
    pub provider_content_type: Option<String>,
    #[serde(default)]
    pub provider_suffix: Option<String>,
}
```

**NEW:**
```rust
pub struct SyncedItem {
    #[serde(rename = "providerItemId")]
    pub jellyfin_id: String,
    pub name: String,
    #[serde(default)]
    pub album: Option<String>,
    #[serde(default)]
    pub artist: Option<String>,
    pub local_path: String,
    pub size_bytes: u64,
    pub synced_at: String,
    #[serde(default)]
    pub original_name: Option<String>,
    #[serde(default)]
    pub etag: Option<String>,
    #[serde(default)]
    pub provider_album_id: Option<String>,
    #[serde(default)]
    pub provider_content_type: Option<String>,
    #[serde(default)]
    pub provider_suffix: Option<String>,
    /// Source bitrate at sync time in bps, for quality upgrade detection.
    #[serde(default)]
    pub original_bitrate: Option<u32>,
    /// Source container/codec at sync time (e.g. "flac", "mp3"), for quality upgrade detection.
    #[serde(default)]
    pub original_container: Option<String>,
}
```

**Rationale:** Both fields use `#[serde(default)]` so old manifests without these fields deserialize cleanly. `original_bitrate` stores the server-reported bitrate in **bps** (matching Jellyfin's `Bitrate` field convention). `original_container` stores the lowercase container string from `provider_suffix` at sync time.

---

#### A2 — `DesiredItem`: Add bitrate field

**OLD (`DesiredItem` in `sync.rs` lines 52–62):**
```rust
pub struct DesiredItem {
    pub jellyfin_id: String,
    pub name: String,
    pub album: Option<String>,
    pub artist: Option<String>,
    pub size_bytes: u64,
    pub etag: Option<String>,
    pub provider_album_id: Option<String>,
    pub provider_content_type: Option<String>,
    pub provider_suffix: Option<String>,
}
```

**NEW:**
```rust
pub struct DesiredItem {
    pub jellyfin_id: String,
    pub name: String,
    pub album: Option<String>,
    pub artist: Option<String>,
    pub size_bytes: u64,
    pub etag: Option<String>,
    pub provider_album_id: Option<String>,
    pub provider_content_type: Option<String>,
    pub provider_suffix: Option<String>,
    /// Current server-side bitrate in bps. Used to detect quality upgrades since last sync.
    pub original_bitrate: Option<u32>,
}
```

**Rationale:** The provider code populates this from `JellyfinItem.bitrate` (already fetched by `api.rs`). Subsonic providers can populate it from the `bitRate` field in `song` responses.

---

#### A3 — `SyncAddItem`: Propagate bitrate field

**OLD (`SyncAddItem`):**
```rust
pub struct SyncAddItem {
    pub jellyfin_id: String,
    pub name: String,
    pub album: Option<String>,
    pub artist: Option<String>,
    pub size_bytes: u64,
    pub etag: Option<String>,
    pub provider_album_id: Option<String>,
    pub provider_content_type: Option<String>,
    pub provider_suffix: Option<String>,
}
```

**NEW:**
```rust
pub struct SyncAddItem {
    pub jellyfin_id: String,
    pub name: String,
    pub album: Option<String>,
    pub artist: Option<String>,
    pub size_bytes: u64,
    pub etag: Option<String>,
    pub provider_album_id: Option<String>,
    pub provider_content_type: Option<String>,
    pub provider_suffix: Option<String>,
    pub original_bitrate: Option<u32>,
}
```

---

#### A4 — `calculate_delta`: Detect quality upgrades

**Section to modify (inside `calculate_delta`, around line 2651):**

The existing filter that builds `current_ids` skips items that the user wants (`desired`) but are already in the manifest. This is where quality detection must be inserted.

**OLD logic (conceptual):**
```
An item is "current" (unchanged) if its jellyfin_id is in the manifest and the profile isn't dirty.
```

**NEW logic:**
```
An item is "current" (unchanged) if:
  - its jellyfin_id is in the manifest, AND
  - the profile isn't dirty, AND
  - the desired bitrate is NOT higher than the synced bitrate
    (i.e. if desired.original_bitrate > synced.original_bitrate, exclude from current_ids → becomes an add)
```

Specifically, in `calculate_delta`, the filter producing `current_ids` should be extended:

```rust
let current_ids: HashSet<&str> = manifest
    .synced_items
    .iter()
    .filter(|i| {
        let desired = desired_items.iter().find(|d| d.jellyfin_id == i.jellyfin_id);
        let Some(desired) = desired else { return false; };

        let outside_music_folder = !device_path_in_or_equal(&i.local_path, &music_folder);
        if (!profile_dirty || !desired_ids.contains(i.jellyfin_id.as_str()))
            && !outside_music_folder
        {
            // Quality upgrade check: if server now reports a higher bitrate, re-sync.
            // If the manifest entry has no bitrate recorded (old manifest), also re-sync
            // once so the field gets populated — prevents being stuck at unknown quality.
            let quality_upgrade = match (desired.original_bitrate, i.original_bitrate) {
                (Some(server), Some(local)) => server > local,
                (Some(_), None) => true, // old entry — re-sync to populate bitrate
                _ => false,
            };
            !quality_upgrade
        } else {
            false
        }
    })
    .map(|i| i.jellyfin_id.as_str())
    .collect();
```

Items that pass the quality-upgrade check are excluded from `current_ids` and therefore land in `adds` — the sync engine will overwrite them.

After the write succeeds, `SyncedItem` is created with `original_bitrate` and `original_container` populated from `add_item.original_bitrate` and `add_item.provider_suffix` respectively.

---

#### A5 — `SyncedItem` construction after write

In `execute_sync` / `execute_provider_sync` at the point where `synced_items.push(...)` is called (around line 1567), add the two new fields:

```rust
synced_items.push(crate::device::SyncedItem {
    jellyfin_id: add_item.jellyfin_id.clone(),
    name: add_item.name.clone(),
    album: add_item.album.clone(),
    artist: add_item.artist.clone(),
    local_path: rel_path.clone(),
    size_bytes: add_item.size_bytes,
    synced_at,
    original_name: construction.original_name,
    etag: add_item.etag.clone(),
    provider_album_id: add_item.provider_album_id.clone(),
    provider_content_type: add_item.provider_content_type.clone(),
    provider_suffix: add_item.provider_suffix.clone(),
    original_bitrate: add_item.original_bitrate,              // NEW
    original_container: add_item.provider_suffix.clone(),     // NEW: use suffix as container
});
```

---

### Change B — Story 4.11 (New): Missing File Recovery in Unchanged Items

**Affected files:**
- `hifimule-daemon/src/sync.rs` — new async function + call in RPC handler
- `hifimule-daemon/src/rpc.rs` — post-delta augmentation step before `execute_sync`

---

#### B1 — New async function: `augment_delta_with_existence_check`

Add a new async function to `sync.rs`:

```rust
/// Moves "unchanged" manifest items that are missing on the device into the delta's adds list.
/// Called after calculate_delta when the device is accessible.
pub async fn augment_delta_with_existence_check(
    delta: &mut SyncDelta,
    desired_items: &[DesiredItem],
    manifest: &DeviceManifest,
    device_io: &dyn crate::device_io::DeviceIO,
) {
    let add_ids: HashSet<&str> = delta.adds.iter().map(|a| a.jellyfin_id.as_str()).collect();
    let delete_ids: HashSet<&str> = delta.deletes.iter().map(|d| d.jellyfin_id.as_str()).collect();
    let id_change_ids: HashSet<&str> = delta.id_changes.iter()
        .map(|c| c.new_jellyfin_id.as_str()).collect();

    for item in &manifest.synced_items {
        // Skip items already in the delta
        if add_ids.contains(item.jellyfin_id.as_str())
            || delete_ids.contains(item.jellyfin_id.as_str())
            || id_change_ids.contains(item.new_jellyfin_id.as_str())
        {
            continue;
        }
        // Only check items still desired
        let Some(desired) = desired_items.iter().find(|d| d.jellyfin_id == item.jellyfin_id) else {
            continue;
        };
        // If file is missing, promote to add
        if !device_file_exists(device_io, &item.local_path).await {
            delta.adds.push(SyncAddItem {
                jellyfin_id: desired.jellyfin_id.clone(),
                name: desired.name.clone(),
                album: desired.album.clone(),
                artist: desired.artist.clone(),
                size_bytes: desired.size_bytes,
                etag: desired.etag.clone(),
                provider_album_id: desired.provider_album_id.clone(),
                provider_content_type: desired.provider_content_type.clone(),
                provider_suffix: desired.provider_suffix.clone(),
                original_bitrate: desired.original_bitrate,
            });
            if delta.unchanged > 0 {
                delta.unchanged -= 1;
            }
        }
    }
}
```

#### B2 — Call site in `handle_sync_calculate_delta` (RPC handler)

In `rpc.rs`, `handle_sync_calculate_delta` (line ~2370), after `calculate_delta` returns, call `augment_delta_with_existence_check` before returning the delta to the UI:

```rust
// After: let mut delta = calculate_delta(&desired_items, &manifest);

if let Some((_, device_io)) = state.device_manager.get_manifest_and_io().await {
    crate::sync::augment_delta_with_existence_check(
        &mut delta,
        &desired_items,
        &manifest,
        device_io.as_ref(),
    ).await;
}
```

This means the UI's "Proposed Changes" preview (which calls `sync_calculate_delta`) will correctly show missing files as items to add. The actual sync execution then picks up the augmented delta, so no second check is needed during `execute_sync`.

**Note:** `device_file_exists` does a `list_files` per directory. For large manifests (hundreds of tracks in dozens of albums), this could be slow. If performance is a concern, the function can be gated behind a manifest flag or a new optional RPC param `checkExistence: bool` (default false for preview, true for pre-sync calculation).

---

### Change C — Story 4.12 (New): Force Sync Mode

**Affected files:**
- `hifimule-daemon/src/rpc.rs` — `handle_sync_execute`, read `force` param
- `hifimule-daemon/src/sync.rs` — `execute_sync` / `execute_provider_sync` or delta pre-processing
- `hifimule-ui/src/components/BasketSidebar.ts` — split-button UI

---

#### C1 — RPC: `sync_execute` accepts optional `force` param

In `handle_sync_execute` (line ~2866), after extracting `delta` and `confirmDestructiveCleanup`, extract `force`:

```rust
let force_sync = params
    .get("force")
    .and_then(Value::as_bool)
    .unwrap_or(false);
```

When `force_sync = true`, rebuild the delta so all currently-manifest items become adds:

```rust
let delta = if force_sync {
    // Treat all desired items as adds — clear the slate
    let manifest = state.device_manager.get_current_device().await.ok_or(...)?;
    let all_adds: Vec<SyncAddItem> = desired_items
        .iter()
        .map(|i| SyncAddItem {
            jellyfin_id: i.jellyfin_id.clone(),
            name: i.name.clone(),
            ...
        })
        .collect();
    // All manifest items that are in the desired set become deletes (their files will be
    // overwritten; the delete step handles the old copy before the new write).
    let all_deletes: Vec<SyncDeleteItem> = manifest
        .synced_items
        .iter()
        .filter(|s| desired_ids.contains(s.jellyfin_id.as_str()))
        .map(|s| SyncDeleteItem { ... })
        .collect();
    SyncDelta { adds: all_adds, deletes: all_deletes, id_changes: vec![], unchanged: 0, playlists: delta.playlists }
} else {
    delta
};
```

*Alternative (simpler):* Clear `manifest.synced_items` in memory before computing the delta, then pass the empty-manifest delta through the existing path. Either approach is valid; the force-rebuilt delta approach is preferred as it doesn't mutate the persisted manifest before sync starts.

---

#### C2 — UI: Split-button on the Sync button

The current sync button in `BasketSidebar.ts` renders as `<sl-button id="start-sync-btn" variant="primary">`. Replace with a Shoelace split-button:

**OLD (line ~943):**
```html
<sl-button id="start-sync-btn" variant="primary" style="width: 100%;">
  ${t('basket.startSync')}
</sl-button>
```

**NEW:**
```html
<sl-button-group style="width: 100%;">
  <sl-button id="start-sync-btn" variant="primary" style="flex: 1;">
    ${t('basket.startSync')}
  </sl-button>
  <sl-dropdown id="sync-mode-dropdown" placement="bottom-end">
    <sl-button slot="trigger" variant="primary" caret></sl-button>
    <sl-menu>
      <sl-menu-item id="force-sync-item">${t('basket.forceSync')}</sl-menu-item>
    </sl-menu>
  </sl-dropdown>
</sl-button-group>
```

In the event handler that calls `rpcCall('sync_execute', ...)` (line ~1064), add `force: boolean` state that defaults to `false` and is set to `true` when the user selects "Force Sync" from the dropdown:

```typescript
let forceSyncMode = false;

// In menu item click handler:
shadowRoot.getElementById('force-sync-item')?.addEventListener('sl-select', () => {
  forceSyncMode = true;
  shadowRoot.getElementById('start-sync-btn')?.click();
});

// After the click handler resets, reset force mode:
// In the sync execution flow, pass:
const result = await rpcCall('sync_execute', { delta, confirmDestructiveCleanup, force: forceSyncMode });
forceSyncMode = false; // reset after use
```

**i18n keys required:**
```json
"basket.forceSync": "Force Sync"        // en
"basket.forceSync": "Forcer la synchro" // fr (if fr locale is active)
```

---

## 4. PRD Amendments

**FR12 — current:**
> Perform a differential sync based on the local manifest.

**FR12 — proposed:**
> Perform a differential sync based on the local manifest, re-syncing files when the server reports a quality improvement (higher bitrate) and recovering files that have been manually removed from the device.

**New FR37 (Force Sync):**
> Provide a "Force Sync" mode that re-downloads and overwrites all currently-synced files on the device, accessible via a dropdown on the Sync button.

---

## 5. Architecture Amendments

**Manifest schema (`.hifimule.json`):**
```json
{
  "synced_items": [
    {
      "providerItemId": "...",
      "localPath": "...",
      "originalBitrate": 1411200,
      "originalContainer": "flac"
    }
  ]
}
```
Both fields are `Option<>` with `#[serde(default)]` — existing manifests without these fields are fully backward-compatible.

**RPC API additions:**
- `sync_execute` params: add optional `force: boolean` (default `false`)
- `sync_calculate_delta` response: `unchanged` count may decrease when missing files are promoted to adds

---

## 6. Story Definitions

### Story 4.11: Quality-Aware Re-Sync and Missing File Recovery

*As a user who upgrades source audio quality on the server or who manually reorganizes their device,  
I want HifiMule to detect quality improvements and re-copy missing files on the next sync,  
So that the device always has the best available copy of each track.*

**Acceptance Criteria:**

**Given** a track already in the manifest with `originalBitrate: 192000` (192 kbps)  
**When** the server now reports `bitrate: 320000` for that same track  
**Then** `calculate_delta` treats it as an add (not unchanged)  
**And** the sync engine downloads and overwrites the lower-quality copy  
**And** the manifest is updated with `originalBitrate: 320000`

**Given** a track in the manifest with a `localPath` that no longer exists on the device  
**When** `sync_calculate_delta` is called  
**Then** the delta includes that track in `adds` (not `unchanged`)  
**And** the sync engine re-downloads and writes the file

**Given** an old manifest entry without `originalBitrate`  
**When** the server reports a bitrate for that track and a sync runs  
**Then** the item is treated as an add (re-synced once to populate bitrate metadata)  
**And** after that sync the manifest records `originalBitrate` and subsequent syncs are differential again

**Technical Notes:**
- `original_bitrate: Option<u32>` added to `SyncedItem`, `DesiredItem`, `SyncAddItem` in `sync.rs` and `device/mod.rs`
- `calculate_delta` extended to exclude quality-upgraded items from `current_ids`
- New `augment_delta_with_existence_check` async fn in `sync.rs`; called from `handle_sync_calculate_delta` in `rpc.rs`
- `SyncedItem` construction in `execute_sync` and `execute_provider_sync` populated with `original_bitrate` and `original_container`
- Provider-to-DesiredItem mapping must pass `JellyfinItem.bitrate` (Jellyfin) / `song.bitRate * 1000` (Subsonic, which reports kbps)

---

### Story 4.12: Force Sync Mode

*As a user who suspects device corruption, has changed a transcoding profile, or has manually modified files on the device,  
I want a "Force Sync" option that re-downloads and overwrites every currently-synced file,  
So that I can guarantee the device content matches the server selection exactly.*

**Acceptance Criteria:**

**Given** the Sync Basket is populated  
**When** I open the dropdown next to the Sync button and select "Force Sync"  
**Then** the UI sends `sync_execute` with `force: true`  
**And** all items currently in the manifest are treated as adds (full re-download)  
**And** the sync runs as normal (streaming, verification, per-file manifest update)

**Given** `force: true` is sent  
**When** the daemon processes the request  
**Then** the delta's `unchanged` count is 0 regardless of existing manifest state  
**And** no extra confirmation is required beyond the existing `confirmDestructiveCleanup` gate

**Given** Force Sync completes  
**When** the manifest is finalized  
**Then** all `synced_items` reflect the updated `originalBitrate` values

**Technical Notes:**
- `handle_sync_execute` in `rpc.rs` reads optional `force: boolean` param
- When `force = true`, rebuild the delta: all desired items become adds; existing manifest items (that are in the desired set) become deletes
- UI: `<sl-button-group>` wrapping the existing sync button + a `<sl-dropdown>` with "Force Sync" option
- i18n key `basket.forceSync` required in all locale files

---

## 7. Implementation Handoff

**Scope classification:** Moderate

**Implementation order (dependency-based):**

1. **Story 4.11 — daemon (Rust):** `SyncedItem` schema → `DesiredItem` → `SyncAddItem` → `calculate_delta` extension → `augment_delta_with_existence_check` → `execute_sync` write path → provider mapping
2. **Story 4.11 — RPC/API:** Call `augment_delta_with_existence_check` from `handle_sync_calculate_delta`
3. **Story 4.12 — daemon (Rust):** `handle_sync_execute` force-delta rebuild
4. **Story 4.12 — UI:** Split-button, i18n keys, `force` param in `rpcCall`

**Success criteria:**

- [ ] Manifests from pre-4.11 (no `originalBitrate`) deserialize without error
- [ ] A track upgraded on server from 192kbps to 320kbps appears in the delta's `adds` list on next sync
- [ ] A manually-deleted file on the device appears in the delta's `adds` list when the UI requests the delta
- [ ] Force Sync results in a delta where `unchanged === 0` regardless of how many items were previously synced
- [ ] Force Sync UI control is accessible via keyboard and meets WCAG 2.1 AA (Shoelace components satisfy this by default)
- [ ] All existing `sync.rs` tests pass without modification

**Routing:** Developer agent (direct implementation — changes are well-bounded, no PM/Architect escalation required)

---

Correct Course workflow complete, Alexis!
