# Deferred Work

Status: open
Last updated: 2026-05-29

## Deferred from: autosync-subsonic-navidrome (2026-05-29)

- **`get_non_jellyfin_provider` case-sensitive server_type matching** [`hifimule-daemon/src/main.rs`] — `matches!(config.server_type.as_str(), "subsonic" | "openSubsonic")` silently skips configs stored with different capitalisation. Pre-existing pattern also used in `restore_provider_from_config` in `rpc.rs`. Fix: `.to_lowercase()` normalisation on both sides.
- **`run_auto_fill_provider` MAX_PER_LIST=2000 hard-coded** [`hifimule-daemon/src/auto_fill.rs`] — Servers with more than 2000 favorites/frequently-played tracks silently return a truncated priority list. No configuration path and no relationship to `max_fill_bytes`. Fix: paginate until capacity is filled or list exhausted, similar to Jellyfin `run_auto_fill`.
- **`run_auto_fill_provider` no free-space safety margin** [`hifimule-daemon/src/auto_fill.rs`, `hifimule-daemon/src/rpc.rs`] — When `maxBytes` is absent, `free_bytes` from `get_device_storage()` is used as the full fill budget with no headroom reserved. Pre-existing behavior in Jellyfin `run_auto_sync` too. Fix: apply a minimum headroom (e.g. 5% or a fixed reserve) if device type supports it.
- **Sequential album fetches in artist basket resolution** [`hifimule-daemon/src/main.rs:resolve_provider_item`] — Artist with many albums triggers sequential `get_album` HTTP calls with no concurrency limit or timeout. Pre-existing pattern in `rpc.rs`'s `provider_sync_items_for_id`. Fix: `futures::join_all` with a concurrency cap.
- **Orphaned SyncOperation on `update_manifest` failure** [`hifimule-daemon/src/main.rs:run_auto_sync_via_provider`] — If `update_manifest(dirty=true)` fails (disk full, device disconnected), the function propagates the error but leaves the `SyncOperation` entry in the manager without a status update. Pre-existing pattern in `run_auto_sync` too. Fix: mark the operation as Failed in the error path before returning.
- **Rapid device-connect race in auto-sync trigger** [`hifimule-daemon/src/main.rs:270-310`] — `has_active_sync` is checked before spawning but the spawn is non-blocking. Two rapid connect events can both pass the check before either spawned task registers an active operation. Pre-existing race. Fix: atomically check-and-set via a `Mutex`-guarded flag or a `Semaphore`.
- **No unit tests for `run_auto_fill_provider`** [`hifimule-daemon/src/auto_fill.rs`] — The new provider-based auto-fill path has zero dedicated tests. AC2 and AC3 are only verified by code reading. Fix: add mock-provider tests for capacity truncation, dedup, and UnsupportedCapability fallback.

## Deferred from: fix MTP create-folder existing-dir (2026-05-12)

- **`find_folder_in_list` leaks folder tree on panic** [`hifimule-daemon/src/device/mtp.rs`] — `LIBMTP_Get_Folder_List` allocates a tree that is freed by `LIBMTP_destroy_folder_t` at the end of `find_folder_in_list`. If `search_folder_tree` panics before that call (e.g. from an allocation failure inside `to_string_lossy`), the tree is leaked. Low impact: a panic in unsafe FFI context crashes the daemon regardless; a drop guard (`scopeguard::defer`) would be the clean fix.
- **Folder hint cache does not propagate `storage_id` when hint is consumed** [`hifimule-daemon/src/device/mtp.rs`] — `ensure_path_raw` updates `parent_id` from the hint but leaves `storage_id` at its previous value. For the reported Garmin case this is safe (root-level folders ARE visible, so `storage_id` is always set before any hint is consumed). For a hypothetical device where every path component is hint-resolved, the storage for any new child folder would fall back to `root_storage_id_raw` (one extra MTP round-trip, not a crash). Fix: extend the hint map value to `(folder_id, storage_id)` if a device with all-hint paths is ever encountered.
- **Stale folder hint IDs after manual folder deletion** [`hifimule-daemon/src/device/mtp.rs`, `hifimule-daemon/src/sync.rs`] — If the user deletes a device directory externally (e.g. via Garmin Connect), the corresponding `folder_ids` entry in `.hifimule.json` becomes a dangling MTP object ID. The next sync will resolve the path via the stale hint and `LIBMTP_Send_File_From_File` will fail with an MTP error. Recovery requires clearing `folder_ids` from the manifest or deleting `.hifimule.json`. Note: factory reset clears `.hifimule.json` automatically, so hints are purged. Fix: validate hint IDs at sync start (one `LIBMTP_Get_Files_And_Folders` call per hinted path component), or retry with folder creation when a hint-directed write fails.

## Deferred from: replace MTP folder cache with live enumeration (2026-05-13, revised 2026-05-13)

- **Regression risk for devices where per-parent BFS is slow** [`hifimule-daemon/src/device/mtp.rs`] — `prime_folder_hints` now does a BFS via per-parent `LIBMTP_Get_Files_And_Folders` calls at open time. For a device with many deeply-nested folders (e.g. thousands of artist/album directories), this issues many round-trips and could add seconds to connect time. Garmin Venu 3 with 87 folders is fine. If this becomes a problem: add a depth limit or prune non-music-path branches early.

## Deferred from: fix unknown MTP device crash (2026-05-12)

- **`LIBMTP_File_t.filename` null-dereference in file traversal** [`hifimule-daemon/src/device/mtp.rs:1587, 1625, 1662`] — Three call sites in `path_to_object_id_raw`, `path_to_object_and_storage_raw`, and `ensure_path_raw` dereference `(*cur).filename` via `CStr::from_ptr` without checking for null. libmtp documents that `filename` can be NULL for unnamed objects on some Android MTP implementations. Would cause SIGSEGV during any directory walk if a device returns a null-named entry. Fix: check `(*cur).filename.is_null()` and skip/substitute the entry.
- **`device_id.clone()` pointless allocation** [`hifimule-daemon/src/device/mtp.rs:1920`] — `format!("{}:{}", r.bus_location, r.devnum)` is immediately cloned and the original moved into the struct, allocating an extra unused `String`. Trivial to remove.

## Deferred from: fix empty-basket sync blocked (2026-05-29)

- **`DaemonState::Idle` not sent before early-return in `run_auto_sync` at line 731** [`hifimule-daemon/src/main.rs`] — When basket items fail to resolve (`desired_items.is_empty() && !basket_items.is_empty()`) the function returns `Ok(())` without sending `DaemonState::Idle`, leaving the UI in `DaemonState::Syncing` until the next state poll. Pre-existing; not introduced by the empty-basket fix.
- **Auto-fill returns empty result but device has stale synced items** [`hifimule-daemon/src/main.rs:575-578`] — When `auto_fill.enabled` is true and `run_auto_fill` returns an empty list, `run_auto_sync` returns early with `DaemonState::Idle` regardless of `synced_items`. Files synced by a prior manual sync are never cleaned up on auto-fill-enabled devices. Pre-existing gap; out of scope for the empty-basket fix.
- **`currentDevice: any` untyped in BasketSidebar** [`hifimule-ui/src/components/BasketSidebar.ts:180`] — `synced_items` is accessed via optional chaining on an `any`-typed field; a Rust-side rename would silently break button state with no compile-time error. Consider typing the daemon state response.

## Deferred from: hide daemon Dock icon (2026-05-11)

- **`ActivationPolicy::Accessory` prevents in-process windows from receiving keyboard focus** [`hifimule-daemon/src/main.rs`] — With the Accessory policy active, any future in-process `NSWindow` (e.g., a settings dialog) will not be able to receive keyboard focus by default. The current code opens no windows; if one is ever added, the policy must be promoted to `Regular` at runtime via `set_activation_policy_at_runtime` before the window opens.
- **`ControlFlow::Poll` + `try_recv` busy-loop** [`hifimule-daemon/src/main.rs`] — The event loop unconditionally sets `ControlFlow::Poll` on every cycle, spinning at full CPU speed even when idle. This burns battery on macOS. Fix: use `ControlFlow::WaitUntil` or wake the event loop from the background thread via a user event.

## Deferred from: code review of 6-7-macos-daemon-launchd-agent (2026-05-11)

- **First match from unordered `read_dir`, no executable-type check** [`lib.rs:25`] — `resolve_daemon_binary_path` returns the first directory entry that starts with `"hifimule-daemon"` without verifying it is an executable regular file. If debug symbols, code-signature files, or multiple sidecar variants exist in the same directory, a non-executable file may be selected non-deterministically. Pre-existing pattern extracted from the original quarantine block.
- **`launchctl load` fails when label already loaded** [`lib.rs:103`] — If the plist file is deleted externally while the daemon label is still registered in launchd, the next app launch rewrites the plist and calls `launchctl load` again, which returns a non-zero exit code on macOS 12+ for an already-loaded label, causing a logged error even though the daemon is functioning correctly.
- **Plist always deleted after failed unload → silent re-enable on next launch** [`lib.rs:132`] — If `launchctl unload` fails for a substantive reason (not just "already unloaded"), the plist is still deleted. On the next launch, `plist_missing` is true and `install_launchd_plist` re-registers the daemon — effectively re-enabling auto-start without user action.
- **Stale plist when app bundle is moved** [`lib.rs:338`] — Moving the `.app` to a different directory leaves the existing plist pointing at the old binary path. Since the plist file still exists, the auto-install block skips reinstallation, and the daemon silently fails to start at login until the user toggles the setting off and on.

## Deferred from: fix MTP Android storage check (2026-05-15)

- **RAII split-lifecycle for `LIBMTP_MtpDevice_t` in `LibmtpHandle::open()`** [`hifimule-daemon/src/device/mtp.rs`] — The error path now calls `LIBMTP_Release_Device(device)` raw, while the success path wraps `device` in an `Arc<Mutex<...>>` whose `Drop` impl calls `LIBMTP_Release_Device`. These two release sites are structurally split; a future refactor that wraps `device` in `Arc` before the storage check could introduce a double-free. Fix: introduce a small local RAII guard immediately after the null-check (e.g. `scopeguard::defer`) that owns the release responsibility, and disarm it before wrapping in `Arc`.
- **Transient `LIBMTP_Get_Storage` failure causes silent exclusion of legitimately MTP-enabled devices** [`hifimule-daemon/src/device/mod.rs`, `hifimule-daemon/src/device/mtp.rs`] — If a device consistently returns non-zero from `LIBMTP_Get_Storage` but MTP file operations still work (theoretical, not observed in any known device), it is silently excluded from recognition. The 2-second retry loop handles truly transient failures, but a persistent quirk would require a device-level workaround. Monitor for user reports; if a specific device model exhibits this, consider a device-flags-based bypass.

## Deferred from: avoid keychain access in tests (2026-05-15)

- **`CONFIG_FILE_PATH` not reset by `clear_credentials`** [`hifimule-daemon/src/api.rs`] — `clear_credentials` removes the on-disk config file and clears `TEST_SECRETS` in test mode, but does not reset the `CONFIG_FILE_PATH` static. If a test sets a custom path and calls `clear_credentials`, subsequent tests still inherit the custom path. Fix: reset `CONFIG_FILE_PATH` to `None` inside `clear_credentials` (or `credential_test_lock`).
- **`save_credentials` silently overwrites all `server_secrets` on keyring read error** [`hifimule-daemon/src/api.rs:1041`] — `Self::load_secrets().unwrap_or_default()` means a corrupt or temporarily inaccessible keyring entry causes all stored `server_secrets` to be silently discarded on the next `save_credentials` call. In tests this is harmless (panics on lock poisoning instead). In production it could wipe legitimate server secrets. Fix: propagate the error instead of silently defaulting, or log a warning and require explicit confirmation.

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

## Deferred from: code review of 9-1-provider-browse-modes-and-capability-contract (2026-05-22)

- **URL parameter encoding in Jellyfin query building** (`hifimule-daemon/src/api.rs`): Query params like `genre_id`, `user_id`, `library_id` are joined into URLs via string formatting without URL percent-encoding. Pre-existing pattern throughout JellyfinClient; IDs are server-provided UUIDs/strings with low practical risk. Revisit if user-visible strings (genre names, playlist titles) ever appear as URL parameters.
- **`list_genres` has no pagination parameter** (`hifimule-daemon/src/providers/mod.rs`): The `MediaProvider::list_genres` trait method fetches all genres in one call with no offset/limit support. Sufficient for typical library sizes, but could be an issue for servers with thousands of genres. Address in Story 9.3 (genre browsing) if large genre lists become a concern.

## Deferred from: fix provider sync track number prefix (2026-05-28)

- **Existing devices keep wrong `00 - ` filenames after fix** (`hifimule-daemon/src/sync.rs`): Files already synced with the `00 - <title>` prefix will remain as-is on device. The delta algorithm marks them "unchanged" (matched by `jellyfin_id`, not by expected path). They are only corrected if the user triggers a force re-sync. Optionally implement a rename/relocation pass that moves files without re-downloading when the manifest path doesn't match the expected `<track_num> - <title>.<ext>` pattern.
- **`SyncIdChangeItem` does not carry `track_number`** (`hifimule-daemon/src/sync.rs`): When an ID-change is detected, the resulting `SyncIdChangeItem` doesn't include `track_number`. Currently harmless because `SyncIdChangeItem` only updates the existing manifest path and never calls `construct_desired_file_path`. Revisit if `SyncIdChangeItem` ever drives a path re-construction.
- **Track number zero-padding caps at 2 digits** (`hifimule-daemon/src/sync.rs:construct_desired_file_path`): `format!("{:02}", n)` produces `"100"` for track 100+, breaking lexicographic sort for albums with 100+ tracks. Pre-existing behavior in `construct_file_path_with_extension` too; fix both together if needed.

## Deferred from: code review of 7-5-machine-bound-credential-vault (2026-05-30)

- **Concurrent `save_secrets` race condition** (`vault.rs:51-54`, `api.rs`): Two async tasks calling `save_credentials`/`save_server_secret` concurrently can interleave their `File::create` + `write_all` calls, producing a corrupted vault. Pre-existing pattern; fix requires a global `Mutex` or advisory file lock across the CredentialManager write path.
- **`derive_key` called twice per encrypt/decrypt round-trip** (`vault.rs:37,66`): Key derivation runs once in `encrypt_file` and once in `decrypt_file`. Currently cheap (BLAKE3 is fast), but if the KDF is ever upgraded to a memory-hard function the doubled cost doubles latency on every write. Consider accepting a `Secret<[u8; 32]>` parameter or caching the derived key at call sites.
- **`get_app_data_dir()` CWD fallback when HOME is unset** (`paths.rs`): If the daemon launches without a `HOME` env var (some launchd/systemd setups), `get_app_data_dir` silently falls back to `$CWD/HifiMule`, making the vault location unpredictable and fragile across restarts. Pre-existing in `paths.rs`; not introduced by this change.

## Deferred from: stop-sync-daemon-cancellation (2026-06-05)

- **M3U generation runs after cancellation** [`hifimule-daemon/src/sync.rs:execute_sync`, `execute_provider_sync`] — The `generate_m3u_files` call after the loops is not guarded by a cancellation check. On cancel, M3U files are written referencing only the tracks transferred before the break. Files are regenerated correctly on the next full sync (delta carries the full playlist intent), so no data loss occurs — but the device briefly holds partial .m3u files. Fix: skip M3U generation when `is_cancelled` is true after the loops.
- **`cleanup_empty_subdirs` runs unconditionally on cancel** [`hifimule-daemon/src/sync.rs`] — After the deletes loop breaks early, `cleanup_empty_subdirs` still executes, pruning partially-emptied directories. Harmless in practice (the next sync recreates them), but logically inconsistent with cancellation semantics. Fix: skip or guard behind a non-cancelled check.
- **Tiny race: cancel arrives just as sync completes** [`hifimule-daemon/src/rpc.rs`] — If `sync_cancel` is received between `execute_sync` returning `Ok` and the spawned task's `is_cancelled` check, the manifest is left dirty even though all files were transferred successfully. Consequence: a false "interrupted sync" resume dialog on next device connect that resolves immediately (delta is empty). Fix: check the cancel token only if `synced_items.len() < delta.adds.len()`, or clear dirty before the cancellation guard.
- **Auto-sync operations report `Complete` when cancelled** [`hifimule-daemon/src/main.rs:run_auto_sync`, `run_auto_sync_via_provider`] — The auto-sync spawned tasks do not check `is_cancelled` after `execute_sync` returns, so a cancelled auto-sync clears the dirty flag, sets status to `Complete`, and fires the completion notification. Fix: add the same `is_cancelled` guard used in the manual-sync paths in `rpc.rs`.

## Deferred from: code review of 11-1-mediaprovider-playlist-write-trait-amendment (2026-06-05)

- Empty `track_ids` boundary unhandled/untested for real providers (hifimule-daemon/src/providers/mod.rs). No provider overrides the playlist-write methods yet; empty-slice validation/behavior belongs to Stories 11.2 (Jellyfin) and 11.3 (Subsonic).
