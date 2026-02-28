# Story 5.4: Visual Manifest Repair Utility

Status: ready-for-dev

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

- [ ] **T1: Implement Discrepancy Detection in Daemon**
  - [ ] T1.1: Add logic in `device/mod.rs` or `sync.rs` to scan a connected device's managed paths.
  - [ ] T1.2: Compare scanned files against `DeviceManifest` to identify missing files (in manifest, not on disk) and orphaned files (on disk, not in manifest).
- [ ] **T2: Add RPC Methods for Repair Operations**
  - [ ] T2.1: Add `manifest.get_discrepancies` to return the lists of missing and orphaned files.
  - [ ] T2.2: Add `manifest.prune` to remove a set of item IDs from the manifest safely using the atomic write pattern.
  - [ ] T2.3: Add `manifest.relink` to update a manifest item's `original_name` or path to match an existing file.
- [ ] **T3: Build the Repair UI Component**
  - [ ] T3.1: Create a `RepairModal` or dedicated repair view in the UI using Shoelace components (`<sl-dialog>`, `<sl-button>`, etc.).
  - [ ] T3.2: Trigger the repair UI from the "Device State" panel when a manifest is flagged as "Dirty".
  - [ ] T3.3: Implement the side-by-side view displaying discrepancies fetched via `manifest.get_discrepancies`.
  - [ ] T3.4: Wire up the "Prune" and "Re-link" buttons to call their respective RPC methods and refresh the UI state.

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



### Completion Notes List



### File List
