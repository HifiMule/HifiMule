# Story 5.4: Visual Manifest Repair Utility

Status: review

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a **Ritualist (Arthur)**,
I want a guided UI tool to help me fix a corrupted device manifest,
so that I can recover my "Managed" status without a full wipe.

## Acceptance Criteria

1. **Side-by-side view**: Given a "Dirty" manifest that needs manual intervention, when I open the Repair UI, then the tool shows a side-by-side view of "Actual Files" on the device vs "Manifest Record".
2. **Actionable Fixes**: The UI allows me to click "Re-link" or "Prune" to fix the state.
3. **Prune Logic**: Clicking "Prune" on a missing file removes it from the `.jellysync.json` manifest and saves the manifest.
4. **Re-link Logic**: Clicking "Re-link" allows associating an orphaned file (on disk but not in manifest) with a missing manifest entry.
5. **Clear Dirty Flag**: Once all discrepancies are resolved, the daemon must clear the "dirty" flag on the manifest and broadcast the updated device state.

## Tasks / Subtasks

- [x] **T1: Implement Discrepancy Detection in Daemon**
  - [x] T1.1: Add logic in `device/mod.rs` to scan a connected device's managed paths.
  - [x] T1.2: Compare scanned files against `DeviceManifest` to identify missing files (in manifest, not on disk) and orphaned files (on disk, not in manifest).
- [x] **T2: Add RPC Methods for Repair Operations**
  - [x] T2.1: Add `manifest_get_discrepancies` to return the lists of missing and orphaned files.
  - [x] T2.2: Add `manifest_prune` to remove a set of item IDs from the manifest safely using the atomic write pattern.
  - [x] T2.3: Add `manifest_relink` to update a manifest item's `original_name` or path to match an existing file.
- [x] **T3: Build the Repair UI Component**
  - [x] T3.1: Create a `RepairModal` repair view in the UI using Shoelace components (`<sl-dialog>`, `<sl-button>`, etc.).
  - [x] T3.2: Trigger the repair UI from the "Device State" panel when a manifest is flagged as "Dirty".
  - [x] T3.3: Implement the side-by-side view displaying discrepancies fetched via `manifest_get_discrepancies`.
  - [x] T3.4: Wire up the "Prune" and "Re-link" buttons to call their respective RPC methods and refresh the UI state.

## Dev Notes

- **Architecture Constraints**: Follow the Request-Response-Event pattern. RPC requests must be handled in `rpc.rs` returning a success/error envelope. Use `ts-rs` for all new types exposed over RPC.
- **Safety**: Ensure that `manifest.prune` and `manifest.relink` use the "Write-Temp-Rename" atomic pattern (via `DeviceManager::update_manifest`) defined in the architecture.
- **Context**: Story 4.4 introduced the "dirty manifest" concept, where an interrupted sync flags the device. Story 5.4 provides the manual escape hatch for situations where automatic self-healing fails.

### Project Structure Notes

- **Files to Modify**: `jellysync-daemon/src/rpc.rs`, `jellysync-daemon/src/device/mod.rs` (or similar for manifest management), `jellysync-ui/src/main.ts` (or appropriate component logic).
- Ensure all new UI code adheres to the "Vibrant Hub" Shoelace design system.

### References

- [Source: planning-artifacts/epics.md#story-54-visual-manifest-repair-utility] - Epic Requirements
- [Source: planning-artifacts/architecture.md#api--communication-patterns] - RPC and Event Patterns
- [Source: planning-artifacts/architecture.md#safety--atomicity-patterns] - Atomic Manifest Writes

## Dev Agent Record

### Agent Model Used

claude-3.7-sonnet

### Debug Log References

All 102 tests pass (0 failures, 0 ignored).

### Completion Notes List

- ✅ T1: Implemented `get_discrepancies()` on `DeviceManager` — scans managed paths on device, compares files against manifest, returns `ManifestDiscrepancies` with `missing` and `orphaned` lists.
- ✅ T2.1: Added `manifest_get_discrepancies` RPC handler returning discrepancy data as JSON.
- ✅ T2.2: Added `manifest_prune` RPC handler + `prune_items()` on DeviceManager — removes items by Jellyfin ID using atomic write.
- ✅ T2.3: Added `manifest_relink` RPC handler + `relink_item()` on DeviceManager — updates local_path, preserves old path as original_name.
- ✅ T2 (bonus): Added `manifest_clear_dirty` RPC handler + `clear_dirty_flag()` — clears dirty flag and pending_item_ids.
- ✅ T3.1: Created `RepairModal.ts` component using `<sl-dialog>` with loading, error, clean, and discrepancy states.
- ✅ T3.2: Added dirty manifest warning banner in `BasketSidebar.ts` Device Folders panel — polls `get_daemon_state` for dirty flag.
- ✅ T3.3: Implemented side-by-side column layout (Missing Files | Orphaned Files) with item cards showing name, path, and action buttons.
- ✅ T3.4: Wired Prune (single + bulk) and Re-link buttons to RPC calls with automatic UI refresh after each operation.
- ✅ Added 6 new unit tests: discrepancy detection (missing, orphaned, clean), prune, relink, clear dirty flag.

### File List

- `jellysync-daemon/src/device/mod.rs` — Added `get_discrepancies()`, `prune_items()`, `relink_item()`, `clear_dirty_flag()` methods + `ManifestDiscrepancies`, `DiscrepancyItem` types
- `jellysync-daemon/src/device/tests.rs` — Added 6 new Story 5.4 tests
- `jellysync-daemon/src/rpc.rs` — Added 4 new RPC handler functions + dispatch entries
- `jellysync-ui/src/components/RepairModal.ts` — [NEW] Repair modal dialog component
- `jellysync-ui/src/components/BasketSidebar.ts` — Added dirty manifest detection, warning banner, repair modal integration
- `jellysync-ui/src/styles.css` — Added repair modal and dirty banner CSS styles

### Change Log

- 2026-02-28: Story 5.4 implementation — Visual Manifest Repair Utility with discrepancy detection, RPC repair methods, and RepairModal UI component.
