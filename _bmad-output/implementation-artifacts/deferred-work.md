# Deferred Work

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
