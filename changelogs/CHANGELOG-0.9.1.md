# HifiMule 0.9.1

Release date: 2026-06-05

## Highlights

- **Cancel a running sync**: A "Cancel Sync" button is now shown while a sync is in progress. Cancellation stops after the current file transfer so no file is left half-written; the device manifest stays dirty, allowing the next connect to resume cleanly.
- **Sync preview explains why each file is changing**: Every file in the sync delta now carries a reason code (new selection, removed from selection, bitrate increased, device file missing, transcoding profile changed, etc.). The preview screen groups these into a readable summary so you know at a glance why a large sync is happening.
- **Clearer UI language throughout the basket and repair flow**: Technical internal terms ("Manifest Dirty", "Repair Manifest First", "Sync Proposed") are replaced with plain-language labels ("Repair needed", "Repair device first", "Basket changed - ready to sync"). The basket clear action now asks for confirmation.

---

## Added

### Sync cancellation (spec-stop-sync-daemon-cancellation)

A per-operation `AtomicBool` cancel token is created in `SyncOperationManager` when each sync starts. The UI exposes a **Cancel Sync** button that calls the new `sync_cancel` RPC. The sync loop in both `execute_sync` (Jellyfin) and `execute_provider_sync` (Subsonic/Navidrome) polls `is_cancelled()` between file transfers and returns early on the first `true` result, leaving the manifest dirty for dirty-resume on next connect. A new `SyncStatus::Cancelled` variant is serialised as `"cancelled"` and drives a "Sync cancelled" state in the UI with a Retry Sync button.

UI states added:
- `isCancelling: boolean` tracks the in-flight cancel request and shows "Cancelling…" on the button.
- On cancelled/failed completion, the basket sidebar shows a **Retry Sync** button.
- On successful completion, a summary line shows the number of files transferred and total size.

---

### Sync delta reason annotations

Each `SyncAddItem`, `SyncDeleteItem`, and `SyncIdChangeItem` now carries `reason_code: Option<String>` and `reason: Option<String>`. These are populated wherever the delta is built:

| Code | Display reason |
|---|---|
| `new-selection` | new selection |
| `removed-selection` | removed from sync selection |
| `transcoding-profile-change` | transcoding profile changed |
| `music-folder-change` | music folder changed |
| `bitrate-increase` | source bitrate increased |
| `bitrate-missing` | previous sync did not record source bitrate |
| `device-file-missing` | device file is missing |
| `server-id-change` | server item ID changed |
| `force-sync` | force sync requested |

`change_reason_summary()` aggregates counts across the full delta (pairing add+delete for the same ID to avoid double-counting replacements) and returns them sorted by descending count. This summary is shown in the sync preview and the destructive-cleanup confirmation dialog.

---

## Fixed

### Navidrome/Subsonic false "sync not needed" result

A code path in `execute_provider_sync` was reaching the "sync not needed" branch even when the calculated delta was non-empty. The delta is now checked for emptiness after it is fully built and annotated, so a sync with pending adds or deletes always proceeds to the confirmation screen.

---

## Changed

### Plain-language basket and repair labels (EN / FR / ES)

All three locales updated:

| Old | New |
|---|---|
| Device (dirty) | Device needs repair |
| Manifest Dirty | Repair needed |
| Open Manifest Repair | Open sync repair |
| Repair Manifest First | Repair device first |
| Sync Proposed - Basket changed | Basket changed - ready to sync |

New keys added: `basket.actions.cancel_sync`, `basket.sync.cancelling`, `basket.sync.cancelled`, `basket.actions.retry_sync`, `basket.confirm.clear_all`, `basket.sync.complete_summary`.

### Basket clear requires confirmation

Clicking the clear-all button in the basket sidebar now triggers a confirmation dialog ("Clear basket? This will remove {count} items.") before removing items, preventing accidental wipes.

### Repair modal fully localised

`RepairModal.ts` previously had all UI strings hardcoded in English. All labels, section headings, button text, and tooltip content are now resolved through the `t()` helper and are translated in English, French, and Spanish. New i18n keys added under the `repair.*` namespace.

### Basket sidebar styling refresh

Layout, spacing, and icon choices in the basket sidebar were revised: the sync button now uses the `box-arrow-in-down` icon instead of `cloud-download`, and the overall visual structure of sync-state panels is tightened.

---

## Internal

- `SyncOperationManager` grows a `cancel_tokens: Arc<RwLock<HashMap<String, Arc<AtomicBool>>>>` field. Tokens are inserted on operation create and never removed (old entries for completed operations are naturally sized at one `AtomicBool` per UUID).
- New `sync_cancel` RPC handler in `rpc.rs` delegates to `SyncOperationManager::request_cancel`.
- `SyncStatus` enum gains a `Cancelled` variant (`#[serde(rename = "cancelled")]`).
- `BasketSidebar` tracks `isCancelling`, `completedFilesCount`, and `completedBytesCount` fields.
