# HifiMule v0.5.0

## macOS Support

This release delivers the first fully operational macOS build. The daemon now starts automatically at login, MTP devices work reliably via libmtp, and the app no longer appears as a Dock entry.

---

## New Features

### Daemon auto-start via launchd on macOS

The daemon is now registered as a launchd user agent (`com.hifimule.daemon`) on first launch. It starts automatically at login without requiring the UI to be open, and restarts if it crashes. The plist is installed into `~/Library/LaunchAgents/` and loaded via `launchctl` at setup time. A `settings_set_launch_on_startup` command is exposed to the UI so the user can toggle the behavior from preferences.

### MTP devices work on macOS via libmtp

The libmtp backend has been significantly extended to support macOS:

- **Storage enumeration on open.** `LIBMTP_Open_Raw_Device_Uncached` skips the storage enumeration step, leaving `device->storage` as NULL. All subsequent `LIBMTP_Get_Files_And_Folders` calls with `storage_id=0` iterate that list and silently return nothing. `LIBMTP_Get_Storage` is now called immediately after open to populate the list.
- **Directory creation.** The new `ensure_path_raw` helper walks path components and calls `LIBMTP_Create_Folder` for any directory that does not already exist, allowing the sync engine to create the target folder structure on first sync.
- **File overwrite via delete-then-create.** `LIBMTP_Send_File_From_File` creates new objects only; it cannot overwrite an existing file by object ID. `write_file` now checks for an existing object at the target path, deletes it, then writes the new content. The existing object's `storage_id` is reused so the replacement lands in the correct storage.
- **Reliable `storage_id` resolution.** New `path_to_object_and_storage_raw` helper returns the storage ID alongside the object ID when resolving a path, removing the need for a separate root-storage probe in most cases.
- **Correct `filetype` on send.** The metadata passed to `LIBMTP_Send_File_From_File` previously hard-coded `filetype: 0`. It now uses the generated `LIBMTP_FILETYPE_UNKNOWN` constant, which is the correct sentinel for generic binary content.

---

## Bug Fixes

### Read-only volumes no longer appear as unrecognized devices (macOS)

The macOS device scanner now calls `statvfs` on each candidate mount point and skips any volume that has the `ST_RDONLY` flag set. This prevents mounted DMG images, NTFS volumes, and hardware write-protected media from triggering the "unrecognized device" prompt.

### Daemon no longer appears in the Dock (macOS)

Two complementary fixes eliminate the unwanted Dock icon:

- The tao event loop is initialized with `ActivationPolicy::Accessory` so the process is treated as a background agent from startup.
- An embedded `Info.plist` (compiled into the binary via `build.rs`) sets `LSUIElement = true`, which signals to macOS that the process should have no Dock presence or application menu regardless of how it is launched.

### Zombie manifest no longer silently suppresses device detection

If the `.hifimule.json` manifest exists on the device as an MTP object but cannot be read (e.g., a partial write left a zero-byte or corrupt file), the previous code did not treat this as "manifest missing" and the device was permanently suppressed rather than shown as unrecognized. The `is_missing_manifest_error` predicate now also matches `libmtp read_file failed`, routing corrupt manifests to the re-initialization flow.

### macOS UI log now written to `~/Library/Application Support/HifiMule/ui.log`

`ui_log` previously only wrote to `%APPDATA%\HifiMule\ui.log` on Windows. A macOS branch now writes to `~/Library/Application Support/HifiMule/ui.log`, making diagnostic output accessible without attaching a debugger.
