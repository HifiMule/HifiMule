# Deferred Work

Status: open
Last updated: 2026-05-12

## Deferred from: fix MTP create-folder existing-dir (2026-05-12)

- **`find_folder_in_list` leaks folder tree on panic** [`hifimule-daemon/src/device/mtp.rs`] — `LIBMTP_Get_Folder_List` allocates a tree that is freed by `LIBMTP_destroy_folder_t` at the end of `find_folder_in_list`. If `search_folder_tree` panics before that call (e.g. from an allocation failure inside `to_string_lossy`), the tree is leaked. Low impact: a panic in unsafe FFI context crashes the daemon regardless; a drop guard (`scopeguard::defer`) would be the clean fix.
- **Folder hint cache does not propagate `storage_id` when hint is consumed** [`hifimule-daemon/src/device/mtp.rs`] — `ensure_path_raw` updates `parent_id` from the hint but leaves `storage_id` at its previous value. For the reported Garmin case this is safe (root-level folders ARE visible, so `storage_id` is always set before any hint is consumed). For a hypothetical device where every path component is hint-resolved, the storage for any new child folder would fall back to `root_storage_id_raw` (one extra MTP round-trip, not a crash). Fix: extend the hint map value to `(folder_id, storage_id)` if a device with all-hint paths is ever encountered.
- **Stale folder hint IDs after manual folder deletion** [`hifimule-daemon/src/device/mtp.rs`, `hifimule-daemon/src/sync.rs`] — If the user deletes a device directory externally (e.g. via Garmin Connect), the corresponding `folder_ids` entry in `.hifimule.json` becomes a dangling MTP object ID. The next sync will resolve the path via the stale hint and `LIBMTP_Send_File_From_File` will fail with an MTP error. Recovery requires clearing `folder_ids` from the manifest or deleting `.hifimule.json`. Note: factory reset clears `.hifimule.json` automatically, so hints are purged. Fix: validate hint IDs at sync start (one `LIBMTP_Get_Files_And_Folders` call per hinted path component), or retry with folder creation when a hint-directed write fails.

## Deferred from: replace MTP folder cache with live enumeration (2026-05-13, revised 2026-05-13)

- **Regression risk for devices where per-parent BFS is slow** [`hifimule-daemon/src/device/mtp.rs`] — `prime_folder_hints` now does a BFS via per-parent `LIBMTP_Get_Files_And_Folders` calls at open time. For a device with many deeply-nested folders (e.g. thousands of artist/album directories), this issues many round-trips and could add seconds to connect time. Garmin Venu 3 with 87 folders is fine. If this becomes a problem: add a depth limit or prune non-music-path branches early.

## Deferred from: fix unknown MTP device crash (2026-05-12)

- **`LIBMTP_File_t.filename` null-dereference in file traversal** [`hifimule-daemon/src/device/mtp.rs:1587, 1625, 1662`] — Three call sites in `path_to_object_id_raw`, `path_to_object_and_storage_raw`, and `ensure_path_raw` dereference `(*cur).filename` via `CStr::from_ptr` without checking for null. libmtp documents that `filename` can be NULL for unnamed objects on some Android MTP implementations. Would cause SIGSEGV during any directory walk if a device returns a null-named entry. Fix: check `(*cur).filename.is_null()` and skip/substitute the entry.
- **`device_id.clone()` pointless allocation** [`hifimule-daemon/src/device/mtp.rs:1920`] — `format!("{}:{}", r.bus_location, r.devnum)` is immediately cloned and the original moved into the struct, allocating an extra unused `String`. Trivial to remove.

## Deferred from: hide daemon Dock icon (2026-05-11)

- **`ActivationPolicy::Accessory` prevents in-process windows from receiving keyboard focus** [`hifimule-daemon/src/main.rs`] — With the Accessory policy active, any future in-process `NSWindow` (e.g., a settings dialog) will not be able to receive keyboard focus by default. The current code opens no windows; if one is ever added, the policy must be promoted to `Regular` at runtime via `set_activation_policy_at_runtime` before the window opens.
- **`ControlFlow::Poll` + `try_recv` busy-loop** [`hifimule-daemon/src/main.rs`] — The event loop unconditionally sets `ControlFlow::Poll` on every cycle, spinning at full CPU speed even when idle. This burns battery on macOS. Fix: use `ControlFlow::WaitUntil` or wake the event loop from the background thread via a user event.

## Deferred from: code review of 6-7-macos-daemon-launchd-agent (2026-05-11)

- **First match from unordered `read_dir`, no executable-type check** [`lib.rs:25`] — `resolve_daemon_binary_path` returns the first directory entry that starts with `"hifimule-daemon"` without verifying it is an executable regular file. If debug symbols, code-signature files, or multiple sidecar variants exist in the same directory, a non-executable file may be selected non-deterministically. Pre-existing pattern extracted from the original quarantine block.
- **`launchctl load` fails when label already loaded** [`lib.rs:103`] — If the plist file is deleted externally while the daemon label is still registered in launchd, the next app launch rewrites the plist and calls `launchctl load` again, which returns a non-zero exit code on macOS 12+ for an already-loaded label, causing a logged error even though the daemon is functioning correctly.
- **Plist always deleted after failed unload → silent re-enable on next launch** [`lib.rs:132`] — If `launchctl unload` fails for a substantive reason (not just "already unloaded"), the plist is still deleted. On the next launch, `plist_missing` is true and `install_launchd_plist` re-registers the daemon — effectively re-enabling auto-start without user action.
- **Stale plist when app bundle is moved** [`lib.rs:338`] — Moving the `.app` to a different directory leaves the existing plist pointing at the old binary path. Since the plist file still exists, the auto-install block skips reinstallation, and the daemon silently fails to start at login until the user toggles the setting off and on.

---

Status: closed
Closed: 2026-05-09

There is no open deferred-work backlog for the current sprint state.

All previously deferred items have either been incorporated into completed Epic 7 stories (7.1-7.4), resolved by completed Epic 8 stories (8.1-8.6), or accepted as non-blocking design/operational trade-offs that do not require a tracked follow-up in the active implementation backlog.

Closure rationale:

- The sprint status file marks Epics 1-8 and all listed implementation stories as `done`.
- Epic 7 already absorbed the accumulated technical hardening and deferred findings from Epics 2-6.
- Epic 8 review deferrals were reviewed on 2026-05-09 and are closed as non-blocking for the completed multi-provider milestone unless a future sprint explicitly reopens one as new scope.
- Packaging, signing, smoke-test, and provider-hardening caveats that remain valid as product considerations are documented in PRD/architecture/story context, not tracked as active deferred implementation work.

If future review findings need follow-up, add them as new story scope or reopen this file with a dated "Deferred from" section.

## Deferred from: spec-fix-subsonic-playlist-browse (2026-05-09)

- **Latent unwrap() in `provider_items_response` else branch** (`hifimule-daemon/src/rpc.rs`): The `else` branch unconditionally calls `parent_id.unwrap()` after the known-sentinel guards. If a future change adds a new sentinel ID and misses the guard, the code silently calls `get_artist(sentinel)` on the upstream server instead of panicking. Pre-existing pattern; not introduced by this change. Future hardening: add an explicit guard or replace the `unwrap()` with a handled error return for unrecognized IDs.

## Deferred from: spec-fix-macos-readonly-volume-filter (2026-05-11)

- **No "device is read-only" UI message for NTFS/write-protected volumes** (`hifimule-daemon/src/device/mod.rs`): The `is_readonly_mount` filter correctly drops all read-only volumes (DMGs, NTFS mounts, hardware write-protected media). However, a user with an NTFS-formatted DAP will see nothing rather than "device is read-only / incompatible". NTFS is a pre-existing incompatibility on macOS (no built-in write driver), so the old behavior — "unrecognized device" → init fails with write error — was also bad UX. A proper fix would detect the read-only condition and emit a `DeviceEvent::Incompatible` variant (or similar) so the UI can show an actionable message.

## Deferred from: spec-fix-macos-daemon-launch (2026-05-11)

- **TOCTOU race on `ui_log` truncation** (`hifimule-ui/src-tauri/src/lib.rs`): The truncation pattern (check size → truncate → append) across both Windows and macOS log branches has no lock. Concurrent `ui_log` calls (main thread, background spawn thread, async daemon-output task) can interleave truncation and append. Pre-existing issue in the Windows branch, now duplicated for macOS. Low impact in practice (log corruption, not a correctness bug), but a future hardening pass should centralise logging behind a `Mutex`-protected writer or a dedicated logging thread.
- **No Linux file logging in `ui_log`** (`hifimule-ui/src-tauri/src/lib.rs`): The `ui_log` refactor added explicit `#[cfg(target_os = "macos")]` and `#[cfg(target_os = "windows")]` branches but has no Linux path. All `ui_log` calls on Linux only go to `println!` (stdout, which is not visible in release builds). If Linux packaging is added, add a `#[cfg(target_os = "linux")]` branch writing to `$XDG_DATA_HOME/HifiMule/ui.log` or `$HOME/.local/share/HifiMule/ui.log`.

## Deferred from: spec-fix-libmtp-write-file-overwrite (2026-05-11)

- **Torn-write on delete-success / send-failure** (`hifimule-daemon/src/device/mtp.rs` `write_file`): If the pre-delete succeeds but `LIBMTP_Send_File_From_File` fails, the target path is permanently absent from the device — no rollback, no retry. libmtp has no transactional write API; the only fix would be a read-backup-restore loop, adding significant complexity. Acceptable trade-off for the manifest (a retry loop in the dirty-mark caller could re-create it), but worth a hardening pass if this write path is extended to music files.
- **Double `path_to_object_id_raw` traversal on new-file writes** (`hifimule-daemon/src/device/mtp.rs` `write_file`): The existence check re-traverses the same parent chain already walked to resolve `parent_id`, adding O(depth) extra `LIBMTP_Get_Files_And_Folders` round-trips on every write-to-new-file. Optimize by scanning the already-resolved parent's children directly (as the WPD backend's `find_child_object_id` does) to determine if the target exists before attempting delete.
