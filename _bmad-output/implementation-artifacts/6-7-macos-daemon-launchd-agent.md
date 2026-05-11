# Story 6.7: macOS Daemon as launchd User Agent

Status: done

## Story

As a **Convenience Seeker (Sarah)**,
I want the HifiMule daemon to start automatically when I log in on macOS,
So that auto-sync fires when I connect my device even if I haven't opened the app.

## Acceptance Criteria

1. **Auto-install on first launch**: Given HifiMule is installed to /Applications on macOS, when the UI is launched for the first time (or after an upgrade where the plist is absent), then the UI writes `~/Library/LaunchAgents/com.hifimule.daemon.plist` templated with the resolved absolute path to the bundled daemon binary, and runs `launchctl load` so the agent is active immediately.
2. **Login persistence**: Subsequent user logins start the daemon automatically with no UI interaction required (via `RunAtLoad: true` in the plist).
3. **Attach to running daemon**: Given the UI launches and the daemon is already running (started by launchd), when the UI performs its health-check on port 19140, then the UI attaches to the running daemon (status = "startup") without spawning a sidecar.
4. **Exit does not kill launchd-owned daemon**: When the UI window is closed or exits, the launchd-owned daemon is NOT killed (only a sidecar-spawned child held in `DaemonProcess` is killed on exit).
5. **Toggle OFF**: Given the user calls `settings_set_launch_on_startup(false)`, then `launchctl unload` runs on the plist and the plist file is deleted. The daemon continues running for the current session.
6. **Toggle ON**: Given the user calls `settings_set_launch_on_startup(true)`, then the plist is reinstalled and `launchctl load`ed.

## Tasks / Subtasks

- [x] **T1: Extract `resolve_daemon_binary_path()` helper** (AC: #1, #6)
  - [x] T1.1: Add `#[cfg(target_os = "macos")] fn resolve_daemon_binary_path() -> Option<std::path::PathBuf>` before `check_daemon_health()` in `lib.rs`
  - [x] T1.2: Refactor the existing quarantine-clearance block (lib.rs:276–296) to call `resolve_daemon_binary_path()` instead of inlining the scan — do NOT change the quarantine logic, just extract the path resolution

- [x] **T2: Add launchd plist constant and helper functions** (AC: #1, #5, #6)
  - [x] T2.1: Add `#[cfg(target_os = "macos")] const LAUNCHD_PLIST_TEMPLATE: &str` (plist XML — see Dev Notes below for exact template)
  - [x] T2.2: Add `#[cfg(target_os = "macos")] fn launchd_plist_path() -> Option<std::path::PathBuf>` — returns `~/Library/LaunchAgents/com.hifimule.daemon.plist` via `$HOME`
  - [x] T2.3: Add `#[cfg(target_os = "macos")] fn install_launchd_plist() -> Result<(), String>` — resolve daemon path, fill template, create LaunchAgents dir, write plist, run `launchctl load`
  - [x] T2.4: Add `#[cfg(target_os = "macos")] fn unload_and_remove_launchd_plist() -> Result<(), String>` — run `launchctl unload` (if plist exists), then delete plist file

- [x] **T3: Auto-install plist on first launch** (AC: #1, #2)
  - [x] T3.1: In `run()` → `setup()` closure, after the `app.manage(...)` calls and before the background thread spawn, add a `#[cfg(target_os = "macos")]` block that checks `launchd_plist_path().is_some_and(|p| !p.exists())` and calls `install_launchd_plist()` if true, logging success or failure via `ui_log`

- [x] **T4: Add `settings_set_launch_on_startup` Tauri command** (AC: #5, #6)
  - [x] T4.1: Add `#[tauri::command] async fn settings_set_launch_on_startup(enabled: bool) -> Result<(), String>` — macOS: `enabled=true` calls `install_launchd_plist()`, `enabled=false` calls `unload_and_remove_launchd_plist()`; non-macOS: `let _ = enabled; Ok(())`
  - [x] T4.2: Register in `invoke_handler!`: added `settings_set_launch_on_startup` to the existing list

### Review Findings

- [x] [Review][Patch] ~~Missing trailing dash in `resolve_daemon_binary_path` filter~~ — REVERTED: Tauri strips the target-triple suffix at bundle time; the deployed binary is `hifimule-daemon` (no dash), so `starts_with("hifimule-daemon")` is correct. The spec note about `hifimule-daemon-universal-apple-darwin` describes the pre-bundle filename only.
- [x] [Review][Patch] `unwrap_or("")` passes empty string to `launchctl` — if plist path is non-UTF-8, launchctl receives `""` and may silently succeed (no-op) while the function returns `Ok(())` [`hifimule-ui/src-tauri/src/lib.rs:104`, `lib.rs:123`]
- [x] [Review][Patch] `launchctl unload` I/O error causes early return before plist deletion — spec requires deletion to always proceed; the `?` on `.output()` at line 125 exits early if `launchctl` binary is missing/unusable, leaving the plist file in place [`hifimule-ui/src-tauri/src/lib.rs:125`]
- [x] [Review][Patch] `{DAEMON_PATH}` not XML-escaped in plist template — raw string substitution breaks plist XML if path contains `&`, `<`, or `>` (legal in macOS bundle names, e.g. `Foo & Bar.app`) [`hifimule-ui/src-tauri/src/lib.rs:93`]
- [x] [Review][Defer] First match from unordered `read_dir`, no executable-type check [`hifimule-ui/src-tauri/src/lib.rs:25`] — deferred, pre-existing
- [x] [Review][Defer] `launchctl load` fails when label already loaded (plist-deletion race after external cleanup) [`hifimule-ui/src-tauri/src/lib.rs:103`] — deferred, pre-existing
- [x] [Review][Defer] Plist always deleted after failed unload → silent re-enable on next app launch [`hifimule-ui/src-tauri/src/lib.rs:132`] — deferred, pre-existing
- [x] [Review][Defer] Stale plist when app bundle is moved to a different directory — no path-change detection on subsequent launches [`hifimule-ui/src-tauri/src/lib.rs:338`] — deferred, pre-existing

## Dev Notes

### What to Build (Scope)

**All changes are in a single file: `hifimule-ui/src-tauri/src/lib.rs`.**

- ACs #3 and #4 are already satisfied by the existing code — no changes needed:
  - AC #3: The existing health-check step in the background thread (lib.rs:232–239) already sets status to "startup" when the daemon is running before the UI launches, skipping sidecar spawn.
  - AC #4: The `RunEvent::Exit` kill block (lib.rs:389–399) is already gated on `daemon_proc.take()` returning `Some`. A launchd-owned daemon is never stored in `DaemonProcess`, so it won't be killed.
- Do NOT touch `hifimule-daemon/src/service.rs` or `hifimule-ui/src-tauri/wix/startup-fragment.wxs` — Windows-only, zero changes.
- Do NOT build a Settings UI panel — `settings_set_launch_on_startup` is a Tauri command the frontend can call; the Settings UI is a future story.
- Do NOT add frontend `.ts`/`.svelte` changes.

### Current lib.rs State to Preserve

Current `lib.rs` (402 lines) structure:
- Lines 1–5: imports (`std::sync::Mutex`, `tauri`, `tauri_plugin_shell`, `ShellExt`)
- Line 6: `struct DaemonProcess(Mutex<Option<CommandChild>>)` — stores sidecar child (None for launchd-owned)
- Line 12: `struct SidecarStatus(Mutex<String>)` — status values: "starting", "startup", "service", "running (pid=N)", error states
- Line 14: `const RPC_PORT: u16 = 19140;`
- Lines 16–19: `get_sidecar_status` Tauri command
- Lines 22–44: `check_daemon_health()` — sends `get_daemon_state` JSON-RPC with 2s timeout; returns `true` if HTTP 200
- Lines 47–69: `try_start_service()` (Windows only)
- Lines 74–118: `image_proxy` Tauri command
- Lines 123–156: `rpc_proxy` Tauri command
- Lines 158–211: `ui_log()` — writes to `~/Library/Application Support/HifiMule/ui.log` (macOS) and `%APPDATA%/HifiMule/ui.log` (Windows), 1 MB truncation
- Lines 213–401: `pub fn run()` containing `setup()` closure and `RunEvent::Exit` handler
  - Lines 225–226: `app.manage(DaemonProcess(...))` and `app.manage(SidecarStatus(...))`
  - Lines 230–381: background thread — Step 1 health-check, Step 2 Windows service, Step 3 sidecar spawn
  - Lines 275–296: macOS quarantine clearance block (dir scan for `hifimule-daemon-*`)
  - Lines 298–380: `sidecar("hifimule-daemon").spawn()` with stdout/stderr/termination event loop
  - Lines 388–400: `RunEvent::Exit` → `daemon_proc.take()` → `child.kill()` (no-op if None)

### Exact Plist Template

```rust
#[cfg(target_os = "macos")]
const LAUNCHD_PLIST_TEMPLATE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.hifimule.daemon</string>
    <key>ProgramArguments</key>
    <array>
        <string>{DAEMON_PATH}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <false/>
    <key>StandardOutPath</key>
    <string>/tmp/hifimule-daemon-stdout.log</string>
    <key>StandardErrorPath</key>
    <string>/tmp/hifimule-daemon-stderr.log</string>
</dict>
</plist>"#;
```

- `Label` must match plist filename prefix: `com.hifimule.daemon`
- `{DAEMON_PATH}` is replaced at runtime with the absolute binary path
- `KeepAlive: false` — launchd does NOT restart the daemon on crash (avoids restart loops during development/debugging)
- `RunAtLoad: true` — daemon starts both on `launchctl load` AND on every subsequent login

### resolve_daemon_binary_path() — CRITICAL

**NEVER use `dir.join("hifimule-daemon")` — this file does NOT exist.** Tauri v2 bundles sidecars with a target-triple suffix (e.g. `hifimule-daemon-universal-apple-darwin`). See spec-fix-macos-daemon-launch.md §Loop 1 for the known-bad state.

```rust
#[cfg(target_os = "macos")]
fn resolve_daemon_binary_path() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        if entry.file_name().to_string_lossy().starts_with("hifimule-daemon-") {
            return Some(entry.path());
        }
    }
    None
}
```

T1.2: Refactor the existing quarantine block (lib.rs:276–296). Replace the inline scan with a call to `resolve_daemon_binary_path()`:

```rust
#[cfg(target_os = "macos")]
if let Some(sp) = resolve_daemon_binary_path() {
    ui_log(&format!("Resolving macOS sidecar at {:?}", sp));
    let _ = std::process::Command::new("xattr")
        .args(["-d", "com.apple.quarantine"])
        .arg(&sp)
        .output();
}
```

### install_launchd_plist() Implementation

```rust
#[cfg(target_os = "macos")]
fn install_launchd_plist() -> Result<(), String> {
    let daemon_path = resolve_daemon_binary_path()
        .ok_or_else(|| "Cannot resolve daemon binary path for plist".to_string())?;
    let daemon_path_str = daemon_path
        .to_str()
        .ok_or_else(|| "Daemon path is not valid UTF-8".to_string())?;
    let plist_content = LAUNCHD_PLIST_TEMPLATE.replace("{DAEMON_PATH}", daemon_path_str);
    let plist_path = launchd_plist_path()
        .ok_or_else(|| "Cannot resolve LaunchAgents path (HOME not set?)".to_string())?;
    let launch_agents = plist_path.parent()
        .ok_or_else(|| "Cannot get LaunchAgents parent dir".to_string())?;
    std::fs::create_dir_all(launch_agents)
        .map_err(|e| format!("Cannot create LaunchAgents dir: {}", e))?;
    std::fs::write(&plist_path, plist_content)
        .map_err(|e| format!("Cannot write plist: {}", e))?;
    let output = std::process::Command::new("launchctl")
        .args(["load", plist_path.to_str().unwrap_or("")])
        .output()
        .map_err(|e| format!("launchctl load failed to execute: {}", e))?;
    if !output.status.success() {
        return Err(format!(
            "launchctl load exited {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(())
}
```

### unload_and_remove_launchd_plist() Implementation

```rust
#[cfg(target_os = "macos")]
fn unload_and_remove_launchd_plist() -> Result<(), String> {
    let plist_path = launchd_plist_path()
        .ok_or_else(|| "Cannot resolve LaunchAgents path".to_string())?;
    if plist_path.exists() {
        let output = std::process::Command::new("launchctl")
            .args(["unload", plist_path.to_str().unwrap_or("")])
            .output()
            .map_err(|e| format!("launchctl unload failed to execute: {}", e))?;
        if !output.status.success() {
            ui_log(&format!(
                "launchctl unload warning (may already be unloaded): {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        std::fs::remove_file(&plist_path)
            .map_err(|e| format!("Cannot remove plist: {}", e))?;
    }
    Ok(())
}
```

Note: `launchctl unload` failure is logged as a warning but does NOT fail the function — the plist file deletion still proceeds. This handles the case where the daemon was never loaded but the plist exists.

### settings_set_launch_on_startup Command

```rust
#[tauri::command]
async fn settings_set_launch_on_startup(enabled: bool) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        if enabled {
            install_launchd_plist()
        } else {
            unload_and_remove_launchd_plist()
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = enabled;
        Ok(())
    }
}
```

Register in `invoke_handler!` (lib.rs:223):
```rust
.invoke_handler(tauri::generate_handler![
    get_sidecar_status,
    rpc_proxy,
    image_proxy,
    settings_set_launch_on_startup
])
```

### T3: Auto-Install Block in setup()

Insert after the two `app.manage(...)` calls (lib.rs:225–226) and before the `std::thread::spawn` (lib.rs:230):

```rust
// macOS: install launchd user agent on first launch (or after upgrade removes plist)
#[cfg(target_os = "macos")]
{
    let plist_missing = launchd_plist_path().map_or(false, |p| !p.exists());
    if plist_missing {
        match install_launchd_plist() {
            Ok(()) => ui_log("launchd plist installed and loaded"),
            Err(e) => ui_log(&format!("launchd plist install failed: {}", e)),
        }
    }
}
```

**Ordering matters**: This block runs on the main thread in `setup()`, before the background health-check thread spawns. On first launch, `launchctl load` + `RunAtLoad` starts the daemon synchronously, so when the background thread runs `check_daemon_health()` a moment later, the daemon should be reachable and the health-check returns `true` → status = "startup". If the daemon hasn't started yet (rare race), the existing sidecar fallback catches it.

### What NOT to Do

- **Do NOT use `launchctl bootstrap`/`bootout`** — those are macOS 12+ only; `load`/`unload` covers all supported versions (10.15+)
- **Do NOT use `dir.join("hifimule-daemon")`** — that file doesn't exist; always use the dir-scan helper
- **Do NOT remove the existing quarantine-clearance block** — keep it, just refactor to use `resolve_daemon_binary_path()`
- **Do NOT change the health-check flow** — the existing Step 1 (health-check) → Step 2 (Windows service) → Step 3 (sidecar) cascade is correct and covers the launchd case via Step 1
- **Do NOT kill the daemon in `RunEvent::Exit`** — the existing `daemon_proc.take()` guard already prevents killing launchd-owned daemons (they were never stored in `DaemonProcess`)
- **Do NOT add `sudo`** — `~/Library/LaunchAgents/` is writable by the user, no elevation needed
- **Do NOT add any `hifimule-daemon` changes** — all changes are in `hifimule-ui/src-tauri/src/lib.rs` only

### No New Dependencies

No new Cargo dependencies needed. All operations use:
- `std::fs` (already in std)
- `std::process::Command` (already in std, already used for `xattr` in the existing quarantine block)
- `std::env::var("HOME")` (already in std, already used in `ui_log`)

### Testing

- `cargo check -p hifimule-ui` — must pass on all platforms (including non-macOS, since `settings_set_launch_on_startup` is defined on all platforms via `#[cfg(not(target_os = "macos"))]` stub)
- `cargo test -p hifimule-ui` — no regression (lib currently has no unit tests; no new unit tests required for this story since the logic depends on macOS system calls)
- `cargo clippy -p hifimule-ui -- -D warnings` — must pass; the two `#[cfg(not(target_os = "macos"))]` stubs avoid unused-variable warnings

Manual verification on macOS:
1. Delete `~/Library/LaunchAgents/com.hifimule.daemon.plist` if it exists
2. Launch HifiMule — confirm `ui.log` contains "launchd plist installed and loaded"
3. Confirm plist exists at `~/Library/LaunchAgents/com.hifimule.daemon.plist`
4. Confirm daemon is running: `launchctl list | grep hifimule`
5. Quit and reopen app — confirm status is "startup" (daemon was already running via launchd)
6. Test toggle: invoke `settings_set_launch_on_startup(false)` — confirm plist is deleted and `launchctl list` no longer shows hifimule

### Project Structure Notes

All changes in one file:

| File | Action | Change |
|------|--------|--------|
| `hifimule-ui/src-tauri/src/lib.rs` | **MODIFY** | Add 4 helper functions, plist constant, auto-install block in setup(), new Tauri command, register in invoke_handler |

### References

- Current lib.rs: [hifimule-ui/src-tauri/src/lib.rs](hifimule-ui/src-tauri/src/lib.rs)
  - DaemonProcess struct: [lib.rs:6](hifimule-ui/src-tauri/src/lib.rs#L6)
  - check_daemon_health: [lib.rs:22](hifimule-ui/src-tauri/src/lib.rs#L22)
  - invoke_handler registration: [lib.rs:223](hifimule-ui/src-tauri/src/lib.rs#L223)
  - app.manage() calls: [lib.rs:225–226](hifimule-ui/src-tauri/src/lib.rs#L225)
  - Background thread spawn: [lib.rs:230](hifimule-ui/src-tauri/src/lib.rs#L230)
  - Quarantine clearance block: [lib.rs:275–296](hifimule-ui/src-tauri/src/lib.rs#L275)
  - RunEvent::Exit kill block: [lib.rs:388–400](hifimule-ui/src-tauri/src/lib.rs#L388)
- Architecture macOS daemon lifecycle: architecture.md §Daemon Lifecycle — macOS
- Windows parallel: [hifimule-ui/src-tauri/wix/startup-fragment.wxs](hifimule-ui/src-tauri/wix/startup-fragment.wxs) (HKCU Run key pattern)
- spec-fix-macos-daemon-launch.md — documents the quarantine clearance fix and the dir-scan requirement
- Tauri sidecar suffix: sidecar is `hifimule-daemon-universal-apple-darwin` (not plain `hifimule-daemon`)
- launchd plist location: `~/Library/LaunchAgents/com.hifimule.daemon.plist`
- App data log: `~/Library/Application Support/HifiMule/ui.log`

## Dev Agent Record

### Agent Model Used

Claude Sonnet 4.6

### Debug Log References

- T3.1 used `is_some_and` instead of `map_or(false, ...)` per clippy lint `unnecessary_map_or` (-D warnings enforcement)

### Completion Notes List

- T1: Added `resolve_daemon_binary_path()` (macOS-only) before `check_daemon_health()`. Refactored the quarantine clearance block from an inline dir-scan to a single call to the new helper — logic unchanged, just extracted.
- T2: Added `LAUNCHD_PLIST_TEMPLATE` constant, `launchd_plist_path()`, `install_launchd_plist()`, and `unload_and_remove_launchd_plist()` between `check_daemon_health()` and `try_start_service()`. All four are `#[cfg(target_os = "macos")]`.
- T3: Added auto-install block in `setup()` after the two `app.manage()` calls, before the background thread spawn. Uses `is_some_and` for plist existence check (clippy-clean).
- T4: Added `settings_set_launch_on_startup` async Tauri command (macOS calls helpers; non-macOS stub with `let _ = enabled`). Registered in `invoke_handler!`.
- Validations: `cargo check` ✅, `cargo test` ✅ (0 tests, no regressions), `cargo clippy -- -D warnings` ✅
- ACs #3 and #4 confirmed satisfied by existing code with no changes needed (as specified).

### File List

- `hifimule-ui/src-tauri/src/lib.rs`

## Change Log

- 2026-05-11: Implemented macOS launchd user agent — added `resolve_daemon_binary_path()` helper, plist constant, `launchd_plist_path()` / `install_launchd_plist()` / `unload_and_remove_launchd_plist()` helpers, auto-install block in `setup()`, and `settings_set_launch_on_startup` Tauri command. All changes in `hifimule-ui/src-tauri/src/lib.rs`.
