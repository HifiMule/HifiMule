# Story 2.8: Enhanced Multi-Device Hub

Status: done

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a System Admin (Alexis) and Ritualist (Arthur),
I want a persistent device hub I can always interact with — switching between connected devices or deselecting one entirely,
So that I have full, iTunes-style control over which device I'm working with at all times.

## Acceptance Criteria

1. **Hub Always Visible:**
   - **Given** the main UI is open and 1 or more managed devices are connected
   - **Then** the device hub panel is always displayed — NOT hidden for a single device (replaces the `connectedDevices.length <= 1` guard from Story 2.7)
   - **And** each device card shows its icon (from manifest `icon` field; fallback: `usb-drive` Shoelace icon) and its display name (`name` fallback: `deviceId`)
   - **And** the currently selected device card is highlighted with an active/accent style

2. **No-Device-Selected Locked State:**
   - **Given** no device is selected (`selectedDevicePath === null`) with one or more devices connected
   - **Then** the basket body shows a placeholder: "Select a device to start curating"
   - **And** no storage projection bar is rendered
   - **And** all (+) add buttons in the library browser are visually disabled (greyed-out, `pointer-events: none`)
   - **And** the "Start Sync" button is disabled

3. **Device Selection via Hub:**
   - **Given** the device hub is visible
   - **When** I click a device card
   - **Then** the UI calls `device.select` RPC and reloads that device's basket
   - **And** the clicked card becomes highlighted as active

4. **All Devices Disconnected:**
   - **Given** all devices are disconnected (`connectedDevices.length === 0`)
   - **Then** the device hub is not shown (existing no-device behavior from Story 2.7 is preserved)
   - **And** the no-device-selected locked state is shown (no add buttons, no Start Sync)

## Tasks / Subtasks

- [x] **Frontend: Update `connectedDevices` type in `BasketSidebar.ts`** (AC: #1)
  - [x] Add `icon?: string | null` to the array type: `Array<{ path: string; deviceId: string; name: string; icon?: string | null }>`
  - [x] Forward-compatible with Story 2.9 which adds `icon` to the daemon response

- [x] **Frontend: Rename and rewrite `renderDevicePicker()` → `renderDeviceHub()`** (AC: #1, #2, #4)
  - [x] Remove `if (this.connectedDevices.length <= 1) return '';` guard
  - [x] Add new guard: `if (this.connectedDevices.length === 0) return '';` (no hub when no devices at all)
  - [x] Replace `<sl-select>` / `<sl-option>` with device card divs:
    ```html
    <div class="device-hub-panel">
      <div class="device-hub-cards">
        ${this.connectedDevices.map(d => `
          <div class="device-hub-card ${d.path === this.selectedDevicePath ? 'active' : ''}"
               data-path="${this.escapeHtml(d.path)}">
            <sl-icon name="${this.escapeHtml(d.icon ?? 'usb-drive')}"
                     class="device-hub-icon"></sl-icon>
            <span class="device-hub-name">${this.escapeHtml(d.name || d.deviceId)}</span>
          </div>
        `).join('')}
      </div>
    </div>
    ```
  - [x] Update all call sites: rename `renderDevicePicker()` → `renderDeviceHub()` in both render paths (empty basket at line ~592 and non-empty basket at line ~642)

- [x] **Frontend: Add locked basket placeholder when no device selected** (AC: #2)
  - [x] In `render()`, after the existing early-exit guards (sync/error/progress) and BEFORE the `const items = basketStore.getItems()` check, add:
    ```typescript
    if (this.selectedDevicePath === null && this.connectedDevices.length > 0) {
        this.renderLockedBasket();
        return;
    }
    ```
  - [x] Implement `private renderLockedBasket(): void`:
    ```typescript
    private renderLockedBasket(): void {
        this.container.innerHTML = `
            <div class="basket-header">
                <h2>Basket</h2>
                <sl-badge variant="neutral" pill>0</sl-badge>
            </div>
            <div class="basket-placeholder">
                <sl-icon name="usb-drive" style="font-size: 2rem; opacity: 0.5;"></sl-icon>
                <p style="opacity: 0.5;">Select a device to start curating</p>
            </div>
            <div class="basket-footer">
                ${this.renderDeviceHub()}
                ${this.renderDeviceFolders()}
            </div>
            <div class="basket-actions">
                <sl-button id="start-sync-btn" variant="primary" style="width: 100%;" disabled>
                    <sl-icon slot="prefix" name="cloud-download"></sl-icon>
                    Start Sync
                </sl-button>
            </div>
        `;
        this.bindDeviceHubEvents();
    }
    ```

- [x] **Frontend: Disable Start Sync when no device selected** (AC: #2)
  - [x] In the empty basket render path, update the disabled logic on `#start-sync-btn`:
    ```typescript
    ${(!basketStore.isDirty() && !this.autoFillEnabled) || !this.selectedDevicePath ? 'disabled' : ''}
    ```
  - [x] In the non-empty basket render path, add `|| !this.selectedDevicePath` to the disabled condition on the "Start Sync" / repair-first paths where applicable

- [x] **Frontend: Emit `device-locked` CSS class on library container** (AC: #2, #4)
  - [x] At the end of `render()` (and `renderLockedBasket()`), toggle the `device-locked` class on the `#library-content` container:
    ```typescript
    const libraryContent = document.getElementById('library-content');
    if (libraryContent) {
        libraryContent.classList.toggle('device-locked',
            this.selectedDevicePath === null);
    }
    ```
  - [x] Call this in `render()` unconditionally (before any `return` in the method body) — add it as a side-effect call at the top of `render()` after the syncing/error early-returns, OR ensure every render path calls it
  - [x] Also call in `renderLockedBasket()` and all early-return syncing render paths (or centralize in a `private updateDeviceLockState()` helper)

- [x] **Frontend: Rename `bindDevicePickerEvents()` → `bindDeviceHubEvents()`** (AC: #3)
  - [x] Replace `sl-select.device-picker` + `sl-change` listener with `.device-hub-card` click listener:
    ```typescript
    private bindDeviceHubEvents(): void {
        this.container.querySelectorAll('.device-hub-card').forEach(card => {
            card.addEventListener('click', async () => {
                if (this.deviceSwitchInFlight) return;
                const path = (card as HTMLElement).dataset.path;
                if (!path) return;
                this.deviceSwitchInFlight = true;
                try {
                    await basketStore.flushPendingSave();
                    await rpcCall('device.select', { path });
                    const basketResult = await rpcCall('manifest_get_basket') as any;
                    basketStore.hydrateFromDaemon(basketResult?.basketItems ?? []);
                    this.refreshAndRender();
                } catch (err) {
                    console.error('[DeviceHub] Failed to switch device:', err);
                } finally {
                    this.deviceSwitchInFlight = false;
                }
            });
        });
    }
    ```
  - [x] Update all call sites of `bindDevicePickerEvents()` to `bindDeviceHubEvents()` (2 call sites: in the empty basket render path ~line 611 and in the non-empty basket render path post-render binding)

- [x] **Frontend: CSS — device hub cards and locked state** (AC: #1, #2)
  - [x] Add to `hifimule-ui/src/styles.css`:
    ```css
    /* Device Hub */
    .device-hub-panel {
      padding: 0.5rem 0;
      border-top: 1px solid var(--sl-color-neutral-200);
    }
    .device-hub-cards {
      display: flex;
      flex-direction: row;
      flex-wrap: wrap;
      gap: 0.4rem;
    }
    .device-hub-card {
      display: flex;
      flex-direction: column;
      align-items: center;
      gap: 0.2rem;
      padding: 0.4rem 0.6rem;
      border-radius: var(--sl-border-radius-medium);
      border: 1px solid var(--sl-color-neutral-200);
      cursor: pointer;
      min-width: 60px;
      max-width: 90px;
      overflow: hidden;
      transition: background 0.15s, border-color 0.15s;
    }
    .device-hub-card:hover {
      background: var(--sl-color-neutral-100);
    }
    .device-hub-card.active {
      border-color: var(--sl-color-primary-500);
      background: var(--sl-color-primary-50);
    }
    .device-hub-icon {
      font-size: 1.4rem;
    }
    .device-hub-name {
      font-size: 0.7rem;
      text-align: center;
      white-space: nowrap;
      overflow: hidden;
      text-overflow: ellipsis;
      max-width: 80px;
    }

    /* Locked state: disable library add buttons when no device is selected */
    #library-content.device-locked .basket-toggle-btn {
      opacity: 0.3;
      pointer-events: none;
    }
    ```

- [x] **Verify TypeScript compiles cleanly** (AC: all)
  - [x] `rtk tsc` passes with 0 errors after all changes

## Dev Notes

### What Story 2.7 Already Built (Do NOT Reinvent)

Story 2.7 is fully implemented. The following already exist and must be reused:

- **`DeviceManager` (Rust)**: `connected_devices: HashMap<PathBuf, DeviceManifest>` + `selected_device_path: Option<PathBuf>`. All public API unchanged.
- **RPCs (Rust)**: `device.list` and `device.select` are implemented in `hifimule-daemon/src/rpc.rs`.
- **`get_daemon_state` response** already includes `connectedDevices` and `selectedDevicePath`.
- **`BasketSidebar.ts`**: `connectedDevices` + `selectedDevicePath` instance vars, polling loop, `renderDevicePicker()` (to be refactored), `bindDevicePickerEvents()` (to be refactored), `deviceSwitchInFlight` guard, `basketStore.flushPendingSave()` before switching.

**DO NOT change any Rust daemon code.** The daemon already handles `selectedDevicePath === null` correctly.

### Critical: `selectedDevicePath` Null-Coalescing Bug

In `refreshAndRender()` (line ~193–194 of `BasketSidebar.ts`):
```typescript
this.connectedDevices = state.connectedDevices ?? this.connectedDevices;
this.selectedDevicePath = state.selectedDevicePath ?? this.selectedDevicePath;
```

The `??` (nullish coalescing) operator means if the daemon returns `selectedDevicePath: null`, this line will **NOT** update `this.selectedDevicePath` to `null` — it will keep the old value. This is a pre-existing bug from Story 2.7.

**Story 2.8 must fix this**: When the daemon explicitly returns `selectedDevicePath: null`, the local state must be set to `null`. Fix:
```typescript
if (daemonStateResult.status === 'fulfilled' && daemonStateResult.value) {
    const state = daemonStateResult.value as any;
    this.connectedDevices = state.connectedDevices ?? this.connectedDevices;
    // Use explicit null check: if field present in response, use it; otherwise keep current
    if ('selectedDevicePath' in state) {
        this.selectedDevicePath = state.selectedDevicePath;
    }
    // ... rest of state parsing
}
```
Same fix must apply in the polling loop at line ~418–422.

### Icon Field: Forward-Compatibility with Story 2.9

Story 2.9 will add `icon: Option<String>` to `DeviceManifest` and extend `device.list` / `get_daemon_state` to include the `icon` field. For Story 2.8:
- Add `icon?: string | null` to the TypeScript type for `connectedDevices` entries
- Use `d.icon ?? 'usb-drive'` in hub card rendering — always falls back to `usb-drive` until 2.9 is implemented
- No daemon changes needed

### `device-locked` Class: Where to Apply It

The library container is `<div id="library-content">` rendered in `main.ts`. The BasketSidebar can reach it via `document.getElementById('library-content')`.

Apply the class toggle in `render()` such that it runs on every render path. The cleanest pattern — centralize as a helper called at the start of `render()`:
```typescript
private updateDeviceLockState(): void {
    const libraryContent = document.getElementById('library-content');
    if (libraryContent) {
        libraryContent.classList.toggle('device-locked', this.selectedDevicePath === null);
    }
}
```
Call `this.updateDeviceLockState()` at the beginning of `render()` (after the early syncing/error guards) AND in `renderLockedBasket()`.

Note: When all devices are disconnected (`connectedDevices.length === 0`), `selectedDevicePath` will also be `null`, so the class will still be applied. This is correct — no device selected = no add buttons.

### Render Flow (Updated for Story 2.8)

The `render()` method should have this decision tree:
1. Early return for: `showSyncComplete`, `syncErrorMessages`, `isSyncing` (unchanged from 2.7)
2. **NEW**: If `selectedDevicePath === null && connectedDevices.length > 0` → `renderLockedBasket()`; return
3. Else: normal render (empty basket or items basket), both paths now call `renderDeviceHub()` instead of `renderDevicePicker()`
4. Call `updateDeviceLockState()` appropriately in all paths

Note: When ALL devices are disconnected (`connectedDevices.length === 0`), `renderDeviceHub()` returns empty string, and existing "Connect a device to view folders" message in `renderDeviceFolders()` handles the no-device display. The locked class is still applied via `updateDeviceLockState()` in that case.

### `renderAutoFillControls()` Dependency on `hasManifest`

`renderAutoFillControls()` guards on `this.folderInfo?.hasManifest ?? false`. When `selectedDevicePath === null`, `folderInfo` may still exist (device connected but not selected). In `renderLockedBasket()`, do NOT render auto-fill controls — they require an active device. Only render `renderDeviceHub()` and `renderDeviceFolders()`.

### Event Binding Pattern (CRITICAL — must follow existing pattern)

All post-render event binding follows the `container.querySelector` + `addEventListener` pattern. See existing `bindDevicePickerEvents()` and `bindAutoFillEvents()` for the exact pattern. The `bindDeviceHubEvents()` method must be called AFTER the HTML is set via `innerHTML` assignment:

```typescript
// After: this.container.innerHTML = `...`;
this.bindDeviceHubEvents();
this.bindAutoFillEvents();  // existing — do not remove
// ...other event wiring
```

The new `renderLockedBasket()` private method must also call `bindDeviceHubEvents()` at the end.

### `storageInfo`/`renderCapacityBar` Must NOT Render in Locked State

The epics spec says "The basket renders as empty with no storage projection bar." The `renderLockedBasket()` implementation must NOT include `renderCapacityBar(...)` or `renderAutoFillControls()` — even if `storageInfo` is available.

### File Structure

**Only these files change:**
- `hifimule-ui/src/components/BasketSidebar.ts` — all logic changes
- `hifimule-ui/src/styles.css` — new CSS for hub cards + locked state

No Rust daemon files change. No other TypeScript files change.

### References

- Story 2.7 file: `_bmad-output/implementation-artifacts/2-7-multi-device-selection-panel.md` — full context on what was built
- Architecture (Multi-Device Tracker): `_bmad-output/planning-artifacts/architecture.md` line ~74
- Architecture (Multi-Device IPC): `_bmad-output/planning-artifacts/architecture.md` lines ~103–107
- UX spec (Device Hub section 5.6): `_bmad-output/planning-artifacts/ux-design-specification.md` lines ~101–108
- Current `renderDevicePicker()`: `BasketSidebar.ts:445–456`
- Current `bindDevicePickerEvents()`: `BasketSidebar.ts:323–345`
- Current `connectedDevices`/`selectedDevicePath` vars: `BasketSidebar.ts:154–155`
- Current `refreshAndRender()` state parsing: `BasketSidebar.ts:189–194`
- Polling loop device state parsing: `BasketSidebar.ts:418–422`
- Empty basket render path: `BasketSidebar.ts:579–612`
- Non-empty basket render path: `BasketSidebar.ts:625–670+`
- Library content container: `main.ts:76` (`<div id="library-content">`)
- `BasketSidebar.ts` total render flow: lines ~549–613 (empty) and ~613–680+ (non-empty)

## Dev Agent Record

### Agent Model Used

claude-sonnet-4-6

### Debug Log References

_No blockers encountered._

### Completion Notes List

- Fixed pre-existing `selectedDevicePath` null-coalescing bug in both `refreshAndRender()` and the polling loop: switched from `?? this.selectedDevicePath` to an explicit `'selectedDevicePath' in state` field-presence check so the daemon returning `null` correctly clears local state.
- Replaced the `<sl-select>` device picker with a card-based `renderDeviceHub()` method that shows every connected device (no `<= 1` guard), with icon (fallback `usb-drive`) and name.
- Added `renderLockedBasket()` private method for the no-device-selected locked state: shows "Select a device to start curating" placeholder, renders the device hub and folders, and disables Start Sync.
- Added `updateDeviceLockState()` helper that toggles the `device-locked` class on `#library-content`; called from `render()` normal path and `renderLockedBasket()`.
- Replaced `bindDevicePickerEvents()` / `sl-change` with `bindDeviceHubEvents()` / click listeners on `.device-hub-card` elements.
- Start Sync disabled when `!selectedDevicePath` in both the empty and non-empty basket render paths.
- Removed now-unused `truncatePath()` method (was only used by the old `<sl-option>` labels).
- TypeScript compiles cleanly with 0 errors.

### File List

- `hifimule-ui/src/components/BasketSidebar.ts`
- `hifimule-ui/src/styles.css`

## Change Log

- 2026-04-04: Implemented Enhanced Multi-Device Hub — replaced sl-select device picker with card-based hub, added locked basket state for no-device-selected, fixed selectedDevicePath null-coalescing bug, added device-locked CSS class for library add buttons, and updated Start Sync disabled logic. (claude-sonnet-4-6)
