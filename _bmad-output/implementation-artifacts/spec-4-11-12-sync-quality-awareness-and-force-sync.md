---
title: 'Stories 4.11 + 4.12: Quality-Aware Re-Sync, Missing File Recovery, and Force Sync'
type: 'feature'
created: '2026-05-26'
status: 'done'
baseline_commit: '8ca303a'
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** The sync engine has three gaps in fidelity: (1) it cannot detect when a server file has been updated with higher quality (bitrate) and will leave stale lower-quality copies on the device; (2) if the user manually deletes a synced file, the manifest still says it's present and the next sync skips it; (3) there is no way to force a full re-download of all synced files.

**Approach:** Add `originalBitrate` / `originalContainer` to the manifest's synced items and to the delta pipeline, so quality upgrades are detected at delta-calculation time. Add a post-delta existence check that promotes missing files from "unchanged" to "adds". Add a `force` param to `sync_execute` that rebuilds the delta to re-download everything. Surface force sync as a split-button dropdown in the UI.

## Boundaries & Constraints

**Always:**
- All new fields on `SyncedItem`, `DesiredItem`, `SyncAddItem` use `Option<…>` with `#[serde(default)]` — old manifests must deserialize without error.
- Quality upgrade check: `(Some(server), Some(local)) => server > local` triggers re-sync; `(Some(_), None)` also triggers re-sync (old manifest, no data recorded); `(None, _)` skips the check (no info to compare).
- Existence check runs only during `sync_calculate_delta`, not inside `execute_sync`.
- `force = true` must bypass the `destructiveCleanupCount` gate (no extra confirmation required beyond the existing threshold check which still applies).
- `forceSyncMode` in the UI resets to `false` immediately after the `sync_execute` call — one-shot only.

**Ask First:** None — all design decisions are fully specified.

**Never:**
- Do not add `original_bitrate` to `SyncIdChangeItem` — ID changes are metadata-only (no re-download); set `original_bitrate: None` in the resulting `SyncedItem` so the next sync will re-evaluate quality. This is intentional: it triggers a re-download after an id-change if the server has a bitrate, which is correct.
- Do not run `augment_delta_with_existence_check` inside auto-sync paths (`run_auto_sync` in `main.rs`) — it's a UI-preview concern only.
- Do not change the Subsonic `bitrate_kbps` (kbps) to `bps` anywhere outside `provider_song_to_desired_item`. Subsonic internals use kbps; `DesiredItem.original_bitrate` is always bps.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Quality upgrade | Manifest has `originalBitrate: 192000`; server reports `bitrate: 320000` | Track lands in `delta.adds` | N/A |
| Old manifest entry | `originalBitrate` absent; server has `bitrate: 320000` | Track lands in `delta.adds` (triggers one-time re-sync) | N/A |
| No server bitrate | Both `None` | Track stays in `unchanged` — no spurious re-sync | N/A |
| File manually deleted | `manifest.synced_items` has entry; file absent on device | `augment_delta_with_existence_check` promotes it to `adds` | `device_file_exists` error → skip promotion, leave unchanged |
| Force sync | `force: true`; manifest has 20 synced items, delta has 2 adds | All 20 manifest items become deletes; all 20 + 2 new = 22 become adds; `unchanged = 0` | N/A |

</frozen-after-approval>

## Code Map

- `hifimule-daemon/src/device/mod.rs:12` — `SyncedItem` struct; add `original_bitrate`, `original_container`
- `hifimule-daemon/src/sync.rs:52` — `DesiredItem` struct; add `original_bitrate`
- `hifimule-daemon/src/sync.rs:67` — `SyncAddItem` struct; add `original_bitrate`
- `hifimule-daemon/src/sync.rs:2651` — `calculate_delta`; extend quality check in `current_ids` filter + propagate to `SyncAddItem` construction at line ~2678
- `hifimule-daemon/src/sync.rs:1567` — `execute_sync` adds write path: populate `original_bitrate` + `original_container`
- `hifimule-daemon/src/sync.rs:1749` — `execute_sync` id_changes: set both new fields `None`
- `hifimule-daemon/src/sync.rs:2137` — `execute_provider_sync` adds write path: same as line 1567
- `hifimule-daemon/src/sync.rs:2279` — `execute_provider_sync` id_changes: set both new fields `None`
- `hifimule-daemon/src/sync.rs:2398` — `device_file_exists` (existing async fn) — reused by new helper
- `hifimule-daemon/src/rpc.rs:1482` — `provider_song_to_desired_item`; add `original_bitrate: song.bitrate_kbps.map(|k| k * 1000)`
- `hifimule-daemon/src/rpc.rs:1814` — `jellyfin_item_to_desired_item`; add `original_bitrate: item.bitrate`
- `hifimule-daemon/src/rpc.rs:2429` — inline Jellyfin item→DesiredItem in `handle_sync_calculate_delta`; same as above
- `hifimule-daemon/src/rpc.rs:2720` — auto-fill DesiredItem; add `original_bitrate: None`
- `hifimule-daemon/src/rpc.rs:1767` — provider delta return; call `augment_delta_with_existence_check` before returning
- `hifimule-daemon/src/rpc.rs:2745` — Jellyfin delta return; same
- `hifimule-daemon/src/rpc.rs:2866` — `handle_sync_execute`; read `force` param; rebuild delta when true
- `hifimule-ui/src/components/BasketSidebar.ts:943` — active sync button render; replace with split-button group
- `hifimule-ui/src/components/BasketSidebar.ts:976` — event listener for `#start-sync-btn`; also add listener for `#force-sync-item`
- `hifimule-ui/src/components/BasketSidebar.ts:1064` — `sync_execute` rpcCall; add `force: forceSyncMode`
- `hifimule-i18n/catalog.json:75,224,373` — add `basket.actions.force_sync` to `en`, `fr`, `es` sections

## Tasks & Acceptance

**Execution:**

- [x] `hifimule-daemon/src/device/mod.rs` -- Add `original_bitrate: Option<u32>` and `original_container: Option<String>` (both `#[serde(default)]`) to `SyncedItem` -- backbone for all quality tracking
- [x] `hifimule-daemon/src/sync.rs` -- Add `original_bitrate: Option<u32>` to `DesiredItem` and `SyncAddItem` -- allows quality data to flow through the delta pipeline
- [x] `hifimule-daemon/src/sync.rs` -- Extend `calculate_delta`: in the `current_ids` filter, add quality-upgrade check (see I/O matrix); in the `SyncAddItem` construction at line ~2678 propagate `original_bitrate: i.original_bitrate` -- detects quality upgrades at delta time
- [x] `hifimule-daemon/src/sync.rs` -- Add `pub async fn augment_delta_with_existence_check(delta: &mut SyncDelta, desired_items: &[DesiredItem], manifest: &DeviceManifest, device_io: &dyn DeviceIO)` near line 2398 -- promotes missing unchanged files to adds
- [x] `hifimule-daemon/src/sync.rs` -- In both `execute_sync` (line ~1567) and `execute_provider_sync` (line ~2137) adds write paths: set `original_bitrate: add_item.original_bitrate` and `original_container: add_item.provider_suffix.clone()` on the pushed `SyncedItem` -- records quality at sync time
- [x] `hifimule-daemon/src/sync.rs` -- In id_changes `SyncedItem` pushes (lines ~1749, ~2279): set `original_bitrate: None, original_container: None` -- intentional; triggers quality re-check on next sync
- [x] `hifimule-daemon/src/sync.rs` -- Update all test-only `DesiredItem` / `SyncAddItem` constructions (lines ~3209, ~3332, ~4236, ~4295) to add `original_bitrate: None` -- compile fix
- [x] `hifimule-daemon/src/rpc.rs` -- `provider_song_to_desired_item`: add `original_bitrate: song.bitrate_kbps.map(|k| k * 1000)` -- Subsonic kbps→bps conversion
- [x] `hifimule-daemon/src/rpc.rs` -- `jellyfin_item_to_desired_item` + inline at line ~2429: add `original_bitrate: item.bitrate` -- Jellyfin already reports bps
- [x] `hifimule-daemon/src/rpc.rs` -- Auto-fill DesiredItem at line ~2720: add `original_bitrate: None` -- no quality data in auto-fill path
- [x] `hifimule-daemon/src/rpc.rs` -- After `calculate_delta` calls at lines ~1767 and ~2745: call `augment_delta_with_existence_check(&mut delta, &desired_items, &manifest, device_io.as_ref()).await` (get `device_io` via `state.device_manager.get_manifest_and_io().await`) -- wires the existence check into the preview delta
- [x] `hifimule-daemon/src/rpc.rs` -- `handle_sync_execute`: extract `force: bool = params["force"].as_bool().unwrap_or(false)`; when true, rebuild `delta`: all `manifest.synced_items` not in `delta.deletes` become both new `SyncAddItem` (from SyncedItem fields) and new `SyncDeleteItem` entries; clear `delta.id_changes`; set `delta.unchanged = 0` -- force re-download
- [x] `hifimule-ui/src/components/BasketSidebar.ts` -- Replace active sync button template (line ~943) with `<sl-button-group>` containing `<sl-button id="start-sync-btn">` + `<sl-dropdown id="sync-mode-dropdown"><sl-menu><sl-menu-item id="force-sync-item">${t('basket.actions.force_sync')}</sl-menu-item></sl-menu></sl-dropdown>` -- UI entry point for force sync
- [x] `hifimule-ui/src/components/BasketSidebar.ts` -- Add `private forceSyncMode = false` field; bind `#force-sync-item` click to set it `true` then call `this.handleStartSync()`; in `handleStartSync` pass `force: this.forceSyncMode` to `sync_execute` rpcCall and reset to `false` -- one-shot force flag
- [x] `hifimule-i18n/catalog.json` -- Add `"basket.actions.force_sync"` to `en` ("Force Sync"), `fr` ("Forcer la synchro"), `es` ("Forzar sincronización") sections -- i18n coverage

**Acceptance Criteria:**

- Given a manifest with a track at `originalBitrate: 192000` and the server now reports `bitrate: 320000`, when `sync_calculate_delta` runs, then that track is in `delta.adds` (not `unchanged`).
- Given a manifest entry with no `originalBitrate` field (old manifest) and the server reports a bitrate, when `sync_calculate_delta` runs, then that track is in `delta.adds`.
- Given a manifest entry without `originalBitrate` and the server also has no bitrate, when `sync_calculate_delta` runs, then that track stays `unchanged` (no spurious re-sync).
- Given a manifest entry whose file is absent on the device, when `sync_calculate_delta` runs, then that track is in `delta.adds`.
- Given a sync completes successfully, when the manifest is updated, then `originalBitrate` and `originalContainer` are populated from the downloaded item's data.
- Given existing manifests without these fields, when deserialized, then no error occurs and old entries are treated correctly.
- Given `sync_execute` with `force: true`, when the daemon processes it, then `delta.unchanged === 0` and every previously-synced item is re-downloaded.
- Given the user selects "Force Sync" from the split-button dropdown, when the sync runs, then `force: true` is sent to the daemon.

## Design Notes

**`augment_delta_with_existence_check` sketch:**
```rust
pub async fn augment_delta_with_existence_check(
    delta: &mut SyncDelta,
    desired_items: &[DesiredItem],
    manifest: &DeviceManifest,
    device_io: &dyn DeviceIO,
) {
    let already_in_delta: HashSet<&str> = delta.adds.iter().map(|a| a.jellyfin_id.as_str())
        .chain(delta.deletes.iter().map(|d| d.jellyfin_id.as_str()))
        .chain(delta.id_changes.iter().map(|c| c.new_jellyfin_id.as_str()))
        .collect();
    for item in &manifest.synced_items {
        if already_in_delta.contains(item.jellyfin_id.as_str()) { continue; }
        let Some(desired) = desired_items.iter().find(|d| d.jellyfin_id == item.jellyfin_id) else { continue; };
        if !device_file_exists(device_io, &item.local_path).await {
            delta.adds.push(SyncAddItem { /* from desired fields */ original_bitrate: desired.original_bitrate, .. });
            if delta.unchanged > 0 { delta.unchanged -= 1; }
        }
    }
}
```

**Force sync delta rebuild sketch (in `handle_sync_execute`):**
```rust
if force_sync {
    let delete_ids: HashSet<&str> = delta.deletes.iter().map(|d| d.jellyfin_id.as_str()).collect();
    for item in &manifest.synced_items {
        if delete_ids.contains(item.jellyfin_id.as_str()) { continue; }
        delta.adds.push(SyncAddItem { jellyfin_id: item.jellyfin_id.clone(), name: item.name.clone(),
            album: item.album.clone(), artist: item.artist.clone(), size_bytes: item.size_bytes,
            etag: item.etag.clone(), provider_album_id: item.provider_album_id.clone(),
            provider_content_type: item.provider_content_type.clone(),
            provider_suffix: item.provider_suffix.clone(), original_bitrate: None });
        delta.deletes.push(SyncDeleteItem { jellyfin_id: item.jellyfin_id.clone(),
            local_path: item.local_path.clone(), name: item.name.clone() });
    }
    delta.id_changes.clear();
    delta.unchanged = 0;
}
```

## Verification

**Commands:**
- `rtk cargo test -p hifimule-daemon` -- expected: all tests pass, no compilation errors
- `rtk cargo clippy -p hifimule-daemon` -- expected: no new warnings
- `rtk tsc --noEmit -p hifimule-ui/tsconfig.json` -- expected: no TypeScript errors

## Spec Change Log

