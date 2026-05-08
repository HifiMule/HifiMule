# Story 3.8: Lazy Auto-Fill Virtual Slot

Status: done

## Story

As a Convenience Seeker (Sarah),
I want to enable Auto-Fill with a single toggle and have the device fill with my best music at sync time,
so that I don't wait for a slow basket population and always get the freshest track selection when I actually sync.

## Acceptance Criteria

1. **Toggle inserts a virtual slot card (no RPC call):**
   - Given the basket sidebar is visible with a connected device
   - When I enable the "Auto-Fill" toggle
   - Then a single "Auto-Fill Slot" card appears in the basket (not individual tracks)
   - And the card shows the configured capacity target (e.g. "Fill remaining 12.4 GB" or the user-set max)
   - And no Jellyfin API call is made at this point

2. **Slot appears after manual items with capacity projection:**
   - Given manual items and the Auto-Fill Slot are in the basket
   - When I view the basket
   - Then manual items appear as individual cards above the Auto-Fill Slot
   - And the Auto-Fill Slot shows "Will fill ~X GB with top-priority tracks at sync time"
   - And storage projection includes the slot's target bytes in the capacity bar

3. **Daemon expands slot to real tracks at sync time:**
   - Given the basket contains the Auto-Fill Slot
   - When I click "Start Sync"
   - Then the daemon runs the priority algorithm (`run_auto_fill`) at the start of delta calculation
   - And expands the slot to real track IDs (favorites first, then play count, then newest)
   - And excludes any track IDs already covered by manual basket items
   - And the expanded track list is merged with manual items for the sync operation
   - And the UI shows real-time progress exactly as today (files completed, current filename)

4. **Toggle off removes slot immediately:**
   - Given Auto-Fill is enabled
   - When I toggle it off
   - Then the Auto-Fill Slot is removed from the basket immediately (no API call)

## Tasks / Subtasks

- [x] Task 1: Update `basket.ts` — remove eager auto-fill, add AutoFillSlot support (AC: #1, #2, #4)
  - [x] 1.1 Add export `export const AUTO_FILL_SLOT_ID = '__auto_fill_slot__'` at top of file
  - [x] 1.2 Remove the `replaceAutoFilled()` method entirely
  - [x] 1.3 Update `getManualItemIds()`: filter out items where `id === AUTO_FILL_SLOT_ID` (previously filtered `!i.autoFilled`)
  - [x] 1.4 Update `getTotalSizeBytes()`: AutoFillSlot's `sizeBytes` already contributes to total — no change needed, the existing loop covers it
  - [x] 1.5 Keep `autoFilled?: boolean` and `priorityReason?: string` fields in `BasketItem` interface (backward compat: old manifests may have hydrated items with these flags; they'll be treated as regular items)

- [x] Task 2: Update `BasketSidebar.ts` — replace eager trigger with virtual slot insertion (AC: #1, #4)
  - [x] 2.1 Remove these private fields: `autoFillDebounceTimer`, `autoFillInFlight`, `autoFillPendingRetrigger`, `isAutoFillLoading`
  - [x] 2.2 Remove these methods: `triggerAutoFill()`, `scheduleAutoFill()`
  - [x] 2.3 On auto-fill toggle enable (`sl-change` event, `checked = true`): call `persistAutoFillPrefs()` then call new `insertAutoFillSlot()`
  - [x] 2.4 On auto-fill toggle disable (`checked = false`): call `persistAutoFillPrefs()`, call `basketStore.remove(AUTO_FILL_SLOT_ID)`, call `basketStore.replaceAutoFilled([])` — **wait**, `replaceAutoFilled` is removed. Instead: just call `basketStore.remove(AUTO_FILL_SLOT_ID)`
  - [x] 2.5 Add `private insertAutoFillSlot()`: compute `targetBytes` (= `autoFillMaxBytes ?? (storageInfo?.freeBytes ?? 0) - basketStore.getManualSizeBytes()`), insert `basketStore.add({ id: AUTO_FILL_SLOT_ID, name: 'Auto-Fill', type: 'AutoFillSlot', childCount: 0, sizeTicks: 0, sizeBytes: Math.max(targetBytes, 0) })` — only if slot not already present
  - [x] 2.6 On slider change: update `autoFillMaxBytes`, call `persistAutoFillPrefs()`, then call `insertAutoFillSlot()` (which replaces/updates slot via `basketStore.add()` — this overwrites the existing entry since Map key is same `AUTO_FILL_SLOT_ID`)
  - [x] 2.7 On device hydration: remove the `this.triggerAutoFill()` call; instead if `this.autoFillEnabled && !basketStore.has(AUTO_FILL_SLOT_ID)` → call `insertAutoFillSlot()` (handles devices where slot was not saved to manifest)
  - [x] 2.8 On device disconnect / `clearForDevice()`: slot is cleared automatically since `basketStore.clearForDevice()` clears all items including slot
  - [x] 2.9 Add `private renderAutoFillSlotCard(item: BasketItem): string` for the virtual slot card — see Dev Notes for HTML template
  - [x] 2.10 Update `renderItem()`: if `item.id === AUTO_FILL_SLOT_ID`, return `this.renderAutoFillSlotCard(item)` instead of the regular card HTML
  - [x] 2.11 Update start-sync button disabled logic: remove `|| this.isAutoFillLoading` condition (new logic: `!basketStore.isDirty() && !this.autoFillEnabled` stays the same concept; since slot is now a real basket item, `basketStore.isDirty()` will be true when slot is present)

- [x] Task 3: Update `BasketSidebar.ts` — `handleStartSync()` passes auto-fill params to delta (AC: #3)
  - [x] 3.1 Before building `itemIds`, detect slot: `const autoFillSlot = basketStore.getItems().find(i => i.id === AUTO_FILL_SLOT_ID)`
  - [x] 3.2 Build `itemIds` from all items **excluding** the slot: `const manualIds = currentItems.filter(i => i.id !== AUTO_FILL_SLOT_ID).map(i => i.id)`
  - [x] 3.3 Build delta request: `const deltaParams: any = { itemIds: manualIds }; if (autoFillSlot) { deltaParams.autoFill = { enabled: true, maxBytes: autoFillSlot.sizeBytes || undefined, excludeItemIds: manualIds }; }`
  - [x] 3.4 Call `await rpcCall('sync_calculate_delta', deltaParams)` — no other changes to the rest of `handleStartSync()`

- [x] Task 4: Update `rpc.rs` — `handle_sync_calculate_delta` expands auto-fill slot (AC: #3)
  - [x] 4.1 Extract optional `autoFill` params after `item_ids`: `let auto_fill_param = params.get("autoFill")` (optional object with `enabled: bool`, `maxBytes?: u64`, `excludeItemIds: Vec<String>`)
  - [x] 4.2 If `auto_fill_param.enabled == true`: determine `max_fill_bytes` from `maxBytes` param or fall back to `state.device_manager.get_device_storage().await.map(|s| s.free_bytes).unwrap_or(0)`
  - [x] 4.3 Expand `excludeItemIds` using existing `expand_exclude_ids(&state.jellyfin_client, ...)` helper (rpc.rs:1555) — this expands albums/playlists to their track IDs
  - [x] 4.4 Call `crate::auto_fill::run_auto_fill(&state.jellyfin_client, AutoFillParams { exclude_item_ids: expanded_exclude_ids, max_fill_bytes })` — identical to the pattern in `handle_basket_auto_fill` (rpc.rs:1531–1536)
  - [x] 4.5 Convert `AutoFillItem` results into `DesiredItem` using the existing `to_desired_item` closure or inline the same logic: `jellyfin_id: item.id, name: item.name, album: item.album, artist: item.artist, size_bytes: item.size_bytes, etag: None`
  - [x] 4.6 Append auto-fill desired items to `desired_items` vec **before** the `seen_ids` dedup pass (dedup already handles duplicates with manual items — auto-fill items added after manual items lose the race, which is correct: manual > auto-fill)
  - [x] 4.7 If `run_auto_fill` errors, return `Err(JsonRpcError { code: ERR_CONNECTION_FAILED, message: format!("Auto-fill expansion failed: {}", e), data: None })`

- [x] Task 5: CSS — AutoFillSlot card styling (AC: #1, #2)
  - [x] 5.1 Add `.basket-item-auto-fill-slot` class styles in `hifimule-ui/src/styles.css`:
    - Dashed border (e.g. `border: 1.5px dashed var(--sl-color-primary-500)`)
    - Subtle background tint (e.g. `background: color-mix(in srgb, var(--sl-color-primary-100) 20%, transparent)`)
    - No thumbnail/image area (slot card has no artwork)
    - Flex row with slot icon (use `sl-icon name="stars"` or `"magic"`) and text block

## Dev Notes

### Architecture Overview

This story replaces the **eager auto-fill model** (Story 3.6, marked superseded) with a **lazy virtual slot model**:

| Aspect | OLD (Story 3.6 / current) | NEW (Story 3.8) |
|--------|--------------------------|-----------------|
| When auto-fill toggle enabled | Immediately calls `basket.autoFill` RPC, fills basket with N track items | Inserts single `__auto_fill_slot__` virtual item — no RPC |
| Basket contents | Many auto-fill track cards with "Auto" badge | Manual items + one "Auto-Fill Slot" card |
| Auto-fill algorithm | Runs eagerly, blocks UI | Runs at sync time inside `sync_calculate_delta` daemon handler |
| Slider change | Re-triggers full auto-fill RPC | Updates slot's `sizeBytes` — instant, no network |
| Manifest save | Saves many track IDs with `autoFilled:true` | Saves one `__auto_fill_slot__` virtual item |

### Files to Touch

| File | Change |
|------|--------|
| `hifimule-ui/src/state/basket.ts` | Remove `replaceAutoFilled()`, update `getManualItemIds()`, add `AUTO_FILL_SLOT_ID` export |
| `hifimule-ui/src/components/BasketSidebar.ts` | Remove eager trigger, add slot insert/remove, update `renderItem()`, update `handleStartSync()` |
| `hifimule-ui/src/styles.css` | Add `.basket-item-auto-fill-slot` styles |
| `hifimule-daemon/src/rpc.rs` | Extend `handle_sync_calculate_delta` to accept and process `autoFill` param |

**No changes needed:** `main.rs` (daemon-initiated auto-sync path unchanged), `auto_fill.rs` (untouched), `device/mod.rs` (untouched), `BasketItem.autoFilled` field (keep for backwards compat).

### Existing Code to Reuse — DO NOT Reinvent

| What | Where | How to Reuse |
|------|-------|-------------|
| `basketStore.add()` / `basketStore.remove()` | `basket.ts:152,160` | Use directly to insert/remove the slot item |
| `basketStore.getManualItemIds()` | `basket.ts:212` | Returns IDs of non-slot items; update to filter by `id !== AUTO_FILL_SLOT_ID` |
| `basketStore.has()` | `basket.ts:148` | Check `basketStore.has(AUTO_FILL_SLOT_ID)` before inserting |
| `persistAutoFillPrefs()` | `BasketSidebar.ts:344` | Keep as-is — still needed for daemon-initiated auto-sync to know `autoFill.enabled` |
| `handle_basket_auto_fill` pattern | `rpc.rs:1490–1547` | Mirror its `expand_exclude_ids` + `run_auto_fill` + params extraction in `handle_sync_calculate_delta` |
| `expand_exclude_ids()` | `rpc.rs:1555` | Call directly from `handle_sync_calculate_delta` — already in same file |
| `to_desired_item` closure | `rpc.rs:818` | **Cannot reuse directly** — it's a local closure in `handle_sync_calculate_delta`. Since auto-fill items are already `AutoFillItem` structs (not raw `JellyfinItem`), map them inline: `DesiredItem { jellyfin_id: item.id, name: item.name, album: item.album, artist: item.artist, size_bytes: item.size_bytes, etag: None }` |
| `seen_ids` dedup | `rpc.rs:917` | Auto-fill items added after manual items are naturally deduplicated here |

### AutoFillSlot Card HTML Template

```html
<div class="basket-item-card basket-item-auto-fill-slot" data-id="__auto_fill_slot__">
    <div class="basket-item-auto-fill-icon">
        <sl-icon name="stars"></sl-icon>
    </div>
    <div class="basket-item-info">
        <div class="basket-item-name">Auto-Fill Slot</div>
        <div class="basket-item-meta">
            Will fill ~${formatSize(item.sizeBytes)} with top-priority tracks at sync time
        </div>
    </div>
    <sl-icon-button name="x" class="remove-item-btn" data-id="__auto_fill_slot__" label="Remove"></sl-icon-button>
</div>
```

Clicking "×" removes the slot AND sets `autoFillEnabled = false` + updates the toggle UI (call `persistAutoFillPrefs()` after removal).

### `insertAutoFillSlot()` Implementation Hint

```typescript
private insertAutoFillSlot(): void {
    const manualSize = basketStore.getManualSizeBytes();
    const available = this.storageInfo
        ? Math.max(this.storageInfo.freeBytes - manualSize, 0)
        : 0;
    const targetBytes = this.autoFillMaxBytes !== null
        ? Math.min(this.autoFillMaxBytes, available || this.autoFillMaxBytes)
        : available;
    basketStore.add({
        id: AUTO_FILL_SLOT_ID,
        name: 'Auto-Fill',
        type: 'AutoFillSlot',
        childCount: 0,
        sizeTicks: 0,
        sizeBytes: targetBytes,
    });
}
```

### Daemon `sync_calculate_delta` Auto-Fill Extension

Current param: `{ itemIds: string[] }`  
New param: `{ itemIds: string[], autoFill?: { enabled: boolean, maxBytes?: number, excludeItemIds: string[] } }`

Insert auto-fill handling **after** `desired_items` is built from manual `item_ids` (line ~915 in rpc.rs), **before** the delta calculation (line ~935):

```rust
// Auto-fill expansion (Story 3.8): if auto-fill slot was in the basket,
// run the priority algorithm and merge results.
if let Some(af) = params.get("autoFill") {
    if af["enabled"].as_bool().unwrap_or(false) {
        let max_fill_bytes = af["maxBytes"].as_u64().unwrap_or_else(|| {
            state.device_manager.get_device_storage()
                .await  // this is inside an async fn, so .await is fine
                // Note: need to restructure — see below
                .map(|s| s.free_bytes)
                .unwrap_or(0)
        });
        let exclude_ids: Vec<String> = af["excludeItemIds"].as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();
        let expanded_excludes = expand_exclude_ids(&state.jellyfin_client, exclude_ids).await;
        let fill_params = crate::auto_fill::AutoFillParams {
            exclude_item_ids: expanded_excludes,
            max_fill_bytes,
        };
        if let Ok(af_items) = crate::auto_fill::run_auto_fill(&state.jellyfin_client, fill_params).await {
            for item in af_items {
                if seen_ids.insert(item.id.clone()) {
                    desired_items.push(crate::sync::DesiredItem {
                        jellyfin_id: item.id,
                        name: item.name,
                        album: item.album,
                        artist: item.artist,
                        size_bytes: item.size_bytes,
                        etag: None,
                    });
                }
            }
        } else {
            return Err(JsonRpcError {
                code: ERR_CONNECTION_FAILED,
                message: "Auto-fill expansion failed at sync time".to_string(),
                data: None,
            });
        }
    }
}
```

**Important:** `seen_ids` and `desired_items` are populated during the manual-items loop (lines ~917–933 in rpc.rs). Insert the auto-fill block **after** that loop completes, so manual items win dedup. Reconstruct `seen_ids` from `desired_items` before the auto-fill block if necessary, or track `seen_ids` inline.

Wait — looking at the actual code: the current loop at line ~917 uses `seen_ids` to dedup `desired_items`. The auto-fill block must be inserted **after** this loop. Since `seen_ids` is already populated with manual item IDs, auto-fill items with the same IDs will be skipped automatically. ✓

### Critical Constraints

- **No RPC call on toggle**: The whole point of this story is zero API calls until sync starts. If you find yourself calling `basket.autoFill` from the UI on toggle, that's the old pattern — remove it.
- **`basket.autoFill` RPC stays**: It remains in the daemon as a preview/debug endpoint. Do NOT remove it. Just stop calling it from `BasketSidebar.ts`.
- **`autoFillEnabled` field stays**: `BasketSidebar.ts` still tracks `autoFillEnabled` in-memory for UI rendering (toggle state, slot insertion logic). This is fine.
- **`persistAutoFillPrefs()` stays**: The daemon-initiated auto-sync path (`main.rs:503`) still reads `manifest.auto_fill.enabled`. `sync.setAutoFill` must continue to be called on toggle change and slider change so that auto-sync-on-connect continues to work correctly.
- **Slot removal on × click**: When the slot's × button is clicked, `basketStore.remove(AUTO_FILL_SLOT_ID)` is already handled by the existing `.remove-item-btn` event handler (line ~744). But `this.autoFillEnabled` must also be set to `false` and the toggle synced. Hook this in `bindAutoFillEvents()` or by catching the basketStore `update` event and checking if slot was removed externally.
- **Slider still persists via `persistAutoFillPrefs()`**: Do NOT remove the `persistAutoFillPrefs()` call from the slider `sl-change` handler — only remove `scheduleAutoFill()` from it.
- **Backwards compat**: Old manifests with `autoFilled: true` track items (from Story 3.6) will be hydrated normally. They'll appear as regular basket items without the slot. This is acceptable behaviour — next device connect will be clean.

### Previous Story Learnings (from Story 3.7)

- **No Daemon changes for pure UI features** — but this story DOES have daemon changes (rpc.rs). Unlike 3.7, `rpc.rs` must be touched.
- **Device event coordination**: `clearForDevice()` already clears the entire basket including the slot — no additional cleanup needed.
- **`basketStore.add()` is idempotent for the same ID** — calling `insertAutoFillSlot()` when slot already exists overwrites the entry (Map semantics), which is the desired behaviour when the slider changes.
- **Start sync button enable logic**: Currently `(!basketStore.isDirty() && !this.autoFillEnabled) || this.isAutoFillLoading`. New logic: `!basketStore.isDirty() && !this.autoFillEnabled` (remove `|| this.isAutoFillLoading`). When slot is in basket, `basketStore.isDirty()` is `true`, so button is enabled regardless of `autoFillEnabled`.

### Project Structure Notes

- UI files: `hifimule-ui/src/state/basket.ts`, `hifimule-ui/src/components/BasketSidebar.ts`, `hifimule-ui/src/styles.css`
- Daemon file: `hifimule-daemon/src/rpc.rs`
- No new files needed

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Story 3.8 — full tech notes and acceptance criteria]
- [Source: hifimule-ui/src/components/BasketSidebar.ts — autoFill fields at lines 149–158, triggerAutoFill at 267, scheduleAutoFill at 334, renderAutoFillControls at 418, handleStartSync at 784]
- [Source: hifimule-ui/src/state/basket.ts — replaceAutoFilled at 186, getManualItemIds at 212]
- [Source: hifimule-daemon/src/rpc.rs — handle_basket_auto_fill at 1490, expand_exclude_ids at 1555, handle_sync_calculate_delta at 776]
- [Source: hifimule-daemon/src/auto_fill.rs — AutoFillParams/AutoFillItem structs at lines 14–31, run_auto_fill at 37]
- [Source: hifimule-daemon/src/main.rs:503 — daemon-initiated auto-fill path that must remain functional]

## Dev Agent Record

### Agent Model Used

claude-sonnet-4-6

### Debug Log References

### Completion Notes List

- Replaced eager auto-fill model with lazy virtual slot: toggle now inserts a single `__auto_fill_slot__` item with no RPC call, and the daemon expands it at sync time inside `handle_sync_calculate_delta`.
- Removed `replaceAutoFilled()`, `triggerAutoFill()`, `scheduleAutoFill()` and the associated fields (`autoFillDebounceTimer`, `autoFillInFlight`, `autoFillPendingRetrigger`, `isAutoFillLoading`).
- Added `AUTO_FILL_SLOT_ID` constant exported from `basket.ts`. Updated `getManualItemIds()` and `getManualSizeBytes()` to exclude the virtual slot.
- `insertAutoFillSlot()` computes available space and inserts/overwrites the slot via `basketStore.add()` (Map semantics make this idempotent on slider change).
- `handleStartSync()` strips the slot from `itemIds`, passes `autoFill` params to `sync_calculate_delta`; daemon runs `run_auto_fill` and merges results before `calculate_delta`.
- `× click` on slot card also sets `autoFillEnabled = false` and calls `persistAutoFillPrefs()` to keep toggle state consistent.
- `getManualSizeBytes()` also updated to exclude slot (implied by architecture) to prevent recursive size calculation when slot is updated by slider.
- TypeScript: 0 errors. Rust: 0 errors, 0 new warnings, 163 tests passed.

### File List

- hifimule-ui/src/state/basket.ts
- hifimule-ui/src/components/BasketSidebar.ts
- hifimule-ui/src/styles.css
- hifimule-daemon/src/rpc.rs

## Change Log

- 2026-04-03: Implemented lazy auto-fill virtual slot — replaced eager basket.autoFill RPC trigger with a single AutoFillSlot card; daemon now expands the slot to real tracks at sync time inside handle_sync_calculate_delta.
