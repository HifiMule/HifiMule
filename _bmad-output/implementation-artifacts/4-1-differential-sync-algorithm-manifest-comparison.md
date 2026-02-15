# Story 4.1: Differential Sync Algorithm (Manifest Comparison)

Status: review

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a **System Admin (Alexis)**,
I want **the engine to calculate exactly which files to add or delete by comparing the Jellyfin server state with the local `.jellysync.json` manifest**,
so that **only necessary changes are made to the disk, preserving the hardware's life.**

## Acceptance Criteria

1. **Manifest Extension**: The `.jellysync.json` manifest MUST include a `synced_items` array that records every item currently synced to the device, including Jellyfin item ID, server-side metadata hash or version identifier, and the local file path written to disk. (AC: #1)
2. **Delta Calculation**: Given a Selection Basket with N items, the sync engine MUST generate a precise list of "Adds" (items in basket but not on device) and "Deletes" (items on device but no longer in basket) by comparing the basket item IDs against the manifest's `synced_items` records. (AC: #2)
3. **Server ID Change Detection**: The engine MUST detect if a Jellyfin server has reassigned IDs for existing local files (e.g., after a library re-scan) by comparing item metadata (name, album, artist) as a secondary match when IDs don't align. (AC: #3)
4. **Atomic Manifest Updates**: All manifest writes MUST use the "Write-Temp-Rename" pattern: write to `.jellysync.json.tmp`, call `sync_all`, then atomically rename to `.jellysync.json`. (AC: #4)
5. **RPC Integration**: A new `sync_calculate_delta` RPC method MUST accept a list of desired item IDs from the UI basket and return the computed `{ adds: [...], deletes: [...], unchanged: count }` delta. (AC: #5)
6. **Device Status Map**: The existing `sync_get_device_status_map` stub MUST be replaced with a real implementation that reads `synced_items` from the current device manifest and returns the list of synced Jellyfin item IDs. (AC: #6)

## Tasks / Subtasks

- [x] **T1: Extend DeviceManifest struct** (AC: #1, #4)
  - [x] T1.1: Add `synced_items: Vec<SyncedItem>` field to `DeviceManifest` in `jellysync-daemon/src/device/mod.rs`
  - [x] T1.2: Define `SyncedItem` struct with fields: `jellyfin_id: String`, `name: String`, `album: Option<String>`, `artist: Option<String>`, `local_path: String`, `size_bytes: u64`, `synced_at: String`
  - [x] T1.3: Ensure `#[serde(default)]` on `synced_items` for backward compatibility with existing manifests
  - [x] T1.4: Implement atomic manifest write function using Write-Temp-Rename pattern (`write_manifest(path, manifest)`)

- [x] **T2: Create sync engine module** (AC: #2, #3)
  - [x] T2.1: Create `jellysync-daemon/src/sync.rs` module
  - [x] T2.2: Define `SyncDelta` struct: `adds: Vec<SyncAddItem>`, `deletes: Vec<SyncDeleteItem>`, `unchanged: Vec<String>`
  - [x] T2.3: Define `SyncAddItem` (jellyfin_id, name, album, artist, size_bytes) and `SyncDeleteItem` (jellyfin_id, local_path, name)
  - [x] T2.4: Implement `calculate_delta(desired_items: &[DesiredItem], manifest: &DeviceManifest) -> SyncDelta`
  - [x] T2.5: Implement server ID change detection via metadata matching (name + album + artist fallback)

- [x] **T3: RPC integration** (AC: #5, #6)
  - [x] T3.1: Add `sync_calculate_delta` RPC handler in `jellysync-daemon/src/rpc.rs` accepting `{ "itemIds": [...] }` params
  - [x] T3.2: Handler fetches item details from Jellyfin API for each desired ID, then calls `calculate_delta`
  - [x] T3.3: Replace `sync_get_device_status_map` stub with real implementation reading manifest `synced_items`
  - [x] T3.4: Register new method in the RPC handler match block

- [x] **T4: Testing** (AC: #1-#6)
  - [x] T4.1: Unit tests for `calculate_delta` — empty manifest, full overlap, partial overlap, complete replacement
  - [x] T4.2: Unit tests for server ID change detection via metadata fallback
  - [x] T4.3: Unit tests for atomic manifest write (Write-Temp-Rename)
  - [x] T4.4: Unit tests for backward compatibility (reading old manifests without `synced_items`)
  - [x] T4.5: Integration test for `sync_calculate_delta` RPC method

## Dev Notes

### Architecture Compliance

- **IPC Protocol**: JSON-RPC 2.0 over localhost HTTP (port `19140`). All new RPC methods MUST follow existing patterns in `rpc.rs` — match on method name string, delegate to `handle_*` async function, return `Result<Value, JsonRpcError>`.
- **Naming**: Rust uses `snake_case`, JSON payloads use `camelCase` (enforce with `#[serde(rename_all = "camelCase")]` on all structs exposed via RPC).
- **Error Handling**: Use `thiserror` for typed errors in the sync module, `anyhow` at the RPC boundary.
- **Atomic IO**: The "Write-Temp-Rename" pattern is MANDATORY for `.jellysync.json` per architecture doc. Use `std::fs::write` to temp file → `File::sync_all()` → `std::fs::rename`.

### Technical Implementation Details

- **DeviceManifest location**: `jellysync-daemon/src/device/mod.rs` — Current struct at ~line 20. Add `synced_items` field with `#[serde(default)]` for backward compat.
- **RPC handler**: `jellysync-daemon/src/rpc.rs` — The `handler` function matches method names. Add `"sync_calculate_delta"` case. The existing `"sync_get_device_status_map"` handler has a `TODO` comment and returns a stub.
- **DeviceManager access**: Use `state.device_manager.get_current_device()` to get `Option<DeviceManifest>` and `get_current_device_path()` for the device root.
- **Jellyfin API**: Use `state.jellyfin_client.get_item_details(url, token, user_id, item_id)` to fetch item metadata for delta comparison. `JellyfinItem` already has `id`, `name`, `album_artist`, `item_type` fields.
- **Credentials**: Use `crate::api::CredentialManager::get_credentials()` to get `(url, token, user_id)`.

### Key Patterns from Previous Stories

- **Story 3.4** (`device/mod.rs`): Added `managed_paths` to `DeviceManifest` with `#[serde(default)]`. Follow the same pattern for `synced_items`.
- **Story 3.5** (`api.rs`): Defined `MUSIC_ITEM_TYPES` constant for filtering. The sync engine should reuse this when validating items.
- **Tests**: Previous stories use `#[cfg(test)] mod tests` blocks in the same file. Integration tests use `mockito` for HTTP mocking and `tempfile` for filesystem tests.

### Manifest Schema Evolution

The `.jellysync.json` format evolves from:
```json
{
  "device_id": "abc-123",
  "name": "My iPod",
  "version": "1.0",
  "managed_paths": ["Music"]
}
```
To:
```json
{
  "device_id": "abc-123",
  "name": "My iPod",
  "version": "1.1",
  "managed_paths": ["Music"],
  "synced_items": [
    {
      "jellyfinId": "item-uuid-1",
      "name": "Track Name",
      "album": "Album Name",
      "artist": "Artist Name",
      "localPath": "Music/Artist/Album/01 - Track.flac",
      "sizeBytes": 34521088,
      "syncedAt": "2026-02-15T10:30:00Z"
    }
  ]
}
```

### Delta Algorithm Pseudocode

```
fn calculate_delta(desired, manifest):
    current_ids = set(manifest.synced_items.map(|i| i.jellyfin_id))
    desired_ids = set(desired.map(|i| i.id))

    adds = desired.filter(|i| !current_ids.contains(i.id))
    deletes = manifest.synced_items.filter(|i| !desired_ids.contains(i.jellyfin_id))
    unchanged = current_ids.intersection(desired_ids)

    // Server ID change detection:
    for each unmatched_desired in adds:
        if any delete has matching (name, album, artist):
            mark as "id_changed" instead of add+delete

    return SyncDelta { adds, deletes, unchanged }
```

### File Structure & Source Tree

Files to create:
- `jellysync-daemon/src/sync.rs` — New sync engine module

Files to modify:
- `jellysync-daemon/src/main.rs` — Add `mod sync;` declaration
- `jellysync-daemon/src/device/mod.rs` — Extend `DeviceManifest`, add `SyncedItem`, add `write_manifest` function
- `jellysync-daemon/src/rpc.rs` — Replace stub, add `sync_calculate_delta` handler
- `jellysync-daemon/src/device/tests.rs` — Add manifest backward-compat tests

### Testing Standards

- **Framework**: Built-in `#[cfg(test)]` with `#[tokio::test]` for async tests
- **Mocking**: `mockito` for Jellyfin API calls, `tempfile::tempdir()` for filesystem operations
- **Pattern**: Each test function tests one specific scenario with clear Given/When/Then structure
- **Coverage**: All delta calculation edge cases MUST be covered (empty states, full overlaps, ID changes)

### Project Structure Notes

- Alignment with workspace: New `sync.rs` sits alongside `api.rs`, `db.rs`, `device/mod.rs` in `jellysync-daemon/src/`
- The `sync` module is intentionally a single file (not a directory) for Story 4.1 scope — Story 4.2 (Atomic Buffered IO) will expand it if needed
- No UI changes needed for this story — the UI already calls `sync_get_device_status_map` and will benefit from the real implementation

### References

- [Architecture: Safety & Atomicity Patterns](_bmad-output/planning-artifacts/architecture.md) — Write-Temp-Rename mandate
- [Architecture: API Communication Patterns](_bmad-output/planning-artifacts/architecture.md) — JSON-RPC 2.0 protocol
- [Epic 4 Story 4.1](_bmad-output/planning-artifacts/epics.md) — Original story definition
- [PRD: FR12](_bmad-output/planning-artifacts/prd.md) — Differential sync requirement
- [PRD: NFR4-NFR5](_bmad-output/planning-artifacts/prd.md) — Atomic manifest + sync_all requirements
- [Jellyfin API - Items](https://api.jellyfin.org/#tag/Items/operation/GetItems) — Item retrieval endpoint
- [Story 3.4: Managed Zone](_bmad-output/implementation-artifacts/3-4-managed-zone-hardware-shielding.md) — `managed_paths` pattern reference
- [Story 3.5: Music Filtering](_bmad-output/implementation-artifacts/3-5-music-only-library-filtering.md) — `MUSIC_ITEM_TYPES` constant

## Change Log

- 2026-02-15: Implemented differential sync algorithm with manifest comparison, atomic writes, RPC integration, and comprehensive tests (Story 4.1)

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6

### Debug Log References

No blocking issues encountered during implementation.

### Completion Notes List

- **T1**: Extended `DeviceManifest` with `SyncedItem` struct and `synced_items` field (`#[serde(default)]` for backward compat). Implemented `write_manifest()` using Write-Temp-Rename atomic pattern (write to .tmp, sync_all, rename).
- **T2**: Created `sync.rs` module with `calculate_delta()` function. Implements set-based delta calculation (adds/deletes/unchanged) with server ID change detection via case-insensitive metadata matching (name + album + artist). When an ID change is detected, the delete is suppressed and only the add remains to update the manifest.
- **T3**: Added `sync_calculate_delta` RPC handler that accepts `{ "itemIds": [...] }`, fetches item details from Jellyfin API, and returns computed delta. Replaced `sync_get_device_status_map` stub with real implementation reading `synced_items` from manifest.
- **T4**: 49 tests pass total. Added 6 unit tests for delta calculation (empty, full overlap, partial, complete replacement, ID change detection, case-insensitive matching), 4 device tests (backward compat, synced items deserialization, atomic write, overwrite), and 4 RPC integration tests (missing params, no device, status map empty, status map with items).
- Existing test in `tests.rs` updated to include `synced_items` field in `DeviceManifest` literal.

### File List

- `jellysync-daemon/src/sync.rs` (new) — Sync engine module with delta calculation and server ID change detection
- `jellysync-daemon/src/device/mod.rs` (modified) — Added `SyncedItem` struct, `synced_items` field on `DeviceManifest`, `write_manifest()` function
- `jellysync-daemon/src/device/tests.rs` (modified) — Added backward compat, serialization, and atomic write tests
- `jellysync-daemon/src/main.rs` (modified) — Added `mod sync;` declaration
- `jellysync-daemon/src/rpc.rs` (modified) — Added `sync_calculate_delta` handler, replaced `sync_get_device_status_map` stub, added RPC tests
- `jellysync-daemon/src/tests.rs` (modified) — Updated `DeviceManifest` literal to include `synced_items`
- `_bmad-output/implementation-artifacts/sprint-status.yaml` (modified) — Story status updated

## Code Review Findings (Adversarial)
**Date:** 2026-02-15
**Reviewer:** Antigravity

### Critical Issues
- [x] **Data Loss Risk**: `sync_calculate_delta` silently dropped items if API call failed, potentially causing unintended deletions.
    - **Fix**: Updated `rpc.rs` to propagate errors and abort sync if any item fetch fails.

### Medium Issues
- [x] **Unbounded Concurrency**: `handle_sync_calculate_delta` spawned unlimited futures.
    - **Fix**: Implemented `stream::buffer_unordered(10)` to limit concurrent requests.
- [x] **Performance**: Inefficient metadata matching in `calculate_delta`.
    - **Fix**: Optimized `sync.rs` to build metadata map and delete list in a single pass O(N).

### Low Issues
- [ ] **Platform Specificity**: Filesystem case sensitivity assumption.
    - **Note**: Deferring to future story for cross-platform hardening.

## Status
**Review Status**: Passed (with fixes applied)
**Implementation Status**: Complete
