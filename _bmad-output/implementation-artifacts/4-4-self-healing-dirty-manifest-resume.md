# Story 4.4: Self-Healing "Dirty Manifest" Resume

Status: done

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a **System Admin (Alexis)**,
I want **the system to detect an interrupted sync and offer to resume from the last successful file**,
so that **I don't lose progress after an accidental unplug**.

## Acceptance Criteria

1. **Dirty Flag Detection**: When a device is reconnected after an interrupted sync, the engine MUST detect the `dirty: true` flag in the `.jellyfinsync.json` manifest. (AC: #1)

2. **Dirty State Exposure**: `get_daemon_state` MUST include a `dirtyManifest` boolean field. A new `sync_get_resume_state` RPC method MUST return `isDirty`, `pendingItemIds`, and `cleanedTmpFiles`. (AC: #2)

3. **Tmp File Cleanup**: When `sync_get_resume_state` is called and `isDirty` is `true`, the engine MUST scan `device_root/Music/` (recursively) and delete any `.tmp` files left by the interrupted write operations. The count of deleted files MUST be returned as `cleanedTmpFiles`. (AC: #3)

4. **Dirty Flag Set Before Sync**: Before any file operations begin, the daemon MUST mark the manifest as `dirty: true` and write the IDs of the items queued for download to `pending_item_ids`, then write the manifest to disk. An interruption during sync is therefore always detectable. (AC: #4)

5. **Per-File Manifest Updates**: After EACH successful file write (add), after EACH successful file delete, and after EACH ID-change manifest update, the daemon MUST immediately write the updated manifest to disk atomically. This enables true delta-based resume — the manifest on disk always reflects what has actually been completed. (AC: #5)

6. **Dirty Flag Cleared on Completion**: After all sync operations finish (regardless of file-level errors in `errors` vec), the daemon MUST set `dirty: false` and `pending_item_ids: []`, then write the manifest once more. (AC: #6)

7. **Resume Delta Accuracy**: After reconnecting with a dirty manifest, the UI re-submits `pendingItemIds` as the basket to `sync_calculate_delta`. Because the manifest was updated per-file (AC #5), `calculate_delta` produces a delta containing only the truly remaining items. (AC: #7 — emerges from AC #5 + #6; no new engine logic needed)

## Tasks / Subtasks

- [x] **T1: Add `dirty` and `pending_item_ids` fields to `DeviceManifest`** (AC: #1, #4, #6)
  - [x] T1.1: In `jellyfinsync-daemon/src/device/mod.rs`, add two fields to `DeviceManifest` (after `synced_items`):
    ```rust
    #[serde(default)]
    pub dirty: bool,
    #[serde(default)]
    pub pending_item_ids: Vec<String>,
    ```
    Both use `#[serde(default)]` — backward-compatible with existing manifests (missing fields deserialize as `false`/`[]`). `DeviceManifest` does NOT have `#[serde(rename_all)]`, so these serialize as `"dirty"` and `"pending_item_ids"` in JSON (snake_case, matching the existing `device_id`, `managed_paths` etc.).
  - [x] T1.2: Add `dirty: false, pending_item_ids: vec![],` to every `DeviceManifest { ... }` struct literal in the codebase (compilation fails otherwise):
    - `jellyfinsync-daemon/src/device/tests.rs`: `test_write_manifest_creates_files` (~line 196), `test_write_manifest_overwrites_existing` (two constructions, ~line 237 and ~line 246)
    - `jellyfinsync-daemon/src/sync.rs`: `empty_manifest()` test helper (~line 752)
    - `jellyfinsync-daemon/src/rpc.rs`: `test_rpc_sync_get_device_status_map_with_synced_items` (~line 1218), `test_rpc_sync_calculate_delta_partial_failure` (~line 1316)

- [x] **T2: Add `cleanup_tmp_files` async function** (AC: #3)
  - [x] T2.1: In `jellyfinsync-daemon/src/device/mod.rs`, add after `write_manifest`:
    ```rust
    /// Scans the managed zone (`device_root/Music/`) recursively for leftover `.tmp`
    /// files from interrupted writes and deletes them. Returns the count of deleted files.
    /// Non-fatal: individual deletion failures are silently skipped.
    pub async fn cleanup_tmp_files(device_root: &Path) -> Result<usize> {
        let music_path = device_root.join("Music");
        if tokio::fs::metadata(&music_path).await.is_err() {
            return Ok(0); // No Music directory — nothing to clean
        }
        let mut count = 0;
        let mut dirs_to_visit = vec![music_path];
        while let Some(dir) = dirs_to_visit.pop() {
            let mut entries = match tokio::fs::read_dir(&dir).await {
                Ok(e) => e,
                Err(_) => continue,
            };
            while let Some(entry) = entries.next_entry().await? {
                let path = entry.path();
                let file_type = match entry.file_type().await {
                    Ok(ft) => ft,
                    Err(_) => continue,
                };
                if file_type.is_dir() {
                    dirs_to_visit.push(path);
                } else if file_type.is_file() {
                    // Matches files like "01 - Track.flac.tmp" — extension() returns "tmp"
                    if path.extension().and_then(|e| e.to_str()) == Some("tmp") {
                        if tokio::fs::remove_file(&path).await.is_ok() {
                            count += 1;
                        }
                    }
                }
            }
        }
        Ok(count)
    }
    ```
    **Implementation note**: Uses an iterative stack (no async recursion) to avoid Rust's boxed-future requirement for recursive async functions. Targets only `Music/` because that is the hardcoded managed path used by `execute_sync`. The `.jellyfinsync.json.tmp` at device root is NOT in `Music/`, so it is unaffected.

- [x] **T3: Modify `execute_sync` for per-file manifest updates** (AC: #5)
  - [x] T3.1: Add `device_manager: Arc<crate::device::DeviceManager>` as the final parameter to `execute_sync` in `jellyfinsync-daemon/src/sync.rs`. New signature:
    ```rust
    pub async fn execute_sync(
        delta: &SyncDelta,
        device_path: &Path,
        jellyfin_client: &crate::api::JellyfinClient,
        jellyfin_url: &str,
        jellyfin_token: &str,
        jellyfin_user_id: &str,
        operation_manager: Arc<SyncOperationManager>,
        operation_id: String,
        device_manager: Arc<crate::device::DeviceManager>,  // NEW — for per-file manifest updates
    ) -> Result<(Vec<crate::device::SyncedItem>, Vec<SyncFileError>)>
    ```
  - [x] T3.2: At the very top of the `execute_sync` function body (before the `managed_path` line), capture an owned path for use in async closures:
    ```rust
    let device_path_buf = device_path.to_path_buf();
    ```
  - [x] T3.3: **Per-add manifest update** — In the `Ok(_)` branch of `write_result` (after the existing `synced_items.push(...)` call and after the `files_completed += 1` operation manager update), add:
    ```rust
    // Per-file manifest update for dirty-resume support (Story 4.4)
    // Per-file writes ensure manifest always reflects completed work for true delta resume.
    if let Some(mut manifest) = device_manager.get_current_device().await {
        manifest.synced_items.push(synced_items.last().unwrap().clone());
        if let Err(e) = crate::device::write_manifest(&device_path_buf, &manifest).await {
            eprintln!("[Sync] Warning: per-file manifest write failed: {}", e);
            // Non-fatal: sync continues even if per-file write fails
        } else {
            device_manager.update_current_device(manifest).await;
        }
    }
    ```
  - [x] T3.4: **Per-delete manifest update** — In the `Ok(_)` branch of `tokio::fs::remove_file` (after the `files_completed += 1` operation manager update), add:
    ```rust
    // Per-delete manifest update for dirty-resume support (Story 4.4)
    if let Some(mut manifest) = device_manager.get_current_device().await {
        manifest.synced_items.retain(|i| i.jellyfin_id != delete_item.jellyfin_id);
        if let Err(e) = crate::device::write_manifest(&device_path_buf, &manifest).await {
            eprintln!("[Sync] Warning: per-delete manifest write failed: {}", e);
        } else {
            device_manager.update_current_device(manifest).await;
        }
    }
    ```
  - [x] T3.5: **Per-ID-change manifest update** — In the `for id_change in &delta.id_changes` loop, after the existing `synced_items.push(...)` call and after the `files_completed += 1` operation manager update, add:
    ```rust
    // Per-ID-change manifest update for dirty-resume support (Story 4.4)
    // Remove old ID entry, add new ID entry atomically.
    if let Some(mut manifest) = device_manager.get_current_device().await {
        manifest.synced_items.retain(|i| i.jellyfin_id != id_change.old_jellyfin_id);
        manifest.synced_items.push(synced_items.last().unwrap().clone());
        if let Err(e) = crate::device::write_manifest(&device_path_buf, &manifest).await {
            eprintln!("[Sync] Warning: per-ID-change manifest write failed: {}", e);
        } else {
            device_manager.update_current_device(manifest).await;
        }
    }
    ```

- [x] **T4: Update `handle_sync_execute` in rpc.rs for dirty state management** (AC: #4, #6)
  - [x] T4.1: **Derive `pending_item_ids` from delta** — After extracting `delta` from params and before `total_files`, add:
    ```rust
    // Derive basket IDs that need downloading — used for dirty-resume (Story 4.4)
    let pending_item_ids: Vec<String> = delta
        .adds
        .iter()
        .map(|a| a.jellyfin_id.clone())
        .chain(delta.id_changes.iter().map(|c| c.new_jellyfin_id.clone()))
        .collect();
    ```
  - [x] T4.2: **Mark manifest dirty BEFORE `tokio::spawn`** — After the `state.sync_operation_manager.create_operation(...)` call and before the `let jellyfin_client = ...` lines, add:
    ```rust
    // Mark manifest dirty before sync starts — enables interrupted-sync detection (Story 4.4)
    if let Some(path_for_dirty) = state.device_manager.get_current_device_path().await {
        if let Some(mut manifest) = state.device_manager.get_current_device().await {
            manifest.dirty = true;
            manifest.pending_item_ids = pending_item_ids.clone();
            if let Err(e) = crate::device::write_manifest(&path_for_dirty, &manifest).await {
                eprintln!("[Sync] Warning: failed to mark manifest dirty: {}", e);
            } else {
                state.device_manager.update_current_device(manifest).await;
            }
        }
    }
    ```
  - [x] T4.3: **Pass `device_manager` to `execute_sync`** — In the `tokio::spawn` block, the `execute_sync` call gains one new final argument:
    ```rust
    let result = crate::sync::execute_sync(
        &delta,
        &device_path,
        &jellyfin_client,
        &url,
        &token,
        &user_id,
        op_manager.clone(),
        op_id.clone(),
        device_manager.clone(),  // NEW — for per-file manifest updates
    )
    .await;
    ```
  - [x] T4.4: **Replace manifest post-sync logic** — In the `Ok((synced_items, errors))` branch inside `tokio::spawn`, REMOVE the entire existing manifest update block:
    ```rust
    // REMOVE THIS ENTIRE BLOCK:
    if let Some(mut manifest) = device_manager.get_current_device().await {
        manifest.synced_items.extend(synced_items);
        let failed_ids: std::collections::HashSet<&str> = ...;
        manifest.synced_items.retain(|item| { ... });
        if let Err(e) = crate::device::write_manifest(&device_path, &manifest).await { ... }
        device_manager.update_current_device(manifest).await;
    }
    ```
    REPLACE with the dirty-clear write (per-file updates already handled all item changes):
    ```rust
    // Clear dirty flag after sync completes — per-file updates already wrote all items (Story 4.4)
    if let Some(mut manifest) = device_manager.get_current_device().await {
        manifest.dirty = false;
        manifest.pending_item_ids = vec![];
        if let Err(e) = crate::device::write_manifest(&device_path, &manifest).await {
            eprintln!("Failed to write final manifest: {}", e);
        }
        device_manager.update_current_device(manifest).await;
    }
    ```
    **KEEP UNCHANGED**: the operation status update block (`op_manager.get_operation` / `update_operation`).
  - [x] T4.5: `pending_item_ids` is NOT passed into the `move` closure — it is fully consumed by T4.2 before the spawn. No ownership conflict.

- [x] **T5: Add `sync_get_resume_state` RPC method** (AC: #2, #3)
  - [x] T5.1: In the `handler` match statement in `rpc.rs`, add the new route BEFORE the `_ =>` catch-all:
    ```rust
    "sync_get_resume_state" => handle_sync_get_resume_state(&state).await,
    ```
  - [x] T5.2: Implement the handler (add near the other `handle_sync_*` functions):
    ```rust
    async fn handle_sync_get_resume_state(state: &AppState) -> Result<Value, JsonRpcError> {
        let device = state.device_manager.get_current_device().await;
        let device_path = state.device_manager.get_current_device_path().await;

        match (device, device_path) {
            (Some(manifest), Some(path)) => {
                let is_dirty = manifest.dirty;
                let pending_ids = manifest.pending_item_ids.clone();

                let cleaned_tmp_files = if is_dirty {
                    crate::device::cleanup_tmp_files(&path).await.unwrap_or(0)
                } else {
                    0
                };

                Ok(serde_json::json!({
                    "isDirty": is_dirty,
                    "pendingItemIds": pending_ids,
                    "cleanedTmpFiles": cleaned_tmp_files,
                }))
            }
            _ => Ok(serde_json::json!({
                "isDirty": false,
                "pendingItemIds": [],
                "cleanedTmpFiles": 0,
            })),
        }
    }
    ```

- [x] **T6: Update `handle_get_daemon_state` to expose dirty state** (AC: #1, #2)
  - [x] T6.1: In `handle_get_daemon_state`, the `device` variable is moved into `serde_json::json!`. Capture `dirty` BEFORE that move:
    ```rust
    // Before the final Ok(...) return, capture dirty from device:
    let dirty = device.as_ref().map(|d| d.dirty).unwrap_or(false);
    Ok(serde_json::json!({
        "currentDevice": device,
        "deviceMapping": mapping,
        "serverConnected": server_connected,
        "dirtyManifest": dirty,          // NEW
    }))
    ```

- [x] **T7: Tests** (AC: #1–#6)
  - [x] T7.1 (`device/tests.rs`): `test_dirty_flag_serde_default` — old manifest JSON without `dirty`/`pending_item_ids` fields deserializes with `dirty: false` and `pending_item_ids: []`
    ```rust
    #[test]
    fn test_dirty_flag_serde_default() {
        let json = r#"{"device_id": "dev-1", "name": "iPod", "version": "1.0"}"#;
        let manifest: DeviceManifest = serde_json::from_str(json).unwrap();
        assert!(!manifest.dirty, "dirty must default to false");
        assert!(manifest.pending_item_ids.is_empty(), "pending_item_ids must default to []");
    }
    ```
  - [x] T7.2 (`device/tests.rs`): `test_dirty_manifest_roundtrip` — write manifest with `dirty: true, pending_item_ids: ["id-1", "id-2"]`, read back, verify fields preserved exactly
    ```rust
    #[tokio::test]
    async fn test_dirty_manifest_roundtrip() {
        let dir = tempdir().unwrap();
        let manifest = DeviceManifest {
            device_id: "dev-1".to_string(),
            name: None,
            version: "1.0".to_string(),
            managed_paths: vec![],
            synced_items: vec![],
            dirty: true,
            pending_item_ids: vec!["id-1".to_string(), "id-2".to_string()],
        };
        write_manifest(dir.path(), &manifest).await.unwrap();
        let content = tokio::fs::read_to_string(dir.path().join(".jellyfinsync.json")).await.unwrap();
        let loaded: DeviceManifest = serde_json::from_str(&content).unwrap();
        assert!(loaded.dirty);
        assert_eq!(loaded.pending_item_ids, vec!["id-1", "id-2"]);
    }
    ```
  - [x] T7.3 (`device/tests.rs`): `test_cleanup_tmp_files_no_music_dir` — temp dir with no `Music/` subdirectory returns `Ok(0)` without error
  - [x] T7.4 (`device/tests.rs`): `test_cleanup_tmp_files_empty_music_dir` — `Music/` exists but contains no `.tmp` files → returns `Ok(0)`
  - [x] T7.5 (`device/tests.rs`): `test_cleanup_tmp_files_finds_and_deletes` — create `Music/Artist/Album/01 - Track.flac.tmp`, call `cleanup_tmp_files`, verify file is deleted and count = 1
    ```rust
    #[tokio::test]
    async fn test_cleanup_tmp_files_finds_and_deletes() {
        let dir = tempdir().unwrap();
        let tmp_path = dir.path().join("Music").join("Artist").join("Album");
        tokio::fs::create_dir_all(&tmp_path).await.unwrap();
        let tmp_file = tmp_path.join("01 - Track.flac.tmp");
        tokio::fs::write(&tmp_file, b"partial").await.unwrap();
        assert!(tmp_file.exists());

        let count = cleanup_tmp_files(dir.path()).await.unwrap();
        assert_eq!(count, 1);
        assert!(!tmp_file.exists(), ".tmp file must be deleted");
    }
    ```
  - [x] T7.6 (`device/tests.rs`): `test_cleanup_tmp_files_nested_multiple` — create 3 `.tmp` files in different nested dirs, verify all deleted, count = 3
  - [x] T7.7 (`device/tests.rs`): `test_cleanup_tmp_files_non_tmp_preserved` — create `.flac` and `.mp3` files alongside `.tmp`, verify ONLY `.tmp` deleted, real files untouched
  - [x] T7.8 (`rpc.rs` tests): `test_rpc_sync_get_resume_state_no_device` — no device connected → returns `{isDirty: false, pendingItemIds: [], cleanedTmpFiles: 0}`
    ```rust
    #[tokio::test]
    async fn test_rpc_sync_get_resume_state_no_device() {
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let device_manager = Arc::new(crate::device::DeviceManager::new(db.clone()));
        let state = AppState { /* all fields */ };
        let result = handle_sync_get_resume_state(&state).await.unwrap();
        assert_eq!(result["isDirty"], false);
        assert!(result["pendingItemIds"].as_array().unwrap().is_empty());
        assert_eq!(result["cleanedTmpFiles"], 0);
    }
    ```
  - [x] T7.9 (`rpc.rs` tests): `test_rpc_sync_get_resume_state_clean_device` — device with `dirty: false` → returns `isDirty: false`
  - [x] T7.10 (`rpc.rs` tests): `test_rpc_sync_get_resume_state_dirty_device` — device with `dirty: true, pending_item_ids: ["id-1"]`, NO `.tmp` files in temp dir → returns `{isDirty: true, pendingItemIds: ["id-1"], cleanedTmpFiles: 0}`
  - [x] T7.11 (`rpc.rs` tests): `test_rpc_get_daemon_state_includes_dirty_manifest_field` — dirty device → `dirtyManifest: true`; clean device → `dirtyManifest: false`
  - [x] T7.12: Verify `cargo build` succeeds with 0 errors and 0 warnings after all changes

## Dev Notes

### Architecture Compliance

**CRITICAL PATTERNS — MANDATORY:**

- **Atomic Manifest Writes** (from `architecture.md`, Safety & Atomicity): Every `write_manifest` call uses the existing Write-Temp-Rename pattern in `device/mod.rs`. Do NOT bypass this — never write `.jellyfinsync.json` directly, always go through `write_manifest`. Per-file manifest updates use this same function.

- **`sync_all` before rename** (from `architecture.md`, Enforcement Guidelines): `write_manifest` already calls `file.sync_all()` before `rename`. No additional work needed for atomicity.

- **Naming Conventions**: `DeviceManifest` uses snake_case in JSON (NO `#[serde(rename_all)]`). New fields serialize as `"dirty"` and `"pending_item_ids"`. The RPC response JSON uses camelCase (`"isDirty"`, `"pendingItemIds"`, `"cleanedTmpFiles"`) because the response is built via `serde_json::json!()` with literal key names — consistent with existing RPC patterns. The field in `get_daemon_state` is `"dirtyManifest"` (camelCase, consistent with `"serverConnected"`, `"currentDevice"` in the same response).

- **No New Dependencies**: All changes use `tokio::fs`, `std::path`, existing `serde_json`. No new crates required.

- **Non-Fatal Per-File Failures**: Per-file manifest writes that fail (e.g., disk I/O error mid-sync) are logged as warnings but do NOT abort the sync. This preserves the existing behavior where individual file errors go into `errors` vec and sync continues.

### Critical Architecture: Why Per-File Writes Are Correct

**The pre-4.4 manifest update flow (WRONG for resume):**
```
Files downloaded: A, B, C → all kept in memory
→ ONE manifest write at end
```
If interrupted after B: manifest on disk = old state (A, B, C not recorded). Re-sync = re-download ALL.

**The post-4.4 flow (CORRECT for resume):**
```
File A downloaded → manifest written (synced_items=[...A])
File B downloaded → manifest written (synced_items=[...A, B])
Device unplugged during C
```
If interrupted: manifest on disk = [A, B]. Re-submitting basket → delta = only C. True resume.

### Key Implementation Details

**`DeviceManifest` final struct layout after T1:**
```rust
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DeviceManifest {
    pub device_id: String,
    pub name: Option<String>,
    pub version: String,
    #[serde(default)]
    pub managed_paths: Vec<String>,
    #[serde(default)]
    pub synced_items: Vec<SyncedItem>,
    #[serde(default)]
    pub dirty: bool,           // NEW — true if sync was interrupted
    #[serde(default)]
    pub pending_item_ids: Vec<String>,  // NEW — jellyfin IDs queued for download
}
```

**All `DeviceManifest` struct literal locations to update (T1.2):**
```
device/tests.rs:  test_write_manifest_creates_files           (~line 196)
device/tests.rs:  test_write_manifest_overwrites_existing     (~line 237, ~line 246)
sync.rs:          empty_manifest()                            (~line 752)
rpc.rs:           test_rpc_sync_get_device_status_map_...     (~line 1218)
rpc.rs:           test_rpc_sync_calculate_delta_partial_...   (~line 1316)
```
Add to each: `dirty: false, pending_item_ids: vec![],`

**`.tmp` file naming** (from `write_file_streamed` in sync.rs):
```rust
let tmp_path = target_path.with_file_name(format!("{}.tmp", file_name.to_string_lossy()));
// e.g., "01 - Track.flac" → "01 - Track.flac.tmp"
```
`path.extension()` on `"01 - Track.flac.tmp"` returns `Some("tmp")` — this is what `cleanup_tmp_files` detects.

**Dirty mark before spawn — timing:**
The dirty manifest write happens BEFORE `tokio::spawn`. The spawned task runs asynchronously but the dirty mark is synchronous from the caller's perspective. If the dirty write itself fails (edge case), we log a warning and continue — the sync proceeds anyway. A failed dirty write is not a reason to abort the sync.

**`pending_item_ids` derivation (T4.1) — rationale:**
```rust
// Derived from delta, NOT accepted as an extra RPC param
let pending_item_ids = delta.adds.iter().map(|a| a.jellyfin_id.clone())
    .chain(delta.id_changes.iter().map(|c| c.new_jellyfin_id.clone()))
    .collect();
```
These are the items that require actual download work. On resume: UI calls `sync_get_resume_state` → gets `pendingItemIds` → calls `sync_calculate_delta` with those IDs → gets delta with only remaining items → calls `sync_execute`. Unchanged items (already in manifest) are NOT included because they don't need downloading.

**Resume flow end-to-end:**
```
Initial sync:
  basket = [A, B, C]  →  delta: adds=[C], unchanged=[A, B]
  pending_item_ids = ["C"]
  dirty = true, write manifest
  A, B already in manifest (unchanged)
  C downloaded → per-file write → manifest has [A, B, C]
  dirty = false, write manifest

Interrupted sync at 60%:
  basket = [A, B, C, D, E]  →  delta: adds=[D, E], unchanged=[A, B, C]
  pending_item_ids = ["D", "E"]
  dirty = true, write manifest
  D downloaded → per-file write → manifest: [A, B, C, D], dirty=true
  Device unplugged during E's download

On reconnect:
  DeviceProber reads manifest → dirty=true
  get_daemon_state → dirtyManifest: true
  UI calls sync_get_resume_state → {isDirty: true, pendingItemIds: ["D", "E"], cleanedTmpFiles: N}
  UI calls sync_calculate_delta({itemIds: ["D", "E"]})
  calculate_delta: current=[A,B,C,D], desired=[D,E] → adds=[E], unchanged=[D]
  UI calls sync_execute({delta: {adds:[E], ...}})
  E downloaded → dirty cleared → complete ✓
```

**`handle_sync_execute` structural change (T4.4):**

The existing manifest-update block in the `Ok` branch (6 lines with `extend`, `retain`, `write_manifest`, `update_current_device`) is ENTIRELY REPLACED by the 6-line dirty-clear block. The `synced_items` return value from `execute_sync` is no longer used for manifest updates — the per-file updates inside `execute_sync` handle that. The `errors` return value is still used for operation status reporting.

### Project Structure Notes

**Alignment with Unified Structure:**
- `device/mod.rs` gains new fields on existing struct + new `cleanup_tmp_files` function (no new modules)
- `sync.rs` gains one new parameter to `execute_sync` + three inline manifest write blocks (no structural changes)
- `rpc.rs` gains one new handler function + one new route + modifications to two existing handlers

**Files to Modify:**
1. `jellyfinsync-daemon/src/device/mod.rs` — `DeviceManifest` struct extension, `cleanup_tmp_files` function
2. `jellyfinsync-daemon/src/device/tests.rs` — struct literal updates + new tests (T7.1–T7.7)
3. `jellyfinsync-daemon/src/sync.rs` — `execute_sync` signature change + per-file manifest updates + struct literal update in `empty_manifest()`
4. `jellyfinsync-daemon/src/rpc.rs` — `handle_sync_execute` refactor + `handle_get_daemon_state` update + `handle_sync_get_resume_state` (new) + route table + struct literal updates + new tests (T7.8–T7.12)

**Files NOT to Modify:**
- `jellyfinsync-daemon/src/api.rs` — No changes
- `jellyfinsync-daemon/src/db.rs` — No changes (no SQLite needed for this story)
- `jellyfinsync-daemon/Cargo.toml` — No new dependencies
- `Cargo.lock` — No new dependencies

### Critical Developer Guardrails

🚨 **MANDATORY — DO NOT SKIP:**

1. **ALWAYS add `dirty: false, pending_item_ids: vec![],` to ALL `DeviceManifest { ... }` struct literals** — there are exactly 6 locations. Missing any one causes a compile error. Search for `DeviceManifest {` to find them.

2. **NEVER write the manifest with `dirty: false` prematurely** — only clear dirty AFTER all of `execute_sync` has returned (inside the `Ok` branch). If `execute_sync` is somehow interrupted (future change), dirty must persist.

3. **The `failed_ids` / `retain` pattern in the old `Ok` branch of `handle_sync_execute` MUST be deleted entirely** — per-file deletes in `execute_sync` already handle item removal. Keeping the old retain would double-remove nothing (deletes are gone from manifest), but the code is dead weight and confusing.

4. **`device_manager` is already cloned as `let device_manager = state.device_manager.clone()`** in `handle_sync_execute` before the spawn. Pass `device_manager.clone()` to `execute_sync` inside the spawn.

5. **Per-file manifest writes are non-fatal** — wrap in `if let Err(e) = ... { eprintln!(...); }` pattern (no `?` propagation). A manifest write failure during sync should not abort the sync.

6. **Do NOT write manifest inside `execute_sync` for ID-change items that previously removed the old entry from `synced_items`** — The old entry is in the CURRENT manifest at call time, so `retain` removes it correctly. The issue to avoid: if `get_current_device()` returns `None` (device disconnected mid-sync), skip the per-file write silently.

7. **`cleanup_tmp_files` only scans `Music/`** — Do NOT scan the device root. The `.jellyfinsync.json.tmp` at device root is managed by `write_manifest` itself and must not be deleted by cleanup.

🔥 **COMMON MISTAKES TO PREVENT:**

- ❌ Keeping the old `manifest.synced_items.extend(synced_items)` in `handle_sync_execute` → ✅ Replace entire block with dirty-clear write
- ❌ Adding `basket_ids` as a new RPC parameter to `sync_execute` → ✅ Derive from `delta.adds + delta.id_changes` in the handler (no API change needed)
- ❌ Making `cleanup_tmp_files` recursive with `async fn cleanup_tmp_in_dir(...)` → ✅ Use iterative stack (Rust async recursion requires `Box::pin`, unnecessary complexity)
- ❌ Forgetting to update `empty_manifest()` in `sync.rs` tests → ✅ Search for ALL `DeviceManifest {` occurrences
- ❌ Using `serde(rename_all = "camelCase")` on `DeviceManifest` → ✅ `DeviceManifest` does NOT have this attribute; use snake_case field names as-is
- ❌ Marking dirty INSIDE the `tokio::spawn` closure → ✅ Mark dirty BEFORE spawning (if device unplugs immediately after spawn, before first file write, dirty flag is already on disk)
- ❌ Skipping `device_manager.update_current_device(manifest).await` after per-file write → ✅ Must update in-memory state too, so subsequent RPC calls see current state
- ❌ `cleanup_tmp_files` returning error when `Music/` doesn't exist → ✅ Return `Ok(0)` when metadata check fails (no Music dir = nothing to clean, not an error)

### References

**Architecture & Planning:**
- [Architecture: Safety & Atomicity Patterns](../../_bmad-output/planning-artifacts/architecture.md#safety--atomicity-patterns) — Write-Temp-Rename for manifest, `sync_all` requirement
- [Architecture: Enforcement Guidelines](../../_bmad-output/planning-artifacts/architecture.md#enforcement-guidelines) — "Commit manifest changes ONLY after `sync_all` has returned successfully"
- [Architecture: Naming Patterns](../../_bmad-output/planning-artifacts/architecture.md#naming-patterns) — snake_case for manifest JSON, camelCase for RPC payloads
- [Epic 4 Story 4.4](../../_bmad-output/planning-artifacts/epics.md#story-44-self-healing-dirty-manifest-resume) — Original AC
- [PRD: FR16](../../_bmad-output/planning-artifacts/prd.md) — "Resume an interrupted sync session without restarting from scratch"
- [PRD: NFR7](../../_bmad-output/planning-artifacts/prd.md) — "Graceful 'Interrupted' session marking and repair utility trigger on mid-sync disconnect"

**Previous Story References:**
- [Story 4.3: Dev Notes — Architecture Compliance](../../_bmad-output/implementation-artifacts/4-3-legacy-hardware-constraints-path-char-validation.md#architecture-compliance) — `serde(default)` pattern for backward-compatible manifest additions
- [Story 4.3: File List](../../_bmad-output/implementation-artifacts/4-3-legacy-hardware-constraints-path-char-validation.md#file-list) — `device/tests.rs` and `rpc.rs` exist as test locations
- [Story 4.3: Code Review Notes](../../_bmad-output/implementation-artifacts/4-3-legacy-hardware-constraints-path-char-validation.md#senior-developer-review-ai) — Per-file manifest pattern consistent with existing atomicity approach; debug `println!` removal reminder

**Source Code Locations:**
- [jellyfinsync-daemon/src/device/mod.rs:24-33](../../jellyfinsync-daemon/src/device/mod.rs#L24) — `DeviceManifest` struct (add dirty + pending_item_ids after line 33)
- [jellyfinsync-daemon/src/device/mod.rs:35-52](../../jellyfinsync-daemon/src/device/mod.rs#L35) — `write_manifest` (add `cleanup_tmp_files` after this function)
- [jellyfinsync-daemon/src/sync.rs:333-541](../../jellyfinsync-daemon/src/sync.rs#L333) — `execute_sync` function (add device_manager param, per-file writes)
- [jellyfinsync-daemon/src/sync.rs:441-477](../../jellyfinsync-daemon/src/sync.rs#L441) — Successful add block in `execute_sync` (add per-file write after `synced_items.push`)
- [jellyfinsync-daemon/src/sync.rs:494-512](../../jellyfinsync-daemon/src/sync.rs#L494) — Successful delete block in `execute_sync` (add per-file write after `files_completed += 1`)
- [jellyfinsync-daemon/src/sync.rs:515-538](../../jellyfinsync-daemon/src/sync.rs#L515) — ID change block in `execute_sync` (add per-file write after `synced_items.push`)
- [jellyfinsync-daemon/src/sync.rs:751-759](../../jellyfinsync-daemon/src/sync.rs#L751) — `empty_manifest()` test helper (add dirty + pending_item_ids)
- [jellyfinsync-daemon/src/rpc.rs:107-134](../../jellyfinsync-daemon/src/rpc.rs#L107) — `handler` match (add `sync_get_resume_state` route)
- [jellyfinsync-daemon/src/rpc.rs:319-334](../../jellyfinsync-daemon/src/rpc.rs#L319) — `handle_get_daemon_state` (add `dirtyManifest` field)
- [jellyfinsync-daemon/src/rpc.rs:725-847](../../jellyfinsync-daemon/src/rpc.rs#L725) — `handle_sync_execute` (full refactor for T4)
- [jellyfinsync-daemon/src/rpc.rs:792-815](../../jellyfinsync-daemon/src/rpc.rs#L792) — Existing manifest extend/retain block INSIDE `tokio::spawn` (delete entirely, replace with dirty-clear)

## Dev Agent Record

### Agent Model Used

claude-sonnet-4-6

### Debug Log References

No blocking issues. One additional struct literal was found in `jellyfinsync-daemon/src/tests.rs` (crate integration test file, not listed in story's Dev Notes) — added `dirty: false, pending_item_ids: vec![]` to fix compilation.

### Completion Notes List

- **T1**: Added `dirty: bool` and `pending_item_ids: Vec<String>` fields to `DeviceManifest` with `#[serde(default)]` — fully backward-compatible with existing manifests. Updated all 7 struct literals (6 as specified in story + 1 in `src/tests.rs`).
- **T2**: Implemented `cleanup_tmp_files` using an iterative stack pattern to scan `Music/` recursively. Targets only files with `.tmp` extension. Non-fatal deletions; `Ok(0)` returned when Music/ is absent.
- **T3**: Extended `execute_sync` with `device_manager` parameter. Per-file manifest writes after each successful add, delete, and ID-change operation. All writes are non-fatal — sync continues on manifest write failures.
- **T4**: Refactored `handle_sync_execute` — derives `pending_item_ids` from delta, marks manifest dirty before spawn, passes `device_manager` to `execute_sync`, replaces the bulk `extend/retain/write` block with a dirty-clear write in the `Ok` branch.
- **T5**: Added `sync_get_resume_state` RPC route and `handle_sync_get_resume_state` handler. Returns `isDirty`, `pendingItemIds`, and `cleanedTmpFiles` (cleanup triggered only when dirty=true).
- **T6**: Updated `handle_get_daemon_state` to capture `dirty` before `device` moves into `json!()`, and added `"dirtyManifest"` field to the response.
- **T7**: 12 new tests written (T7.1–T7.12). All 82 tests pass (0 failures).
- **Build**: `cargo build` produces 0 errors and 0 warnings.
- **Code Review**: Fixed High and Medium severity issues identified by AI review, including atomic `update_manifest` lock patterns, proper failure abortion for dirty saves, dynamic `managed_paths` handling, symlink mitigation for temp files, and API chunk error text logging.

### File List

- `jellyfinsync-daemon/src/device/mod.rs`
- `jellyfinsync-daemon/src/device/tests.rs`
- `jellyfinsync-daemon/src/sync.rs`
- `jellyfinsync-daemon/src/rpc.rs`
- `jellyfinsync-daemon/src/tests.rs`

## Change Log

- 2026-02-22: Story 4.4 implemented — Self-healing dirty manifest resume. Added `dirty`/`pending_item_ids` fields to `DeviceManifest`, `cleanup_tmp_files` utility, per-file atomic manifest updates in `execute_sync`, dirty flag lifecycle management in `handle_sync_execute`, new `sync_get_resume_state` RPC method, and `dirtyManifest` exposure in `get_daemon_state`. 12 new tests added; all 82 tests pass.
- 2026-02-22: Completed AI adversarial code review. Fixed 3 High and 2 Medium issues (Data race on manifest save, dirty save error ignoring, hardcoded "Music" path, symlink traversal in tmp cleaning, `.tmp` file blanket deletion). Status updated to done.
