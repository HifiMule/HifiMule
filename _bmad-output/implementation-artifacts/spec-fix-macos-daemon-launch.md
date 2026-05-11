---
title: 'Fix macOS daemon launch when installed'
type: 'bugfix'
created: '2026-05-11'
status: 'done'
context: []
baseline_commit: '32564f186908ee5af48c7fdaa6705864e4f1e9ea'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** When HifiMule is installed on macOS (via DMG), the UI fails to launch the daemon sidecar. On macOS Sonoma+, Gatekeeper bypass only clears quarantine on the top-level bundle directory, leaving `Contents/MacOS/hifimule-daemon` with `com.apple.quarantine`, which silently blocks programmatic spawning. Additionally, `ui_log` only writes to `APPDATA` (Windows-only), so all spawn errors on macOS are invisible.

**Approach:** Before spawning the sidecar on macOS, resolve its path via `current_exe()` and strip any quarantine attribute with `xattr -d com.apple.quarantine`. Simultaneously, add macOS file logging to `ui_log` so future failures surface diagnostics in `~/Library/Application Support/HifiMule/ui.log`.

## Boundaries & Constraints

**Always:**
- `#[cfg(target_os = "macos")]` gates all macOS-specific code — no Windows/Linux behaviour changes.
- The quarantine-clearance attempt is best-effort: errors from `xattr` are silently ignored (the binary may not have quarantine, which is fine).
- The sidecar path is resolved from `std::env::current_exe()` parent dir → join `"hifimule-daemon"` (mirrors Tauri shell plugin's own resolution at runtime).
- macOS log path must match the daemon's own log dir: `~/Library/Application Support/HifiMule/ui.log`.

**Ask First:**
- If the root cause turns out to be something other than quarantine (e.g. missing libmtp at the bundled path), halt and report the finding from the new logs before attempting a second fix.

**Never:**
- Do not use `sudo` or request elevated privileges.
- Do not change the sidecar spawn logic for Windows or Linux.
- Do not remove or bypass the existing health-check and service-start steps.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Normal macOS install | Sidecar has `com.apple.quarantine` | `xattr -d` removes it, spawn succeeds, daemon starts | If xattr fails, spawn is still attempted |
| Already cleared quarantine | Sidecar has no quarantine attribute | `xattr -d` exits with error (ignored), spawn proceeds normally | No change in behavior |
| Sidecar binary missing | `current_exe()` parent dir has no `hifimule-daemon` | Path logged, xattr skipped, Tauri `sidecar()` returns `Err`, status set to `command_failed: ...` | Error logged to `ui.log` |
| macOS log dir unwritable | `~/Library/Application Support/HifiMule/` not creatable | Log write silently ignored, daemon spawn unaffected | Graceful skip |

</frozen-after-approval>

## Code Map

- `hifimule-ui/src-tauri/src/lib.rs` -- sole change target: `ui_log` function (add macOS log path) and sidecar spawn thread (add macOS quarantine clearance before `sidecar().spawn()`)

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-ui/src-tauri/src/lib.rs` -- In `ui_log`: add a `#[cfg(target_os = "macos")]` branch that writes to `~/Library/Application Support/HifiMule/ui.log` using the same pattern as the Windows branch (create dir, truncate at 1 MB, timestamped append)
- [x] `hifimule-ui/src-tauri/src/lib.rs` -- In the sidecar spawn thread, just before `app_handle.shell().sidecar("hifimule-daemon")`: add a `#[cfg(target_os = "macos")]` block that calls `std::env::current_exe()`, reads the parent directory with `std::fs::read_dir`, finds the first entry whose name starts with `"hifimule-daemon-"`, logs the resolved path via `ui_log`, then runs `std::process::Command::new("xattr").args(["-d", "com.apple.quarantine"]).arg(&sp).output()` (error silently ignored)

**Acceptance Criteria:**
- Given a macOS installation where `ui.log` is absent, when the app is launched for the first time, then `~/Library/Application Support/HifiMule/ui.log` is created and contains at least the "HifiMule UI starting" message.
- Given a sidecar binary with `com.apple.quarantine`, when the UI spawns it, then `xattr -d com.apple.quarantine` is called before the spawn attempt.
- Given a sidecar binary without quarantine, when the UI spawns it, then behaviour is identical to before (no regressions on Windows/Linux).
- Given a successful spawn, when the daemon health-check passes, then `get_sidecar_status` returns a value starting with `"running"` or `"startup"`.

## Design Notes

The `@executable_path/../Resources/bundled-libs/libmtp.X.dylib` dylib path baked into the sidecar binary at CI time is correct for the installed bundle layout (`Contents/MacOS/hifimule-daemon` → `@executable_path` = `Contents/MacOS/` → lib resolves to `Contents/Resources/bundled-libs/`). If logs after this fix reveal a dylib-not-found error instead, the next step would be to verify the bundled-libs resource staging in the CI pipeline — but that is a separate issue.

Example quarantine-clearance block (place immediately before `app_handle.shell().sidecar(...)`):

**Important:** Tauri v2 bundles sidecars with the target-triple suffix in `Contents/MacOS/` (e.g. `hifimule-daemon-universal-apple-darwin`). A plain `dir.join("hifimule-daemon")` does not exist. Use a directory scan to find the actual binary name at runtime.

```rust
#[cfg(target_os = "macos")]
{
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let name = entry.file_name();
                    let name_str = name.to_string_lossy();
                    if name_str.starts_with("hifimule-daemon-") {
                        let sp = entry.path();
                        ui_log(&format!("Resolving macOS sidecar at {:?}", sp));
                        let _ = std::process::Command::new("xattr")
                            .args(["-d", "com.apple.quarantine"])
                            .arg(&sp)
                            .output();
                        break;
                    }
                }
            }
        }
    }
}
```

## Verification

**Commands:**
- `cargo check -p hifimule-ui` -- expected: zero errors
- `cargo clippy -p hifimule-ui -- -D warnings` -- expected: zero new warnings

**Manual checks (if no CLI):**
- After installing on macOS and launching: confirm `~/Library/Application Support/HifiMule/ui.log` exists and contains startup messages.
- Run `xattr -l /Applications/HifiMule.app/Contents/MacOS/hifimule-daemon` before and after first launch to verify quarantine is cleared.

## Spec Change Log

### Loop 1 — bad_spec: wrong sidecar binary name in quarantine block

**Triggering finding:** Review (edge-case hunter) found that `dir.join("hifimule-daemon")` targets a non-existent path. Tauri v2 bundles sidecars with the full target-triple suffix (e.g. `hifimule-daemon-universal-apple-darwin`), so `xattr` was silently called on a missing file and quarantine was never stripped.

**What was amended:** Design Notes code example replaced with a directory-scan approach (`read_dir` → filter `starts_with("hifimule-daemon-")`). Task description updated to match.

**Known-bad state avoided:** Do NOT use `dir.join("hifimule-daemon")` — this plain name does not exist in the installed bundle.

**KEEP:** macOS `ui_log` branch is correct and must be preserved exactly. Quarantine block structure (cfgated, best-effort, logged, silent-ignore on error) is correct — only the path construction changed.

## Suggested Review Order

**Quarantine clearance (the fix)**

- Entry point: scans `Contents/MacOS/` for `hifimule-daemon-*` and strips quarantine before spawn.
  [`lib.rs:275`](../../hifimule-ui/src-tauri/src/lib.rs#L275)

- The sidecar spawn that immediately follows — verifying ordering and that quarantine is cleared first.
  [`lib.rs:298`](../../hifimule-ui/src-tauri/src/lib.rs#L298)

**macOS file logging (diagnostic visibility)**

- New `#[cfg(target_os = "macos")]` branch in `ui_log` — writes startup and spawn events to `~/Library/Application Support/HifiMule/ui.log`.
  [`lib.rs:191`](../../hifimule-ui/src-tauri/src/lib.rs#L191)

- Windows log branch now correctly gated with `#[cfg(target_os = "windows")]`; `timestamp` moved above both gates.
  [`lib.rs:166`](../../hifimule-ui/src-tauri/src/lib.rs#L166)
