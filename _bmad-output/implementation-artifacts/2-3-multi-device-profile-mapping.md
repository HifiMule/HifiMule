# Story 2.3: Multi-Device Profile Mapping

Status: done

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a Convenience Seeker (Sarah),
I want the tool to remember that my Garmin watch belongs to my "Running" Jellyfin profile,
so that my sync rules are applied automatically on connection.

## Acceptance Criteria

1. **Device Identification:** The daemon MUST extract the unique `id` from the `.hifimule.json` manifest upon device detection. (AC: #1)
2. **Persistent Storage:** The daemon MUST store mappings between Device IDs and Jellyfin User Profiles in a local SQLite database. (AC: #2)
3. **Automatic Configuration Loading:** When a known device (one with an existing mapping) is connected, the daemon MUST automatically load the associated Profile ID and Sync Rules. (AC: #3)
4. **UI Notification:** The daemon MUST emit an event or update state via JSON-RPC to inform the UI that a known device has been recognized and its profile loaded. (AC: #4)
5. **Mapping Management:** Users MUST be able to create or update the mapping between a connected device and a Jellyfin profile via a JSON-RPC method. (AC: #5)

## Tasks / Subtasks

- [x] **T1: Initialize SQLite Database** (AC: #2)
  - [x] Implement `db` module in `hifimule-daemon`.
  - [x] Create `devices` table: `id` (TEXT PRIMARY KEY), `name` (TEXT), `jellyfin_user_id` (TEXT), `sync_rules` (TEXT/JSON), `last_seen_at` (DATETIME).
  - [x] Initialize the database in `main.rs` on startup.
- [x] **T2: Implement Device Mapping Logic** (AC: #1, #3)
  - [x] Create `DeviceManager` or update `DeviceProber` to perform database lookups after manifest detection.
  - [x] Implement `get_device_mapping(device_id)` and `save_device_mapping(mapping)` functions.
- [x] **T3: Update Daemon State & RPC** (AC: #4, #5)
  - [x] Add `DeviceRecognized { device_id, profile_id }` to `DaemonState` or event stream.
  - [x] Implement JSON-RPC method `set_device_profile(device_id, profile_id, rules)` to persist mappings.
- [x] **T4: Integration Testing**
  - [x] Verify that plugging in a device with a known ID triggers the "Recognized" state.
  - [x] Verify that updating a profile via RPC persists to SQLite.

## Dev Notes

- **Architecture Patterns:** 
  - Use `rusqlite` for database operations as defined in `Cargo.toml`.
  - Follow the **Multi-Process Architecture**; ensure the database file is located in the platform-standard AppData directory (similar to `api.rs` credential logic).
  - Use `tokio::task::spawn_blocking` for SQLite operations to avoid blocking the async executor.
- **Source tree components to touch:**
  - `hifimule-daemon/src/db.rs`: Database schema and operations.
  - `hifimule-daemon/src/main.rs`: Database initialization and lifecycle.
  - `hifimule-daemon/src/device/mod.rs`: Integration of lookups after probing.
  - `hifimule-daemon/src/rpc.rs`: New methods for mapping management.
- **Testing standards summary:**
  - Use `tempfile` for database tests.
  - Mock Jellyfin user IDs for testing the mapping logic.

### Project Structure Notes

- Credentials are currently in `credentials.json` (via `api.rs`). Consider if these should eventually move to SQLite, but for this story, focus on the `devices` mapping.
- The `DaemonState` enum in `main.rs` should be expanded to handle the "Known Device" vs "Unknown Device" distinction.

### References

- [Functional Requirements FR4, FR6](file:///c:/Workspaces/HifiMule/_bmad-output/planning-artifacts/epics.md#L19-L21)
- [Architecture Persistence Decisions](file:///c:/Workspaces/HifiMule/_bmad-output/planning-artifacts/architecture.md#L67-L71)
- [UX Design - Predictive Syncing](file:///c:/Workspaces/HifiMule/_bmad-output/planning-artifacts/ux-design-specification.md#L34)

## Dev Agent Record

### Agent Model Used

Antigravity (Adversarial Code Fix)

### Debug Log References

### Completion Notes List
- Successfully implemented SQLite database integration using `rusqlite`.
- Created shared `paths` utility for platform-standard AppData directory management.
- Developed `DeviceManager` to centralize device state management and database lookups.
- Implemented `get_daemon_state` JSON-RPC method to allow UI polling of daemon state.
- Transitioned Jellyfin token storage from plaintext JSON to OS-native `keyring`.
- Fixed fragile time-based state resets in `main.rs` event loop.
- Added comprehensive integration tests in `src/tests.rs` verifying the full detection-to-recognition flow.

### File List
- `hifimule-daemon/src/db.rs` [MODIFIED] (Added Serialize/Deserialize)
- `hifimule-daemon/src/paths.rs` [MODIFIED] (Added Result error handling)
- `hifimule-daemon/src/main.rs` [MODIFIED] (Refactored for DeviceManager)
- `hifimule-daemon/src/rpc.rs` [MODIFIED] (Added get_daemon_state, fixed types)
- `hifimule-daemon/src/api.rs` [MODIFIED] (Implemented keyring storage)
- `hifimule-daemon/src/device/mod.rs` [MODIFIED] (Added DeviceManager)
- `hifimule-daemon/Cargo.toml` [MODIFIED] (Added keyring dependency)
- `hifimule-daemon/src/tests.rs` [MODIFIED] (Added integration test)
