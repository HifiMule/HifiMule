# HifiMule v0.5.4

> Covers all changes introduced after v0.5.1 (releases v0.5.2, v0.5.3, and v0.5.4).

---

## New Features

### Garmin smartwatch support (Forerunner, Fenix, Venu, Vivoactive)

HifiMule can now pair and sync with Garmin watches that have music storage.

**Device profile.** A dedicated `garmin-music` device profile is bundled in `device-profiles.json`. It passes MP3 and AAC/M4A files through directly and transcodes FLAC, OGG, and other formats to MP3 320 kbps (stereo, max 48 kHz).

**Format selection from profile.** The sync engine now reads `DirectPlayProfiles` and `TranscodingProfiles` from the active device profile and requests the correct container and codec from the Jellyfin server. Previously the format was hardcoded regardless of profile.

**MTP folder visibility workaround.** Garmin watches return only root-level objects from all MTP enumeration APIs (`LIBMTP_Get_Files_And_Folders`, `LIBMTP_Get_Folder_List`, and the all-objects ROOT scan). This makes it impossible to rediscover sub-folders such as `Music/Artist/Album` after they have been created.

The fix persists folder object IDs in `DeviceManifest.folder_ids` (the `.hifimule.json` file stored on the device) whenever a folder is created. On subsequent syncs, `ensure_path_raw` consults the loaded hints before attempting enumeration. Hints are loaded via `load_folder_hints` at sync start and written back via `drain_folder_hints` at the end of each sync job. Both `execute_sync` and `execute_provider_sync` participate in the load/drain cycle. The existing `find_folder_in_list` and `find_folder_in_all_objects` fallbacks are retained for devices where enumeration does work.

---

## Bug Fixes

### Crash on connection of unknown MTP devices (SIGSEGV)

`LIBMTP_Detect_Raw_Devices` can return device entries with a null `vendor` or `product` pointer for unrecognized hardware. The enumeration loop now null-checks both fields before formatting the device description, preventing a segfault when such a device is connected.

### macOS notifications not delivered

The daemon's notification delivery on macOS required the `macos-private-api` Tauri feature, which was not enabled. The feature is now declared, and notifications fire correctly on macOS 10.15+.

### Daemon Dock icon on macOS

The daemon process was briefly visible in the Dock after the v0.5.1 release. Two complementary fixes ensure it never appears:

- `hifimule-daemon/Info.plist` sets `LSUIElement = true`. The plist is embedded into the binary at compile time via `build.rs` using the `embed-plist` crate.
- The tao event loop is initialized with `ActivationPolicy::Accessory` (already present since v0.5.1; retained).

---

## Security

### All credentials stored under a single keyring entry

Previously each credential type (auth token, per-server secrets) was stored as a separate keyring entry, which caused issues on some system keyrings that limit the number of entries per application. All secrets are now serialised as a JSON blob under a single entry keyed by `(hifimule.github.io, secrets)`. Existing entries stored under the old keys (`hifimule-daemon / jellyfin-token`, `hifimule-daemon / jellyfin-token-{server_type}`) will be migrated on first access.

---

## Infrastructure

### macOS binaries are now code-signed

The release CI workflow now signs `hifimule-daemon` and the bundled `libmtp` dylibs with the Apple Developer certificate before packaging. This eliminates the Gatekeeper quarantine prompt on macOS 13+ for users who download the app directly.
