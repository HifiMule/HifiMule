# Sprint Change Proposal — MTP Device Support
**Date:** 2026-04-30
**Project:** JellyfinSync
**Author:** Alexi

---

## 1. Issue Summary

**Problem Statement:**
Garmin watches (and any device using MTP — Media Transfer Protocol) are invisible to JellyfinSync on Windows. When connected, they appear in Windows Explorer as portable devices under "This PC" but receive no drive letter. The daemon's current detection pipeline fires on `WM_DEVICECHANGE` for MSC drive arrivals only, and the entire sync engine assumes `std::fs` writes to a mounted filesystem path. MTP devices expose a virtual filesystem via the Windows Portable Devices (WPD) COM API — a completely different IO model.

**Discovery context:**
Surfaced during manual testing of the auto-sync flow on Windows with a Garmin watch (Sarah's "Pre-Run Dash" user journey). The daemon produced no device-detected event.

**Evidence:**
- Garmin watch connected → no `on_device_detected` event fired
- Device visible as a portable device in Windows Explorer, not as a lettered drive
- Sarah's user journey is an explicit MVP use case; this is a gap in MVP coverage, not an edge case

---

## 2. Impact Analysis

### Epic Impact

| Epic | Impact |
|---|---|
| **Epic 2** (Connection & Verification) | Stories 2.2 and 2.6 need modification; new Story 2.10 added |
| **Epic 3** (Curation Hub) | None — basket/library logic is Jellyfin API only |
| **Epic 4** (Sync Engine) | New Story 4.0 required; all existing stories adopt `DeviceIO` trait |
| **Epic 5** (Ecosystem / Scrobble) | Story 5.1 scrobble log read needs MTP path |
| **Epic 6** (Packaging) | Stories 6.5 and 6.6 need build dependency and test scope notes |

### Story Impact

**Modified stories:**
- Story 2.2 — scoping note added (MSC only; MTP covered by 2.10)
- Story 2.6 — manifest write via `write_with_verify()` instead of Write-Temp-Rename directly
- Story 5.1 — scrobble log read via `device_io.read_file()`
- Story 6.5 — `libmtp` system library install step on Linux/macOS CI runners
- Story 6.6 — MTP hardware testing scoped to manual verification

**New stories:**
- **Story 2.10** — MTP Device Detection (cross-platform: WPD on Windows, libmtp on Linux/macOS)
- **Story 4.0** — Device IO Abstraction Layer (`DeviceIO` trait + `MscBackend` + `MtpBackend`)

### Artifact Conflicts

| Artifact | Sections Changed |
|---|---|
| `prd.md` | FR1, FR2, FR14 |
| `architecture.md` | Technical Constraints (OS detection events), new Device IO Abstraction section |
| `ux-design-specification.md` | Device `path` concept noted as synthetic `mtp://` identifier for MTP devices |
| `epics.md` | 5 story modifications + 2 new stories |

### Technical Impact

The core change is a new `DeviceIO` trait with two concrete backends:
- `MscBackend` — wraps existing `std::fs` logic (zero behavior change for MSC)
- `MtpBackend` — WPD COM API on Windows; `libmtp` on Linux and macOS

All existing sync, manifest, and scrobble code becomes backend-agnostic. The atomic Write-Temp-Rename pattern is replaced by `write_with_verify()`, which delegates to Write-Temp-Rename (MSC) or dirty-marker + overwrite (MTP).

---

## 3. Recommended Approach

**Selected path: Option 1 — Direct Adjustment**

### Rationale
- Purely additive: the MSC path is preserved unchanged inside `MscBackend`
- Sarah's Garmin watch scenario is an explicitly stated MVP user journey; deferring MTP would break MVP integrity
- The `DeviceIO` trait future-proofs the sync engine for Wi-Fi sync (Phase 3) at no extra design cost
- Risk is manageable: `libmtp` is a mature C library; WPD is verbose but well-documented

### Effort & Risk
- **Effort:** Medium-High (new cross-platform IO layer)
- **Risk:** Medium (`libmtp` version quirks on Linux/macOS; WPD COM API verbosity on Windows)
- **Timeline impact:** ~2–3 sprints

### Atomic Write Limitation (MTP)
MTP has no native rename operation. Mitigation: `write_with_verify()` writes a `".dirty"` marker object first, overwrites the target in-place, then removes the marker. On reconnect, a present dirty marker triggers the existing `on_device_dirty` flow — same recovery path as MSC.

---

## 4. Detailed Change Proposals

All proposals approved incrementally. Grouped by artifact below.

---

### 4.1 PRD Changes (`prd.md`)

**FR1**
```
OLD:
- **FR1:** The system can automatically detect Mass Storage devices (USB)
  on Windows, Linux, and macOS.

NEW:
- **FR1:** The system can automatically detect Mass Storage (USB MSC) and
  MTP (Media Transfer Protocol) devices on Windows, Linux, and macOS.
```

**FR2**
```
OLD:
- **FR2:** Users can manually select a target device folder if automatic
  detection fails.

NEW:
- **FR2:** Users can manually select a target device folder if automatic
  detection fails. Manual fallback applies to Mass Storage devices only;
  MTP devices must be detected automatically via the OS device manager.
```

**FR14**
```
OLD:
- **FR14:** The system can stream media files directly from the Jellyfin
  server to the device via memory-to-disk buffering.

NEW:
- **FR14:** The system can stream media files directly from the Jellyfin
  server to the device via memory-to-disk buffering, using the appropriate
  device IO backend (filesystem writes for MSC devices, WPD/libmtp object
  transfers for MTP devices).
```

---

### 4.2 Architecture Changes (`architecture.md`)

**Technical Constraints — OS detection events**
```
OLD:
- **OS Native IO:** Dependence on `udev` (Linux), `WM_DEVICECHANGE`
  (Windows), and `DiskArbitration` (macOS) for event-driven discovery.

NEW:
- **OS Native IO:** Dual-mode event-driven discovery per platform:
  - **Windows:** `WM_DEVICECHANGE` + `DBT_DEVICEARRIVAL` for MSC (drive
    letters) and `GUID_DEVINTERFACE_WPD` registration for MTP portable
    devices, both via `windows-rs`.
  - **Linux:** `udev` for MSC block devices; `udev` USB subsystem +
    `libmtp` device enumeration for MTP.
  - **macOS:** `DiskArbitration` for MSC; `IOKit` USB matching +
    `libmtp` notification callbacks for MTP.
```

**New section — Device IO Abstraction**
(Insert after "Safety & Atomicity Patterns" in Implementation Patterns & Consistency Rules)
```
### Device IO Abstraction

All device file operations MUST go through the `DeviceIO` trait. Direct
`std::fs` calls targeting device paths are forbidden outside the
`MscBackend` implementation.

trait DeviceIO: Send + Sync {
    fn read_file(&self, path: &str) -> Result<Vec<u8>>;
    fn write_file(&self, path: &str, data: &[u8]) -> Result<()>;
    fn list_files(&self, path: &str) -> Result<Vec<FileEntry>>;
    fn delete_file(&self, path: &str) -> Result<()>;
    fn free_space(&self) -> Result<u64>;
    fn write_with_verify(&self, path: &str, data: &[u8]) -> Result<()>;
}

struct MscBackend { root: PathBuf }          // std::fs — MSC drive path
struct MtpBackend { device: MtpHandle }      // WPD (Win) / libmtp (Linux, macOS)

Atomic writes over MTP: MTP has no native rename operation. The
Write-Temp-Rename pattern is MSC-only. For MTP, write_with_verify()
writes a "dirty" marker object first, overwrites the target in-place,
then removes the marker.

Backend selection: DeviceManager instantiates the correct backend at
detection time based on device class (MSC vs MTP) and passes it as
Arc<dyn DeviceIO> to all downstream callers.

Enforcement: All AI agents MUST use DeviceIO methods for any read/write
targeting the device. Never call std::fs with a device path directly.
```

---

### 4.3 Epic 2 Changes (`epics.md`)

**Story 2.2 — Scoping note**
```
ADD to bottom of Story 2.2:

**Note:** This story covers MSC (Mass Storage Class) devices only — USB
devices that mount as drive letters. MTP device detection is covered by
Story 2.10.
```

**Story 2.6 — Manifest write via DeviceIO**
```
CHANGE in Story 2.6 Acceptance Criteria:

OLD:
**Then** the daemon writes an initial `.jellyfinsync.json` to the device
  using the atomic Write-Temp-Rename pattern...

NEW:
**Then** the daemon writes an initial `.jellyfinsync.json` to the device
  via `device_io.write_with_verify()`...

ADD to Technical Notes:
- `device_io.write_with_verify()` abstracts the write strategy per
  backend: Write-Temp-Rename for MSC, dirty-marker + overwrite for MTP.
- The `device.initialize` RPC handler receives the `Arc<dyn DeviceIO>`
  for the target device from `DeviceManager`.
```

**New Story 2.10 — MTP Device Detection**
```
### Story 2.10: MTP Device Detection (Cross-Platform)

As a Convenience Seeker (Sarah),
I want the daemon to detect my Garmin watch (or any MTP device) the
moment I plug it in,
So that it appears in the device hub without requiring manual steps.

**Acceptance Criteria:**

**Given** the daemon is running on Windows
**When** an MTP device is connected
**Then** the daemon receives a `WM_DEVICECHANGE` event with
  `GUID_DEVINTERFACE_WPD`.
**And** it enumerates the device via `IPortableDeviceManager` to retrieve
  its device ID and friendly name.
**And** it checks for a `.jellyfinsync.json` object in the device root
  storage.
**And** it fires a `on_device_detected` or `on_device_unrecognized` event
  (identical behavior to MSC Story 2.2).

**Given** the daemon is running on Linux
**When** an MTP device is connected
**Then** the daemon receives a `udev` USB event.
**And** `libmtp` enumerates the device and retrieves its serial/device ID.
**And** it checks for `.jellyfinsync.json` and fires the appropriate event.

**Given** the daemon is running on macOS
**When** an MTP device is connected
**Then** the daemon receives an `IOKit` USB match notification.
**And** `libmtp` enumerates the device and fires the appropriate event.

**Given** an MTP device is detected (any platform)
**When** the daemon creates the device IO backend
**Then** it instantiates `MtpBackend` with the device handle.
**And** passes `Arc<dyn DeviceIO>` to all downstream device operations.

**Technical Notes:**
- Windows: `windows-rs` with WPD COM API (`IPortableDeviceManager`,
  `IPortableDevice`)
- Linux/macOS: `libmtp-rs` crate or direct FFI to `libmtp`
- `DeviceManager` gains `device_class: DeviceClass { Msc, Mtp }` per
  connected device entry
- MTP device `path` in `device.list` RPC: use synthetic identifier
  `mtp://<device_id>` — no real filesystem path exists
- Device enumeration runs in `tokio::task::spawn_blocking` (libmtp is
  synchronous)
```

---

### 4.4 Epic 4 Changes (`epics.md`)

**New Story 4.0 — Device IO Abstraction Layer**
```
### Story 4.0: Device IO Abstraction Layer

As a System Admin (Alexis),
I want all device file operations to go through a single abstract
interface,
So that the sync engine works identically for both MSC and MTP devices
without duplicated IO logic.

**Acceptance Criteria:**

**Given** the `DeviceIO` trait is defined in `jellyfinsync-daemon`
**When** any sync, manifest, or scrobble operation targets a device
**Then** it calls methods on `Arc<dyn DeviceIO>` exclusively — no direct
  `std::fs` calls with a device path anywhere outside `MscBackend`.

**Given** a connected MSC device
**When** `DeviceManager` instantiates the backend
**Then** it creates `MscBackend { root: PathBuf }`.
**And** `MscBackend::write_with_verify()` uses Write-Temp-Rename +
  `sync_all()` (existing behavior, unchanged).

**Given** a connected MTP device
**When** `DeviceManager` instantiates the backend
**Then** it creates `MtpBackend { handle: Arc<MtpHandle> }`.
**And** `MtpBackend::write_with_verify()` writes a `".dirty"` marker
  object first, overwrites the target, then deletes the marker.
**And** all read/write/list/delete/free_space methods delegate to the
  appropriate WPD or libmtp API call.

**Given** the daemon reconnects to a device with a `".dirty"` marker
  present
**When** device detection completes
**Then** the daemon fires `on_device_dirty` (same as the existing MSC
  dirty-manifest path).

**Given** all existing callers in `sync.rs`, `rpc.rs`, `device/mod.rs`,
  and `scrobble.rs`
**When** Story 4.0 is complete
**Then** every direct `std::fs` call targeting a device path has been
  replaced with the corresponding `DeviceIO` method.
**And** all existing unit tests pass without modification.

**Technical Notes:**
- `device_io.rs`: new file defining `DeviceIO` trait, `FileEntry` struct,
  `MscBackend`, `MtpBackend`
- Windows MTP: `windows-rs`, `IPortableDevice`, `IPortableDeviceContent`,
  `IPortableDeviceDataStream`
- Linux/macOS MTP: `libmtp-rs` crate; build script links `libmtp` via
  `pkg-config`
- `DeviceManager` stores `Arc<dyn DeviceIO>` per connected device
- `execute_sync()` gains `device_io: Arc<dyn DeviceIO>` param; both
  callers retrieve it from `DeviceManager`
```

---

### 4.5 Epic 5 Changes (`epics.md`)

**Story 5.1 — Scrobble log read via DeviceIO**
```
ADD to Story 5.1 Acceptance Criteria:

**Given** the connected device is an MTP device
**When** the daemon scans for a `.scrobbler.log`
**Then** it uses `device_io.read_file(".scrobbler.log")` to retrieve the
  log contents.
**And** parsing and submission logic is identical to the MSC path.

ADD to Story 5.1 Technical Notes:
- Replace direct `std::fs::read` targeting the scrobbler log path with
  `device_io.read_file()`
- `device_io` passed to scrobble handler from `DeviceManager` (same
  pattern as sync engine — established by Story 4.0)
```

---

### 4.6 Epic 6 Changes (`epics.md`)

**Story 6.5 — CI build dependency**
```
ADD to Story 6.5 Acceptance Criteria:

**Given** the Linux and macOS build runners
**When** the workflow runs
**Then** `libmtp` and its development headers are installed before
  `cargo build` (e.g. `sudo apt-get install -y libmtp-dev` on Ubuntu;
  `brew install libmtp` on macOS).
**And** `pkg-config` can resolve `libmtp` for the build script.
**And** the Windows runner requires no additional system libraries
  (`windows-rs` WPD bindings are pure Rust).
```

**Story 6.6 — Smoke test scope**
```
ADD to Story 6.6 Acceptance Criteria:

**Given** the smoke test environment has no physical MTP device
**When** the test suite runs
**Then** MTP device IO is verified by unit tests in `device_io.rs`
  (mock `MtpBackend` returning fixture data).
**And** the smoke test explicitly notes: "MTP end-to-end detection
  requires manual hardware verification on each platform."
```

---

## 5. Implementation Handoff

### Change Scope Classification: **Moderate**

Two new stories, five modified stories, three artifact updates. No epic reordering. MSC behavior is fully preserved.

### Recommended Implementation Sequence

1. **Story 4.0** — Device IO Abstraction Layer *(prerequisite for everything)*
2. **Story 2.10** — MTP Device Detection *(can start in parallel with 4.0 detection work)*
3. **Story 2.6** — Update manifest initialization *(depends on 4.0)*
4. **Story 5.1** — Update scrobble log read *(depends on 4.0)*
5. **Story 6.5** — CI dependency update *(can be done anytime)*
6. **Story 6.6** — Smoke test scope note *(can be done anytime)*
7. **Artifact updates** — PRD, Architecture, UX Spec *(can be done before implementation begins)*

### Handoff Recipients
- **Developer agent** — Stories 4.0, 2.10, 2.6 update, 5.1 update
- **Developer agent** — PRD, Architecture, UX Spec wording updates
- **Developer agent** — Stories 6.5, 6.6 additions

### Success Criteria
- Garmin watch (or any MTP device) appears in the device hub on all three platforms
- Sync completes successfully to an MTP device (files verified on device)
- Scrobble log is read and submitted from an MTP device
- All existing MSC tests pass unmodified
- CI pipeline builds successfully on all three platforms with `libmtp` dependency
