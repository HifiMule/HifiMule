# Changelog

## v0.5.1 — 2026-05-11

HifiMule now runs on macOS. The daemon starts automatically at login, MTP devices (phones, DAPs) connect and sync reliably via libmtp, and the app no longer shows a Dock icon. Read-only volumes such as mounted disk images are silently skipped and will no longer trigger the "unrecognized device" prompt.

## v0.4.2 — 2026-05-10

MTP device connection is more reliable on Windows. WPD `device.Open()` now retries on transient errors, and the init liveness probe for MTP no longer triggers a full recursive storage walk.

## v0.4.1 — 2026-05-10

Auto-sync now fires reliably when a device is connected, even if the app was never opened after pairing. Jellyfin playlists are correctly written to the device as `.m3u` files in both manual and auto-sync modes. The login screen shows the detected server type (Jellyfin / Subsonic) before you submit, and a logout button is now accessible directly from the main view.
