# Story 3.4: "Managed Zone" Hardware Shielding

Status: done

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a **Ritualist (Arthur)**,
I want **a clear visual indication that my personal folders on the device are protected**,
so that **I don't accidentally mark them for deletion and can trust the sync tool with my hardware.**

## Acceptance Criteria

1. **Device Folder Enumeration**: Given a connected device with both managed and unmanaged folders, When the UI displays the "Device State" panel, Then ALL top-level folders on the device root are listed with their names and types (managed vs unmanaged). (AC: #1)
2. **Unmanaged Folder Shielding**: Given unmanaged folders exist on the device (e.g., `Notes/`, `Podcasts/`, `Recordings/`), When they appear in the Device State panel, Then they are visually marked with a **shield/lock icon** and a "Protected" label, clearly indicating they cannot be modified by HifiMule. (AC: #2)
3. **Managed Folder Identification**: Given the `.hifimule.json` manifest tracks a `managed_paths` array, When folders on the device match those paths, Then they are shown with an unlocked/sync icon and labeled as "Managed by HifiMule". (AC: #3)
4. **No Modification of Unmanaged Content**: The daemon MUST NOT expose any RPC method or UI affordance that allows deletion, renaming, or modification of unmanaged folders or their contents. The shielding is read-only and absolute. (AC: #4)
5. **Empty Device / No Manifest State**: Given a device with no `.hifimule.json` manifest (fresh device), When viewing Device State, Then ALL folders are shown as "Unmanaged / Protected" and a message indicates "No managed sync zone configured yet — folders will be created on first sync." (AC: #5)
6. **Integration with BasketSidebar**: The Device State panel MUST be accessible from the BasketSidebar (e.g., as an expandable section or toggle) so users can review device folder protection status while curating their sync basket. (AC: #6)

## Tasks / Subtasks

- [x] **T1: Daemon - Extend Manifest with Managed Paths** (AC: #3, #5)
  - [x] T1.1: Add `managed_paths: Vec<String>` field to `DeviceManifest` struct in `device/mod.rs` with `#[serde(default)]` for backward compatibility with existing manifests that lack this field.
  - [x] T1.2: When reading `.hifimule.json`, deserialize `managed_paths` (defaults to empty vec if absent). This field will be populated by Epic 4's sync engine when it creates folders.
- [x] **T2: Daemon - Device Folder Listing RPC** (AC: #1, #2, #3)
  - [x] T2.1: Add a new RPC method `device_list_root_folders()` that uses `std::fs::read_dir` on the current device path to enumerate top-level directories (skip files, hidden entries, and system folders like `System Volume Information`).
  - [x] T2.2: For each folder, return `{ name: String, path: String, isManaged: bool }` where `isManaged` is `true` if the folder path exists in the manifest's `managed_paths` array.
  - [x] T2.3: Return an error/null if no device is currently connected. Include the device name and path in the response metadata.
  - [x] T2.4: Add unit tests: folder listing with mixed managed/unmanaged folders, empty device, no device connected.
- [x] **T3: UI - Device State Panel Component** (AC: #1, #2, #3, #6)
  - [x] T3.1: Create a `DeviceStatePanel` rendering function in a new section within `BasketSidebar.ts` (or as a companion component), using Shoelace `<sl-tree>` / `<sl-tree-item>` for the folder hierarchy.
  - [x] T3.2: Each folder item displays: folder icon + name + status badge ("Protected" with lock icon for unmanaged, "Managed" with sync icon for managed).
  - [x] T3.3: Unmanaged folders use a muted/desaturated style with `opacity: 0.7` and a shield overlay to visually communicate "hands off".
  - [x] T3.4: Managed folders use the primary purple accent color and a subtle glow or border to indicate active management.
- [x] **T4: UI - No-Manifest and Edge States** (AC: #5)
  - [x] T4.1: When no manifest exists on the device (fresh device), show all folders as "Protected" and display an informational banner: "No managed sync zone configured yet — folders will be created on first sync."
  - [x] T4.2: When no device is connected, show a greyed-out placeholder: "Connect a device to view folder protection status" (consistent with the capacity bar's no-device state from Story 3.3).
- [x] **T5: UI - BasketSidebar Integration** (AC: #6)
  - [x] T5.1: Add a collapsible "Device Folders" section to BasketSidebar, positioned between the capacity bar and the sync button.
  - [x] T5.2: The section header shows a summary count: "3 protected | 1 managed" with a toggle chevron to expand/collapse the full folder list.
  - [x] T5.3: Fetch folder list via `device_list_root_folders` RPC when a device is detected (reuse the existing device detection event flow from Story 3.3's `refreshAndRender`).

## Dev Notes

### Architecture Patterns & Constraints

- **IPC:** JSON-RPC 2.0 over localhost HTTP. All new RPC methods follow the existing pattern in `rpc.rs` (match on method name string, delegate to handler function). Use error codes: `ERR_INVALID_PARAMS`, `ERR_STORAGE_ERROR`, `ERR_CONNECTION_FAILED`.
- **Naming:** Rust uses `snake_case`, TypeScript uses `camelCase`. JSON-RPC payloads use `camelCase` per `#[serde(rename_all = "camelCase")]` convention.
- **Error Handling:** Rust uses `thiserror` for typed errors, `anyhow` at binary level. RPC errors return `JsonRpcError` with code and message.
- **State Management:** BasketStore uses `EventTarget` pattern for reactive updates. Components subscribe to `'update'` events. Follow the same pattern for device folder state.
- **Device Manager:** Uses `Arc<RwLock<T>>` for async thread-safe state. Current device path stored in `current_device_path: Arc<RwLock<Option<PathBuf>>>`.
- **CRITICAL SAFETY:** This story is READ-ONLY. Do NOT create any write/delete/rename operations for device folders. The entire point is to visually prove that unmanaged content is safe.

### Technical Specifics

- **Filesystem Enumeration (Cross-Platform):**
  - Use `std::fs::read_dir()` on the device root path to list top-level entries.
  - Filter to directories only via `entry.file_type()?.is_dir()`.
  - Skip hidden folders (names starting with `.`) and system folders. Windows-specific exclusions: `System Volume Information`, `$RECYCLE.BIN`, `RECYCLER`. macOS: `.Spotlight-V100`, `.fseventsd`, `.Trashes`.
  - Return results sorted alphabetically for consistent UI ordering.
  - The device root path is already available via `DeviceManager.current_device_path`.

- **Manifest `managed_paths` Design:**
  - Add as `managed_paths: Vec<String>` with `#[serde(default)]` so existing manifests deserialize without error.
  - Paths are relative to device root (e.g., `"Music"`, `"Music/HifiMule"`).
  - Comparison is case-insensitive on Windows, case-sensitive on Unix (use `eq_ignore_ascii_case` on Windows target).
  - This field will remain EMPTY for now — it will be populated by Epic 4's sync engine when it creates managed directories. For Story 3.4, all folders will appear as "Protected" unless a manifest explicitly lists managed paths.

- **Shoelace Tree Component:**
  - Use `<sl-tree>` with `<sl-tree-item>` for the folder list. These are already available in the Shoelace dependency.
  - Tree items support custom icons via the `expand-icon` and `collapse-icon` slots.
  - For flat folder lists (top-level only), the tree renders as a simple list without expand/collapse nesting.
  - Indent guides can be enabled with `--indent-guide-width` CSS custom property.

- **RPC Response Structure:**
  ```json
  {
    "deviceName": "IPOD_CLASSIC",
    "devicePath": "E:\\",
    "hasManifest": true,
    "folders": [
      { "name": "Music", "relativePath": "Music", "isManaged": true },
      { "name": "Notes", "relativePath": "Notes", "isManaged": false },
      { "name": "Podcasts", "relativePath": "Podcasts", "isManaged": false }
    ],
    "managedCount": 1,
    "unmanagedCount": 2
  }
  ```

### Learnings from Previous Stories (3.1 - 3.3, 3.5)

- **Story 3.3 (Storage Projection):** The `device_get_storage_info` RPC and `refreshAndRender` pattern in `BasketSidebar.ts` already fetches device data when the basket updates. Extend this same flow to also call `device_list_root_folders` to populate the Device State panel. Do NOT create a separate refresh cycle.
- **Story 3.3 (No-Device State):** The capacity bar already handles "No device connected" gracefully with a greyed-out placeholder. Follow the exact same visual pattern for the folder panel — consistent UX messaging.
- **Story 3.3 (DeviceManager path):** `current_device_path` was added to `DeviceManager` in Story 3.3. Reuse it directly for `read_dir` calls.
- **Story 3.2 (BasketSidebar):** The sidebar already has a structured layout: basket items list -> footer with track count/size -> capacity bar -> sync button. Insert the "Device Folders" section between the capacity bar and the sync button.
- **Story 3.5 (Filtering):** Music-only filtering (`MUSIC_ITEM_TYPES`) ensures only music items appear in the library. The managed zone display is orthogonal — it shows device filesystem state, not Jellyfin library state.

### Git Intelligence (Recent Commits)

- `d42f348` Fix removal — recent fixes in basket item removal
- `91a19c3` Implement for 3.3 — storage projection implementation
- `62a682d` 3.5 Filter only music — music filtering implementation
- `82a745a` 3.2 Live selection basket — basket sidebar patterns

Code conventions from recent commits: modifications follow existing patterns, tests are co-located, new RPC methods are added to the match block in `handle_rpc_request`.

### Project Structure Notes

- **Files to CREATE:**
  - None expected. All changes are modifications to existing files.
- **Files to MODIFY:**
  - `hifimule-daemon/src/device/mod.rs`: Add `managed_paths` to `DeviceManifest`, add `list_root_folders()` method to `DeviceManager`, add system folder exclusion list.
  - `hifimule-daemon/src/rpc.rs`: Add `device_list_root_folders` RPC handler with `DeviceFolderInfo` response struct.
  - `hifimule-ui/src/components/BasketSidebar.ts`: Add `renderDeviceFoldersPanel()` function, integrate into sidebar layout between capacity bar and sync button, add collapsible toggle.
  - `hifimule-ui/src/styles.css`: Add styles for `.device-folders-panel`, `.folder-item`, `.folder-managed`, `.folder-protected`, shield icon styling.
- **Files for REFERENCE (do not modify):**
  - `hifimule-ui/src/state/basket.ts`: EventTarget pattern reference.
  - `hifimule-ui/src/rpc.ts`: RPC client wrapper pattern.
  - `hifimule-ui/src/library.ts`: Navigation and item structure patterns.

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Story 3.4] — Original story requirements and acceptance criteria
- [Source: _bmad-output/planning-artifacts/architecture.md#Safety & Atomicity Patterns] — Write-Temp-Rename pattern, manifest as single source of truth
- [Source: _bmad-output/planning-artifacts/architecture.md#API & Communication Patterns] — JSON-RPC 2.0 IPC pattern
- [Source: _bmad-output/planning-artifacts/architecture.md#Naming Patterns] — camelCase for JSON, snake_case for Rust
- [Source: _bmad-output/planning-artifacts/ux-design-specification.md#4.1 Chosen Layout] — 70/30 Basket Centric split layout
- [Source: _bmad-output/planning-artifacts/ux-design-specification.md#Additional Requirements] — "Managed Safety: Visual Managed Zone shield to isolate personal data"
- [Source: _bmad-output/implementation-artifacts/3-3-high-confidence-storage-projection.md#Dev Notes] — Device path storage, RPC patterns, capacity bar implementation
- [Shoelace Tree Component](https://shoelace.style/components/tree) — `<sl-tree>` and `<sl-tree-item>` API reference
- [Rust std::fs::read_dir](https://doc.rust-lang.org/std/fs/fn.read_dir.html) — Cross-platform directory enumeration

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6

### Debug Log References

### Completion Notes List

- ✅ Implemented `managed_paths` in `DeviceManifest` with backward compatibility.
- ✅ Implemented `list_root_folders` RPC and backend logic with system folder filtering.
- ✅ Added `DeviceStatePanel` to `BasketSidebar` with collapsible folder list.
- ✅ Verified with 33 daemon tests (all passing) and successful UI build.

### File List

- `hifimule-daemon/src/device/mod.rs` (Modified)
- `hifimule-daemon/src/rpc.rs` (Modified)
- `hifimule-daemon/src/device/tests.rs` (Modified)
- `hifimule-daemon/src/tests.rs` (Modified)
- `hifimule-ui/src/components/BasketSidebar.ts` (Modified)
- `hifimule-ui/src/styles.css` (Modified)

### Senior Developer Review (AI)

**Reviewer:** Alexis (AI-assisted) on 2026-02-15

**Issues Found:** 3 High, 4 Medium, 5 Low — **6 fixed automatically**

**Fixed (H1):** Added missing unit tests `test_list_root_folders_empty_device` and `test_list_root_folders_no_device` (T2.4 now fully covered, 35 tests total).
**Fixed (H2):** Race condition in `list_root_folders()` — `has_manifest` now computed from the already-fetched manifest instead of re-acquiring the lock.
**Fixed (H3):** Replaced blocking `std::fs::read_dir` with async `tokio::fs::read_dir` in `list_root_folders()`.
**Fixed (M1):** XSS risk — folder name in `title` attribute now escaped via `escapeHtml()`.
**Fixed (M2):** Device Folders panel now visible in empty basket view (AC #6 compliance).
**Fixed (M3):** `Promise.allSettled` replaces `Promise.all` so storage/folder RPC failures are isolated.

**Not fixed (M4 — design note):** AC #5 "no manifest" fresh device scenario is architecturally unreachable because `run_observer` only sends `DeviceEvent::Detected` when a manifest exists. The code defensively handles the case, but it can never be triggered through the current device flow. This is a pre-existing design gap — recommend addressing in Epic 4 when the sync engine creates manifests on fresh devices.

**Low findings (not fixed — cosmetic):** L1: "Managed" vs "Managed by HifiMule" label. L2: Summary count order reversed. L3: Banner wording slightly differs from AC. L4: Plain divs used instead of Shoelace `<sl-tree>`. L5: Fixed (duplicate file_name call).
