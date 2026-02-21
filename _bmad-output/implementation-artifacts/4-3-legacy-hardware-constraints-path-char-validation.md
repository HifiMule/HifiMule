# Story 4.3: Legacy Hardware Constraints (Path & Char Validation)

Status: review

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a **Ritualist (Arthur)**,
I want **the engine to automatically shorten paths or rename files that exceed legacy hardware limits (e.g., FAT32 or Rockbox 255-char limits)**,
so that **my sync never fails due to filesystem errors**.

## Acceptance Criteria

1. **Path Component Length Validation**: The engine MUST validate each path component (artist folder, album folder, full filename) against the legacy hardware limit of **255 characters** before any file write is attempted. (AC: #1)

2. **Automatic Truncation**: If a component exceeds 255 characters, the engine MUST automatically truncate it to fit within the limit. For filenames, the file **extension MUST be preserved** after truncation — truncation applies only to the base name. (AC: #2)

3. **Trailing Character Cleanup**: After truncation, path components MUST NOT end with spaces or dots — these are forbidden by FAT32. Trailing spaces and dots must be stripped after any truncation. (AC: #3)

4. **Original-to-Sanitized Mapping in Manifest**: When a track name (the filename base) is truncated or modified due to length constraints, the engine MUST log the original Jellyfin track name in the device manifest by populating `SyncedItem.original_name`. Items that required no truncation MUST have `original_name: null` (not set). (AC: #4)

5. **No Sync Failure on Long Paths**: A track with a 300-character name MUST NOT cause the sync to fail — the engine must handle it gracefully by truncating to within hardware limits before writing. (AC: #5)

6. **Short Names Unaffected**: Items whose artist, album, and filename components are all within 255 characters pass through unchanged with `original_name` unset. (AC: #6)

## Tasks / Subtasks

- [x] **T1: Define constants and result type** (AC: #1, #2, #4)
  - [x] T1.1: Add `pub const MAX_PATH_COMPONENT_LEN: usize = 255;` constant to `sync.rs` (FAT32/Rockbox per-component limit)
  - [x] T1.2: Define `pub struct PathConstructionResult` in `sync.rs` with fields:
    - `pub path: std::path::PathBuf` — the final resolved path (truncated as necessary)
    - `pub original_name: Option<String>` — the original Jellyfin track name, set only if the filename was truncated

- [x] **T2: Implement truncation helpers** (AC: #2, #3)
  - [x] T2.1: Add `fn truncate_component(component: &str, max_len: usize) -> String` in `sync.rs`
    - If `component.chars().count() <= max_len` → return `component.to_string()` unchanged
    - Otherwise collect the first `max_len` chars: `component.chars().take(max_len).collect::<String>()`
    - Strip trailing `' '` and `'.'` via `.trim_end_matches(|c| c == ' ' || c == '.')`
    - Return the cleaned truncated string
  - [x] T2.2: Add `fn truncate_filename(base: &str, extension: &str, max_len: usize) -> String` in `sync.rs`
    - `extension_with_dot_len = extension.chars().count() + 1` (for the dot separator)
    - If `extension_with_dot_len >= max_len` → return `base.chars().take(max_len).collect()` (pathological edge case)
    - Otherwise `max_base_len = max_len - extension_with_dot_len`
    - Truncate base to `max_base_len` chars, then strip trailing `' '` and `'.'`
    - Return `format!("{}.{}", truncated_base, extension)`

- [x] **T3: Update `construct_file_path` return type and add length validation** (AC: #1, #2, #3, #4, #5, #6)
  - [x] T3.1: Change return type from `Result<std::path::PathBuf>` to `Result<PathConstructionResult>`
  - [x] T3.2: After sanitizing `artist_clean` and `album_clean` with `sanitize_path_component`, apply `truncate_component(&artist_clean, MAX_PATH_COMPONENT_LEN)` and same for album
  - [x] T3.3: Construct the full filename string: `format!("{} - {}.{}", track_number, track_name_clean, extension)`
  - [x] T3.4: Check if the filename component (not the full path) exceeds `MAX_PATH_COMPONENT_LEN` chars
    - If yes: call `truncate_filename` with the base part (`"{track_number} - {track_name_clean}"`), extension, and max len; set `original_name = Some(item.name.clone())`
    - If no: use filename as-is; set `original_name = None`
  - [x] T3.5: Remove the existing `// TODO: Add path length validation for legacy hardware (Story 4.3)` comment
  - [x] T3.6: Build `PathBuf` from components, return `Ok(PathConstructionResult { path, original_name })`

- [x] **T4: Add `original_name` field to `SyncedItem`** (AC: #4)
  - [x] T4.1: In `jellysync-daemon/src/device/mod.rs`, add to `SyncedItem` struct:
    ```rust
    #[serde(default)]
    pub original_name: Option<String>,
    ```
    (the struct-level `#[serde(rename_all = "camelCase")]` already applies — field serializes as `"originalName"`)

- [x] **T5: Update `execute_sync` to use new return type** (AC: #4, #5)
  - [x] T5.1: In `execute_sync` (`sync.rs`), change the `construct_file_path` call to unpack `PathConstructionResult`:
    ```rust
    let construction = match construct_file_path(&managed_path, &item) {
        Ok(result) => result,
        Err(e) => { /* existing error handling */ continue; }
    };
    let target_path = construction.path;
    ```
  - [x] T5.2: When pushing to `synced_items` on successful write, include `original_name`:
    ```rust
    synced_items.push(crate::device::SyncedItem {
        // ... existing fields ...
        original_name: construction.original_name,
    });
    ```

- [x] **T6: Update test suite for new `construct_file_path` return type** (AC: #1–#6)
  - [x] T6.1: Update `test_construct_file_path_basic` and `test_construct_file_path_missing_fields_uses_defaults` in `sync.rs` — unpack `.path` from the result
  - [x] T6.2: Add `test_truncate_component_short_name_unchanged` — name ≤ 255 chars returns identical string
  - [x] T6.3: Add `test_truncate_component_300_char_name` — 300-char string truncated to exactly 255 chars
  - [x] T6.4: Add `test_truncate_component_trailing_dots_stripped` — trailing dots removed after truncation
  - [x] T6.5: Add `test_truncate_component_trailing_spaces_stripped` — trailing spaces removed after truncation
  - [x] T6.6: Add `test_construct_file_path_short_name_no_original_name` — short track name yields `original_name: None`
  - [x] T6.7: Add `test_construct_file_path_long_filename_extension_preserved` — 300-char track name: filename ≤ 255 chars, extension preserved, `original_name` set
  - [x] T6.8: Add `test_construct_file_path_long_album_artist_truncated` — long artist and album strings are truncated, each component ≤ 255 chars
  - [x] T6.9: Add `test_synced_item_original_name_serializes_as_camel_case` — verify JSON field name is `"originalName"` via `serde_json::to_value`
  - [x] T6.10: Verify `cargo build` succeeds with 0 errors and 0 warnings

## Dev Notes

### Architecture Compliance

**CRITICAL PATTERNS — MANDATORY:**

- **Architecture Mandate** (from `architecture.md`, "All AI Agents MUST" section):
  > "Validate filesystem path lengths before attempting write operations on legacy hardware."
  This is Story 4.3's entire purpose. Do not skip or defer.

- **Naming Conventions**:
  - Rust code: `snake_case` (functions, fields, constants)
  - JSON/RPC payloads: `camelCase` enforced with `#[serde(rename_all = "camelCase")]`
  - The new `original_name` Rust field → serializes as `"originalName"` in JSON (automatic via struct attribute)

- **No New RPC Methods**: This story adds **internal path validation only**. The `construct_file_path` change is transparent to all callers.

- **No New Dependencies**: All truncation logic uses Rust `std` only — no new crates required.

- **Atomic Manifest Updates Unchanged**: `write_manifest` in `device/mod.rs` uses Write-Temp-Rename. Adding `original_name` to `SyncedItem` is a backward-compatible schema addition — the manifest file format gains an optional field with `#[serde(default)]`.

### Key Implementation Details

**Hardware Path Limits Reference:**
```
FAT32 (all platforms):  255 characters per path component (artist folder, album folder, filename)
Rockbox:                255 characters per path component
macOS HFS+/APFS:        255 bytes per component (UTF-8), ~85–255 chars depending on encoding
Windows NTFS:           255 UTF-16 code units per component
Safe universal limit:   255 characters (chars().count()) — this is what we enforce
```

**Exact Hook Point in `sync.rs` (Story 4.2 left this TODO):**
```rust
// Line 181 in sync.rs:
// TODO: Add path length validation for legacy hardware (Story 4.3)
```
Replace this comment with the actual validation call.

**Truncation Algorithm — Character-Aware (UTF-8 Safety):**
```rust
fn truncate_component(component: &str, max_len: usize) -> String {
    if component.chars().count() <= max_len {
        return component.to_string();
    }
    // chars().take(max_len) respects char boundaries — safe for Unicode
    let truncated: String = component.chars().take(max_len).collect();
    // FAT32: trailing spaces and dots are forbidden
    truncated.trim_end_matches(|c| c == ' ' || c == '.').to_string()
}
```

**Filename Truncation — Preserve Extension:**
```rust
fn truncate_filename(base: &str, extension: &str, max_len: usize) -> String {
    let ext_len = extension.chars().count() + 1; // +1 for the '.' separator
    if ext_len >= max_len {
        // Pathological: extension itself is too long — truncate base to nothing
        return base.chars().take(max_len).collect();
    }
    let max_base_len = max_len - ext_len;
    let truncated_base: String = base.chars().take(max_base_len).collect();
    let clean_base = truncated_base.trim_end_matches(|c| c == ' ' || c == '.');
    format!("{}.{}", clean_base, extension)
}
```

**Updated `construct_file_path` Logic Flow:**
```
1. Extract raw fields: artist, album, track_name, track_number, extension
2. sanitize_path_component → removes invalid chars (< > : " / \ | ? *)
3. truncate_component (artist, 255) → length enforcement
4. truncate_component (album, 255) → length enforcement
5. Build filename = "{track_number} - {track_name_clean}.{extension}"
6. If filename.chars().count() > 255:
     base = "{track_number} - {track_name_clean}"
     filename = truncate_filename(base, extension, 255)
     original_name = Some(item.name.clone())
   Else:
     original_name = None
7. Return PathConstructionResult { path: managed_path/artist/album/filename, original_name }
```

**`PathConstructionResult` struct (add to `sync.rs`, near top of file with other structs):**
```rust
/// The result of constructing a file path from Jellyfin metadata.
///
/// Contains the resolved filesystem path and an optional mapping of the
/// original Jellyfin track name if truncation was applied.
pub struct PathConstructionResult {
    /// The final path where the file will be written (truncated as necessary).
    pub path: std::path::PathBuf,
    /// The original Jellyfin track name, set only if the filename component
    /// was truncated due to legacy hardware path length constraints.
    pub original_name: Option<String>,
}
```

**Updated `SyncedItem` in `device/mod.rs`:**
```rust
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SyncedItem {
    pub jellyfin_id: String,
    pub name: String,                          // The (potentially truncated) local name
    #[serde(default)]
    pub album: Option<String>,
    #[serde(default)]
    pub artist: Option<String>,
    pub local_path: String,
    pub size_bytes: u64,
    pub synced_at: String,
    #[serde(default)]
    pub original_name: Option<String>,         // Set when name was truncated for hardware limits
}
```

**Updated `execute_sync` caller in `sync.rs`:**
```rust
// Before (Story 4.2):
let target_path = match construct_file_path(&managed_path, &item) {
    Ok(path) => path,
    Err(e) => {
        errors.push(SyncFileError { ... });
        continue;
    }
};

// After (Story 4.3):
let construction = match construct_file_path(&managed_path, &item) {
    Ok(result) => result,
    Err(e) => {
        errors.push(SyncFileError {
            jellyfin_id: add_item.jellyfin_id.clone(),
            filename: add_item.name.clone(),
            error_message: format!("Failed to construct file path: {}", e),
        });
        continue;
    }
};
let target_path = construction.path;
// ... then when building SyncedItem:
synced_items.push(crate::device::SyncedItem {
    jellyfin_id: add_item.jellyfin_id.clone(),
    name: add_item.name.clone(),
    album: add_item.album.clone(),
    artist: add_item.artist.clone(),
    local_path: target_path
        .strip_prefix(device_path)
        .unwrap_or(&target_path)
        .to_string_lossy()
        .to_string(),
    size_bytes: add_item.size_bytes,
    synced_at: unix_timestamp.to_string(),
    original_name: construction.original_name,   // NEW FIELD
});
```

**Existing Tests to Update (change `.unwrap()` to unpack `.path`):**
```rust
// test_construct_file_path_basic — before:
let path = construct_file_path(&managed, &item).unwrap();
// after:
let path = construct_file_path(&managed, &item).unwrap().path;

// test_construct_file_path_missing_fields_uses_defaults — same pattern
let path = construct_file_path(&managed, &item).unwrap().path;
```

**New Test Helper for Long Names:**
```rust
fn make_test_item(
    name: &str,
    album_artist: Option<&str>,
    album: Option<&str>,
    index: Option<u32>,
    container: Option<&str>,
) -> crate::api::JellyfinItem {
    crate::api::JellyfinItem {
        id: "test-id".to_string(),
        name: name.to_string(),
        item_type: "Audio".to_string(),
        album: album.map(|s| s.to_string()),
        album_artist: album_artist.map(|s| s.to_string()),
        index_number: index,
        container: container.map(|s| s.to_string()),
        production_year: None,
        recursive_item_count: None,
        cumulative_run_time_ticks: None,
        media_sources: None,
    }
}
```

**Example test for 300-char track name:**
```rust
#[test]
fn test_construct_file_path_long_filename_extension_preserved() {
    let long_track_name: String = "A".repeat(300);
    let managed = std::path::PathBuf::from("Music");
    let item = make_test_item(
        &long_track_name,
        Some("Artist"),
        Some("Album"),
        Some(1),
        Some("flac"),
    );
    let result = construct_file_path(&managed, &item).unwrap();

    let filename = result.path.file_name().unwrap().to_string_lossy();
    // Extension must be preserved
    assert!(filename.ends_with(".flac"), "Extension must be .flac, got: {}", filename);
    // Filename must be within component limit
    assert!(
        filename.chars().count() <= 255,
        "Filename too long: {} chars",
        filename.chars().count()
    );
    // original_name must be set since we truncated
    assert!(result.original_name.is_some(), "original_name must be set when truncated");
    assert_eq!(result.original_name.unwrap(), long_track_name);
}
```

### Project Structure Notes

**Alignment with Unified Structure:**
- `sync.rs` remains a single file (not a directory) — no structural changes
- `PathConstructionResult` added near the top of `sync.rs` with other public structs
- `truncate_component` and `truncate_filename` are private (`fn`, not `pub fn`) — internal helpers
- `original_name` field in `SyncedItem` follows established schema pattern with `#[serde(default)]`

**Detected Conflicts/Variances:**
- **Return type change**: `construct_file_path` now returns `Result<PathConstructionResult>` instead of `Result<PathBuf>`. All existing callers are in `sync.rs` itself (`execute_sync`) and in the test module. Both must be updated.
- **`SyncedItem` struct addition**: Adding `original_name` is backward-compatible — existing manifests without this field will deserialize with `None` due to `#[serde(default)]`. No migration needed.

**Files to Modify:**
1. `jellysync-daemon/src/sync.rs` — Add constant, struct, truncation helpers, update `construct_file_path` signature and body, update `execute_sync` caller, update and add tests
2. `jellysync-daemon/src/device/mod.rs` — Add `original_name` field to `SyncedItem`

**Files NOT to Modify:**
- `jellysync-daemon/src/api.rs` — No changes
- `jellysync-daemon/src/rpc.rs` — No changes
- `jellysync-daemon/Cargo.toml` — No new dependencies
- `Cargo.lock` — No new dependencies

### Critical Developer Guardrails

🚨 **MANDATORY REQUIREMENTS — DO NOT SKIP:**

1. **ALWAYS sanitize invalid chars FIRST**, then check length — order matters. `sanitize_path_component` runs before `truncate_component`.
2. **ALWAYS use `chars().count()` for length checking** — not `.len()`. `.len()` returns byte count; FAT32 limits are character count.
3. **ALWAYS preserve the file extension** — truncate the base name only. `"very_long_name.flac"` → `"very_long_na.flac"`, NOT `"very_long_name_tr"`.
4. **ALWAYS strip trailing spaces and dots** after truncation — FAT32 forbids these.
5. **ALWAYS set `original_name: None`** for items that were NOT truncated — don't set it to empty string or the same value.
6. **NEVER fail the sync** due to long names — truncation is the recovery mechanism, not an error condition.
7. **ALWAYS update both existing `construct_file_path` tests** to unpack `.path` from the result — they will break compilation otherwise.

🔥 **COMMON MISTAKES TO PREVENT:**

- ❌ Using `.len()` for length limit comparison (returns bytes, not chars) → ✅ Use `.chars().count()`
- ❌ Forgetting to update `test_construct_file_path_basic` to use `.path` → ✅ Update all callers immediately
- ❌ Truncating at byte boundary (panic risk on multi-byte UTF-8) → ✅ Use `chars().take(n).collect::<String>()`
- ❌ Truncating the file extension along with the base name → ✅ Split base and extension before truncating
- ❌ Setting `original_name` when NO truncation occurred → ✅ `original_name` is only set when truncation happened
- ❌ Adding `original_name` without `#[serde(default)]` → ✅ Must have `#[serde(default)]` or existing manifests fail to deserialize

### References

**Architecture & Planning Documents:**
- [Architecture: Enforcement Guidelines](../../_bmad-output/planning-artifacts/architecture.md#enforcement-guidelines) — "Validate filesystem path lengths before attempting write operations on legacy hardware." (direct mandate)
- [Architecture: Naming Patterns](../../_bmad-output/planning-artifacts/architecture.md#naming-patterns) — camelCase for JSON, snake_case for Rust
- [Architecture: Safety & Atomicity Patterns](../../_bmad-output/planning-artifacts/architecture.md#safety--atomicity-patterns) — Write-Temp-Rename for manifest (unchanged)
- [Epic 4 Story 4.3](../../_bmad-output/planning-artifacts/epics.md#story-43-legacy-hardware-constraints-path--char-validation) — Original story definition and AC
- [PRD: FR15](../../_bmad-output/planning-artifacts/prd.md) — "Validate hardware-specific constraints (path length, character sets) before writing files"

**Previous Story References:**
- [Story 4.2: Atomic Buffered-IO Streaming](../../_bmad-output/implementation-artifacts/4-2-atomic-buffered-io-streaming.md#file-structure--source-tree) — `construct_file_path` definition (lines 145–184 in sync.rs), `SyncedItem` structure, `execute_sync` orchestration, established testing patterns
- [Story 4.1: Differential Sync](../../_bmad-output/implementation-artifacts/4-1-differential-sync-algorithm-manifest-comparison.md) — `write_manifest` function, manifest format

**Source Code Locations:**
- [jellysync-daemon/src/sync.rs:145-184](../../jellysync-daemon/src/sync.rs#L145) — `construct_file_path` (change return type, add truncation, remove TODO comment)
- [jellysync-daemon/src/sync.rs:187-201](../../jellysync-daemon/src/sync.rs#L187) — `sanitize_path_component` (used BEFORE length truncation — do not modify)
- [jellysync-daemon/src/sync.rs:227-256](../../jellysync-daemon/src/sync.rs#L227) — `execute_sync` path construction block (update to unpack PathConstructionResult)
- [jellysync-daemon/src/sync.rs:309-334](../../jellysync-daemon/src/sync.rs#L309) — `execute_sync` SyncedItem push (add `original_name` field)
- [jellysync-daemon/src/device/mod.rs:7-19](../../jellysync-daemon/src/device/mod.rs#L7) — `SyncedItem` struct (add `original_name` field)
- [jellysync-daemon/src/sync.rs:709-758](../../jellysync-daemon/src/sync.rs#L709) — Existing `test_construct_file_path_*` tests (update to unpack `.path`)

## Dev Agent Record

### Agent Model Used

Claude Sonnet 4.6 (claude-sonnet-4-6)

### Debug Log References

None — implementation proceeded without blockers.

### Completion Notes List

- Implemented `MAX_PATH_COMPONENT_LEN = 255` constant and `PathConstructionResult` struct in `sync.rs`
- Added `truncate_component` (strips trailing spaces/dots, char-aware) and `truncate_filename` (preserves extension) private helpers
- Updated `construct_file_path` return type from `Result<PathBuf>` to `Result<PathConstructionResult>`; sanitize-then-truncate order enforced for all components
- Updated `execute_sync` to unpack `PathConstructionResult`; `original_name` propagated to `SyncedItem` on successful write
- Added `original_name: Option<String>` with `#[serde(default)]` to `SyncedItem` — backward-compatible with existing manifests
- Updated 4 `SyncedItem` constructors in test files (`sync.rs`, `device/tests.rs`, `rpc.rs`) to include `original_name: None`
- Added 8 new Story 4.3 tests; updated 2 existing `construct_file_path` tests to unpack `.path`
- All 65 tests pass, 0 warnings, 0 errors

### File List

- `jellysync-daemon/src/sync.rs`
- `jellysync-daemon/src/device/mod.rs`
- `jellysync-daemon/src/device/tests.rs`
- `jellysync-daemon/src/rpc.rs`

## Change Log

- 2026-02-21: Story 4.3 implemented — added legacy hardware path/char validation with automatic truncation, `PathConstructionResult` struct, `SyncedItem.original_name` field, and comprehensive test suite (65 tests passing)
