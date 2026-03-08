# Story 1.3: Detachable Tauri UI Skeleton

Status: done

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a Ritualist (Arthur),
I want a detachable window that can be opened and closed from the tray without killing the sync engine,
so that I can browse my library while the background sync remains active.

## Acceptance Criteria

1. **Given** the daemon is active in the tray, **When** I click "Open UI", **Then** a Tauri window appears using the "Vibrant Hub" Shoelace foundation.
2. **When** I close the window, **Then** the daemon remains running in the tray.
3. **And** the UI layout follows the "Basket Centric" (70/30 split view) requirement.
4. **And** the aesthetics match the "Vibrant Hub" definition (Dark mode, modern typography, glassmorphism hints).

## Tasks / Subtasks

- [x] Task 1: UI Launch Mechanism (AC: 1, 2)
  - [x] Implement `Open UI` action in `jellyfinsync-daemon` using `std::process::Command`.
  - [x] Ensure the daemon correctly handles the spawn event across platforms.
- [x] Task 2: Shoelace "Vibrant Hub" Skeleton (AC: 1, 3, 4)
  - [x] Clean up default Tauri boilerplate in `jellyfinsync-ui`.
  - [x] Implement the 70/30 split layout using Shoelace components.
  - [x] Set up the "Basket" sidebar and "Library" main view placeholders.
- [x] Task 3: Lifecycle and Verification (AC: 2)
  - [x] Verify window closure does not kill the daemon.
  - [x] Verify the tray icon remains responsive after UI is closed.

## Dev Notes

### Architecture Guardrails
- **Multi-Process Isolation:** The daemon and UI are separate workspace members. The daemon's 10MB idle goal depends on this separation.
- **macOS Main Thread:** The daemon's tray icon MUST run on the main thread.
- **IPC Pattern:** While this story focuses on window management, any future communication should use JSON-RPC over localhost HTTP as per [Architecture Decision Document](file:///c:/Workspaces/JellyfinSync/_bmad-output/planning-artifacts/architecture.md).

### UI / UX Requirements
- **Framework:** Tauri v2 + Vanilla TypeScript.
- **Components:** Shoelace (integrated via CDN or local assets).
- **Layout:** "Basket Centric" (70/30 split view).
- **Styling:** Vibrant Hub aesthetics (Dark mode, glassmorphism).

### References

- [Epics: Story 1.3](file:///c:/Workspaces/JellyfinSync/_bmad-output/planning-artifacts/epics.md#Story%201.3:%20Detachable%20Tauri%20UI%20Skeleton)
- [Architecture: Multi-Process Isolation](file:///c:/Workspaces/JellyfinSync/_bmad-output/planning-artifacts/architecture.md#Starter%20Options%20Considered)
- [Story 1.2: Tray Hub Context](file:///c:/Workspaces/JellyfinSync/_bmad-output/implementation-artifacts/1-2-cross-platform-system-tray-hub.md)

## Dev Agent Record

### Agent Model Used

Antigravity (Gemini 2.0 Flash)

### Debug Log References

### Completion Notes List
- [Code Review Fix] Added Google Fonts 'Inter' to index.html.
- [Code Review Fix] Added unit tests for DaemonState and icon loading in `tests.rs`.
- [Code Review Fix] Refactored `load_icon` for testability.
- Implemented `Open UI` in `jellyfinsync-daemon` using `std::process::Command`.
- The UI is launched via `npm run tauri dev` for development.
- Implemented "Vibrant Hub" skeleton in `jellyfinsync-ui` using Shoelace.
- Established 70/30 split layout (Library/Basket).
- Applied dark theme and glassmorphism styling.

### File List
- `jellyfinsync-daemon/src/main.rs` (Modified)
- `jellyfinsync-ui/index.html` (Modified)
- `jellyfinsync-ui/package.json` (Modified)
- `jellyfinsync-ui/src/main.ts` (Modified)
- `jellyfinsync-ui/src/styles.css` (Modified)
