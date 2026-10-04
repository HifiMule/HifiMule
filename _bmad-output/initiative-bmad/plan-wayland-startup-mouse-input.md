---
title: 'Restore Wayland window mouse input on startup'
type: 'bugfix'
ticket: ''
created: '2026-10-04'
status: 'built'
baseline_revision: '07fc2c992b902df832b956f25eb226e55122950a'
route: 'full'
route_source: 'auto'
risk: 'medium'
review: 'quick'
review_source: 'pinned'
lenses_ran: ['quick']
review_loop_iteration: 0
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** On Fedora 44, the installed HifiMule UI ignores window-control and application clicks after startup. Double-clicking the title bar to maximize restores interaction, which continues after restoring the window; dragging or edge resizing does not repair it.

**Approach:** Upgrade the UI's Tauri runtime to the stable 2.12 release family, which depends on Tao 0.37 containing the upstream Wayland decoration and mouse-event fixes. Build a candidate and verify startup interaction before any maximize operation.

**User correction (2026-10-04):** The debug candidate fixes clicks but displays only Close. Preserve the original Minimize, Maximize and Close controls while retaining the upstream input fix. Apply any GTK layout override only inside HifiMule's process; do not change desktop preferences.

## Boundaries & Constraints

**Always:** Preserve splash readiness, window dimensions, daemon ownership, existing-instance activation, and release version 0.16.1. Use stable dependencies compatible with workspace Rust 1.93. Keep dependency resolution changes limited to the UI runtime and necessary transitive dependencies.

**Never:** Add timed maximize/restore or resize workarounds, force X11, disable rendering features, alter user settings, or install a candidate over the user's installed package without an explicit request.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Fresh launch | Fedora Wayland; main starts hidden behind splash | After readiness, window controls and menu respond immediately | Existing lifecycle failure handling remains available |
| Restore | Main is maximized and subsequently restored | Window controls and menu remain usable | No forced geometry cycle |
| Startup failure | Daemon readiness fails | Failure view appears; native close and failure actions respond | Existing Retry/Close behavior |
| Existing instance | Another UI launch requests activation | Existing window is shown and focused | Existing ownership and activation behavior |

</frozen-after-approval>

## Code Map

- `hifimule-ui/src-tauri/Cargo.toml` currently permits Tauri 2; set a 2.12 floor so old affected runtimes cannot resolve.
- `Cargo.lock` currently resolves Tauri 2.10.3, runtime-wry 2.10.1, and UI Tao 0.34.5. The daemon independently uses Tao 0.31.1; preserve that dependency.
- `hifimule-ui/src/main.ts` calls monitor fitting before show and closes splash after showing main. These are initial suspects, but upstream evidence identifies native event interception; leave this application code unchanged unless verification establishes another defect.
- Cached Tao 0.34.5 `linux/wayland/header.rs` sets its EventBox above the header, and `linux/event_loop.rs` returns `Propagation::Stop` for press/release events. Upstream PR https://github.com/tauri-apps/tao/pull/1218 restores GTK decoration handling and mouse propagation.
- Published registry metadata confirms Tauri 2.12.1 requires Rust 1.90 and runtime-wry ~2.12.1; runtime-wry 2.12.1 uses Tao ^0.37.0 and Wry ^0.57.0.
- `scripts/tests/tauri-platform-config.test.mjs`, `run-tauri.test.mjs`, and `release-contract.test.mjs` cover platform packaging and release integration. `hifimule-ui/tests/` covers frontend behavior.

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-ui/src-tauri/Cargo.toml` — require stable Tauri 2.12 — ensure the fixed runtime is selected.
- [x] `Cargo.lock` — refresh the UI Tauri dependency closure selectively — bring in Tao 0.37 without unrelated dependency upgrades.
- [x] Verify dependency graph and downloaded source — confirm fixed decoration/event behavior and unchanged daemon Tao.
- [x] Run UI Rust tests, frontend build and frontend/platform/release tests; build a Linux candidate when toolchain and sidecars permit.
- [x] Record native Fedora Wayland startup, restore, failure and existing-instance validation, clearly distinguishing automated evidence from user-confirmed clicks.

**Acceptance Criteria:**
- Given a fresh Fedora Wayland launch, when the main window appears, then menu actions respond before any maximize or restore.
- Given a fresh launch, when native close is clicked immediately, then the UI closes without first maximizing.
- Given a successful launch, when maximize and restore occur, then controls remain usable.
- Given an existing UI instance, when another launch requests activation, then the existing window becomes usable without creating a second instance.
- Given a dependency resolution, when the UI graph is inspected, then its Tao contains the upstream fix and the daemon's Tao remains in its existing family.

## Implementation Notes

- Follow-up investigation: Fedora's GNOME desktop preference is `appmenu:close`. Tao 0.37 now uses GTK default decoration handling, so it inherits that close-only layout. Restore the prior three controls through the app-local GTK setting, with a GTK integration test and candidate rebuild. This is an authorized user correction to the intent.

- UI Tauri requirement is `2.12`; selective `cargo update -p tauri --precise 2.12.1` resolved runtime-wry 2.12.1, Tao 0.37.1 and Wry 0.57.0. Daemon Tao remains 0.31.1. Application lifecycle, geometry and version 0.16.1 are unchanged.
- Downloaded Tao 0.37.1 uses default GTK decorations for decorated windows (`linux/window.rs:192-196`) and returns `Propagation::Proceed` from mouse press/release handlers (`linux/event_loop.rs:736-791`), matching the upstream fix.
- All 51 added/upgraded registry manifests were inspected; declared minimum Rust versions are at most 1.90. Actual verification compiler was Fedora Rust 1.98.1; execution under Rust 1.93 was not performed.

## Plan Change Log

- Packaged-build follow-up: user reports Tauri CLI rejects Rust 2.12.1 with JavaScript API 2.10.1. The plan's Cargo-only build missed the CLI version gate. Align `hifimule-ui/package.json` API and CLI to stable 2.12 and regenerate its npm lockfile. Verify through `npm run tauri build`, preserving the user-confirmed runtime and three-button fix. Add a meaningful version-family regression check to release contract tests so a future partial runtime update fails validation.

## Review Triage Log

- Packaging correction quick review: no code defect reported. Full package completion remains unverified because the duplicate verification build was stopped while the user's build ran. The reported mismatch is resolved: API/CLI manifest and lock entries match Rust 2.12, regression passes, and the CLI proceeds beyond the original gate. Do not claim a completed package build from this evidence.

- Final user validation on 2026-10-04: "it's working with 3 buttons." The rebuilt candidate restores working Minimize, Maximize and Close on the affected desktop. This closes the follow-up control regression and the original startup-click report. Separate packaged-release, startup-failure and existing-instance scenarios remain validation limitations; no implementation defect was found by quick review.

- Follow-up quick review: no code defect reported. Medium verification gap remains for real application control clicks on the rebuilt candidate; the explicit native GTK integration verifies button creation but not application Minimize/Maximize/Close actions. User was asked to relaunch and check the candidate. Startup-failure and existing-instance scenarios remain unverified as previously recorded.

- Quick review: one medium verification gap, no implementation defect reported. Native startup menu/close, maximize/restore, failure and existing-instance activation checks have not been performed. Source and build verification establish that the upstream fix is included, but do not prove the reported behavior is resolved. Keep the plan in review pending desktop validation; do not mark behavioral acceptance criteria complete. Candidate instructions were sent to the user on 2026-10-04.
- Follow-up on 2026-10-04: user reports "it works with the debug version" after receiving instructions to test immediate menu and native close clicks. This confirms the reported startup issue is resolved in the debug candidate on the affected Fedora desktop. Separate startup-failure, existing-instance activation, and packaged release checks were not explicitly confirmed; retain them as validation limitations, not verified results. No code changes followed review.

## Verification

- Final native user check on 2026-10-04: rebuilt debug candidate works with all three buttons, following the earlier confirmation that the startup click issue is fixed.

**Commands:**
- `rtk cargo tree -p hifimule-ui` — fixed UI Tao resolved.
- `rtk cargo test -p hifimule-ui --lib` — Rust UI tests pass.
- `rtk npm run build --prefix hifimule-ui` — TypeScript and Vite build pass.
- `rtk node --test hifimule-ui/tests/*.test.mjs scripts/tests/tauri-platform-config.test.mjs scripts/tests/run-tauri.test.mjs scripts/tests/release-contract.test.mjs` — frontend and integration tests pass.

**Native checks:** Fresh launch and immediately click a menu action, relaunch and immediately click native close, maximize/restore, exercise startup failure and existing-instance activation. Native mouse interaction remains unverified until performed on the affected desktop; source matching alone cannot establish that all reported symptoms have the same cause.

### Automated evidence (2026-10-04)

- UI Rust library tests: 13 passed, exit 0.
- TypeScript/Vite frontend build: passed, exit 0; existing dynamic import and chunk-size warnings remain.
- Frontend/platform/release Node tests: 13 passed, exit 0.
- Dependency graph and downloaded source checks: passed; logs in `/tmp/wayland-startup-verification.log` and `/tmp/wayland-dependency-msrv.log`.
- Native Fedora Wayland fresh-launch menu/close, maximize/restore, startup failure, and existing-instance activation clicks remain unverified.
- Linux candidate: `rtk cargo build -p hifimule-ui --locked --offline` passed, exit 0. Executable is `target/debug/hifimule-ui`; existing sidecar used. Build log: `/tmp/wayland-candidate-build.log`. Candidate was not installed or launched. Existing dead-code warning for `log_timestamp` remains.

### User validation (2026-10-04)

- User tested the debug candidate and confirmed the reported issue is fixed: "it works with the debug version".
- Packaged release, startup-failure and existing-instance activation interaction remain unverified. Maximize/restore was included in the requested check but was not separately described in the reply.

### App-local native controls follow-up (2026-10-04)

- Linux setup now sets process-local GTK `gtk-decoration-layout` to `menu:minimize,maximize,close`. No desktop settings are written, and Tao native decoration/event handling is retained. Added direct GTK 0.18 dependency, already present transitively.
- Native GTK regression test creates a window/HeaderBar with `appmenu:close`, confirms one native button, and checks that setup helper restores three native buttons. Before helper implementation it failed (1 != 3); after implementation it passed. Run explicitly with `rtk cargo test -p hifimule-ui --lib close_only_desktop_layout_restores_three_native_controls --locked --offline -- --ignored --test-threads=1`. Test is normally ignored because it requires a native display; GTK work remains on one test thread. Red/green evidence: `/tmp/wayland-controls-red.log`, `/tmp/wayland-controls-green.log`.
- User validation of the rebuilt candidate controls and maximize/restore remains pending; the GTK regression verifies widget creation, not user click behavior.
- Formatting tool unavailable (`rustfmt` not installed); `git diff --check` passes.
- Follow-up Rust suite: 13 passed, 1 ignored; explicit native test: 1 passed. Debug candidate rebuilt successfully with `rtk cargo build -p hifimule-ui --locked --offline` (exit 0), preserving existing sidecar and installed package. Logs: `/tmp/wayland-controls-rust-tests.log`, `/tmp/wayland-controls-candidate-build.log`, `/tmp/wayland-controls-verification.log`.

### Packaging version alignment (2026-10-04)

- UI JavaScript API and CLI now require stable `~2.12`, both resolving to 2.12.1, matching Cargo.lock Tauri 2.12.1. Only UI package manifest/lock changed; root npm package/lock untouched.
- Added release contract regression checking selected Rust Tauri major/minor against manifest and resolved API/CLI versions, including manifest/lock agreement and stable-version validation. Original mismatch failed before the fix; replaying the old API declaration produces the exact `2.10 != 2.12` assertion (`/tmp/wayland-packaging-red-detail.log`). Updated release-contract tests pass (12 subtests, `/tmp/wayland-packaging-green-detail.log`).
- Frontend TypeScript/Vite build passes with updated API (`/tmp/wayland-packaging-frontend.log`). Full `rtk npm run tauri build` was invoked through the project wrapper and reached sidecar Cargo preparation, past the version mismatch gate. A concurrent user-owned release build was already compiling the daemon; our duplicate build was waiting on its Cargo lock. Only our verified process tree was gracefully stopped (exit 143) to avoid contention. The user build was left running; package completion is not claimed.
