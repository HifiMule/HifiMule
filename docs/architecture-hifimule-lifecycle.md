# HifiMule Lifecycle — Architecture

**Last Updated:** 2026-11-05 | **Scan depth:** Deep | **Workspace version:** 0.21.0

## Overview

`hifimule-lifecycle` is a shared Rust crate that coordinates exactly one daemon and one UI per app-data profile. The OS file lock — not descriptor metadata or a PID probe — is the ownership authority. Secrets and discovery remain on the native side.

## Key constants

| Constant | Value | Meaning |
|----------|-------|---------|
| `SCHEMA_VERSION` | 1 | Owner descriptor schema version |
| `PROTOCOL_VERSION` | 1 | Lifecycle protocol version |
| `DESCRIPTOR_MAX_BYTES` | 16 KB | Max descriptor read size |
| `STARTUP_DEADLINE` | 30s | Max daemon startup wait |
| `HEALTH_TIMEOUT` | 2s | Health-check deadline |
| `POLL_INTERVAL` | 250ms | Status polling interval |

## Key types

- **`OwnerDescriptor`** — `{ schema_version, protocol_version, instance_id, pid, port, token, launch_generation }`. Custom `Debug` redacts the token. `validate_compatible()` enforces version match, 64-char hex token, non-zero pid/port.
- **`LifecycleState`** — `Starting`, `Ready`, `Failed`, `Stopping`, `Stopped`
- **`LifecycleErrorCode`** — 12 variants: `StartupTimeout`, `SpawnFailed`, `UnsafeRuntimePath`, `LocalAccessDenied`, `ProtocolMismatch`, `LegacyDaemonRunning`, `LegacyEndpointOccupied`, `OwnerChanged`, `DaemonStopped`, `QuitBlockedActiveSync`, `QuitPersistenceFailed`, `ShutdownTimeout`
- **`LaunchTicketGuard`** — owns app-data path + lock file; `validate(attempt_id, expected_generation)`; `Drop` unlocks.

## Key functions

- `lock_launch_ticket(app_data)` / `lock_launch_ticket_until(app_data, deadline)` — acquire OS file lock for daemon ownership
- `publish_ui_ready()`, `publish_shutdown_rendered()`, `publish_launch_failure()` — sanitized JSON markers in private `runtime/` dir
- `read_launch_failure()`, `clear_launch_failure()` — cross-process diagnostics without leaking secrets

## Contract

1. Daemon acquires exclusive OS lock on `<app-data>/runtime/owner.lock`
2. On readiness, publishes `owner.json` descriptor with port + bearer token
3. UI acquires its own lock, reads descriptor, validates compatibility, connects to daemon
4. Second UI launch writes an activation marker; running UI picks it up and focuses its window
5. Quit advances `launch_generation` so stale launch attempts cannot revive the daemon
