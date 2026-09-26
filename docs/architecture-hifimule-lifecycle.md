# HifiMule Lifecycle — Architecture

**Last Updated:** 2026-09-27 | **Scan depth:** Deep | **Workspace version:** 0.15.0

## Purpose and consumers

`hifimule-lifecycle` is a shared Rust library used by both `hifimule-daemon` and the native Tauri shell in `hifimule-ui/src-tauri`. It coordinates daemon ownership, UI instance ownership, launch attempts, authenticated health, and explicit Quit across the two processes. Its source is `hifimule-lifecycle/src/lib.rs`; its cross-process contract tests are in `hifimule-lifecycle/tests/contract.rs`.

## Ownership and startup

The daemon holds an operating-system lock on `<app-data>/runtime/owner.lock`. While holding that lock, it publishes a private `owner.json` descriptor with schema and protocol versions, instance ID, PID, port, a 32-byte random bearer token encoded as 64 hex characters, and launch generation. The descriptor is discovery metadata; the lock establishes ownership. The UI validates the descriptor and calls authenticated `daemon.health`, checking protocol and instance identity before attaching. `OwnerGuard` removes only its own descriptor on drop.

`create_launch_ticket_until` records an expiring attempt for an expected generation. The daemon validates the ticket and generation when claiming ownership. A durable `launch-generation.json` increment fences older attempts after explicit Quit. Timeouts are bounded by `STARTUP_DEADLINE` (30 seconds), `HEALTH_TIMEOUT` (2 seconds), and polling intervals from this crate. The Tauri `StartupCoordinator` tracks an epoch so late results from abandoned attempts cannot change current UI state.

## Single UI and activation

`UiInstanceGuard` locks `runtime/ui.lock` for one UI per app-data profile. A second launch writes an idempotent `ui-activation.json` mailbox request and waits briefly for the owner to acknowledge it in `ui-activation-ack.json`. If the owner exits before acknowledgement, the second process can acquire the released lock and become the UI. The daemon tray uses this activation path to reopen the existing UI. The stable lock files remain on disk; their OS locks, rather than file existence, determine ownership.

## Local trust boundary

`resolve_app_data_dir` requires an absolute profile path and supports the `HIFIMULE_APP_DATA_DIR` override. Runtime files are private regular files under a private directory; Unix uses owner-only permissions and rejects symlinks, while Windows applies protected ownership and ACL checks. Atomic writes publish descriptor, ticket, generation, and mailbox records. `constant_time_token_eq` authenticates local RPC bearer tokens. Lifecycle errors use stable codes such as `PROTOCOL_MISMATCH`, `OWNER_CHANGED`, `DAEMON_STOPPED`, and `QUIT_PERSISTENCE_FAILED` for UI status rather than exposing secrets.

## Test and development entry points

Run `rtk cargo test -p hifimule-lifecycle`. The integration tests exercise descriptor validation, exclusive ownership, generation fencing, malformed metadata, and cross-process UI activation with `lifecycle-owner-probe`. For installed-app behavior, use the platform smoke scripts under `scripts/smoke-tests/`.

## Related documentation

- [Daemon architecture](./architecture-hifimule-daemon.md)
- [UI architecture](./architecture-hifimule-ui.md)
- [Integration architecture](./integration-architecture.md)
- [Development guide](./development-guide.md)
