---
title: 'Daemon Tray Icon Quality Fix (Windows)'
type: 'bugfix'
created: '2026-05-08'
status: 'done'
route: 'one-shot'
---

## Intent

**Problem:** The daemon system-tray icons are sourced from 1024×1024 PNG assets. Passing a full-resolution RGBA buffer to `Icon::from_rgba` forces Windows to scale 1024→16 px in one step, producing blurry tray icons at all DPI levels.

**Approach:** Pre-scale each icon to exactly 32×32 px using Lanczos3 inside `load_icon` before constructing the tray `Icon`. 32 px matches the highest common tray slot size (200 % DPI), and Windows can cleanly halve it for 100 % displays. `resize_exact` is used instead of `resize` to guarantee exact output dimensions regardless of aspect ratio.

## Suggested Review Order

1. [hifimule-daemon/src/main.rs](../../hifimule-daemon/src/main.rs) — `load_icon` fn (~line 822): single-line change from `to_rgba8()` to `resize_exact(32, 32, Lanczos3).to_rgba8()`

## Spec Change Log

