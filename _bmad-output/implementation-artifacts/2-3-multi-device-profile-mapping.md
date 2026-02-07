# Story 2.3: Multi-Device Profile Mapping

Status: ready-for-dev

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a Convenience Seeker (Sarah),
I want the tool to remember that my Garmin watch belongs to my "Running" Jellyfin profile,
so that my sync rules are applied automatically on connection.

## Acceptance Criteria

1. **Device Identification:** The daemon MUST extract the unique `id` from the `.jellysync.json` manifest upon device detection. (AC: #1)
2. **Persistent Storage:** The daemon MUST store mappings between Device IDs and Jellyfin User Profiles in a local SQLite database. (AC: #2)
3. **Automatic Configuration Loading:** When a known device (one with an existing mapping) is connected, the daemon MUST automatically load the associated Profile ID and Sync Rules. (AC: #3)
4. **UI Notification:** The daemon MUST emit an event or update state via JSON-RPC to inform the UI that a known device has been recognized and its profile loaded. (AC: #4)
5. **Mapping Management:** Users MUST be able to create or update the mapping between a connected device and a Jellyfin profile via a JSON-RPC method. (AC: #5)

## Tasks / Subtasks

- [ ] **T1: Initialize SQLite Database** (AC: #2)
  - [ ] Implement `db` module in `jellysync-daemon`.
  - [ ] Create `devices` table: `id` (TEXT PRIMARY KEY), `name` (TEXT), `jellyfin_user_id` (TEXT), `sync_rules` (TEXT/JSON), `last_seen_at` (DATETIME).
  - [ ] Initialize the database in `main.rs` on startup.
- [ ] **T2: Implement Device Mapping Logic** (AC: #1, #3)
  - [ ] Create `DeviceManager` or update `DeviceProber` to perform database lookups after manifest detection.
  - [ ] Implement `get_device_mapping(device_id)` and `save_device_mapping(mapping)` functions.
- [ ] **T3: Update Daemon State & RPC** (AC: #4, #5)
  - [ ] Add `DeviceRecognized { device_id, profile_id }` to `DaemonState` or event stream.
  - [ ] Implement JSON-RPC method `set_device_profile(device_id, profile_id, rules)` to persist mappings.
- [ ] **T4: Integration Testing**
  - [ ] Verify that plugging in a device with a known ID triggers the "Recognized" state.
  - [ ] Verify that updating a profile via RPC persists to SQLite.

## Dev Notes

- **Architecture Patterns:** 
  - Use `rusqlite` for database operations as defined in `Cargo.toml`.
  - Follow the **Multi-Process Architecture**; ensure the database file is located in the platform-standard AppData directory (similar to `api.rs` credential logic).
  - Use `tokio::task::spawn_blocking` for SQLite operations to avoid blocking the async executor.
- **Source tree components to touch:**
  - `jellysync-daemon/src/db.rs`: [NEW] Database schema and operations.
  - `jellysync-daemon/src/main.rs`: Database initialization and lifecycle.
  - `jellysync-daemon/src/device/mod.rs`: Integration of lookups after probing.
  - `jellysync-daemon/src/rpc.rs`: New methods for mapping management.
- **Testing standards summary:**
  - Use `tempfile` for database tests.
  - Mock Jellyfin user IDs for testing the mapping logic.

### Project Structure Notes

- Credentials are currently in `credentials.json` (via `api.rs`). Consider if these should eventually move to SQLite, but for this story, focus on the `devices` mapping.
- The `DaemonState` enum in `main.rs` should be expanded to handle the "Known Device" vs "Unknown Device" distinction.

### References

- [Functional Requirements FR4, FR6](file:///c:/Workspaces/JellyfinSync/_bmad-output/planning-artifacts/epics.md#L19-L21)
- [Architecture Persistence Decisions](file:///c:/Workspaces/JellyfinSync/_bmad-output/planning-artifacts/architecture.md#L67-L71)
- [UX Design - Predictive Syncing](file:///c:/Workspaces/JellyfinSync/_bmad-output/planning-artifacts/ux-design-specification.md#L34)

## Dev Agent Record

### Agent Model Used

Antigravity (BMad Create-Story Workflow)

### Debug Log References

### Completion Notes List

### File List
