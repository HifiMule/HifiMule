---
title: 'Readable daemon and UI log datetimes'
type: 'chore'
created: '2026-07-12'
status: 'done'
baseline_commit: 'b3b63392bc156a3c100852368fd93c41032e0fa4'
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Daemon and UI log files prefix entries with Unix epoch seconds, for example `[1783841252]`, which makes routine troubleshooting needlessly difficult to read.

**Approach:** Have both file-log writers emit the local calendar date and time in a consistent, sortable format: `[YYYY-MM-DD HH:MM:SS]`.

## Boundaries & Constraints

**Always:** Change both daemon and UI file log prefixes; preserve the log messages, file locations, rotation behavior, and stdout behavior; use the local system time; format each entry as `[YYYY-MM-DD HH:MM:SS] message`.

**Ask First:** Any change to log storage, rotation limits, structured logging, time zone configuration, or a timestamp format other than the stated local date/time format.

**Never:** Change sync transfer calculations or their message contents; edit individual logging call sites; alter persisted timestamps used by the database or scrobbler.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Daemon log write | A `daemon_log!` message, including sync completion | `daemon.log` receives `[YYYY-MM-DD HH:MM:SS] message` | Keep existing best-effort file-write behavior |
| UI log write | A UI startup, sidecar, or forwarded-daemon message | `ui.log` receives `[YYYY-MM-DD HH:MM:SS] message` on Windows and macOS | Keep existing best-effort file-write behavior |

</frozen-after-approval>

## Code Map

- `hifimule-daemon/src/main.rs` -- centralized `log_to_file` implementation behind every `daemon_log!` call.
- `hifimule-ui/src-tauri/src/lib.rs` -- centralized `ui_log` implementation shared by Windows and macOS file writers.
- `hifimule-ui/src-tauri/Cargo.toml` -- UI Tauri crate dependency manifest; needs the existing workspace-compatible time formatter available to the daemon.

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-daemon/src/main.rs` -- format the file-log prefix from local time using the established `chrono` dependency; keep the macro and all call sites unchanged.
- [x] `hifimule-ui/src-tauri/Cargo.toml` and `hifimule-ui/src-tauri/src/lib.rs` -- add the matching direct dependency and format the single shared UI timestamp once before the platform-specific writes.
- [x] `hifimule-daemon/src/main.rs` and `hifimule-ui/src-tauri/src/lib.rs` -- add focused unit coverage for the timestamp representation, without writing user log files.

**Acceptance Criteria:**
- Given a daemon sync-completion event, when it is appended to `daemon.log`, then its prefix is a local `[YYYY-MM-DD HH:MM:SS]` date/time rather than Unix epoch seconds.
- Given a UI log event on Windows or macOS, when it is appended to `ui.log`, then its prefix uses the same local readable format.
- Given all existing log callers, when the format implementation changes, then their message content and destination remain unchanged.

## Spec Change Log

## Design Notes

Use `chrono::Local`, already present in the daemon and explicitly preferred by the project for local civil time. The UI needs it as a direct dependency because Rust dependencies are not inherited from a sibling crate.

Example: `[2026-07-12 14:05:33] [Sync] 'What’s That You’re Doing?' size=4039572B download=113.41ms(35.6MB/s) write=494.09ms(8.2MB/s)`

## Verification

**Commands:**
- `rtk cargo test -p hifimule-daemon` -- expected: daemon tests, including timestamp-format coverage, pass.
- `rtk cargo test -p hifimule-ui` -- expected: UI Tauri tests, including timestamp-format coverage, pass.

## Suggested Review Order

**Daemon file logging**

- Formats every daemon file entry once at the centralized writer.
  [main.rs:23](../../hifimule-daemon/src/main.rs#L23)

- Verifies the readable, sortable timestamp shape without writing a log file.
  [main.rs:51](../../hifimule-daemon/src/main.rs#L51)

**UI file logging**

- Uses the same local format before either platform-specific file write.
  [lib.rs:286](../../hifimule-ui/src-tauri/src/lib.rs#L286)

- Declares the formatter directly for the independently compiled UI crate.
  [Cargo.toml:23](../../hifimule-ui/src-tauri/Cargo.toml#L23)

- Covers the UI timestamp representation without touching user log files.
  [lib.rs:340](../../hifimule-ui/src-tauri/src/lib.rs#L340)
