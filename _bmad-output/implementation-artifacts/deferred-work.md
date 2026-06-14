# Deferred Work

Status: open
Last updated: 2026-06-12

## Deferred from: code review of 9-11-list-view-multi-selection-and-bulk-actions (2026-06-12)

- **Partial/empty batched RPC response silently adds zero count/size** [`hifimule-ui/src/library.ts` `addBrowseItemsToBasket`] — `jellyfin_get_item_counts`/`jellyfin_get_item_sizes` results are mapped by `id`; any requested id missing from the response (partial response, id-shape mismatch) falls back to `{recursiveItemCount:0, totalSizeBytes:0}` with no warning, and the success toast counts it as added. Generalizes the pre-existing single-item `metadata[0] || {…}` fallback to N items. A guard would detect needs-fetch ids absent from the result maps and warn/skip.
- **device-locked gate does not block keyboard activation of bulk "Add to basket"** [`hifimule-ui/src/library.ts`] — the bulk button relies on `#library-content.device-locked .basket-toggle-btn { pointer-events:none }`, which blocks mouse but not keyboard Enter/Space on a focusable `sl-button`. Faithfully mirrors the pre-existing per-row mechanism that AC 7 explicitly mandates; a real fix (set `disabled` from device state) should be applied to both per-row and bulk together.
- **Per-row (+) basket-add failure shows no user toast** [`hifimule-ui/src/library.ts` `renderListRow`] — the refactored per-row handler only `console.error`s on RPC failure, while the bulk path shows a danger toast. Unchanged pre-existing behavior, surfaced by the factoring.
- **Bulk-bar sticky `top` offset goes stale on resize** [`hifimule-ui/src/library.ts` `updateBulkBar`] — `bar.style.top` is computed once from `quick-nav` height at 0→1 creation; the quick-nav wraps on window resize, so the offset can drift and overlap/gap. No resize recompute.
- **Shift-range only grows; anchor resets on deselect** [`hifimule-ui/src/library.ts` `selectRange`/`toggleRowSelection`] — `selectRange` only adds (never shrinks) and never moves the anchor, and `toggleRowSelection` sets the anchor even when deselecting. Beyond AC 3 (which only requires inclusive range selection); standard range-shrink/anchor-move semantics are unimplemented.
- **Bulk playlist add has no already-in-playlist skip / skipped accounting** [`hifimule-ui/src/library.ts` `bulkAddSelectionToPlaylist`] — unlike the basket path, all N ids are sent to `playlist.addItems` with no duplicate skip or skipped-count feedback. Beyond AC 6; the daemon resolves/dedupes server-side.

## Deferred from: code review of 2-11-multi-server-hub (2026-06-09)

- **Cross-server playlist guard (AC33) checks only the `items` param, not the `itemIds` used to build the track list** [`hifimule-daemon/src/rpc.rs:875-915` `handle_playlist_create`] — defense-in-depth only. The guard inspects `params["items"]` (`{id, serverId}`), but `track_ids` is built from `params["itemIds"]`. A non-UI caller sending legacy `itemIds: string[]` (no `items`) skips the guard; however those ids resolve only against the *selected* provider, so cross-server ids fail to resolve and are skipped (not mis-assigned). The UI always sends `items` (AC34 pre-filter), so the supported flow is fully covered. A complete fix needs per-item serverIds in the bare-`itemIds` path; forcing `items` would break the documented legacy contract.
- **load_vault masks decryptable-but-unparseable vault as empty** [`hifimule-daemon/src/api.rs` `load_vault`] — `serde_json::from_str(&json).unwrap_or_default()` turns a corrupted-but-decryptable vault into an empty map (previously errored on parse failure). Worst case the user re-authenticates; the legacy-migration path reads the raw blob separately so migration is unaffected. Low impact.
- **`handle_server_connect` with `ServerType::Unknown` caches a provider but saves no credential** [`hifimule-daemon/src/rpc.rs` `handle_server_connect`] — the Unknown arm stores no vault entry yet inserts the provider into the cache; a later lazy reconnect fails with "No credential found" → `ERR_UNAUTHORIZED`, leaving a broken row the user must remove. Only reachable for an unrecognized server type.

## Deferred from: code review of 9-10-tracks-browse-mode-dual-panel-ui (2026-06-08)

- **No row virtualization in the three panels** [`hifimule-ui/src/components/TracksBrowseView.ts:351`] — builds a real DOM node per item (artists/albums/tracks accumulate without recycling), unlike the main library's virtual rows. Deliberate deviation; perf concern only on very large libraries.
- **Provider total-undercount → premature pagination stop** [`hifimule-ui/src/components/TracksBrowseView.ts:293`] — exhaustion heuristic `items.length >= result.total || newItems.length < LIMIT` can stop early when Subsonic reports `total` as the page length rather than the global count. Provider-dependent; acknowledged in Dev Notes "Track Panel — Exhaustion Detection". Related to the deferred 9.9 Subsonic letter-filter pagination item above.
- **Scroll/selection restoration is conditional on nav-cache state** [`hifimule-ui/src/library.ts:956`] — returning to Tracks restores scroll only when `clearNavigationCache` was not called in between; otherwise a fresh instance loses position. Minor restoration-contract inconsistency.
- **`loadTracksView()` calls `load()` fire-and-forget** [`hifimule-ui/src/library.ts:964`] — the returned promise is neither awaited nor `.catch`-ed; low risk because each per-panel fetch catches internally.

## Deferred from: code review of 9-9-tracks-browse-mode-provider-contract-and-daemon-rpc (2026-06-08)

- **Letter filter post-application in Subsonic Branch 3 causes early pagination exhaustion** [`hifimule-daemon/src/providers/subsonic.rs:803`] — `search3_paged` pages server-side with `offset=start`, then `apply_letter_filter` discards songs; `filtered.len() < limit` makes the UI's exhaustion heuristic fire prematurely when matching songs exist beyond the current page window. Explicit v1 limitation documented in spec ("UI may need to fetch additional pages if prefix is rare"). Story 9.10 must account for this.
- **Subsonic Branch 3 with `limit=0` sends `songCount=0` to `search3` → empty response** [`hifimule-daemon/src/providers/subsonic.rs:792`] — Branches 1 and 2 treat `limit=0` as "return all" but Branch 3 passes `Some(0)` directly to `search3_paged` which yields zero results. Low probability since `browse_pagination` defaults to 50; only affects callers explicitly passing `limit=0`.
- **Jellyfin `list_tracks` with `limit=0` omits `Limit` param → unbounded server response** [`hifimule-daemon/src/providers/jellyfin.rs:765`] — Same pattern as `list_artists`/`list_albums`. Normal UI calls are safe (default 50). Large libraries could cause high memory usage if `limit=0` slips through.
- **`get_items` inserts `ArtistIds`/`AlbumIds`/`SortBy` via raw `format!` without URL encoding** [`hifimule-daemon/src/api.rs:332`] — All existing params use the same pattern. GUIDs don't contain injection characters, but the pattern is technically unsafe if future callers pass non-GUID values.
- **Subsonic `album_id`/`artist_id` branches in `list_tracks` callable without `open_subsonic` check** [`hifimule-daemon/src/providers/subsonic.rs:761`] — Defence-in-depth only; the RPC capability gate prevents classic Subsonic from reaching these branches. The underlying `get_album`/`get_artist` methods are also available on classic Subsonic.

Status: open
Last updated: 2026-06-06

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

## Deferred from: code review of 11-3-subsonicprovider-playlist-write-adapter (2026-06-05)

- Unbounded GET query-string length / HTTP 414 on large create/add/remove lists (hifimule-daemon/src/providers/subsonic.rs:841,854,867). All Subsonic playlist writes pack one query param per track/index into a single GET; very large playlists can exceed server/proxy URL limits (~8KB). Chunking is out of scope for 11.3 but should be considered before bulk playlist operations ship in later Epic 11 stories.
- `create_playlist` returns an empty string silently if the server responds `ok` with a missing `playlist.id` (hifimule-daemon/src/providers/subsonic.rs:841). `PlaylistWithSongsDto.id` is a non-`Option` `String` with `Default`, so a malformed-but-ok response yields `""` rather than an error. Low likelihood; matches the existing pattern in other read methods.
- `create_playlist` does not validate an empty/whitespace `name` (hifimule-daemon/src/providers/subsonic.rs:841). Not required by AC; the server rejects it and the error propagates.

## Deferred from: code review of 11-4-playlist-rpcs-and-selection-to-tracks-resolution (2026-06-05)

- **Input-validation leniency at playlist RPC handlers** (`hifimule-daemon/src/rpc.rs`): The four new playlist handlers accept empty `name`, empty `playlistId`, and silently drop non-string elements from `itemIds`/`trackIds` arrays via `filter_map`. This matches the established rpc.rs param-parsing convention (e.g. `rpc.rs:581` checks only `.as_str()` presence, not emptiness) and the only client is the trusted HifiMule UI, so it is consistent rather than a regression. A future codebase-wide param-validation hardening pass could reject empty/malformed values with `ERR_INVALID_PARAMS`.
- **Large `itemIds`/`trackIds` may exceed Subsonic GET URL length** (`hifimule-daemon/src/providers/subsonic.rs`): `playlist.create`/`addTracks` with very large resolved lists feed one query param per track into a single GET, which can hit ~8KB URL limits. Pre-existing provider-layer concern already noted in the 11.3 review deferral; the new RPC entry points make it reachable from larger basket selections. Chunking should be considered before bulk playlist operations ship.

## Deferred from: code review of story-11.5 (2026-06-06)

- No server-side empty/whitespace `name` validation in the `playlist.create` RPC handler (`hifimule-daemon/src/rpc.rs:844`). Pre-existing from Story 11.4; the daemon forwards any non-null string to the provider. Both 11.5 UI paths trim client-side, so the current UI is safe — defense-in-depth gap only, not introduced by this change.

## Deferred from: code review of 11-6-dual-panel-playlist-curation-view-and-stats (2026-06-06)

- **`basketStore` event listener leak compounded by curation close pattern** [`hifimule-ui/src/components/MediaCard.ts`] — `MediaCard.create()` attaches a `basketStore 'update'` listener for every card rendered and never removes it when the card is discarded from the DOM. Each `loadPlaylists()` call (including the one triggered on close from `openCurationView`) rebuilds all playlist cards and attaches fresh listeners, while prior listeners remain held by `basketStore`. Pre-existing issue across all browse modes; the curation close path adds one more cycle that reproduces it.

## Deferred from: code review of 11-6-dual-panel-playlist-curation-view-and-stats (2026-06-07)

- **`t()` i18n return values interpolated unescaped into `innerHTML`** [`hifimule-ui/src/components/PlaylistCurationView.ts`] — Strings from `t('playlist.curation.*')` are inserted directly into the HTML template without `escapeHtml`. While the catalog.json is a trusted static file (not user-supplied), escaping all i18n calls is a systemic change needed across the whole codebase, not scoped to this story.
- **Empty playlist shows two simultaneous empty-state messages** [`hifimule-ui/src/components/PlaylistCurationView.ts`] — When a playlist has zero tracks, both the artist panel ("Playlist is empty") and the track panel ("No tracks for this selection") display at once. No spec coverage for this edge case; low practical impact since empty playlists are rare.
- **`listViewMode` simplified from per-mode `Map<BrowseMode, 'grid' | 'list'>` to a single global value** [`hifimule-ui/src/library.ts`] — Switching to list mode in artists now affects albums too; the previous per-mode memory is lost. Part of the approved sprint-change-proposal-2026-06-07 (autoload-on-scroll) redesign; a deliberate simplification but may surprise users who expected independent per-mode preferences.

## Deferred from: code review of 11-7-add-tracks-to-playlist-browse-and-curation (2026-06-07)

- **`handle_playlist_add_items` reports partial success as full success** [`hifimule-daemon/src/rpc.rs`] — Unresolvable items are logged to stderr and skipped, returning `{ ok: true }` even if only some resolved (cf. `playlist.create` which returns `skippedItemIds`). Not reachable from the current UI (context menu always sends a single item), so robustness-only for now.
- **No single-dialog guard — overlapping Add-to-playlist / Add-tracks dialogs possible** [`hifimule-ui/src/components/MediaCard.ts`, `hifimule-ui/src/components/PlaylistCurationView.ts`] — Rapid re-trigger can spawn multiple stacked dialogs (the context menu uses `dismissActiveMenu`, dialogs do not). Low-impact UX polish.
- **Initial `browse.listPlaylists` failure leaves Add-to-playlist dialog with no retry** [`hifimule-ui/src/components/MediaCard.ts`] — Shows an error but clears the list with no in-dialog retry; user must close/reopen. Low-impact UX.

## Deferred from: code review of 11-8-playlist-rename-and-delete (2026-06-07)

- **No Enter-to-save / blur-to-commit on rename input** [`hifimule-ui/src/components/PlaylistCurationView.ts`:354] — Only Escape and explicit Save/Cancel resolve the inline rename edit; pressing Enter does nothing and clicking away leaves uncommitted text. UX enhancement, outside AC1–AC3 scope.
- **Re-render race during rename** [`hifimule-ui/src/components/PlaylistCurationView.ts`:127] — A concurrent `render()` (e.g. a track removal completing in another panel) rebuilds `innerHTML` and re-reads `this.playlistName`, wiping unsaved rename text and re-creating an open delete dialog. Low likelihood (requires editing the name while another async op completes).

## Deferred from: code review of 11-9-playlist-reorder-provider-and-rpc (2026-06-08)

- Jellyfin reorder is non-atomic — a mid-sequence `move_playlist_item` failure leaves the playlist partially reordered with no rollback; the RPC returns `Err` while the first N moves are already persisted server-side. Inherent to the per-entry Move design (no Jellyfin batch/transaction API); acceptable for DAP-sized playlists. [`hifimule-daemon/src/providers/jellyfin.rs` reorder loop]
- Jellyfin set-mismatch surfaces as `ERR_UNSUPPORTED_CAPABILITY` — a "track not in playlist" desync returns `ProviderError::UnsupportedCapability`, misleading an RPC client into thinking the provider lacks the capability rather than that the request was bad. Spec-prescribed code (Task 3); revisit if a dedicated desync error variant is introduced. [`hifimule-daemon/src/providers/jellyfin.rs` reorder loop]

## Deferred from: code review of story-11.10 (2026-06-08)

- **Remove/rename controls not gated by `supportsPlaylistWrite`** [`hifimule-ui/src/components/PlaylistCurationView.ts`] — Per-track/artist/album remove buttons and the title-click inline rename render unconditionally; only the new ↑/↓ move controls and the header delete button are capability-gated. On a read-only provider the user can still trigger `playlist.removeTracks`/`playlist.rename`, which only fail at the RPC. Pre-existing from stories 11.6/11.8; out of 11.10's frontend-reorder scope (AC6 is satisfied for the move controls).
- **Selecting a specific artist row does not reset `selectedAlbum`** [`hifimule-ui/src/components/PlaylistCurationView.ts`] — The All-artists handler resets both selections, but the per-artist row handler (unchanged by 11.10) sets `selectedArtist` without clearing `selectedAlbum`. The render-time guard only clears the album when its name is absent from the new artist's albums; a same-named album across artists leaves a stale filter on the track panel. Pre-existing.
- **Full re-render orphans an open delete dialog** [`hifimule-ui/src/components/PlaylistCurationView.ts`] — `#playlist-delete-dialog` lives inside the wholesale `innerHTML` replace; any concurrent `render()` (now including `moveTrack`'s optimistic/rollback renders) destroys an open dialog mid-interaction. Pre-existing architecture, widened by reorder.
- **Add-tracks confirm post-await runs after dialog/view dismissed** [`hifimule-ui/src/components/PlaylistCurationView.ts`] — If the add-tracks dialog is cancelled or the curation view is closed while the `addTracks` RPC is in flight, the post-await block still calls `render()` (on a possibly-detached container) and shows a success toast. No mounted/cancelled guard. Pre-existing from story 11.7.
- **fr/es `playlist.curation.error` & `add_tracks_error` hold English text** [`hifimule-i18n/catalog.json`] — The remove/add failure messages fall back to literal English in the fr and es blocks (placeholders are consistent). Pre-existing i18n gap; the new `reorder_error` key is correctly translated in all three blocks.
- **Duplicate track ids: #N badge collapses + reorder id-list ambiguous** [`hifimule-ui/src/components/PlaylistCurationView.ts`] — `positionById` (id→index Map) collapses to the last index for a repeated id, so duplicate rows show the same #N; and `playlist.reorder` receives `[…, X, X, …]` which the backend cannot disambiguate. The #N collapse is explicitly accepted by the spec (Task 3 caveat) and the swap logic itself is duplicate-safe (object-reference `indexOf`); the id-list ambiguity is the Story 11.9 RPC contract. Duplicates are reachable only because add-tracks (11.7) does not dedupe.

## Deferred from: code review of story-9.8 (2026-06-08)

- **Stale-mode race in `loadMoreForListView`** [hifimule-ui/src/library.ts:783] — the function reads `state.browseMode` and `breadcrumbStack.length` after the awaited fetch, with no load-sequence/mode token. If the user switches browse mode or navigates during the in-flight fetch, the returned items and `pagination.total` get written into the new view's `state.items`, corrupting the list and virtual-scroll height. Pre-existing pattern (already present for artists/albums prior to 9.8); widened to genres/history modes by this change. No sequence-token guard exists anywhere in the codebase yet.
- **`subsonic.rs::get_songs_by_genre` full-fetch + local pagination** [hifimule-daemon/src/providers/subsonic.rs:654] — now calls `get_songs_by_genre(genre, 0, 10_000)` and paginates locally. (a) Genres with >10,000 songs are silently truncated and report a capped total; (b) every paginated page request re-downloads the full song set (O(n²) network/CPU per scroll). Contingent on keeping the daemon change — resolve review Decision #1 (AC 5 autoload scope) first. A cache of the full per-genre fetch, or true server-side paging, would fix the perf issue.

## Deferred from: code review of story-2.13 (2026-06-09)

- **`get_provider_by_server_id` stale-manager race during concurrent reconnect** [hifimule-daemon/src/server_manager.rs:1583-1604] — reads in-memory `manager.servers` first, only falls back to `db.list_servers()` if no record found. During a reconnect that flips the portable id, the in-memory manager and DB can disagree for a window; a sync running concurrently may resolve to a stale provider. Narrow concurrency window — daemon currently expects connect operations to be quiescent.
- **`reconcile_manifest_server_ids` silent persistence failure** [hifimule-daemon/src/device/mod.rs:~340-356] — when `write_manifest` fails after an in-memory remap, the function logs and continues. Idempotent so harmless per session, but a persistent write failure (read-only device, permission error) is never surfaced. Consider one-shot toast or N-retry cap.
- **`handle_playlist_create` cross-server guard silently no-ops when `current_server_portable_id == None`** [hifimule-daemon/src/rpc.rs:~947-953] — backfill in `Database::init` guarantees `Some` in practice, but the new code introduces a window where the guard is a no-op rather than an error. Treat None as inconsistency error if this ever surfaces in logs.
- **`current_server_portable_id` connect→sync race vs credential file write** [hifimule-daemon/src/rpc.rs:1394-1417] — DB upsert + credential vault write + config.json `selected_server_id` are not atomic. Concurrent `server.connect` + `sync_calculate_delta` could see a portable id from the new server but credentials/selection from the old. Document "no concurrent connect" invariant or wrap the post-connect writes in a single critical section.
- **`serverIdsInBasket` / `removeItemsForServer` unaware of portable/local duality** [hifimule-ui/src/state/basket.ts:82-108] — exact-string match on stored `serverId`. If a reconciliation gap leaves some items local-tagged, `serverIdsInBasket()` would count a single logical server twice. Addressed indirectly by `ServerHub.handleRemove`'s double sweep, but other consumers (e.g. future stats panels) could be misled. Consider a canonicalize helper.

## Deferred from: code review of 9-12-track-multi-selection-and-bulk-actions (2026-06-12)

- **9.11 list-view bulk bar keyboard bypass of the device-locked gate** [hifimule-ui/src/library.ts:753-761] — same `pointer-events`-only gating the 9.12 Tracks-view bar replicated (the 9.12 instance is being patched in-story). Already on record from the 9.11 review above ("device-locked gate does not block keyboard activation"); re-confirmed here. Fix the 9.11 bulk bar and per-row gating with the `disabled` property in one pass.
- **Existing-playlist add path never invalidates the playlists cache** [hifimule-ui/src/components/MediaCard.ts:497-516] — `playlist.addItems` success runs hide → toast → `onSuccess` without `invalidatePlaylistsCache()`; only the create-new flow invalidates, leaving stale `playlists:*` pages in `state.pageCache`. Violates the letter of 9.12 AC 8 for the bulk path, but it is pre-existing 9.11 dialog behavior and the 9.12 story forbids touching MediaCard.ts ("consume as-is"). Deferred per review decision (2026-06-12): fix is one line in the existing-playlist success path whenever MediaCard.ts is next open for changes.

## Deferred from: code review of story-12.2 (2026-06-14)

- **Multi-server accessor seam — `resolve_pipeline` single-entry fallback + unkeyed `main.rs` reads.** `resolve_pipeline` (`device/mod.rs:279-289`) returns the sole pipeline even when the caller passed a *non-matching* `Some(server_id)` (cross-server config bleed), and returns `None` for an unkeyed `None` caller once 2+ pipelines exist — so the `main.rs` auto-sync paths reading via `legacy_enabled()`/`legacy_max_bytes()` (`:578,581,1119,1141,1143`) would report auto-fill **disabled** on a multi-server install whose connected server's pipeline is enabled. Not triggerable in Story 12.2 (only one pipeline is ever created — migration/`set_for` target the single selected server), so behavior is correct today. Becomes a real regression when Story 12.3 introduces multi-slot/multi-pipeline expansion: 12.3 must give the `main.rs` consumers server context (resolve the selected/connected portable id, as `rpc.rs` already does) and add test coverage for the mismatched-id and multi-pipeline-unkeyed cases.
- **`autofill_history` timestamp unit & NULL semantics undefined (Epic 13).** `last_synced_at INTEGER` and `tier TEXT` are nullable with no documented meaning for NULL, and the timestamp unit (seconds vs millis) is unspecified. Pure scaffolding in 12.2 (no reads/writes), but the column set becomes a contract Epic 13 inherits — pin the unit and NULL semantics (ideally in a comment) when Epic 13 first writes rows.

## Deferred from: code review of story-12.3 (2026-06-14)

- **Duplicate `serverId` across two enabled auto-fill descriptors → redundant full provider pagination** (`hifimule-daemon/src/rpc.rs:~3640`, `multi_provider_calculate_delta` loop). When the array carries two enabled descriptors for the same server, `run_auto_fill_provider` paginates that server's entire library twice; the second pass excludes everything the first already selected via `seen_ids`, so the result is correct but the network/server cost is doubled. Deferred: low priority — the Story 12.6 UI owns descriptor generation and will not emit duplicate per-server slots, and no current caller triggers it. Revisit if manual/array payloads can ever carry duplicate serverIds.

## Deferred from: code review of story-12.4 (2026-06-14)

- **Invalid share totals can starve later sources** (`hifimule-daemon/src/auto_fill/pipeline.rs:559`). `SourceEntry.share` is documented as `0.0..=1.0`, but the 12.1 pure engine does not validate total explicit share weight. A malformed hand-written manifest with shares summing above 1.0 can let earlier sources consume the global ceiling before later sources get useful budget. Deferred as pre-existing engine/config-validation work; address with future pipeline validation or UI/RPC config hardening.
