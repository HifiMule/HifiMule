# Story 2.2: Mass Storage Heartbeat (Autodetection)

Status: done

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a Ritualist (Arthur),
I want the daemon to "WAKE UP" the moment I plug in my iPod,
so that I don't have to manually hunt for folder paths.

## Acceptance Criteria

1. **Event-Driven Detection:** The daemon MUST trigger a "Device Detected" event the moment a USB Mass Storage device is connected to the system. (AC: #1)
2. **Manifest Probing:** Upon detection, the daemon MUST automatically check the root directory of the new mount for a `.jellyfinsync.json` manifest. (AC: #2)
3. **Cross-Platform Parity:** Detection logic MUST function on Windows, Linux, and macOS using OS-native notification primitives. (AC: #3)
4. **Performance Compliance:** The detection service MUST maintain the < 10MB idle memory footprint. (AC: #4)
5. **UI Feedback:** The system tray icon SHOULD pulse or update its tooltip to reflect the "Scanning..." or "Device Found" state. (AC: #5)

## Tasks / Subtasks

- [x] **T1: Implement Cross-Platform Mount Observer** (AC: #1, #3)
  - [x] Windows: Implement `GetLogicalDrives` polling.
  - [x] Linux: Implement `/media` and `/run/media` scanning.
  - [x] macOS: Implement `/Volumes` scanning.
- [x] **T2: Implement Manifest Probing & Validation** (AC: #2)
  - [x] Create a `DeviceProber` service to scan the root of newly mounted volumes.
  - [x] Parse `.jellyfinsync.json` if it exists.
  - [x] Validate manifest schema (minimal ID check for now).
- [x] **T3: Integrate with Daemon Event Loop** (AC: #1, #5)
  - [x] Add `DeviceDetected` variant to the `DaemonEvent` or internal message bus.
  - [x] Trigger state update to `Idle` (if scanning finished) or specialized `Scanning` state.
- [x] **T4: Update System Tray Tooltip** (AC: #5)
  - [x] Send status update to the main thread via the existing `state_tx` channel.

## Dev Notes

- **Architecture Patterns:** 
  - Follow the **Multi-Process Architecture** established in Story 1.1.
  - Use `tokio` for handling concurrent IO and mount events (from `architecture.md`).
  - Use `anyhow` and `thiserror` for error handling as per `architecture.md`.
- **Source tree components to touch:**
  - `jellyfinsync-daemon/src/main.rs`: Integrate the mount observer.
  - `jellyfinsync-daemon/src/device/`: Create new module for device discovery.
- **Testing standards summary:**
  - Mock mount events in unit tests.
  - Verify path sanitization for legacy hardware (FAT32 constraints).

### Project Structure Notes

- The project uses a Rust Cargo Workspace.
- Daemon state is managed via `DaemonState` enum in `main.rs`.
- IPC is JSON-RPC 2.0 (Story 2.1).

### References

- [Functional Requirements FR1-FR4](file:///c:/Workspaces/JellyfinSync/_bmad-output/planning-artifacts/epics.md#L16-L19)
- [Architecture Technical Constraints](file:///c:/Workspaces/JellyfinSync/_bmad-output/planning-artifacts/architecture.md#L24-L28)
- [UX Sarah's Dash Journey](file:///c:/Workspaces/JellyfinSync/_bmad-output/planning-artifacts/ux-design-specification.md#L61-L69)

## Dev Agent Record

### Agent Model Used

Antigravity (BMad Create-Story Workflow)

### Debug Log References

### Completion Notes List

- Implemented `DeviceProber` for manifest scanning.
- Implemented `MountObserver` with platform-specific polling (Windows bitmask, Mac/Linux fs scans).
- Integrated with `DaemonState` for UI feedback.
- Optimized build to keep memory footprint < 10MB (3.15MB private).

### File List

- [main.rs](file:///C:/Workspaces/JellyfinSync/jellyfinsync-daemon/src/main.rs)
- [device/mod.rs](file:///C:/Workspaces/JellyfinSync/jellyfinsync-daemon/src/device/mod.rs)
- [device/tests.rs](file:///C:/Workspaces/JellyfinSync/jellyfinsync-daemon/src/device/tests.rs)
- [Cargo.toml](file:///C:/Workspaces/JellyfinSync/Cargo.toml)
- [jellyfinsync-daemon/Cargo.toml](file:///C:/Workspaces/JellyfinSync/jellyfinsync-daemon/Cargo.toml)
