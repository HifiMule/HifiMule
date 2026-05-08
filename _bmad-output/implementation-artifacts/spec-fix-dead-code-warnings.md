---
title: 'Suppress dead-code compiler warnings'
type: 'chore'
created: '2026-05-08'
status: 'done'
route: 'one-shot'
---

## Intent

**Problem:** `cargo check` emitted four dead-code warnings across the daemon crate — all for intentionally-written but currently-uncalled code (a streaming download API, auto-fill helpers, DeviceManager methods, and ScrobblerEntry fields).

**Approach:** Add item-level `#[allow(dead_code)]` attributes co-located with each suppressed item, plus a comment on the scrobbler struct explaining why the parsed fields are retained but not yet forwarded to any backend.

## Suggested Review Order

1. [scrobbler.rs:11-19](../../jellyfinsync-daemon/src/scrobbler.rs) — field-level suppressions + rationale comment (highest signal, closest to a real design debt)
2. [auto_fill.rs:183-184](../../jellyfinsync-daemon/src/auto_fill.rs) — `rank_and_truncate` suppression (only called from tests; dead_code lint ignores test usage in binary crates)
3. [device/mod.rs:357-358](../../jellyfinsync-daemon/src/device/mod.rs) — `get_unrecognized_device_path` suppression
4. [device/mod.rs:928-929](../../jellyfinsync-daemon/src/device/mod.rs) — `save_auto_fill_prefs` suppression
5. [api.rs:640-641](../../jellyfinsync-daemon/src/api.rs) — `download_item_stream` suppression (no callers anywhere; streamed download path not yet wired up)
