---
title: 'Identify HifiMule as the Windows notification sender'
type: 'bugfix'
created: '2026-09-20'
status: 'done'
baseline_commit: '0fe45a68e5d5ccc4da072f7793b225b144e2c7d9'
context:
  - '{project-root}/_bmad-output/planning-artifacts/project-context.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Windows displays HifiMule sync notifications as sent by Windows PowerShell, which makes their origin misleading.

**Approach:** Give every Windows desktop notification the installed HifiMule application identity, while preserving notification content and existing macOS/Linux behavior.

## Boundaries & Constraints

**Always:** Use the existing stable application identifier `hifimule.github.io`; apply the identity consistently to every daemon-originated desktop notification on Windows; retain localized titles/bodies and error logging.

**Ask First:** Any change to the public application identifier, installer identity, notification activation behavior, or notification interactions.

**Never:** Do not alter notification wording merely to hide the sender; do not modify macOS/Linux delivery paths; do not introduce a PowerShell wrapper or a separate notification process.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Sync completion | Windows daemon emits a completion notification | Toast is associated with HifiMule rather than Windows PowerShell | Existing failure logging remains |
| Sync interruption or playback error | Windows daemon emits any other desktop notification | Toast uses the same HifiMule application identity | Existing failure logging remains |
| Non-Windows delivery | macOS or Linux daemon emits a notification | Existing platform behavior is unchanged | Existing failure logging remains |

</frozen-after-approval>

## Code Map

- `hifimule-daemon/src/rpc.rs` -- emits manual sync completion notifications.
- `hifimule-daemon/src/main.rs` -- emits auto-sync and playback-failure notifications.
- `hifimule-daemon/Cargo.toml` -- contains the Windows-specific notification dependencies.
- `hifimule-ui/src-tauri/tauri.conf.json` -- declares the canonical HifiMule identifier.

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-daemon/src/notifications.rs` (or the established equivalent) -- centralize platform-aware notification construction and set the Windows app ID to `hifimule.github.io` -- prevents one notification path from retaining the PowerShell fallback.
- [x] `hifimule-daemon/src/main.rs` and `hifimule-daemon/src/rpc.rs` -- route every existing notification through that helper without changing message selection or asynchronous behavior -- makes sender attribution consistent.
- [x] `hifimule-daemon/src/tests.rs` (or focused notification tests) -- add a regression test for the Windows notification configuration -- catches removal or substitution of the HifiMule app ID.

**Acceptance Criteria:**
- Given a packaged HifiMule installation on Windows, when sync completion, sync interruption, sync failure, or playback failure creates a system toast, then Windows attributes the toast to HifiMule instead of Windows PowerShell.
- Given the same events on macOS or Linux, when the daemon emits a system notification, then existing platform-specific delivery continues unchanged.

## Spec Change Log

## Design Notes

`notify-rust` selects `Toast::POWERSHELL_APP_ID` whenever a notification has no `app_id`; the current daemon never supplies one. The Tauri bundle already owns `hifimule.github.io`, so reuse that identifier rather than inventing another notification identity.

## Verification

**Commands:**
- `rtk cargo test -p hifimule-daemon` -- expected: focused regression and daemon test suite pass.
- `rtk cargo check -p hifimule-daemon --target x86_64-pc-windows-msvc` -- expected: Windows-specific notification code compiles.

## Suggested Review Order

**Windows notification identity**

- Centralizes the Windows application identity while preserving other platforms.
  [`notifications.rs:4`](../../hifimule-daemon/src/notifications.rs#L4)

- Routes automatic-sync and playback notifications through the shared constructor.
  [`main.rs:489`](../../hifimule-daemon/src/main.rs#L489)

- Routes manual sync completion through the same identity-aware constructor.
  [`rpc.rs:173`](../../hifimule-daemon/src/rpc.rs#L173)

- Locks the Windows sender identity against regression.
  [`notifications.rs:20`](../../hifimule-daemon/src/notifications.rs#L20)
