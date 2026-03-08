# Story 1.2: Cross-Platform System Tray Hub

Status: done

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a Convenience Seeker (Sarah),
I want a persistent system tray icon with status indicators,
so that I can monitor the sync engine's health (Idle/Syncing/Error) without opening the main window.

## Acceptance Criteria

1. **Given** the `jellyfinsync-daemon` is running, **When** I check the system taskbar/menu bar, **Then** I see the JellyfinSync icon.
2. **And** the icon provides a "Quit" and "Open UI" menu option.
3. **And** the icon changes visually or provides tooltips to reflect the current daemon state (Idle, Syncing, Error).
4. **And** the implementation works natively on Windows, Linux, and macOS.
5. **And** the absolute idle memory footprint remains below 10MB.

## Tasks / Subtasks

- [x] Task 1: Integrate `tray-icon` crate (AC: 1, 4)
  - [x] Add `tray-icon` dependency to `jellyfinsync-daemon/Cargo.toml`.
  - [x] Implement cross-platform icon loading (embed icon assets).
- [x] Task 2: Implement Tray Menu and Actions (AC: 2)
  - [x] Create menu with "Open UI" and "Quit" options.
  - [x] Implement "Quit" action (graceful shutdown).
  - [x] Placeholder for "Open UI" (will eventually launch/show Tauri window).
- [x] Task 3: State-Aware Icon/Tooltip Updates (AC: 3, 5)
  - [x] Implement logic to update tray icon or tooltip based on daemon state.
  - [x] Verify memory usage during idle state remains < 10MB.

## Dev Notes

- **Crate Choice:** Use the `tray-icon` crate for cross-platform support.
- **Platform Requirements:**
  - **Windows:** Ensure a Win32 message loop is running (can be on a separate thread).
  - **macOS:** Tray icon must be created and the event loop must run on the **main thread**.
  - **Linux:** Requires system dependencies like `libappindicator` or `libayatana-appindicator`.
- **Memory Constraint:** Be mindful of heavy dependencies in the daemon. The `tray-icon` crate is relatively lean, but verify impact against the 10MB goal.
- **Assets:** Embed icons using `include_bytes!` to avoid external dependencies.

### Project Structure Notes

- Daemon logic remains in `jellyfinsync-daemon`.
- UI communication for "Open UI" will be handled via the existing architecture (Story 1.3).

### References

- [Epics: Story 1.2](file:///c:/Workspaces/JellyfinSync/_bmad-output/planning-artifacts/epics.md#Story%201.2:%20Cross-Platform%20System%20Tray%20Hub)
- [Architecture: Multi-Process Isolation](file:///c:/Workspaces/JellyfinSync/_bmad-output/planning-artifacts/architecture.md#Architectural%20Decisions%20Provided%20by%20Foundation)

## Dev Agent Record

### Agent Model Used

Antigravity (Gemini 2.0 Flash)

### Debug Log References

### Completion Notes List

**Code Review Fixes Applied (2026-01-31):**
- Optimized `image` crate dependency to use minimal features (PNG only) for reduced memory footprint
- Added proper error handling throughout (replaced `.expect()` and `let _` with proper error propagation)
- Fixed icon cloning inefficiency by using `Arc<Icon>` to avoid unnecessary allocations
- Implemented graceful shutdown mechanism with `AtomicBool` signal between threads
- Added basic integration tests for daemon state, icon loading, and shutdown mechanism
- Removed demo simulation code (magic numbers and state cycling)
- Committed untracked assets directory to git

### File List
- `jellyfinsync-daemon/Cargo.toml` (Modified: Added dependencies)
- `jellyfinsync-daemon/src/main.rs` (Modified: Implemented tray hub, multi-threading, error handling, graceful shutdown)
- `jellyfinsync-daemon/src/tests.rs` (New: Integration tests)
- `jellyfinsync-daemon/assets/icon.png` (New: Idle icon)
- `jellyfinsync-daemon/assets/icon_syncing.png` (New: Syncing icon)
- `jellyfinsync-daemon/assets/icon_error.png` (New: Error icon)
- `Cargo.toml` (Modified: Added workspace dependencies with optimized image crate)
- `Cargo.lock` (Modified: Dependency resolution updates)

