# Story 2.4: Startup Splash Screen with Connection Status

Status: ready-for-dev

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a Convenience Seeker (Sarah),
I want to see a splash screen while the app is starting and connecting to my server,
so that I know the application hasn't frozen during its initialization phase.

## Acceptance Criteria

1. **Native Splash Screen:** A native Tauri splash screen featuring the JellyfinSync logo and name MUST be displayed upon launch. (AC: #1)
2. **Dynamic Status Text:** The splash screen MUST indicate the current initialization state (e.g., "Initializing Daemon...", "Connecting to Server..."). (AC: #2)
3. **Auto-Dismissal:** The splash screen MUST auto-dismiss and show the main window only after the daemon is confirmed ready and the Jellyfin server connection is verified. (AC: #3)
4. **Error Handling & Timeout:** If initialization or connection fails (including a 10-second timeout), the splash screen MUST display a clear error message with "Retry" and "Open Settings" options. (AC: #4)
5. **Main Window Lifecycle:** The main window MUST remain hidden until the splash screen logic explicitly triggers its display. (AC: #5)

## Tasks / Subtasks

- [ ] **T1: Configure Tauri Splashscreen Window** (AC: #1, #5)
  - [ ] Update `tauri.conf.json` to include a `splashscreen` window (label: `splashscreen`, visible: true).
  - [ ] Set `visible: false` for the `main` window in `tauri.conf.json`.
- [ ] **T2: Create Splashscreen Frontend** (AC: #1, #2, #4)
  - [ ] Implementation of `splashscreen.html` in the UI project.
  - [ ] Design with "Vibrant Hub" aesthetics (Glassmorphism, Jellyfin Purple).
  - [ ] Add status text container and error/action buttons (hidden by default).
- [ ] **T3: Implement Initialization Coordination** (AC: #3, #4)
  - [ ] Update `jellysync-ui/src/main.ts` to poll the daemon's `get_daemon_state` RPC method.
  - [ ] Handle transition: `appWindow.get('main').show()` and `appWindow.get('splashscreen').close()`.
  - [ ] Implement the 10-second timeout logic.
- [ ] **T4: Verification & Polish**
  - [ ] Verify splash screen dismisses correctly on successful connection.
  - [ ] Verify error state displays correctly when the daemon is offline or connection fails.

## Dev Notes

- **Architecture Patterns:**
  - Follow the **Multi-Process Architecture**. The UI acts as the coordinator for the splash screen flow.
  - Use `tauri::window::WindowBuilder` if dynamic window creation is preferred over `tauri.conf.json`.
  - Rely on the `get_daemon_state` RPC method (implemented in Story 2.3) to check registration and connection status.
- **Source tree components to touch:**
  - `jellysync-ui/src-tauri/tauri.conf.json`: Window configurations.
  - `jellysync-ui/splashscreen.html`: [NEW] The splash screen UI.
  - `jellysync-ui/src/main.ts`: Initialization logic and window management.
- **Testing standards summary:**
  - Manual verification of the startup flow on at least one platform.
  - Mock the daemon RPC response to test the timeout and error states.

### Project Structure Notes

- The splash screen should reside in the `jellysync-ui` project to maintain separation from the daemon.
- Ensure the JellyfinSync logo is correctly referenced from `assets`.

### References

- [Functional Requirements FR24](file:///c:/Workspaces/JellyfinSync/_bmad-output/planning-artifacts/epics.md#L95)
- [UX Design - Success Criteria](file:///c:/Workspaces/JellyfinSync/_bmad-output/planning-artifacts/ux-design-specification.md#L34-L35)
- [Architecture - Event System Patterns](file:///c:/Workspaces/JellyfinSync/_bmad-output/planning-artifacts/architecture.md#L121-L124)
- [Previous Story (2.3) RPC additions](file:///c:/Workspaces/JellyfinSync/_bmad-output/implementation-artifacts/2-3-multi-device-profile-mapping.md#L75)

## Dev Agent Record

### Agent Model Used

Antigravity (Workflow Engine)

### Debug Log References

### Completion Notes List

### File List
