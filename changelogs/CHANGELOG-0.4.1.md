# HifiMule v0.4.1

## Bug Fixes

### Auto-sync no longer silently skips when the device connects

Auto-sync was gated on both the manifest flag **and** a separate SQLite record. If the UI had never opened after pairing the device, the database entry was absent and auto-sync would never fire even though the device had it enabled. The manifest is now the authoritative source: on device detection the daemon copies `auto_sync_on_connect` from the manifest into SQLite, and the auto-sync trigger reads the manifest directly rather than double-checking the database.

The sidebar toggles for **Auto-fill** and **Auto-sync on connect** were also only loaded on the first device hydration, which could leave the checkboxes visually out of sync after a reconnect. Toggle state is now refreshed on every daemon-state poll while a device is connected.

### Jellyfin playlist `.m3u` files are now reliably written

Two related issues prevented `.m3u` files from landing on the device:

- **Auto-sync** expanded playlist basket items into individual tracks but never assembled the playlist metadata needed to write the `.m3u` file. Auto-sync now builds the full playlist list (ID, name, ordered tracks with artist and duration) and passes it to the sync engine.
- **Manual sync** would skip regeneration if the manifest recorded a playlist as unchanged, even when the `.m3u` file was missing from the device. The sync engine now checks actual file presence before skipping, and rewrites if absent.

## Improvements

### Server label, type hint, and logout

The login form now shows which server type (Jellyfin / Subsonic) the entered URL resolves to. Once connected, the server is labelled in the interface and a **Log out** action is available directly from the main view.
