---
title: 'Fix daemon CPU spin on macOS (ControlFlow::Poll → WaitUntil)'
type: 'bugfix'
created: '2026-05-11'
status: 'done'
route: 'one-shot'
---

## Intent

**Problem:** The tray-icon event loop used `ControlFlow::Poll`, which causes `tao` to call the closure as fast as the CPU allows — a busy-loop — consuming 70%+ CPU on macOS even when the daemon is completely idle.

**Approach:** Replace `ControlFlow::Poll` with `ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(250))`. The OS puts the main thread to sleep until a native event arrives (tray click, menu event) or 250 ms elapse, whichever comes first. Idle CPU drops to ~0%.

## Suggested Review Order

- [main.rs:393-397](../../hifimule-daemon/src/main.rs#L393) — one-line change: `Poll` → `WaitUntil(250 ms)`. Confirm `WaitUntil` is still polled (not blocked) on `try_recv` paths.

## Code Map

- `hifimule-daemon/src/main.rs` — tray event loop; `run_interactive()` owns the `ControlFlow` assignment

## Spec Change Log

