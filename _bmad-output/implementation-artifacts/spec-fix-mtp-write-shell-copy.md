---
title: 'Fix MTP write_file: Use Shell IFileOperation copy instead of CreateObjectWithPropertiesAndData'
type: 'bugfix'
created: '2026-05-03'
status: 'done'
baseline_commit: '5e1e5a1'
context: ['spec-fix-mtp-wpd-com-threading.md', 'spec-fix-mtp-write-not-ready.md']
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Garmin MTP devices (e.g. Forerunner 945, vid_091e pid_50a4) ignore `WPD_OBJECT_CONTENT_TYPE = WPD_CONTENT_TYPE_GENERIC_FILE` in `IPortableDeviceContent::CreateObjectWithPropertiesAndData`. The firmware creates a folder object regardless of the requested content type or format, making `write_file` non-functional on this device class. Files copied via Windows Explorer (Shell `IFileOperation`) land correctly because the Shell MTP namespace extension uses a different code path.

**Approach:** Replace `CreateObjectWithPropertiesAndData` with a temp-file + `IFileOperation::CopyItem` write strategy.  
(1) Add `friendly_name: String` to `WpdHandle` (needed to locate the device in the Shell namespace).  
(2) Rewrite `write_file` in two phases: WPD phase (ensure parent directories exist via `CreateObjectWithPropertiesOnly`; delete any existing object at the path); Shell phase (write data to a local temp file in `%TEMP%`; copy the temp file to the device folder via `IFileOperation::CopyItem` + `PerformOperations`; delete the temp file).  
(3) Add three private Shell helpers in `windows_wpd`: `find_shell_child_by_name`, `first_shell_folder_child`, `shell_copy_to_device`.  
(4) Add `Win32_UI_Shell` to the windows crate feature list in `Cargo.toml` and define any needed constants (`FOLDERID_ComputerFolder`, `BHID_SFObject`) inline if absent from the crate.

`ensure_dir_chain` (uses `CreateObjectWithPropertiesOnly`) is confirmed working on Garmin and is unchanged. The `b"\x00"` dirty sentinel (from the prior spec) stays.

## Boundaries & Constraints

**Always:**
- The WPD phase (ensure dirs + delete existing) runs first; the Shell phase runs second. The WPD session (`_com` + `device`) is fully dropped before the Shell phase begins — no concurrent WPD + Shell sessions.
- `IFileOperation` is called with `FOF_NOCONFIRMATION | FOF_NOERRORUI | FOF_SILENT` (decimal: 0x0414). No parent HWND.
- The temp file is always deleted (best-effort) after `PerformOperations` returns, regardless of success or failure.
- Temp file name: `hifimule_{nanos}.tmp` in `std::env::temp_dir()`. Uses SystemTime nanoseconds for uniqueness (sufficient for sequential daemon writes).
- All Shell helpers are private free functions in `windows_wpd`; they are only called from `write_file`.
- `read_file`, `delete_file`, `list_files`, `free_space` are not touched.
- Diagnostic `daemon_log!` is added to the new Shell phase: before and after `PerformOperations`.

**Ask First:**
- If `IShellFolder::EnumObjects` or `IShellFolder::ParseDisplayName` returns a COM apartment error (`RPC_E_WRONG_THREAD` or `CO_E_NOTINITIALIZED`) from the MTA context, halt and ask — a dedicated STA thread may be needed.
- If `SHCreateItemWithParent` is not available in `windows = 0.58`, halt and ask before substituting an alternative.

**Never:**
- Do not add a persistent background thread for Shell operations — one operation per call is acceptable for the initialization use case.
- Do not change the `MtpHandle` trait, `MtpBackend`, `DeviceIO` trait, or `device_io.rs` (except `deferred-work.md`).
- Do not add `daemon_log!` inside `find_shell_child_by_name` or `first_shell_folder_child` (called per enumeration iteration).
- Do not set `FOFX_SHOWELEVATIONPROMPT` or any flag that could trigger UAC on the daemon service account.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Write dirty marker to storage root | `write_file(".hifimule.json.dirty", b"\x00")` | WPD phase: no parent dirs, no existing object; Shell: 1-byte temp file copied to storage root | Error from any phase propagates |
| Write manifest to storage root | `write_file(".hifimule.json", b"{\x22...}")` | Same as above but with JSON payload | Same |
| Write to nested path | `write_file("Music/track.mp3", data)` | WPD: `ensure_dir_chain(["Music"])` creates folder if missing; Shell: temp file copied to `Music/` | If dir creation fails, WPD error propagates before Shell phase |
| Overwrite existing file | Existing `.hifimule.json` on device | WPD delete removes it; Shell copy writes new version | Delete failure: `best-effort` (ignored); copy still attempted |
| Device not in Shell namespace | Friendly name not found under "This PC" | `find_shell_child_by_name` returns error | Error propagates; initialization fails with descriptive message |
| Temp file write fails | `%TEMP%` full | `std::fs::write` returns `Err` | Error propagates before Shell phase |
| `PerformOperations` aborts | Shell copy aborted | `GetAnyOperationsAborted()` returns true | Returns `Err` with message including filename |

</frozen-after-approval>

## Code Map

- `../../hifimule-daemon/Cargo.toml` — add `"Win32_UI_Shell"` to windows features
- `../../hifimule-daemon/src/device/mtp.rs:195-197` — `WpdHandle` struct; add `friendly_name: String`
- `../../hifimule-daemon/src/device/mtp.rs:200-202` — `WpdHandle::open`; add `friendly_name` parameter and store it
- `../../hifimule-daemon/src/device/mtp.rs:1136` — `create_mtp_backend`; pass `info.friendly_name` to `WpdHandle::open`
- `../../hifimule-daemon/src/device/mtp.rs:571-650` — `WpdHandle::write_file`; rewrite to two-phase approach
- NEW helpers added after `find_child_object_id` in `windows_wpd`: `shell_copy_to_device`, `find_shell_child_by_name`, `first_shell_folder_child`

## Tasks & Acceptance

**Execution:**

- [x] `hifimule-daemon/Cargo.toml` — In the windows features list, add `"Win32_UI_Shell"` after `"Win32_UI_Shell_PropertiesSystem"`.

- [x] `hifimule-daemon/src/device/mtp.rs` — After the existing manually-defined constants block (around line 165), add any required Shell constants not present in the windows crate:
  ```rust
  // FOLDERID_ComputerFolder = {0AC0837C-BBF8-452A-850D-79D08E667CA7}
  const FOLDERID_ComputerFolder: windows::core::GUID = windows::core::GUID::from_values(
      0x0AC0837C, 0xBBF8, 0x452A, [0x85, 0x0D, 0x79, 0xD0, 0x8E, 0x66, 0x7C, 0xA7],
  );
  // BHID_SFObject = {3981E224-F559-11D3-8E3A-00C04F6837D5}
  const BHID_SFObject: windows::core::GUID = windows::core::GUID::from_values(
      0x3981E224, 0xF559, 0x11D3, [0x8E, 0x3A, 0x00, 0xC0, 0x4F, 0x68, 0x37, 0xD5],
  );
  ```
  If `FOLDERID_ComputerFolder` or `BHID_SFObject` are already re-exported by the `Win32_UI_Shell` feature, skip the corresponding manual definition.

- [x] `hifimule-daemon/src/device/mtp.rs` — Update `WpdHandle` struct (line ~195):
  ```rust
  pub struct WpdHandle {
      device_id: String,
      friendly_name: String,
  }
  ```
  Update `WpdHandle::open` (line ~200) to accept and store the friendly name:
  ```rust
  pub fn open(wpd_device_id: &str, friendly_name: &str) -> Result<Self> {
      Ok(Self {
          device_id: wpd_device_id.to_string(),
          friendly_name: friendly_name.to_string(),
      })
  }
  ```

- [x] `hifimule-daemon/src/device/mtp.rs` — Update `create_mtp_backend` (line ~1136):
  ```rust
  Arc::new(windows_wpd::WpdHandle::open(wpd_device_id, &info.friendly_name)?)
  ```

- [x] `hifimule-daemon/src/device/mtp.rs` — Add new imports to the `windows_wpd` `use` block:
  ```rust
  use windows::Win32::UI::Shell::{
      FileOperation, IEnumIDList, IFileOperation, IShellFolder, IShellItem,
      SHCreateItemFromParsingName, SHCreateItemWithParent, SHGetKnownFolderItem,
      StrRetToStrW, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT,
      KF_FLAG_DEFAULT, SHCONTF_FOLDERS, SHCONTF_NONFOLDERS, SHGDN_NORMAL, STRRET,
  };
  use windows::Win32::UI::Shell::Common::ITEMIDLIST;
  ```
  Adjust the import list if any of these symbols are in different submodules or named differently in `windows = 0.58`. Check by attempting `cargo build` and fixing unresolved paths.

- [x] `hifimule-daemon/src/device/mtp.rs` — Add the three Shell helpers as private free functions inside `windows_wpd`, placed after the existing `find_child_object_id` function:

  **`shell_copy_to_device`**: Initializes COM (MTA), navigates Shell namespace to find device and destination folder, copies temp file via `IFileOperation`.
  ```rust
  fn shell_copy_to_device(
      friendly_name: &str,
      parent_components: &[&str],
      filename: &str,
      temp_path: &std::path::Path,
  ) -> Result<()> {
      let _com = CoInitGuard::init()?;
      unsafe {
          let computer_item: IShellItem =
              SHGetKnownFolderItem(&FOLDERID_ComputerFolder, KF_FLAG_DEFAULT, None)?;
          let device_item = find_shell_child_by_name(&computer_item, friendly_name)?;
          let storage_item = first_shell_folder_child(&device_item)?;
          let dest_folder = navigate_shell_path(storage_item, parent_components)?;

          let temp_hstr = HSTRING::from(temp_path.to_string_lossy().as_ref());
          let source_item: IShellItem =
              SHCreateItemFromParsingName(PCWSTR(temp_hstr.as_ptr()), None)?;

          crate::daemon_log!("[WPD] shell_copy_to_device: PerformOperations path={}/{}",
              parent_components.join("/"), filename);
          let file_op: IFileOperation =
              CoCreateInstance(&FileOperation, None, CLSCTX_INPROC_SERVER)?;
          file_op.SetOperationFlags(
              FOF_NOCONFIRMATION.0 | FOF_NOERRORUI.0 | FOF_SILENT.0
          )?;
          let fname_hstr = HSTRING::from(filename);
          file_op.CopyItem(&source_item, &dest_folder, PCWSTR(fname_hstr.as_ptr()), None)?;
          file_op.PerformOperations()?;
          crate::daemon_log!("[WPD] shell_copy_to_device: PerformOperations OK");

          if file_op.GetAnyOperationsAborted()?.as_bool() {
              return Err(anyhow::anyhow!(
                  "WPD shell copy aborted for '{}'", filename
              ));
          }
          Ok(())
      }
  }
  ```
  
  **`navigate_shell_path`**: Walks `IShellItem` down `components` via `ParseDisplayName`.
  ```rust
  fn navigate_shell_path(root: IShellItem, components: &[&str]) -> Result<IShellItem> {
      let mut current = root;
      for &component in components {
          unsafe {
              let folder: IShellFolder = current.BindToHandler(None, &BHID_SFObject)?;
              let mut wide: Vec<u16> =
                  component.encode_utf16().chain(std::iter::once(0)).collect();
              let mut child_pidl: *mut ITEMIDLIST = std::ptr::null_mut();
              let mut eaten = 0u32;
              folder.ParseDisplayName(
                  None, None, PWSTR(wide.as_mut_ptr()),
                  &mut eaten, &mut child_pidl, None,
              )?;
              let child: IShellItem =
                  SHCreateItemWithParent(std::ptr::null_mut(), &folder, child_pidl)?;
              CoTaskMemFree(Some(child_pidl as *const _));
              current = child;
          }
      }
      Ok(current)
  }
  ```

  **`find_shell_child_by_name`**: Enumerates children of `parent` and returns the first whose display name equals `name`.
  ```rust
  fn find_shell_child_by_name(parent: &IShellItem, name: &str) -> Result<IShellItem> {
      unsafe {
          let folder: IShellFolder = parent.BindToHandler(None, &BHID_SFObject)?;
          let enum_list: IEnumIDList =
              folder.EnumObjects(None, SHCONTF_FOLDERS | SHCONTF_NONFOLDERS)?;
          loop {
              let mut pidl: *mut ITEMIDLIST = std::ptr::null_mut();
              let mut fetched = 0u32;
              if enum_list.Next(1, &mut pidl, &mut fetched).is_err() || fetched == 0 {
                  return Err(anyhow::anyhow!(
                      "WPD: device '{}' not found in Shell namespace under This PC", name
                  ));
              }
              let mut strret: STRRET = std::mem::zeroed();
              let matches = if folder.GetDisplayNameOf(pidl, SHGDN_NORMAL, &mut strret).is_ok() {
                  let mut str_ptr = PWSTR::null();
                  if StrRetToStrW(&mut strret, pidl, &mut str_ptr).is_ok()
                      && !str_ptr.is_null()
                  {
                      let child_name = str_ptr.to_string().unwrap_or_default();
                      CoTaskMemFree(Some(str_ptr.0 as *const _));
                      child_name == name
                  } else {
                      false
                  }
              } else {
                  false
              };
              if matches {
                  let item: IShellItem =
                      SHCreateItemWithParent(std::ptr::null_mut(), &folder, pidl)?;
                  CoTaskMemFree(Some(pidl as *const _));
                  return Ok(item);
              }
              CoTaskMemFree(Some(pidl as *const _));
          }
      }
  }
  ```

  **`first_shell_folder_child`**: Returns the first folder child (storage root) of `parent`.
  ```rust
  fn first_shell_folder_child(parent: &IShellItem) -> Result<IShellItem> {
      unsafe {
          let folder: IShellFolder = parent.BindToHandler(None, &BHID_SFObject)?;
          let enum_list: IEnumIDList = folder.EnumObjects(None, SHCONTF_FOLDERS)?;
          let mut pidl: *mut ITEMIDLIST = std::ptr::null_mut();
          let mut fetched = 0u32;
          enum_list.Next(1, &mut pidl, &mut fetched)?;
          if fetched == 0 || pidl.is_null() {
              return Err(anyhow::anyhow!("WPD: no storage root found on MTP device"));
          }
          let item: IShellItem =
              SHCreateItemWithParent(std::ptr::null_mut(), &folder, pidl)?;
          CoTaskMemFree(Some(pidl as *const _));
          Ok(item)
      }
  }
  ```

- [x] `hifimule-daemon/src/device/mtp.rs` — Rewrite `WpdHandle::write_file` (line ~571). Replace the current body wholesale with the two-phase approach:
  ```rust
  fn write_file(&self, path: &str, data: &[u8]) -> Result<()> {
      let components = super::split_path_components(path);
      if components.is_empty() {
          return Err(anyhow::anyhow!("WPD write_file: empty path"));
      }
      let filename = components[components.len() - 1];
      let parent_components = &components[..components.len() - 1];

      // Phase 1 (WPD): ensure parent directories exist; delete any existing object.
      {
          let (_com, device) = self.session()?;
          unsafe {
              let content = device.Content()?;
              ensure_dir_chain(&content, parent_components)?;
              if let Ok(existing_id_hstr) = path_to_object_id(&device, path) {
                  let existing_id = existing_id_hstr.to_string();
                  let col = make_object_id_collection(&existing_id)?;
                  let mut pp: Option<IPortableDevicePropVariantCollection> = None;
                  let _ = content.Delete(0, &col, &mut pp); // best-effort
              }
          }
          // _com and device drop here → COM uninitialized on this thread.
      }

      // Phase 2 (Shell): write temp file, copy to device, clean up.
      let temp_path = std::env::temp_dir().join(format!(
          "hifimule_{}.tmp",
          std::time::SystemTime::now()
              .duration_since(std::time::UNIX_EPOCH)
              .map(|d| d.as_nanos())
              .unwrap_or(0),
      ));
      std::fs::write(&temp_path, data)?;
      let copy_result =
          shell_copy_to_device(&self.friendly_name, parent_components, filename, &temp_path);
      let _ = std::fs::remove_file(&temp_path);
      copy_result
  }
  ```

**Acceptance Criteria:**
- Given a Garmin Forerunner 945 is connected and the user submits the Initialize form, then `write_with_verify` writes `.hifimule.json.dirty` as a **file** (not a folder) on the device storage root and completes without error; the device transitions to recognized state.
- Given `cargo build --manifest-path hifimule-daemon/Cargo.toml`, then zero errors, zero new warnings.
- Given `cargo test --manifest-path hifimule-daemon/Cargo.toml`, then all existing tests pass.
- Given any path with no parent components (e.g. `".hifimule.json"`), `write_file` calls `ensure_dir_chain` with an empty slice (no-op) and `navigate_shell_path` with an empty slice (returns storage root unchanged).
- Given `PerformOperations` is aborted (e.g. no free space), `write_file` returns `Err` and the temp file is deleted.

## Spec Change Log

## Design Notes

**Why Shell copy works where WPD does not:** Windows' MTP Shell namespace extension (`portabl.dll`) uses the MTP `SendObject` protocol command sequence directly via the device driver, bypassing the generic `IPortableDeviceContent::CreateObjectWithPropertiesAndData` codepath. Garmin's WPD driver partially implements WPD and misdirects `CreateObjectWithPropertiesAndData` to folder creation, but the raw MTP `SendObject` path (used by the Shell) is functional.

**Two-phase design:** The WPD phase keeps `ensure_dir_chain` (which uses `CreateObjectWithPropertiesOnly` — confirmed working on Garmin) for directory setup. The Shell phase handles only the file transfer. The WPD session is dropped before the Shell phase because the Garmin driver may not tolerate simultaneous WPD + Shell sessions to the same device.

**COM apartment:** Both phases use `CoInitGuard` (COINIT_MULTITHREADED). `IFileOperation` is explicitly documented as MTA-safe. If `IShellFolder::EnumObjects` returns a threading error from MTA, the fix is to replace `CoInitGuard::init()` in `shell_copy_to_device` with a dedicated `std::thread::spawn` + `CoInitializeEx(COINIT_APARTMENTTHREADED)` STA thread (halt and ask per the constraints above).

**`FOF_SILENT` value:** Check whether the windows 0.58 crate exposes `FOF_SILENT` as `FILEOPERATION_FLAGS` or as a bare `u32`. If `FOF_SILENT` is not in scope, use `0x0004u32`. Similarly `FOF_NOCONFIRMATION = 0x0010` and `FOF_NOERRORUI = 0x0400`. Use whichever form compiles.

**`IShellFolder::EnumObjects` signature variance:** In some windows crate versions `EnumObjects` returns `IEnumIDList` directly (on success); in others it uses an out-parameter `Option<IEnumIDList>`. Adjust the call site to match the actual generated signature. Use `cargo build` errors to guide corrections.

**`SHCreateItemWithParent` null first argument:** The first parameter is the absolute parent PIDL (`PCIDLIST_ABSOLUTE`), which is null when using `IShellFolder` as the parent (the common case). Pass `std::ptr::null_mut()` and cast if the type system requires it.

## Suggested Review Order

**Cargo.toml — feature gate**
- New `Win32_UI_Shell` feature unlocks all Shell types used in this spec.
  [`Cargo.toml`, windows features]

**WpdHandle — friendly_name field**
- Minimal struct change; `open` now accepts the display name needed for Shell namespace lookup.
  [`mtp.rs`, `WpdHandle`, `open`, `create_mtp_backend`]

**Shell helpers — bottom-up**
- `find_shell_child_by_name`: PIDL enumeration + `StrRetToStrW` match; foundational for device lookup.
  [`mtp.rs`, `find_shell_child_by_name`]
- `first_shell_folder_child`: Gets storage root; mirrors WPD's "first child of DEVICE" pattern.
  [`mtp.rs`, `first_shell_folder_child`]
- `navigate_shell_path`: `ParseDisplayName` loop; mirrors `path_to_object_id` for Shell side.
  [`mtp.rs`, `navigate_shell_path`]
- `shell_copy_to_device`: Orchestrates helpers + `IFileOperation`; the actual transfer.
  [`mtp.rs`, `shell_copy_to_device`]

**write_file rewrite**
- Phase 1 (WPD): dir creation + delete; Phase 2 (Shell): temp file + copy.
  [`mtp.rs`, `WpdHandle::write_file`]

## Verification

**Commands:**
- `rtk cargo build --manifest-path hifimule-daemon/Cargo.toml` — expected: zero errors, zero new warnings
- `rtk cargo test --manifest-path hifimule-daemon/Cargo.toml` — expected: all tests pass
