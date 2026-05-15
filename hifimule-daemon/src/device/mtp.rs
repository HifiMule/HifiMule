// Platform-specific MTP handle implementations and device enumeration.
// Windows: Windows Portable Devices (WPD) via IPortableDeviceManager.
// Linux/macOS: libmtp FFI bindings.
// TODO: replace unix FFI with libmtp-rs when a stable crate is available.

use crate::device_io::{MtpBackend, MtpHandle};
use anyhow::Result;
use std::sync::Arc;

// ── Platform-independent types ────────────────────────────────────────────────

#[allow(dead_code)]
#[derive(Clone)]
pub struct MtpDeviceInfo {
    /// Unique identifier: WPD device ID (Windows) or "bus_location:devnum" (Unix).
    pub device_id: String,
    pub friendly_name: String,
    pub inner: MtpDeviceInner,
}

#[derive(Clone)]
pub enum MtpDeviceInner {
    #[cfg(target_os = "windows")]
    Wpd { wpd_device_id: String },
    // dev_num matches LIBMTP_RawDevice_t.devnum (uint8_t) — store as u8 to avoid silent truncation.
    #[cfg(unix)]
    Libmtp { bus_location: u32, dev_num: u8 },
}

/// Splits a relative path into traversal components, filtering empty, dot, and parent segments.
/// Used by both WPD and libmtp handles to walk object hierarchies.
pub(super) fn split_path_components(path: &str) -> Vec<&str> {
    path.split('/')
        .filter(|c| !c.is_empty() && *c != "." && *c != "..")
        .collect()
}

fn resolve_path_with_lookup<F>(
    root_id: String,
    components: &[&str],
    mut find_child: F,
) -> Result<String>
where
    F: FnMut(&str, &str) -> Result<Option<String>>,
{
    let mut current_id = root_id;
    for component in components {
        current_id = find_child(&current_id, component)?
            .ok_or_else(|| anyhow::anyhow!("MTP path component not found: {}", component))?;
    }
    Ok(current_id)
}

// ── Windows WPD implementation ────────────────────────────────────────────────

#[cfg(target_os = "windows")]
pub mod windows_wpd {
    use super::{resolve_path_with_lookup, MtpDeviceInfo, MtpDeviceInner};
    use crate::device_io::{FileEntry, MtpHandle};
    use anyhow::Result;
    use std::mem::ManuallyDrop;
    use std::sync::{mpsc, Mutex};
    use std::thread::JoinHandle;
    use windows::core::{Interface, HSTRING, PCWSTR, PWSTR};
    use windows::Win32::Devices::PortableDevices::{
        IPortableDeviceDataStream, IPortableDevicePropVariantCollection,
        PortableDevicePropVariantCollection, *,
    };
    use windows::Win32::System::Com::*;
    use windows::Win32::System::Variant::VT_LPWSTR;
    use windows::Win32::UI::Shell::Common::{ITEMIDLIST, STRRET};
    use windows::Win32::UI::Shell::PropertiesSystem::PROPERTYKEY;
    use windows::Win32::UI::Shell::{
        FileOperation, IEnumIDList, IFileOperation, IShellFolder, IShellItem,
        SHCreateItemFromParsingName, SHCreateItemWithParent, SHGetKnownFolderItem, StrRetToStrW,
        FILEOPERATION_FLAGS, KF_FLAG_DEFAULT, SHCONTF_FOLDERS, SHCONTF_NONFOLDERS, SHGDN_NORMAL,
    };

    const ERROR_FILE_NOT_FOUND_HRESULT: i32 = 0x80070002u32 as i32;
    const ERROR_GEN_FAILURE_HRESULT: i32 = 0x8007001Fu32 as i32;

    fn is_retryable_wpd_open_error(error: &windows::core::Error) -> bool {
        matches!(
            error.code().0,
            ERROR_FILE_NOT_FOUND_HRESULT | ERROR_GEN_FAILURE_HRESULT
        )
    }

    // WPD_OBJECT_ORIGINAL_FILE_NAME = {EF6B490D-5CD8-437A-AFFC-DA8B60EE4A3C}, pid=12
    const WPD_OBJECT_ORIGINAL_FILE_NAME: PROPERTYKEY = PROPERTYKEY {
        fmtid: windows::core::GUID::from_values(
            0xEF6B490D,
            0x5CD8,
            0x437A,
            [0xAF, 0xFC, 0xDA, 0x8B, 0x60, 0xEE, 0x4A, 0x3C],
        ),
        pid: 12,
    };

    // WPD_RESOURCE_DEFAULT = {E81E79BE-34F0-41BF-B53F-F1A06AE87842}, pid=0
    const WPD_RESOURCE_DEFAULT: PROPERTYKEY = PROPERTYKEY {
        fmtid: windows::core::GUID::from_values(
            0xE81E79BE,
            0x34F0,
            0x41BF,
            [0xB5, 0x3F, 0xF1, 0xA0, 0x6A, 0xE8, 0x78, 0x42],
        ),
        pid: 0,
    };

    // ── Additional WPD PROPERTYKEY constants ──────────────────────────────────
    // All share GUID {EF6B490D-5CD8-437A-AFFC-DA8B60EE4A3C}

    const WPD_OBJECT_PARENT_ID: PROPERTYKEY = PROPERTYKEY {
        fmtid: windows::core::GUID::from_values(
            0xEF6B490D,
            0x5CD8,
            0x437A,
            [0xAF, 0xFC, 0xDA, 0x8B, 0x60, 0xEE, 0x4A, 0x3C],
        ),
        pid: 3,
    };

    const WPD_OBJECT_NAME: PROPERTYKEY = PROPERTYKEY {
        fmtid: windows::core::GUID::from_values(
            0xEF6B490D,
            0x5CD8,
            0x437A,
            [0xAF, 0xFC, 0xDA, 0x8B, 0x60, 0xEE, 0x4A, 0x3C],
        ),
        pid: 4,
    };

    const WPD_OBJECT_FORMAT: PROPERTYKEY = PROPERTYKEY {
        fmtid: windows::core::GUID::from_values(
            0xEF6B490D,
            0x5CD8,
            0x437A,
            [0xAF, 0xFC, 0xDA, 0x8B, 0x60, 0xEE, 0x4A, 0x3C],
        ),
        pid: 6,
    };

    const WPD_OBJECT_CONTENT_TYPE: PROPERTYKEY = PROPERTYKEY {
        fmtid: windows::core::GUID::from_values(
            0xEF6B490D,
            0x5CD8,
            0x437A,
            [0xAF, 0xFC, 0xDA, 0x8B, 0x60, 0xEE, 0x4A, 0x3C],
        ),
        pid: 7,
    };

    const WPD_OBJECT_SIZE: PROPERTYKEY = PROPERTYKEY {
        fmtid: windows::core::GUID::from_values(
            0xEF6B490D,
            0x5CD8,
            0x437A,
            [0xAF, 0xFC, 0xDA, 0x8B, 0x60, 0xEE, 0x4A, 0x3C],
        ),
        pid: 11,
    };

    // WPD_STORAGE_FREE_SPACE_IN_BYTES = {01A3057A-74D6-4E80-BEA7-DC4C212CE50A}, pid=5
    const WPD_STORAGE_FREE_SPACE_IN_BYTES: PROPERTYKEY = PROPERTYKEY {
        fmtid: windows::core::GUID::from_values(
            0x01A3057A,
            0x74D6,
            0x4E80,
            [0xBE, 0xA7, 0xDC, 0x4C, 0x21, 0x2C, 0xE5, 0x0A],
        ),
        pid: 5,
    };

    // ── WPD content-type and format GUIDs ─────────────────────────────────────

    const WPD_CONTENT_TYPE_GENERIC_FILE: windows::core::GUID = windows::core::GUID::from_values(
        0x0EBC0471,
        0xA718,
        0x4C0F,
        [0xBC, 0x31, 0x18, 0xCE, 0x37, 0xF4, 0xF2, 0x84],
    );

    const WPD_CONTENT_TYPE_FOLDER: windows::core::GUID = windows::core::GUID::from_values(
        0x27E2E392,
        0xA111,
        0x48E0,
        [0xAB, 0x0C, 0xE1, 0x77, 0x05, 0xA0, 0x5F, 0x85],
    );

    const WPD_OBJECT_FORMAT_UNDEFINED: windows::core::GUID = windows::core::GUID::from_values(
        0x30010000,
        0xAE6C,
        0x4804,
        [0x98, 0xBA, 0xC5, 0x7B, 0x46, 0x96, 0x5F, 0xE7],
    );

    // ── Shell namespace GUIDs ─────────────────────────────────────────────────

    // FOLDERID_ComputerFolder = {0AC0837C-BBF8-452A-850D-79D08E667CA7}
    #[allow(non_upper_case_globals)]
    const FOLDERID_ComputerFolder: windows::core::GUID = windows::core::GUID::from_values(
        0x0AC0837C,
        0xBBF8,
        0x452A,
        [0x85, 0x0D, 0x79, 0xD0, 0x8E, 0x66, 0x7C, 0xA7],
    );

    // BHID_SFObject = {3981E224-F559-11D3-8E3A-00C04F6837D5}
    #[allow(non_upper_case_globals)]
    const BHID_SFObject: windows::core::GUID = windows::core::GUID::from_values(
        0x3981E224,
        0xF559,
        0x11D3,
        [0x8E, 0x3A, 0x00, 0xC0, 0x4F, 0x68, 0x37, 0xD5],
    );

    // RAII guard: initialises COM on construction, uninitialises on drop.
    struct CoInitGuard;

    impl CoInitGuard {
        fn init() -> Result<Self> {
            let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            // RPC_E_CHANGED_MODE: thread already owns a different COM apartment — error.
            if hr == windows::Win32::Foundation::RPC_E_CHANGED_MODE {
                return Err(anyhow::anyhow!(
                    "COM: thread already initialized with incompatible apartment model"
                ));
            }
            // S_OK (first init) or S_FALSE (already initialized) — both need a CoUninitialize.
            Ok(CoInitGuard)
        }

        fn init_sta() -> Result<Self> {
            let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
            if hr == windows::Win32::Foundation::RPC_E_CHANGED_MODE {
                return Err(anyhow::anyhow!(
                    "COM: thread already initialized with incompatible apartment model"
                ));
            }
            Ok(CoInitGuard)
        }
    }

    impl Drop for CoInitGuard {
        fn drop(&mut self) {
            unsafe { CoUninitialize() };
        }
    }

    // WpdHandle stores the WPD device ID and the device's Shell display name.
    // A fresh IPortableDevice session (CoCreateInstance + Open) is opened at the
    // start of every MtpHandle method via session() and released on return.
    // IPortableDevice sessions are bound to the COM context that called Open() and
    // cannot be reused across different spawn_blocking threads, so each operation
    // must open its own session.
    pub struct WpdHandle {
        device_id: String,
        friendly_name: String,
        /// Cached storage object ID from DeviceManifest. When Some, WPD calls skip the
        /// first-child enumeration under DEVICE and use this ID directly.
        storage_id: Option<String>,
        shell_worker: Mutex<Option<ShellCopyWorker>>,
        warnings: Mutex<Vec<String>>,
    }

    impl WpdHandle {
        pub fn open(
            wpd_device_id: &str,
            friendly_name: &str,
            storage_id: Option<String>,
        ) -> Result<Self> {
            Ok(Self {
                device_id: wpd_device_id.to_string(),
                friendly_name: friendly_name.to_string(),
                storage_id,
                shell_worker: Mutex::new(None),
                warnings: Mutex::new(Vec::new()),
            })
        }

        // Opens a fresh COM MTA context and WPD device session.
        // Always destructure as `let (_com, device) = self.session()?` — the
        // reverse local-variable drop order ensures IPortableDevice is released
        // before CoUninitialize is called (correct COM teardown sequence).
        fn session(&self) -> Result<(CoInitGuard, IPortableDevice)> {
            crate::daemon_log!("[WPD] session(): opening device {}", self.device_id);
            let com = CoInitGuard::init()?;
            unsafe {
                let id_hstr = HSTRING::from(self.device_id.as_str());
                let mut last_error: Option<windows::core::Error> = None;
                for attempt in 1..=4 {
                    let device: IPortableDevice =
                        CoCreateInstance(&PortableDevice, None, CLSCTX_INPROC_SERVER)?;
                    let client_info: IPortableDeviceValues =
                        CoCreateInstance(&PortableDeviceValues, None, CLSCTX_INPROC_SERVER)?;
                    match device.Open(PCWSTR(id_hstr.as_ptr()), &client_info) {
                        Ok(()) => {
                            if attempt > 1 {
                                crate::daemon_log!(
                                    "[WPD] session(): device.Open() succeeded on attempt {}",
                                    attempt
                                );
                            } else {
                                crate::daemon_log!("[WPD] session(): device.Open() succeeded");
                            }
                            return Ok((com, device));
                        }
                        Err(e) if is_retryable_wpd_open_error(&e) && attempt < 4 => {
                            crate::daemon_log!(
                                "[WPD] session(): device.Open() transient failure on attempt {}: {:?}; retrying",
                                attempt,
                                e
                            );
                            last_error = Some(e);
                            std::thread::sleep(std::time::Duration::from_millis(
                                200 * attempt as u64,
                            ));
                        }
                        Err(e) => {
                            crate::daemon_log!("[WPD] session(): device.Open() failed: {:?}", e);
                            return Err(e.into());
                        }
                    }
                }
                let e = last_error.expect("WPD session retry loop must record a retryable failure");
                crate::daemon_log!("[WPD] session(): device.Open() failed: {:?}", e);
                Err(e.into())
            }
        }

        fn prefers_shell_copy(&self) -> bool {
            let device_id = self.device_id.to_ascii_lowercase();
            let friendly_name = self.friendly_name.to_ascii_lowercase();
            device_id.contains("vid_091e") || friendly_name.contains("garmin")
        }

        fn shell_copy(
            &self,
            parent_components: &[&str],
            filename: &str,
            temp_path: &std::path::Path,
        ) -> Result<()> {
            if let Some(worker) = self.shell_worker.lock().unwrap().as_ref() {
                return worker.copy(parent_components, filename, temp_path);
            }
            shell_copy_to_device(&self.friendly_name, parent_components, filename, temp_path)
        }

        fn push_warning(&self, warning: String) {
            self.warnings.lock().unwrap().push(warning);
        }
    }

    // ── Private WPD helpers (free functions; COM must already be initialised) ──

    // Resolves a slash-delimited path to a WPD object ID.
    // `storage_id`: when Some, skips DEVICE enumeration and uses it as the storage root directly.
    fn path_to_object_id(
        content: &IPortableDeviceContent,
        path: &str,
        storage_id: Option<&str>,
    ) -> Result<HSTRING> {
        let components = super::split_path_components(path);
        unsafe {
            let root_id = if let Some(id) = storage_id {
                id.to_string()
            } else {
                let device_hstr = HSTRING::from("DEVICE");
                let root_enum = content.EnumObjects(0, PCWSTR(device_hstr.as_ptr()), None)?;
                let mut storage_buf = [PWSTR::null()];
                let mut fetched = 0u32;
                let _ = root_enum.Next(&mut storage_buf, &mut fetched);
                if fetched == 0 || storage_buf[0].is_null() {
                    return Err(anyhow::anyhow!(
                        "WPD: no storage objects found under DEVICE"
                    ));
                }
                let s = storage_buf[0].to_string()?;
                CoTaskMemFree(Some(storage_buf[0].0 as *const _));
                s
            };

            if components.is_empty() {
                return Ok(HSTRING::from(&root_id));
            }

            let resolved_id =
                resolve_path_with_lookup(root_id, &components, |parent, component| {
                    find_child_object_id(content, parent, component)
                })?;

            Ok(HSTRING::from(&resolved_id))
        }
    }

    fn first_storage_id(content: &IPortableDeviceContent) -> Result<String> {
        unsafe {
            let device_hstr = HSTRING::from("DEVICE");
            let root_enum = content.EnumObjects(0, PCWSTR(device_hstr.as_ptr()), None)?;
            let mut storage_buf = [PWSTR::null()];
            let mut fetched = 0u32;
            let _ = root_enum.Next(&mut storage_buf, &mut fetched);
            if fetched == 0 || storage_buf[0].is_null() {
                return Err(anyhow::anyhow!(
                    "WPD: no storage objects found under DEVICE"
                ));
            }
            let storage_id = storage_buf[0].to_string()?;
            CoTaskMemFree(Some(storage_buf[0].0 as *const _));
            Ok(storage_id)
        }
    }

    // Returns the first child object ID under `parent_id` whose
    // WPD_OBJECT_ORIGINAL_FILE_NAME matches `name` (case-insensitive), or None.
    fn find_child_object_id(
        content: &IPortableDeviceContent,
        parent_id: &str,
        name: &str,
    ) -> Result<Option<String>> {
        unsafe {
            let props = content.Properties()?;
            let parent_hstr = HSTRING::from(parent_id);
            let child_enum = content.EnumObjects(0, PCWSTR(parent_hstr.as_ptr()), None)?;
            let mut batch: Vec<PWSTR> = vec![PWSTR::null(); 64];
            let mut found: Option<String> = None;
            'outer: loop {
                let mut fetched = 0u32;
                let _ = child_enum.Next(batch.as_mut_slice(), &mut fetched);
                if fetched == 0 {
                    break;
                }
                for i in 0..fetched as usize {
                    if batch[i].is_null() {
                        continue;
                    }
                    let obj_id = match batch[i].to_string() {
                        Ok(s) => s,
                        Err(_) => {
                            CoTaskMemFree(Some(batch[i].0 as *const _));
                            batch[i] = PWSTR::null();
                            continue;
                        }
                    };
                    CoTaskMemFree(Some(batch[i].0 as *const _));
                    batch[i] = PWSTR::null();

                    let obj_hstr = HSTRING::from(&obj_id);
                    if let Ok(values) = props.GetValues(PCWSTR(obj_hstr.as_ptr()), None) {
                        if let Ok(name_pwstr) =
                            values.GetStringValue(&WPD_OBJECT_ORIGINAL_FILE_NAME)
                        {
                            if !name_pwstr.is_null() {
                                let child_name = name_pwstr.to_string().unwrap_or_default();
                                CoTaskMemFree(Some(name_pwstr.0 as *const _));
                                if child_name.eq_ignore_ascii_case(name) {
                                    found = Some(obj_id);
                                    break 'outer;
                                }
                            }
                        }
                    }
                }
            }
            Ok(found)
        }
    }

    // ── Shell helpers (used only by write_file) ───────────────────────────────

    // Finds the first child of `parent` whose Shell display name equals `name`.
    fn find_shell_child_by_name(parent: &IShellItem, name: &str) -> Result<IShellItem> {
        unsafe {
            let folder: IShellFolder = parent.BindToHandler(None, &BHID_SFObject)?;
            let mut enum_opt: Option<IEnumIDList> = None;
            folder
                .EnumObjects(
                    None,
                    (SHCONTF_FOLDERS.0 | SHCONTF_NONFOLDERS.0) as u32,
                    &mut enum_opt,
                )
                .ok()?;
            let enum_list =
                enum_opt.ok_or_else(|| anyhow::anyhow!("WPD: no Shell enumerator for This PC"))?;
            loop {
                // IEnumIDList::Next takes a slice; slice length = requested count.
                let mut pidl: *mut ITEMIDLIST = std::ptr::null_mut();
                let mut fetched = 0u32;
                // S_FALSE (end of list) is not an error — check fetched count instead.
                let _ = enum_list.Next(std::slice::from_mut(&mut pidl), Some(&mut fetched));
                if fetched == 0 || pidl.is_null() {
                    return Err(anyhow::anyhow!(
                        "WPD: device '{}' not found in Shell namespace under This PC",
                        name
                    ));
                }
                let mut strret: STRRET = std::mem::zeroed();
                let matches = if folder
                    .GetDisplayNameOf(pidl, SHGDN_NORMAL, &mut strret as *mut _)
                    .is_ok()
                {
                    let mut str_ptr = PWSTR::null();
                    if StrRetToStrW(&mut strret as *mut _, Some(pidl as *const _), &mut str_ptr)
                        .is_ok()
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
                    let item: IShellItem = SHCreateItemWithParent(None, &folder, pidl as *const _)?;
                    CoTaskMemFree(Some(pidl as *const _));
                    return Ok(item);
                }
                CoTaskMemFree(Some(pidl as *const _));
            }
        }
    }

    // Returns the first folder child of `parent` (the storage root of an MTP device).
    fn first_shell_folder_child(parent: &IShellItem) -> Result<IShellItem> {
        unsafe {
            let folder: IShellFolder = parent.BindToHandler(None, &BHID_SFObject)?;
            let mut enum_opt: Option<IEnumIDList> = None;
            folder
                .EnumObjects(None, SHCONTF_FOLDERS.0 as u32, &mut enum_opt)
                .ok()?;
            let enum_list = enum_opt
                .ok_or_else(|| anyhow::anyhow!("WPD: no Shell enumerator for MTP device"))?;
            let mut pidl: *mut ITEMIDLIST = std::ptr::null_mut();
            let mut fetched = 0u32;
            let _ = enum_list.Next(std::slice::from_mut(&mut pidl), Some(&mut fetched));
            if fetched == 0 || pidl.is_null() {
                return Err(anyhow::anyhow!("WPD: no storage root found on MTP device"));
            }
            let item: IShellItem = SHCreateItemWithParent(None, &folder, pidl as *const _)?;
            CoTaskMemFree(Some(pidl as *const _));
            Ok(item)
        }
    }

    // Navigates `root` down each path component via ParseDisplayName.
    fn navigate_shell_path(root: IShellItem, components: &[&str]) -> Result<IShellItem> {
        let mut current = root;
        for &component in components {
            unsafe {
                let folder: IShellFolder = current.BindToHandler(None, &BHID_SFObject)?;
                let mut wide: Vec<u16> = component
                    .encode_utf16()
                    .chain(std::iter::once(0u16))
                    .collect();
                let mut child_pidl: *mut ITEMIDLIST = std::ptr::null_mut();
                let mut attrs = 0u32;
                // ParseDisplayName: pcheaten=None (Option<*const u32>), ppidl, pdwattributes
                folder.ParseDisplayName(
                    None,
                    None,
                    PWSTR(wide.as_mut_ptr()),
                    None,
                    &mut child_pidl,
                    &mut attrs,
                )?;
                let child: IShellItem =
                    SHCreateItemWithParent(None, &folder, child_pidl as *const _)?;
                CoTaskMemFree(Some(child_pidl as *const _));
                current = child;
            }
        }
        Ok(current)
    }

    fn delete_shell_child_if_exists(parent: &IShellItem, name: &str) -> Result<()> {
        let Ok(existing) = find_shell_child_by_name(parent, name) else {
            return Ok(());
        };

        unsafe {
            let file_op: IFileOperation =
                CoCreateInstance(&FileOperation, None, CLSCTX_INPROC_SERVER)?;
            file_op.SetOperationFlags(FILEOPERATION_FLAGS(0x0414))?;
            file_op.DeleteItem(&existing, None)?;
            file_op.PerformOperations()?;
            if file_op.GetAnyOperationsAborted()?.as_bool() {
                return Err(anyhow::anyhow!(
                    "WPD shell delete aborted for existing '{}'",
                    name
                ));
            }
            Ok(())
        }
    }

    // Spawns a fresh OS thread with an STA COM context and runs `shell_copy_inner` on it.
    // `shell_copy_to_device` must execute on a dedicated STA thread because
    // `IShellFolder::EnumObjects` requires STA, and spawn_blocking uses MTA threads.
    fn shell_copy_to_device(
        friendly_name: &str,
        parent_components: &[&str],
        filename: &str,
        temp_path: &std::path::Path,
    ) -> Result<()> {
        let friendly_name = friendly_name.to_string();
        let parent_components: Vec<String> =
            parent_components.iter().map(|s| s.to_string()).collect();
        let filename = filename.to_string();
        let temp_path = temp_path.to_path_buf();
        let (tx, rx) = std::sync::mpsc::channel::<Result<()>>();
        std::thread::spawn(move || {
            let result = (|| -> Result<()> {
                let _com = CoInitGuard::init_sta()?;
                let computer_item: IShellItem = unsafe {
                    SHGetKnownFolderItem(&FOLDERID_ComputerFolder, KF_FLAG_DEFAULT, None)?
                };
                let device_item = find_shell_child_by_name(&computer_item, &friendly_name)?;
                let storage_item = first_shell_folder_child(&device_item)?;
                shell_copy_in_session(
                    storage_item,
                    &parent_components
                        .iter()
                        .map(|s| s.as_str())
                        .collect::<Vec<_>>(),
                    &filename,
                    &temp_path,
                )
            })();
            let _ = tx.send(result);
        });
        rx.recv()
            .map_err(|_| anyhow::anyhow!("WPD: shell_copy_to_device STA thread panicked"))?
    }

    enum ShellCopyCommand {
        Copy {
            parent_components: Vec<String>,
            filename: String,
            temp_path: std::path::PathBuf,
            reply: mpsc::Sender<Result<()>>,
        },
        Shutdown,
    }

    struct ShellCopyWorker {
        tx: mpsc::Sender<ShellCopyCommand>,
        join: Option<JoinHandle<()>>,
    }

    impl ShellCopyWorker {
        fn start(friendly_name: String) -> Result<Self> {
            let (tx, rx) = mpsc::channel::<ShellCopyCommand>();
            let (ready_tx, ready_rx) = mpsc::channel::<Result<()>>();
            let join = std::thread::spawn(move || {
                let init_result = (|| -> Result<(CoInitGuard, IShellItem)> {
                    let com = CoInitGuard::init_sta()?;
                    let computer_item: IShellItem = unsafe {
                        SHGetKnownFolderItem(&FOLDERID_ComputerFolder, KF_FLAG_DEFAULT, None)?
                    };
                    let device_item = find_shell_child_by_name(&computer_item, &friendly_name)?;
                    let storage_item = first_shell_folder_child(&device_item)?;
                    Ok((com, storage_item))
                })();

                let Ok((com, storage_item)) = init_result else {
                    let _ = ready_tx.send(init_result.map(|_| ()));
                    return;
                };
                let _ = ready_tx.send(Ok(()));

                while let Ok(command) = rx.recv() {
                    match command {
                        ShellCopyCommand::Copy {
                            parent_components,
                            filename,
                            temp_path,
                            reply,
                        } => {
                            let parent_refs: Vec<&str> =
                                parent_components.iter().map(|s| s.as_str()).collect();
                            let result = shell_copy_in_session(
                                storage_item.clone(),
                                &parent_refs,
                                &filename,
                                &temp_path,
                            );
                            let _ = reply.send(result);
                        }
                        ShellCopyCommand::Shutdown => break,
                    }
                }
                drop(storage_item);
                drop(com);
            });

            ready_rx.recv().map_err(|_| {
                anyhow::anyhow!("WPD: Shell session worker failed during startup")
            })??;
            Ok(Self {
                tx,
                join: Some(join),
            })
        }

        fn copy(
            &self,
            parent_components: &[&str],
            filename: &str,
            temp_path: &std::path::Path,
        ) -> Result<()> {
            let (reply, rx) = mpsc::channel::<Result<()>>();
            self.tx
                .send(ShellCopyCommand::Copy {
                    parent_components: parent_components.iter().map(|s| s.to_string()).collect(),
                    filename: filename.to_string(),
                    temp_path: temp_path.to_path_buf(),
                    reply,
                })
                .map_err(|_| anyhow::anyhow!("WPD: Shell session worker stopped"))?;
            rx.recv()
                .map_err(|_| anyhow::anyhow!("WPD: Shell session worker dropped copy result"))?
        }
    }

    impl Drop for ShellCopyWorker {
        fn drop(&mut self) {
            let _ = self.tx.send(ShellCopyCommand::Shutdown);
            if let Some(join) = self.join.take() {
                let _ = join.join();
            }
        }
    }

    // Inner implementation — called with COM already initialized on an STA thread.
    fn shell_copy_in_session(
        storage_item: IShellItem,
        parent_components: &[&str],
        filename: &str,
        temp_path: &std::path::Path,
    ) -> Result<()> {
        unsafe {
            let dest_folder = navigate_shell_path(storage_item, parent_components)?;
            delete_shell_child_if_exists(&dest_folder, filename)?;

            let temp_hstr = HSTRING::from(temp_path.to_string_lossy().as_ref());
            let source_item: IShellItem =
                SHCreateItemFromParsingName(PCWSTR(temp_hstr.as_ptr()), None)?;

            crate::daemon_log!(
                "[WPD] shell_copy_to_device: PerformOperations path={}/{}",
                parent_components.join("/"),
                filename
            );
            let file_op: IFileOperation =
                CoCreateInstance(&FileOperation, None, CLSCTX_INPROC_SERVER)?;
            // FOF_SILENT(0x0004) | FOF_NOCONFIRMATION(0x0010) | FOF_NOERRORUI(0x0400)
            file_op.SetOperationFlags(FILEOPERATION_FLAGS(0x0414))?;
            file_op.CopyItem(&source_item, &dest_folder, PCWSTR::null(), None)?;
            file_op.PerformOperations()?;
            crate::daemon_log!("[WPD] shell_copy_to_device: PerformOperations OK");

            if file_op.GetAnyOperationsAborted()?.as_bool() {
                return Err(anyhow::anyhow!("WPD shell copy aborted for '{}'", filename));
            }

            for attempt in 1..=10 {
                if find_shell_child_by_name(&dest_folder, filename).is_ok() {
                    crate::daemon_log!(
                        "[WPD] shell_copy_to_device: verified destination after {} attempt(s)",
                        attempt
                    );
                    return Ok(());
                }
                std::thread::sleep(std::time::Duration::from_millis(200));
            }

            Err(anyhow::anyhow!(
                "WPD shell copy reported success but '{}' was not visible on device",
                filename
            ))
        }
    }

    // Creates an IPortableDevicePropVariantCollection containing a single
    // VT_LPWSTR PROPVARIANT holding the given object ID string.
    fn make_object_id_collection(obj_id: &str) -> Result<IPortableDevicePropVariantCollection> {
        unsafe {
            let collection: IPortableDevicePropVariantCollection = CoCreateInstance(
                &PortableDevicePropVariantCollection,
                None,
                CLSCTX_INPROC_SERVER,
            )?;

            let utf16: Vec<u16> = obj_id.encode_utf16().chain(std::iter::once(0u16)).collect();
            let byte_len = utf16.len() * 2;
            let ptr = CoTaskMemAlloc(byte_len) as *mut u16;
            if ptr.is_null() {
                return Err(anyhow::anyhow!("CoTaskMemAlloc failed for PROPVARIANT"));
            }
            std::ptr::copy_nonoverlapping(utf16.as_ptr(), ptr, utf16.len());

            let mut raw: windows::core::imp::PROPVARIANT = std::mem::zeroed();
            raw.Anonymous.Anonymous.vt = VT_LPWSTR.0;
            raw.Anonymous.Anonymous.Anonymous.pwszVal = ptr;
            let pv: ManuallyDrop<windows::core::PROPVARIANT> =
                ManuallyDrop::new(std::mem::transmute(raw));

            collection.Add(&*pv as *const _)?;
            CoTaskMemFree(Some(ptr as *const _));

            Ok(collection)
        }
    }

    // Walks `components` from the storage root, creating any missing folder objects.
    // Returns the HSTRING object ID of the final component (or storage root if empty).
    // `storage_id`: when Some, skips DEVICE enumeration and uses it as the storage root directly.
    // HRESULT_FROM_WIN32(ERROR_ALREADY_EXISTS) = 0x800700B7 — tolerated on concurrent creation.
    fn ensure_dir_chain(
        content: &IPortableDeviceContent,
        components: &[&str],
        storage_id: Option<&str>,
    ) -> Result<HSTRING> {
        const ERROR_ALREADY_EXISTS_HRESULT: i32 = 0x800700B7u32 as i32;
        unsafe {
            let root_id = if let Some(id) = storage_id {
                id.to_string()
            } else {
                let device_hstr = HSTRING::from("DEVICE");
                let root_enum = content.EnumObjects(0, PCWSTR(device_hstr.as_ptr()), None)?;
                let mut storage_buf = [PWSTR::null()];
                let mut fetched = 0u32;
                let _ = root_enum.Next(&mut storage_buf, &mut fetched);
                if fetched == 0 || storage_buf[0].is_null() {
                    return Err(anyhow::anyhow!(
                        "WPD: no storage objects found under DEVICE"
                    ));
                }
                let s = storage_buf[0].to_string()?;
                CoTaskMemFree(Some(storage_buf[0].0 as *const _));
                s
            };

            if components.is_empty() {
                return Ok(HSTRING::from(&root_id));
            }

            let mut current_id = root_id;

            for &component in components {
                match find_child_object_id(content, &current_id, component)? {
                    Some(child_id) => {
                        current_id = child_id;
                    }
                    None => {
                        let props: IPortableDeviceValues =
                            CoCreateInstance(&PortableDeviceValues, None, CLSCTX_INPROC_SERVER)?;
                        let parent_hstr = HSTRING::from(&current_id);
                        props
                            .SetStringValue(&WPD_OBJECT_PARENT_ID, PCWSTR(parent_hstr.as_ptr()))?;
                        let comp_hstr = HSTRING::from(component);
                        props.SetStringValue(&WPD_OBJECT_NAME, PCWSTR(comp_hstr.as_ptr()))?;
                        props.SetGuidValue(&WPD_OBJECT_CONTENT_TYPE, &WPD_CONTENT_TYPE_FOLDER)?;
                        let mut new_id_pwstr = PWSTR::null();
                        let create_result =
                            content.CreateObjectWithPropertiesOnly(&props, &mut new_id_pwstr);

                        // T3: free PWSTR before propagating any error.
                        // T13: tolerate "already exists" from a concurrent creator.
                        match create_result {
                            Ok(()) => {
                                let new_id_str = new_id_pwstr.to_string();
                                CoTaskMemFree(Some(new_id_pwstr.0 as *const _));
                                current_id = new_id_str?;
                            }
                            Err(ref e) if e.code().0 == ERROR_ALREADY_EXISTS_HRESULT => {
                                // Another process created the directory concurrently — retrieve it.
                                let existing = find_child_object_id(content, &current_id, component)?
                                    .ok_or_else(|| anyhow::anyhow!(
                                        "WPD ensure_dir_chain: concurrent dir '{}' vanished after create",
                                        component
                                    ))?;
                                current_id = existing;
                            }
                            Err(e) => return Err(e.into()),
                        }
                    }
                }
            }

            Ok(HSTRING::from(&current_id))
        }
    }

    // Recursive helper for list_files: collects all non-folder entries under `parent_id`.
    // `prefix` is the path accumulated so far (empty string at the root).
    fn collect_files_recursive(
        content: &IPortableDeviceContent,
        props: &IPortableDeviceProperties,
        parent_id: &str,
        prefix: &str,
        acc: &mut Vec<FileEntry>,
        warnings: &mut Vec<String>,
    ) -> Result<()> {
        unsafe {
            let parent_hstr = HSTRING::from(parent_id);
            let child_enum = content.EnumObjects(0, PCWSTR(parent_hstr.as_ptr()), None)?;
            let mut batch: Vec<PWSTR> = vec![PWSTR::null(); 64];
            loop {
                let mut fetched = 0u32;
                let _ = child_enum.Next(batch.as_mut_slice(), &mut fetched);
                if fetched == 0 {
                    break;
                }
                for i in 0..fetched as usize {
                    if batch[i].is_null() {
                        continue;
                    }
                    let obj_id = match batch[i].to_string() {
                        Ok(s) => s,
                        Err(_) => {
                            CoTaskMemFree(Some(batch[i].0 as *const _));
                            batch[i] = PWSTR::null();
                            continue;
                        }
                    };
                    CoTaskMemFree(Some(batch[i].0 as *const _));
                    batch[i] = PWSTR::null();

                    let obj_hstr = HSTRING::from(&obj_id);
                    let values = match props.GetValues(PCWSTR(obj_hstr.as_ptr()), None) {
                        Ok(v) => v,
                        Err(_) => continue,
                    };

                    // Read filename — skip entry if missing.
                    let name_pwstr = match values.GetStringValue(&WPD_OBJECT_ORIGINAL_FILE_NAME) {
                        Ok(p) if !p.is_null() => p,
                        _ => continue,
                    };
                    let name = name_pwstr.to_string().unwrap_or_default();
                    CoTaskMemFree(Some(name_pwstr.0 as *const _));
                    if name.is_empty() {
                        continue;
                    }

                    // Read content type.
                    let content_type = values
                        .GetGuidValue(&WPD_OBJECT_CONTENT_TYPE)
                        .unwrap_or(windows::core::GUID::zeroed());

                    if content_type == WPD_CONTENT_TYPE_FOLDER {
                        // Recurse into folder.
                        let new_prefix = if prefix.is_empty() {
                            name.clone()
                        } else {
                            format!("{}/{}", prefix, name)
                        };
                        if let Err(e) = collect_files_recursive(
                            content,
                            props,
                            &obj_id,
                            &new_prefix,
                            acc,
                            warnings,
                        ) {
                            let warning = format!(
                                "[WPD WARN] collect_files_recursive: failed to enumerate {} ({}): {}",
                                new_prefix, obj_id, e
                            );
                            crate::daemon_log!("{}", warning);
                            warnings.push(warning);
                        }
                    } else {
                        // File: read size and push entry.
                        let size = values
                            .GetUnsignedLargeIntegerValue(&WPD_OBJECT_SIZE)
                            .unwrap_or(0);
                        let entry_path = if prefix.is_empty() {
                            name.clone()
                        } else {
                            format!("{}/{}", prefix, name)
                        };
                        acc.push(FileEntry {
                            path: entry_path,
                            name,
                            size,
                        });
                    }
                }
            }
        }
        Ok(())
    }

    impl MtpHandle for WpdHandle {
        fn begin_sync_job(&self) -> Result<()> {
            if !self.prefers_shell_copy() {
                return Ok(());
            }
            let mut worker = self.shell_worker.lock().unwrap();
            if worker.is_none() {
                *worker = Some(ShellCopyWorker::start(self.friendly_name.clone())?);
            }
            Ok(())
        }

        fn read_file(&self, path: &str) -> Result<Vec<u8>> {
            let (_com, device) = self.session()?;
            unsafe {
                // T1: single Content() handle shared between path lookup and Transfer.
                let content = device.Content()?;
                let obj_id = path_to_object_id(&content, path, self.storage_id.as_deref())?;
                let resources: IPortableDeviceResources = content.Transfer()?;
                let mut optimal_buf = 0u32;
                let mut stream_opt: Option<windows::Win32::System::Com::IStream> = None;
                resources.GetStream(
                    PCWSTR(obj_id.as_ptr()),
                    &WPD_RESOURCE_DEFAULT,
                    0u32, // STGM_READ
                    &mut optimal_buf,
                    &mut stream_opt,
                )?;
                let stream = stream_opt
                    .ok_or_else(|| anyhow::anyhow!("WPD: GetStream returned no stream"))?;
                let chunk = (optimal_buf as usize).max(4096);
                let mut result = Vec::new();
                let mut buf = vec![0u8; chunk];
                loop {
                    let mut bytes_read = 0u32;
                    let _ = stream.Read(
                        buf.as_mut_ptr() as *mut _,
                        chunk as u32,
                        Some(&mut bytes_read),
                    );
                    if bytes_read == 0 {
                        break;
                    }
                    result.extend_from_slice(&buf[..bytes_read as usize]);
                }
                Ok(result)
            }
        }

        fn write_file(&self, path: &str, data: &[u8]) -> Result<()> {
            let components = super::split_path_components(path);
            if components.is_empty() {
                return Err(anyhow::anyhow!("WPD write_file: empty path"));
            }
            let filename = components[components.len() - 1];
            let parent_components = &components[..components.len() - 1];

            if self.prefers_shell_copy() {
                {
                    let (_com, device) = self.session()?;
                    unsafe {
                        let content = device.Content()?;
                        // T4: pass storage_id to avoid double DEVICE enumeration.
                        ensure_dir_chain(&content, parent_components, self.storage_id.as_deref())?;
                    }
                }

                crate::daemon_log!(
                    "[WPD] write_file: using Shell copy first for Garmin-style WPD device path={}",
                    path
                );
                // T7: UUID temp naming to guarantee uniqueness under concurrent writes.
                let temp_dir =
                    std::env::temp_dir().join(format!("hifimule_{}", uuid::Uuid::new_v4()));
                std::fs::create_dir_all(&temp_dir)?;
                let temp_path = temp_dir.join(filename);
                std::fs::write(&temp_path, data)?;
                let copy_result = self.shell_copy(parent_components, filename, &temp_path);
                let _ = std::fs::remove_file(&temp_path);
                let _ = std::fs::remove_dir(&temp_dir);
                return copy_result;
            }

            // Primary path (WPD): ensure dirs, delete existing, attempt
            // CreateObjectWithPropertiesAndData. Works correctly on standard WPD devices
            // (USB drives, most MTP devices). Falls back to Shell copy if the WPD write
            // fails — e.g. Garmin's driver creates a folder object instead of a file, so the
            // subsequent stream write stalls and we fall through to the Shell path.
            let wpd_result: Result<()> = {
                let (_com, device) = self.session()?;
                unsafe {
                    let content = device.Content()?;
                    // T4: pass storage_id.
                    let parent_id_hstr =
                        ensure_dir_chain(&content, parent_components, self.storage_id.as_deref())?;
                    let parent_id_str = parent_id_hstr.to_string();

                    if let Some(existing_id) =
                        find_child_object_id(&content, &parent_id_str, filename)?
                    {
                        // T12: log the ID of the object being deleted before replace.
                        crate::daemon_log!(
                            "[WPD] write_file: deleting existing object {:?} at path={}",
                            existing_id,
                            path
                        );
                        let col = make_object_id_collection(&existing_id)?;
                        let mut pp: Option<IPortableDevicePropVariantCollection> = None;
                        let _ = content.Delete(0, &col, &mut pp);
                    }

                    let result = (|| -> Result<()> {
                        let props: IPortableDeviceValues =
                            CoCreateInstance(&PortableDeviceValues, None, CLSCTX_INPROC_SERVER)?;
                        props.SetStringValue(
                            &WPD_OBJECT_PARENT_ID,
                            PCWSTR(parent_id_hstr.as_ptr()),
                        )?;
                        let fname_hstr = HSTRING::from(filename);
                        props.SetStringValue(
                            &WPD_OBJECT_ORIGINAL_FILE_NAME,
                            PCWSTR(fname_hstr.as_ptr()),
                        )?;
                        props.SetStringValue(&WPD_OBJECT_NAME, PCWSTR(fname_hstr.as_ptr()))?;
                        props.SetGuidValue(
                            &WPD_OBJECT_CONTENT_TYPE,
                            &WPD_CONTENT_TYPE_GENERIC_FILE,
                        )?;
                        props.SetGuidValue(&WPD_OBJECT_FORMAT, &WPD_OBJECT_FORMAT_UNDEFINED)?;
                        props.SetUnsignedLargeIntegerValue(&WPD_OBJECT_SIZE, data.len() as u64)?;

                        crate::daemon_log!(
                            "[WPD] write_file: CreateObjectWithPropertiesAndData path={} size={}",
                            path,
                            data.len()
                        );
                        let mut stream_opt: Option<IStream> = None;
                        let mut optimal_buf = 0u32;
                        content.CreateObjectWithPropertiesAndData(
                            &props,
                            &mut stream_opt,
                            &mut optimal_buf,
                            std::ptr::null_mut(),
                        )?;
                        let stream = stream_opt
                            .ok_or_else(|| anyhow::anyhow!("WPD write_file: no stream returned"))?;

                        // T2: explicit S_FALSE (partial write) handling.
                        let chunk = (optimal_buf as usize).max(4096);
                        let mut offset = 0usize;
                        while offset < data.len() {
                            let end = (offset + chunk).min(data.len());
                            let slice = &data[offset..end];
                            let mut written = 0u32;
                            let hr = stream.Write(
                                slice.as_ptr() as *const _,
                                slice.len() as u32,
                                Some(&mut written),
                            );
                            if hr == windows::Win32::Foundation::S_FALSE {
                                return Err(anyhow::anyhow!(
                                    "WPD write_file: partial write at offset {}",
                                    offset
                                ));
                            }
                            hr.ok()?;
                            offset += written as usize;
                            if written == 0 {
                                return Err(anyhow::anyhow!(
                                    "WPD write_file: stream stalled (zero bytes written)"
                                ));
                            }
                        }

                        crate::daemon_log!("[WPD] write_file: Commit path={}", path);
                        let data_stream: IPortableDeviceDataStream = stream.cast()?;
                        data_stream.Commit(STGC_DEFAULT)?;
                        crate::daemon_log!("[WPD] write_file: Commit OK path={}", path);
                        Ok(())
                    })();

                    if result.is_err() {
                        // Clean up any erroneous object left by the failed write (e.g. the
                        // folder that Garmin's driver creates in place of a file).
                        if let Ok(Some(bad_id)) =
                            find_child_object_id(&content, &parent_id_str, filename)
                        {
                            // T12: log the erroneous object ID so the incomplete state is diagnosable.
                            crate::daemon_log!(
                                "[WPD] write_file: deleted erroneous object {:?} at path={}",
                                bad_id,
                                path
                            );
                            if let Ok(col) = make_object_id_collection(&bad_id) {
                                let mut pp: Option<IPortableDevicePropVariantCollection> = None;
                                let _ = content.Delete(0, &col, &mut pp);
                            }
                        }
                    }
                    result
                }
            };

            if wpd_result.is_ok() {
                return Ok(());
            }
            // T9: log the original WPD error at warn level before attempting Shell fallback.
            eprintln!(
                "[WPD WARN] write_file: WPD write failed for '{}' ({}), trying Shell copy",
                path,
                wpd_result.as_ref().unwrap_err()
            );

            // Shell copy fallback: write to a local temp file then IFileOperation::CopyItem.
            // Used for devices like Garmin where CreateObjectWithPropertiesAndData creates
            // folder objects; Shell copy uses the MTP SendObject path that works correctly.
            // T7: UUID temp naming.
            let temp_dir = std::env::temp_dir().join(format!("hifimule_{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&temp_dir)?;
            let temp_path = temp_dir.join(filename);
            std::fs::write(&temp_path, data)?;
            let copy_result = self.shell_copy(parent_components, filename, &temp_path);
            let _ = std::fs::remove_file(&temp_path);
            let _ = std::fs::remove_dir(&temp_dir);
            copy_result
        }

        fn delete_file(&self, path: &str) -> Result<()> {
            let (_com, device) = self.session()?;
            unsafe {
                // T1: single Content() handle shared between path lookup and Delete.
                let content = device.Content()?;
                let obj_id_hstr = path_to_object_id(&content, path, self.storage_id.as_deref())?;
                let obj_id_str = obj_id_hstr.to_string();
                let collection = make_object_id_collection(&obj_id_str)?;
                let mut pp_results: Option<IPortableDevicePropVariantCollection> = None;
                content.Delete(0, &collection, &mut pp_results)?;
                Ok(())
            }
        }

        fn list_files(&self, path: &str) -> Result<Vec<FileEntry>> {
            let (_com, device) = self.session()?;
            unsafe {
                // T1: single Content() handle shared between path lookup and recursive listing.
                let content = device.Content()?;
                let root_id_hstr = path_to_object_id(&content, path, self.storage_id.as_deref())?;
                let root_id_str = root_id_hstr.to_string();
                let props = content.Properties()?;
                let mut acc = Vec::new();
                let mut warnings = Vec::new();
                collect_files_recursive(
                    &content,
                    &props,
                    &root_id_str,
                    "",
                    &mut acc,
                    &mut warnings,
                )?;
                for warning in warnings {
                    self.push_warning(warning);
                }
                Ok(acc)
            }
        }

        fn free_space(&self) -> Result<u64> {
            let (_com, device) = self.session()?;
            unsafe {
                let content = device.Content()?;
                let props = content.Properties()?;

                // T4.5: use manifest storage_id when available to avoid first-child enumeration.
                let storage_id_str = if let Some(ref id) = self.storage_id {
                    id.clone()
                } else {
                    first_storage_id(&content)?
                };

                let storage_hstr = HSTRING::from(&storage_id_str);
                let values = props.GetValues(PCWSTR(storage_hstr.as_ptr()), None)?;
                Ok(values.GetUnsignedLargeIntegerValue(&WPD_STORAGE_FREE_SPACE_IN_BYTES)?)
            }
        }

        fn storage_id(&self) -> Result<Option<String>> {
            if let Some(id) = self.storage_id.clone() {
                return Ok(Some(id));
            }
            let (_com, device) = self.session()?;
            unsafe {
                let content = device.Content()?;
                first_storage_id(&content).map(Some)
            }
        }

        fn preferred_audio_container(&self) -> Option<&'static str> {
            self.prefers_shell_copy().then_some("mp3")
        }

        fn take_warnings(&self) -> Vec<String> {
            std::mem::take(&mut *self.warnings.lock().unwrap())
        }

        fn end_sync_job(&self) -> Result<()> {
            let _ = self.shell_worker.lock().unwrap().take();
            Ok(())
        }
    }

    pub fn enumerate() -> Vec<MtpDeviceInfo> {
        let _com_guard = match CoInitGuard::init() {
            Ok(g) => g,
            Err(_) => return vec![],
        };
        unsafe {
            let Ok(manager): std::result::Result<IPortableDeviceManager, _> =
                CoCreateInstance(&PortableDeviceManager, None, CLSCTX_INPROC_SERVER)
            else {
                return vec![];
            };

            let mut count = 0u32;
            if manager
                .GetDevices(std::ptr::null_mut(), &mut count)
                .is_err()
                || count == 0
            {
                return vec![];
            }

            let mut ids: Vec<PWSTR> = vec![PWSTR::null(); count as usize];
            if manager.GetDevices(ids.as_mut_ptr(), &mut count).is_err() {
                return vec![];
            }

            // Build the result vector before freeing the COM-allocated PWSTR buffers.
            let result = ids
                .iter()
                .filter_map(|&id| {
                    if id.is_null() {
                        return None;
                    }
                    let id_str = id.to_string().ok()?;
                    let id_hstr = HSTRING::from(&id_str);
                    let id_pcwstr = PCWSTR(id_hstr.as_ptr());

                    let mut name_len = 0u32;
                    let _ = manager.GetDeviceFriendlyName(id_pcwstr, PWSTR::null(), &mut name_len);

                    let friendly = if name_len > 0 {
                        let mut name_buf: Vec<u16> = vec![0u16; name_len as usize];
                        let _ = manager.GetDeviceFriendlyName(
                            id_pcwstr,
                            PWSTR(name_buf.as_mut_ptr()),
                            &mut name_len,
                        );
                        String::from_utf16_lossy(&name_buf[..name_len.saturating_sub(1) as usize])
                    } else {
                        id_str.clone()
                    };

                    Some(MtpDeviceInfo {
                        friendly_name: friendly,
                        inner: MtpDeviceInner::Wpd {
                            wpd_device_id: id_str.clone(),
                        },
                        device_id: id_str,
                    })
                })
                .collect();

            // Free COM-allocated PWSTR device ID buffers.
            for &id in &ids {
                if !id.is_null() {
                    CoTaskMemFree(Some(id.0 as *const _));
                }
            }

            result
        }
    }
}

// ── Linux / macOS libmtp FFI implementation ───────────────────────────────────
// TODO: replace with libmtp-rs when stable
// Requires libmtp installed: apt install libmtp-dev / brew install libmtp

#[cfg(unix)]
pub mod libmtp {
    use super::{MtpDeviceInfo, MtpDeviceInner};
    use crate::device_io::{FileEntry, MtpHandle};
    use anyhow::Result;
    use libc::{c_char, c_int};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};

    // LIBMTP_Init must be called exactly once globally.
    static LIBMTP_INIT_ONCE: std::sync::Once = std::sync::Once::new();
    fn ensure_libmtp_init() {
        LIBMTP_INIT_ONCE.call_once(|| unsafe { LIBMTP_Init() });
    }

    // Unique suffix per temp file to prevent collision across concurrent handles.
    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);
    fn temp_path() -> std::path::PathBuf {
        let n = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("hifimule-mtp-{}-{}.tmp", std::process::id(), n))
    }

    // Minimal libmtp C ABI subset needed for enumeration and file I/O.
    #[repr(C)]
    struct LIBMTP_DeviceEntry_t {
        vendor: *const c_char,
        vendor_id: u16,
        product: *const c_char,
        product_id: u16,
        device_flags: u32,
    }

    #[repr(C)]
    struct LIBMTP_RawDevice_t {
        device_entry: LIBMTP_DeviceEntry_t,
        bus_location: u32,
        devnum: u8,
    }

    #[repr(C)]
    struct LIBMTP_MtpDevice_t {
        _opaque: [u8; 0],
    }



    #[repr(C)]
    struct LIBMTP_File_t {
        item_id: u32,
        parent_id: u32,
        storage_id: u32,
        filename: *mut c_char,
        filesize: u64,
        modificationdate: i64,
        filetype: u32,
        next: *mut LIBMTP_File_t,
    }

    #[repr(C)]
    struct LIBMTP_folder_t {
        folder_id: u32,
        parent_id: u32,
        storage_id: u32,
        name: *mut c_char,
        sibling: *mut LIBMTP_folder_t,
        child: *mut LIBMTP_folder_t,
    }

    include!(concat!(env!("OUT_DIR"), "/libmtp_constants.rs")); // LIBMTP_FILETYPE_* constants

    const LIBMTP_FILES_AND_FOLDERS_ROOT: u32 = 0xFFFF_FFFF;

    fn filetype_for_extension(ext: &str) -> u32 {
        match ext.to_ascii_lowercase().as_str() {
            "mp3"  => LIBMTP_FILETYPE_MP3,
            "mp2"  => LIBMTP_FILETYPE_MP2,
            "wav"  => LIBMTP_FILETYPE_WAV,
            "ogg"  => LIBMTP_FILETYPE_OGG,
            "flac" => LIBMTP_FILETYPE_FLAC,
            "aac"  => LIBMTP_FILETYPE_AAC,
            "m4a"  => LIBMTP_FILETYPE_M4A,
            "mp4"  => LIBMTP_FILETYPE_MP4,
            "wma"  => LIBMTP_FILETYPE_WMA,
            "m3u" | "m3u8" => LIBMTP_FILETYPE_PLAYLIST,
            _      => LIBMTP_FILETYPE_UNKNOWN,
        }
    }
    const LIBMTP_ERROR_NONE: c_int = 0;

    extern "C" {
        fn LIBMTP_Init();
        fn LIBMTP_Detect_Raw_Devices(
            devices: *mut *mut LIBMTP_RawDevice_t,
            numdevs: *mut c_int,
        ) -> c_int;
        fn LIBMTP_Open_Raw_Device_Uncached(
            rawdevice: *mut LIBMTP_RawDevice_t,
        ) -> *mut LIBMTP_MtpDevice_t;
        fn LIBMTP_Release_Device(device: *mut LIBMTP_MtpDevice_t);
        fn LIBMTP_Get_Files_And_Folders(
            device: *mut LIBMTP_MtpDevice_t,
            storage_id: u32,
            parent_id: u32,
        ) -> *mut LIBMTP_File_t;
        fn LIBMTP_destroy_file_t(file: *mut LIBMTP_File_t);
        fn LIBMTP_Get_File_To_File(
            device: *mut LIBMTP_MtpDevice_t,
            id: u32,
            path: *const c_char,
            callback: *const libc::c_void,
            data: *const libc::c_void,
        ) -> c_int;
        fn LIBMTP_Send_File_From_File(
            device: *mut LIBMTP_MtpDevice_t,
            path: *const c_char,
            filedata: *mut LIBMTP_File_t,
            callback: *const libc::c_void,
            data: *const libc::c_void,
        ) -> c_int;
        fn LIBMTP_Delete_Object(device: *mut LIBMTP_MtpDevice_t, object_id: u32) -> c_int;
        // Returns file metadata by object ID (direct lookup, bypasses enumeration).
        // Returns NULL if the object does not exist. Caller must free with LIBMTP_destroy_file_t.
        fn LIBMTP_Get_Filemetadata(
            device: *mut LIBMTP_MtpDevice_t,
            fileid: u32,
        ) -> *mut LIBMTP_File_t;
        fn LIBMTP_Get_Storage(device: *mut LIBMTP_MtpDevice_t, sortby: c_int) -> c_int;
        fn LIBMTP_Create_Folder(
            device: *mut LIBMTP_MtpDevice_t,
            name: *mut c_char,
            parent_id: u32,
            storage_id: u32,
        ) -> u32;
        // Returns a tree of all folders across all storages. Used by find_folder_in_list
        // to search for an existing folder by name+parent when per-parent enumeration misses it.
        fn LIBMTP_Get_Folder_List(device: *mut LIBMTP_MtpDevice_t) -> *mut LIBMTP_folder_t;
        fn LIBMTP_destroy_folder_t(folder: *mut LIBMTP_folder_t);
    }

    const LIBMTP_STORAGE_SORTBY_NOTSORTED: c_int = 0;

    pub struct LibmtpHandle {
        // libmtp is not thread-safe. The Mutex must be held for the full duration of every
        // FFI call — do NOT copy the raw pointer out and drop the guard before calling FFI.
        device: Arc<Mutex<*mut LIBMTP_MtpDevice_t>>,
        bus_location: u32,
        dev_num: u8,
        // Folder IDs populated lazily on the first path-not-found miss in ensure_path_raw, and
        // extended when new folders are created. Replaced fresh each open — not persisted to the
        // manifest. None = BFS not yet attempted; Some = BFS ran (map may be empty for flat devices).
        folder_hints: Mutex<std::collections::HashMap<String, u32>>,
        hints_primed: std::sync::atomic::AtomicBool,
    }

    unsafe impl Send for LibmtpHandle {}
    unsafe impl Sync for LibmtpHandle {}

    impl Drop for LibmtpHandle {
        fn drop(&mut self) {
            if let Ok(guard) = self.device.lock() {
                let dev = *guard;
                if !dev.is_null() {
                    unsafe { LIBMTP_Release_Device(dev) };
                }
            }
            // If lock is poisoned (prior FFI call panicked), accept the device handle leak.
        }
    }

    impl LibmtpHandle {
        pub fn open(bus_location: u32, dev_num: u8) -> Result<Self> {
            ensure_libmtp_init();
            unsafe {
                let mut raw_devs: *mut LIBMTP_RawDevice_t = std::ptr::null_mut();
                let mut numdevs: c_int = 0;
                let rc = LIBMTP_Detect_Raw_Devices(&mut raw_devs, &mut numdevs);
                if rc != LIBMTP_ERROR_NONE || numdevs == 0 {
                    return Err(anyhow::anyhow!("No MTP devices found (rc={})", rc));
                }
                let raws = std::slice::from_raw_parts_mut(raw_devs, numdevs as usize);
                let found = raws
                    .iter_mut()
                    .find(|r| r.bus_location == bus_location && r.devnum == dev_num);
                let device = match found {
                    Some(r) => LIBMTP_Open_Raw_Device_Uncached(r as *mut _),
                    None => {
                        libc::free(raw_devs as *mut libc::c_void);
                        return Err(anyhow::anyhow!(
                            "MTP device {}:{} not found",
                            bus_location,
                            dev_num
                        ));
                    }
                };
                libc::free(raw_devs as *mut libc::c_void);
                if device.is_null() {
                    return Err(anyhow::anyhow!(
                        "Failed to open MTP device {}:{}",
                        bus_location,
                        dev_num
                    ));
                }
                // Uncached open skips storage enumeration; populate it now so that
                // Get_Files_And_Folders with storage_id=0 resolves correctly.
                let storage_rc = LIBMTP_Get_Storage(device, LIBMTP_STORAGE_SORTBY_NOTSORTED);
                if storage_rc != 0 {
                    crate::daemon_log!(
                        "[libmtp] open: LIBMTP_Get_Storage failed rc={} — root enumeration may not work",
                        storage_rc
                    );
                }
                let handle = Self {
                    device: Arc::new(Mutex::new(device)),
                    bus_location,
                    dev_num,
                    folder_hints: Mutex::new(std::collections::HashMap::new()),
                    hints_primed: std::sync::atomic::AtomicBool::new(false),
                };
                Ok(handle)
            }
        }

        // Resolves a path to a libmtp object ID. Caller must already hold the device Mutex
        // and pass the raw device pointer — this avoids re-entrant locking.
        unsafe fn path_to_object_id_raw(dev: *mut LIBMTP_MtpDevice_t, path: &str) -> Result<u32> {
            let components = super::split_path_components(path);
            if components.is_empty() {
                return Ok(LIBMTP_FILES_AND_FOLDERS_ROOT);
            }
            let mut parent_id = LIBMTP_FILES_AND_FOLDERS_ROOT;
            for component in &components {
                // libmtp documents storage 0 as searching the parent across all available storages.
                // Debian mtp_files(3) and libmtp source map 0 to PTP_GOH_ALL_STORAGE.
                let files = LIBMTP_Get_Files_And_Folders(dev, 0, parent_id);
                if files.is_null() {
                    return Err(anyhow::anyhow!(
                        "libmtp: path component '{}' not found",
                        component
                    ));
                }
                let mut found = None;
                let mut cur = files;
                while !cur.is_null() {
                    let fname = std::ffi::CStr::from_ptr((*cur).filename).to_string_lossy();
                    if found.is_none() && fname.eq_ignore_ascii_case(component) {
                        found = Some((*cur).item_id);
                    }
                    let next = (*cur).next;
                    LIBMTP_destroy_file_t(cur);
                    cur = next;
                }
                parent_id = found.ok_or_else(|| {
                    anyhow::anyhow!("libmtp: path component '{}' not found", component)
                })?;
            }
            Ok(parent_id)
        }

        // Searches for a direct child of `parent_id` named `name` (case-insensitive).
        // Tries storage_id=0 first (all storages), then the given storage_id if non-zero,
        // because some devices (e.g. Garmin) only return sub-folder contents when the
        // specific storage is supplied. Returns (item_id, storage_id) of the first match.
        unsafe fn find_child_object(
            dev: *mut LIBMTP_MtpDevice_t,
            storage_id: u32,
            parent_id: u32,
            name: &str,
        ) -> Option<(u32, u32)> {
            let mut scan_storages = vec![0u32];
            if storage_id != 0 {
                scan_storages.push(storage_id);
            }
            for scan in scan_storages {
                let files = LIBMTP_Get_Files_And_Folders(dev, scan, parent_id);
                if files.is_null() {
                    continue;
                }
                let mut found: Option<(u32, u32)> = None;
                let mut cur = files;
                while !cur.is_null() {
                    if found.is_none() && !(*cur).filename.is_null() {
                        let fname =
                            std::ffi::CStr::from_ptr((*cur).filename).to_string_lossy();
                        if fname.eq_ignore_ascii_case(name) {
                            found = Some(((*cur).item_id, (*cur).storage_id));
                        }
                    }
                    let next = (*cur).next;
                    LIBMTP_destroy_file_t(cur);
                    cur = next;
                }
                if found.is_some() {
                    return found;
                }
            }
            None
        }

        // Like path_to_object_id_raw but propagates storage_id through the traversal
        // and falls back to the hints cache for intermediate folder components that are
        // invisible to MTP enumeration (e.g. Garmin sub-folders).
        unsafe fn path_to_object_id_with_hints_raw(
            dev: *mut LIBMTP_MtpDevice_t,
            path: &str,
            hints: &std::collections::HashMap<String, u32>,
        ) -> Result<u32> {
            let components = super::split_path_components(path);
            if components.is_empty() {
                return Ok(LIBMTP_FILES_AND_FOLDERS_ROOT);
            }
            let mut parent_id = LIBMTP_FILES_AND_FOLDERS_ROOT;
            let mut storage_id: u32 = 0;
            let mut acc_path = String::new();
            for (idx, component) in components.iter().enumerate() {
                if !acc_path.is_empty() {
                    acc_path.push('/');
                }
                acc_path.push_str(component);
                if let Some((item_id, sid)) =
                    Self::find_child_object(dev, storage_id, parent_id, component)
                {
                    parent_id = item_id;
                    if sid != 0 {
                        storage_id = sid;
                    }
                } else {
                    // For intermediate folder components, fall back to hints cache.
                    // Garmin hides sub-folder objects from enumeration; ensure_path_raw lazily
                    // BFS-primes the hints cache on first miss so it can resolve the hidden ID.
                    let is_last_component = idx + 1 == components.len();
                    if !is_last_component {
                        if let Some(&hint_id) = hints.get(&acc_path) {
                            parent_id = hint_id;
                            continue;
                        }
                    }
                    return Err(anyhow::anyhow!(
                        "libmtp: path component '{}' not found",
                        component
                    ));
                }
            }
            Ok(parent_id)
        }

        // Like path_to_object_id_raw but also returns the storage_id of the found leaf object.
        // For an empty path (root) returns (LIBMTP_FILES_AND_FOLDERS_ROOT, 0).
        unsafe fn path_to_object_and_storage_raw(
            dev: *mut LIBMTP_MtpDevice_t,
            path: &str,
        ) -> Result<(u32, u32)> {
            let components = super::split_path_components(path);
            if components.is_empty() {
                return Ok((LIBMTP_FILES_AND_FOLDERS_ROOT, 0));
            }
            let mut parent_id = LIBMTP_FILES_AND_FOLDERS_ROOT;
            let mut storage_id: u32 = 0;
            for component in &components {
                let files = LIBMTP_Get_Files_And_Folders(dev, 0, parent_id);
                if files.is_null() {
                    return Err(anyhow::anyhow!(
                        "libmtp: path component '{}' not found",
                        component
                    ));
                }
                let mut found: Option<(u32, u32)> = None;
                let mut cur = files;
                while !cur.is_null() {
                    let fname = std::ffi::CStr::from_ptr((*cur).filename).to_string_lossy();
                    if found.is_none() && fname.eq_ignore_ascii_case(component) {
                        found = Some(((*cur).item_id, (*cur).storage_id));
                    }
                    let next = (*cur).next;
                    LIBMTP_destroy_file_t(cur);
                    cur = next;
                }
                let (item_id, sid) = found.ok_or_else(|| {
                    anyhow::anyhow!("libmtp: path component '{}' not found", component)
                })?;
                parent_id = item_id;
                storage_id = sid;
            }
            Ok((parent_id, storage_id))
        }

        // Searches a LIBMTP_folder_t tree for a folder whose parent_id matches
        // `target_parent` and whose name matches `name` (case-insensitive ASCII).
        // Siblings are iterated (not recursed) to keep stack depth bounded by nesting
        // depth, not sibling count. Returns (folder_id, storage_id) of the first match.
        unsafe fn search_folder_tree(
            mut node: *mut LIBMTP_folder_t,
            target_parent: u32,
            name: &str,
        ) -> Option<(u32, u32)> {
            while !node.is_null() {
                if (*node).parent_id == target_parent && !(*node).name.is_null() {
                    let node_name = std::ffi::CStr::from_ptr((*node).name).to_string_lossy();
                    if node_name.eq_ignore_ascii_case(name) {
                        return Some(((*node).folder_id, (*node).storage_id));
                    }
                }
                if let Some(found) =
                    Self::search_folder_tree((*node).child, target_parent, name)
                {
                    return Some(found);
                }
                node = (*node).sibling;
            }
            None
        }

        // Fallback for devices that omit folder objects from LIBMTP_Get_Files_And_Folders.
        // Fetches the full folder tree and searches for a folder named `name` at `parent_id`.
        // Returns (folder_id, storage_id) if found.
        unsafe fn find_folder_in_list(
            dev: *mut LIBMTP_MtpDevice_t,
            parent_id: u32,
            name: &str,
        ) -> Option<(u32, u32)> {
            let root = LIBMTP_Get_Folder_List(dev);
            if root.is_null() {
                return None;
            }
            // Root-level folders in LIBMTP_folder_t use parent_id=0 (PTP_GOH_ROOT),
            // while ensure_path_raw tracks root as LIBMTP_FILES_AND_FOLDERS_ROOT (0xFFFF_FFFF).
            let search_parent = if parent_id == LIBMTP_FILES_AND_FOLDERS_ROOT {
                0
            } else {
                parent_id
            };
            let result = Self::search_folder_tree(root, search_parent, name);
            LIBMTP_destroy_folder_t(root);
            result
        }

        // Last-resort fallback: enumerate ALL objects on device using
        // LIBMTP_Get_Files_And_Folders with LIBMTP_FILES_AND_FOLDERS_ROOT (PTP "all objects")
        // and match by parent_id + name. Some devices store sub-folders as non-association PTP
        // objects that are invisible to LIBMTP_Get_Folder_List's association-only query and to
        // parent-filtered LIBMTP_Get_Files_And_Folders, but visible in the flat all-objects dump.
        // NOTE: Garmin smartwatches do NOT support a true all-objects dump; their
        // GetObjectHandles(0xFFFFFFFF) returns only root-level items, so this fallback cannot
        // recover hidden sub-folders on those devices. The folder_hints cache (lazily BFS-primed
        // on the first path-not-found miss in ensure_path_raw) handles the Garmin case.
        unsafe fn find_folder_in_all_objects(
            dev: *mut LIBMTP_MtpDevice_t,
            parent_id: u32,
            storage_id: u32,
            name: &str,
        ) -> Option<(u32, u32)> {
            let target_parent = if parent_id == LIBMTP_FILES_AND_FOLDERS_ROOT {
                0
            } else {
                parent_id
            };
            // Try with the specific storage first (storage=0 may behave as root-only on
            // some devices, while the explicit storage ID triggers a flat all-objects dump).
            for &scan_storage in &[storage_id, 0u32] {
                let files =
                    LIBMTP_Get_Files_And_Folders(dev, scan_storage, LIBMTP_FILES_AND_FOLDERS_ROOT);
                if files.is_null() {
                    continue;
                }
                let mut cur = files;
                let mut result = None;
                while !cur.is_null() {
                    if result.is_none()
                        && !(*cur).filename.is_null()
                        && (*cur).parent_id == target_parent
                    {
                        let fname =
                            std::ffi::CStr::from_ptr((*cur).filename).to_string_lossy();
                        if fname.eq_ignore_ascii_case(name) {
                            result = Some(((*cur).item_id, (*cur).storage_id));
                        }
                    }
                    let next = (*cur).next;
                    LIBMTP_destroy_file_t(cur);
                    cur = next;
                }
                if result.is_some() {
                    return result;
                }
            }
            None
        }

        // Walks path components, creating missing directories with LIBMTP_Create_Folder.
        // Returns (leaf_object_id, storage_id). For an empty path returns (ROOT, 0).
        // storage_id is propagated from found objects; for root-level creates where the
        // root is otherwise empty, falls back to root_storage_id_raw.
        //
        // hints: previously-cached folder IDs (path → object_id). Used when enumeration
        //        does not return a folder that is known to exist (e.g. Garmin devices that
        //        hide sub-folder association objects from GetObjectHandles responses).
        // discovered: output map populated with the IDs of any folders newly created by
        //        this call. Callers merge this into their persistent hint store.
        unsafe fn ensure_path_raw(
            dev: *mut LIBMTP_MtpDevice_t,
            path: &str,
            hints: &mut std::collections::HashMap<String, u32>,
            // When true and hints is still empty, perform a one-shot BFS on the first miss so
            // that devices hiding sub-folders from normal enumeration (e.g. Garmin) don't
            // require an upfront full-device scan at open time.
            can_lazy_prime: bool,
            lazy_primed: &mut bool,
            discovered: &mut std::collections::HashMap<String, u32>,
        ) -> Result<(u32, u32)> {
            let components = super::split_path_components(path);
            if components.is_empty() {
                return Ok((LIBMTP_FILES_AND_FOLDERS_ROOT, 0));
            }
            let mut parent_id = LIBMTP_FILES_AND_FOLDERS_ROOT;
            let mut storage_id: u32 = 0;
            let mut acc_path = String::new();
            for component in &components {
                if !acc_path.is_empty() {
                    acc_path.push('/');
                }
                acc_path.push_str(component);

                let files = LIBMTP_Get_Files_And_Folders(dev, 0, parent_id);
                let mut found: Option<(u32, u32)> = None;
                if !files.is_null() {
                    let mut cur = files;
                    while !cur.is_null() {
                        let fname = std::ffi::CStr::from_ptr((*cur).filename).to_string_lossy();
                        if found.is_none() && fname.eq_ignore_ascii_case(component) {
                            found = Some(((*cur).item_id, (*cur).storage_id));
                        }
                        let next = (*cur).next;
                        LIBMTP_destroy_file_t(cur);
                        cur = next;
                    }
                }
                if let Some((item_id, sid)) = found {
                    parent_id = item_id;
                    storage_id = sid;
                } else {
                    // Check hint cache before attempting to create. Devices that hide
                    // existing sub-folders from MTP enumeration (e.g. Garmin smartwatches)
                    // will never return the folder via LIBMTP_Get_Files_And_Folders even
                    // though it exists; the cached ID lets us skip the doomed create attempt.
                    if let Some(&hint_id) = hints.get(&acc_path) {
                        crate::daemon_log!(
                            "[libmtp] ensure_path: '{}' not visible via enumeration, using cached id={}",
                            component,
                            hint_id
                        );
                        parent_id = hint_id;
                        // storage_id propagates from the parent component (same physical storage).
                        continue;
                    }
                    // Hints not yet built and we are allowed to do so: BFS-prime now so that
                    // Garmin-style hidden folders are discoverable without paying the cost on
                    // every device open (large storage like a smartphone would scan thousands
                    // of folders unnecessarily).
                    //
                    // Scope the BFS to the subtree rooted at the last *successfully resolved*
                    // folder (parent_id). This avoids scanning unrelated top-level trees
                    // (Photos, Downloads, …) when the music folder simply has no sub-folders
                    // yet (e.g. first sync to an empty device).
                    if can_lazy_prime && !*lazy_primed {
                        *lazy_primed = true;
                        // Derive the device-relative path of the parent we're scanning from.
                        // acc_path includes the failing component, so strip it off.
                        let parent_path_prefix =
                            &acc_path[..acc_path.len().saturating_sub(component.len() + 1)];
                        crate::daemon_log!(
                            "[libmtp] ensure_path: '{}' not found — lazy-priming hints under '{}'",
                            component,
                            if parent_path_prefix.is_empty() { "<root>" } else { parent_path_prefix }
                        );
                        let primed = Self::build_folder_hints_raw(
                            dev,
                            parent_id,
                            storage_id,
                            parent_path_prefix,
                        );
                        crate::daemon_log!(
                            "[libmtp] ensure_path: lazy-prime complete, {} hints loaded",
                            primed.len()
                        );
                        hints.extend(primed);
                        if let Some(&hint_id) = hints.get(&acc_path) {
                            crate::daemon_log!(
                                "[libmtp] ensure_path: '{}' resolved via lazy hints id={}",
                                component,
                                hint_id
                            );
                            parent_id = hint_id;
                            continue;
                        }
                    }

                    let create_storage = if parent_id == LIBMTP_FILES_AND_FOLDERS_ROOT {
                        Self::root_storage_id_raw(dev)
                    } else {
                        storage_id
                    };
                    let name_cstr = std::ffi::CString::new(component.as_bytes())?;
                    let new_id = LIBMTP_Create_Folder(
                        dev,
                        name_cstr.as_ptr() as *mut _,
                        parent_id,
                        create_storage,
                    );
                    if new_id == 0 {
                        // Create failed. Some devices (e.g. smartwatches) omit folder
                        // objects from LIBMTP_Get_Files_And_Folders, so an already-existing
                        // folder appears missing and the create fails. Try two fallbacks to
                        // recover the existing folder's ID.
                        if let Some((existing_id, existing_storage)) =
                            Self::find_folder_in_list(dev, parent_id, component)
                                .or_else(|| {
                                    Self::find_folder_in_all_objects(
                                        dev,
                                        parent_id,
                                        storage_id,
                                        component,
                                    )
                                })
                        {
                            crate::daemon_log!(
                                "[libmtp] ensure_path: '{}' already exists as id={} (recovered via fallback)",
                                component,
                                existing_id
                            );
                            parent_id = existing_id;
                            storage_id = existing_storage;
                            continue;
                        }
                        return Err(anyhow::anyhow!(
                            "libmtp: failed to create directory '{}'",
                            component
                        ));
                    }
                    crate::daemon_log!(
                        "[libmtp] ensure_path: created '{}' id={} parent={} storage={}",
                        component,
                        new_id,
                        parent_id,
                        create_storage
                    );
                    discovered.insert(acc_path.clone(), new_id);
                    parent_id = new_id;
                    storage_id = create_storage;
                }
            }
            Ok((parent_id, storage_id))
        }

        // Recursively walks a LIBMTP_folder_t tree (child/sibling linked structure) and inserts
        // every folder into `out` as a device-relative path (e.g. "Music/Artist") → folder_id.
        // Called with parent_path="" for the root siblings returned by LIBMTP_Get_Folder_List_For_Storage.
        // Nodes with a null or empty name are skipped entirely (including their subtrees)
        // because a valid path cannot be constructed without a name component.
        // Returns the storage_id of the first object found at the device root.
        // Used to infer the correct storage when creating root-level files where
        // storage_id=0 is ambiguous (Samsung and other Android devices reject it).
        // Returns 0 if the root is empty (no children found).
        unsafe fn root_storage_id_raw(dev: *mut LIBMTP_MtpDevice_t) -> u32 {
            let files = LIBMTP_Get_Files_And_Folders(dev, 0, LIBMTP_FILES_AND_FOLDERS_ROOT);
            if files.is_null() {
                return 0;
            }
            let storage_id = (*files).storage_id;
            let mut cur = files;
            while !cur.is_null() {
                let next = (*cur).next;
                LIBMTP_destroy_file_t(cur);
                cur = next;
            }
            storage_id
        }

        // BFS over a folder subtree using per-parent LIBMTP_Get_Files_And_Folders.
        // Garmin firmware only exposes sub-folder contents when queried with a specific parent ID;
        // all-parent queries (storage_id, ALL_PARENTS) return only the root level. This BFS mirrors
        // the per-parent enumeration that ensure_path_raw already uses for path traversal.
        //
        // `root_folder_id`  — start BFS here; pass LIBMTP_FILES_AND_FOLDERS_ROOT for a full scan.
        // `root_storage_id` — storage to use when querying root's direct children; 0 = all storages.
        // `path_prefix`     — prepend to all discovered paths (e.g. "Music" when rooted there).
        //
        // Callers must already hold the device Mutex and pass the raw pointer.
        unsafe fn build_folder_hints_raw(
            dev: *mut LIBMTP_MtpDevice_t,
            root_folder_id: u32,
            root_storage_id: u32,
            path_prefix: &str,
        ) -> std::collections::HashMap<String, u32> {
            let mut map = std::collections::HashMap::new();
            // (folder_id, storage_id, device-relative path)
            let mut queue: std::collections::VecDeque<(u32, u32, String)> = Default::default();

            // Seed: direct children of root_folder_id
            let mut cur = LIBMTP_Get_Files_And_Folders(dev, root_storage_id, root_folder_id);
            while !cur.is_null() {
                let next = (*cur).next;
                if (*cur).filetype == LIBMTP_FILETYPE_FOLDER && !(*cur).filename.is_null() {
                    let name = std::ffi::CStr::from_ptr((*cur).filename).to_string_lossy();
                    if !name.is_empty() {
                        let child_path = if path_prefix.is_empty() {
                            name.to_string()
                        } else {
                            format!("{}/{}", path_prefix, name)
                        };
                        map.insert(child_path.clone(), (*cur).item_id);
                        queue.push_back(((*cur).item_id, (*cur).storage_id, child_path));
                    }
                }
                LIBMTP_destroy_file_t(cur);
                cur = next;
            }

            // BFS: for each folder, enumerate its children
            while let Some((folder_id, storage_id, folder_path)) = queue.pop_front() {
                let mut cur = LIBMTP_Get_Files_And_Folders(dev, storage_id, folder_id);
                while !cur.is_null() {
                    let next = (*cur).next;
                    if (*cur).filetype == LIBMTP_FILETYPE_FOLDER && !(*cur).filename.is_null() {
                        let name = std::ffi::CStr::from_ptr((*cur).filename).to_string_lossy();
                        if !name.is_empty() {
                            let path = format!("{}/{}", folder_path, name);
                            map.insert(path.clone(), (*cur).item_id);
                            queue.push_back(((*cur).item_id, (*cur).storage_id, path));
                        }
                    }
                    LIBMTP_destroy_file_t(cur);
                    cur = next;
                }
            }
            map
        }
    }

    impl MtpHandle for LibmtpHandle {
        fn load_folder_hints(&self, hints: std::collections::HashMap<String, u32>) {
            // Treat externally loaded hints as equivalent to having run the BFS — no need
            // to lazy-prime again on the first path miss.
            if !hints.is_empty() {
                self.hints_primed
                    .store(true, std::sync::atomic::Ordering::SeqCst);
            }
            if let Ok(mut guard) = self.folder_hints.lock() {
                *guard = hints;
            }
        }

        fn drain_folder_hints(&self) -> std::collections::HashMap<String, u32> {
            self.folder_hints
                .lock()
                .map(|mut g| std::mem::take(&mut *g))
                .unwrap_or_default()
        }

        fn read_file(&self, path: &str) -> Result<Vec<u8>> {
            let guard = self.device.lock().unwrap();
            let dev = *guard;
            let obj_id = unsafe { Self::path_to_object_id_raw(dev, path)? };
            let tmp = temp_path();
            let tmp_cstr = std::ffi::CString::new(tmp.to_string_lossy().as_bytes())?;
            let rc = unsafe {
                LIBMTP_Get_File_To_File(
                    dev,
                    obj_id,
                    tmp_cstr.as_ptr(),
                    std::ptr::null(),
                    std::ptr::null(),
                )
            };
            drop(guard); // release lock before filesystem I/O
            if rc != LIBMTP_ERROR_NONE {
                let _ = std::fs::remove_file(&tmp);
                return Err(anyhow::anyhow!("libmtp read_file failed: rc={}", rc));
            }
            let data = std::fs::read(&tmp);
            let _ = std::fs::remove_file(&tmp);
            data.map_err(anyhow::Error::from)
        }

        fn write_file(&self, path: &str, data: &[u8]) -> Result<()> {
            let guard = self.device.lock().unwrap();
            let dev = *guard;
            let components = super::split_path_components(path);
            let filename = components
                .last()
                .ok_or_else(|| anyhow::anyhow!("Empty path"))?;
            let parent_path = components[..components.len() - 1].join("/");
            // Snapshot hints without holding the hints lock during FFI.
            let mut hints = self
                .folder_hints
                .lock()
                .map(|g| g.clone())
                .unwrap_or_default();
            // Only allow lazy BFS priming if it hasn't happened yet for this device handle.
            // Using SeqCst for safety; the MTP device lock serialises actual FFI calls anyway.
            let can_lazy_prime = !self
                .hints_primed
                .load(std::sync::atomic::Ordering::SeqCst);
            let mut lazy_primed = false;
            let mut discovered = std::collections::HashMap::new();
            // ensure_path_raw creates any missing parent directories and returns the
            // parent object ID with its storage_id — avoiding the need for a separate
            // root_storage_id_raw fallback for non-root writes.
            let (parent_id, mut storage_id) = unsafe {
                Self::ensure_path_raw(
                    dev,
                    &parent_path,
                    &mut hints,
                    can_lazy_prime,
                    &mut lazy_primed,
                    &mut discovered,
                )?
            };
            // Persist any lazy-primed and newly discovered folder IDs.
            if lazy_primed {
                self.hints_primed
                    .store(true, std::sync::atomic::Ordering::SeqCst);
                hints.extend(discovered);
                if let Ok(mut h) = self.folder_hints.lock() {
                    *h = hints;
                }
            } else if !discovered.is_empty() {
                if let Ok(mut h) = self.folder_hints.lock() {
                    h.extend(discovered);
                }
            }
            // LIBMTP_Send_File_From_File only creates new objects; overwrite requires
            // delete-then-create. Also capture storage_id from the existing object so
            // the send targets the correct storage (storage_id=0 is unreliable for root-level files).
            if let Ok((existing_id, existing_storage)) =
                unsafe { Self::path_to_object_and_storage_raw(dev, path) }
            {
                storage_id = existing_storage;
                crate::daemon_log!(
                    "[libmtp] write_file: overwrite '{}' id={} storage_id={} - deleting",
                    path,
                    existing_id,
                    storage_id
                );
                let del_rc = unsafe { LIBMTP_Delete_Object(dev, existing_id) };
                if del_rc != LIBMTP_ERROR_NONE {
                    crate::daemon_log!(
                        "[libmtp] write_file: pre-delete of '{}' failed rc={} (proceeding)",
                        path,
                        del_rc
                    );
                }
            }
            // For root-level new files, storage_id=0 is ambiguous — the device cannot
            // determine which physical storage to use. Discover it from an existing root child.
            if storage_id == 0 && parent_id == LIBMTP_FILES_AND_FOLDERS_ROOT {
                storage_id = unsafe { Self::root_storage_id_raw(dev) };
                crate::daemon_log!(
                    "[libmtp] write_file: inferred root storage_id={} for '{}'",
                    storage_id,
                    path
                );
            }
            let tmp = temp_path();
            std::fs::write(&tmp, data)?;
            let tmp_cstr = std::ffi::CString::new(tmp.to_string_lossy().as_bytes())?;
            let fname_cstr = std::ffi::CString::new(*filename)?;
            let ext = std::path::Path::new(filename)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("");
            let filetype = filetype_for_extension(ext);
            crate::daemon_log!(
                "[libmtp] write_file: '{}' ext='{}' filetype={}",
                path, ext, filetype
            );
            let mut file_meta = LIBMTP_File_t {
                item_id: 0,
                parent_id,
                storage_id,
                filename: fname_cstr.as_ptr() as *mut _,
                filesize: data.len() as u64,
                modificationdate: 0,
                filetype,
                next: std::ptr::null_mut(),
            };
            let rc = unsafe {
                LIBMTP_Send_File_From_File(
                    dev,
                    tmp_cstr.as_ptr(),
                    &mut file_meta,
                    std::ptr::null(),
                    std::ptr::null(),
                )
            };
            let _ = std::fs::remove_file(&tmp);
            if rc != LIBMTP_ERROR_NONE {
                drop(guard);
                crate::daemon_log!(
                    "[libmtp] write_file: send '{}' failed rc={} parent_id={} storage_id={}",
                    path,
                    rc,
                    parent_id,
                    storage_id
                );
                return Err(anyhow::anyhow!("libmtp write_file failed: rc={}", rc));
            }
            // Verify the object actually exists on the device using its assigned item_id.
            // Some devices (e.g. Garmin watches) hide files from LIBMTP_Get_Files_And_Folders
            // enumeration but are reachable via direct object-ID lookup. This catches the case
            // where the device accepts the transfer and reports success but silently discards it.
            let assigned_id = file_meta.item_id;
            let meta_ptr = unsafe { LIBMTP_Get_Filemetadata(dev, assigned_id) };
            drop(guard);
            if meta_ptr.is_null() {
                crate::daemon_log!(
                    "[libmtp] write_file: '{}' sent OK (item_id={}) but LIBMTP_Get_Filemetadata returned NULL — device silently discarded transfer",
                    path,
                    assigned_id
                );
                return Err(anyhow::anyhow!(
                    "libmtp write_file: file not found on device after transfer (item_id={})",
                    assigned_id
                ));
            }
            unsafe { LIBMTP_destroy_file_t(meta_ptr) };
            crate::daemon_log!(
                "[libmtp] write_file: '{}' confirmed on device (item_id={})",
                path,
                assigned_id
            );
            Ok(())
        }

        fn delete_file(&self, path: &str) -> Result<()> {
            let hints = self
                .folder_hints
                .lock()
                .map(|g| g.clone())
                .unwrap_or_default();
            let guard = self.device.lock().unwrap();
            let dev = *guard;
            let obj_id =
                unsafe { Self::path_to_object_id_with_hints_raw(dev, path, &hints)? };
            let rc = unsafe { LIBMTP_Delete_Object(dev, obj_id) };
            drop(guard);
            if rc != LIBMTP_ERROR_NONE {
                return Err(anyhow::anyhow!("libmtp delete_file failed: rc={}", rc));
            }
            Ok(())
        }

        fn list_files(&self, path: &str) -> Result<Vec<FileEntry>> {
            let guard = self.device.lock().unwrap();
            let dev = *guard;
            let parent_id = unsafe { Self::path_to_object_id_raw(dev, path)? };
            let mut entries = Vec::new();
            unsafe {
                // libmtp documents storage 0 as searching the parent across all available storages.
                // Debian mtp_files(3) and libmtp source map 0 to PTP_GOH_ALL_STORAGE.
                let files = LIBMTP_Get_Files_And_Folders(dev, 0, parent_id);
                let mut cur = files;
                while !cur.is_null() {
                    let name = std::ffi::CStr::from_ptr((*cur).filename)
                        .to_string_lossy()
                        .into_owned();
                    let size = (*cur).filesize;
                    let entry_path = if path.is_empty() {
                        name.clone()
                    } else {
                        format!("{}/{}", path, name)
                    };
                    entries.push(FileEntry {
                        path: entry_path,
                        name,
                        size,
                    });
                    let next = (*cur).next;
                    LIBMTP_destroy_file_t(cur);
                    cur = next;
                }
            }
            drop(guard);
            Ok(entries)
        }

        fn free_space(&self) -> Result<u64> {
            Err(anyhow::anyhow!("libmtp free_space: not yet implemented"))
        }
    }

    pub fn enumerate() -> Vec<MtpDeviceInfo> {
        ensure_libmtp_init();
        unsafe {
            let mut raw_devs: *mut LIBMTP_RawDevice_t = std::ptr::null_mut();
            let mut numdevs: c_int = 0;
            let rc = LIBMTP_Detect_Raw_Devices(&mut raw_devs, &mut numdevs);
            if rc != LIBMTP_ERROR_NONE || numdevs == 0 {
                return vec![];
            }
            let raws = std::slice::from_raw_parts(raw_devs, numdevs as usize);
            // Build the result before freeing the raw device array.
            let result = raws
                .iter()
                .map(|r| {
                    // libmtp sets vendor/product to NULL for devices not in its device table.
                    // Guard against the null dereference that would otherwise cause SIGSEGV.
                    let vendor = if r.device_entry.vendor.is_null() {
                        "Unknown".to_owned()
                    } else {
                        std::ffi::CStr::from_ptr(r.device_entry.vendor)
                            .to_string_lossy()
                            .into_owned()
                    };
                    let product = if r.device_entry.product.is_null() {
                        format!(
                            "USB Device ({:04x}:{:04x})",
                            r.device_entry.vendor_id, r.device_entry.product_id
                        )
                    } else {
                        std::ffi::CStr::from_ptr(r.device_entry.product)
                            .to_string_lossy()
                            .into_owned()
                    };
                    let device_id = format!("{}:{}", r.bus_location, r.devnum);
                    MtpDeviceInfo {
                        device_id: device_id.clone(),
                        friendly_name: format!("{} {}", vendor, product),
                        inner: MtpDeviceInner::Libmtp {
                            bus_location: r.bus_location,
                            dev_num: r.devnum,
                        },
                    }
                })
                .collect();
            libc::free(raw_devs as *mut libc::c_void);
            result
        }
    }
}

// ── Public API dispatchers ────────────────────────────────────────────────────

/// Enumerate currently connected MTP devices. Synchronous — call via spawn_blocking.
pub fn enumerate_mtp_devices() -> Vec<MtpDeviceInfo> {
    #[cfg(target_os = "windows")]
    return windows_wpd::enumerate();
    #[cfg(unix)]
    return libmtp::enumerate();
    #[cfg(not(any(target_os = "windows", unix)))]
    return vec![];
}

/// Open an MTP device and return a ready-to-use IO backend.
/// `storage_id`: optional cached storage object ID from the device's manifest.
pub fn create_mtp_backend(info: &MtpDeviceInfo, storage_id: Option<String>) -> Result<MtpBackend> {
    let handle: Arc<dyn MtpHandle> = match &info.inner {
        #[cfg(target_os = "windows")]
        MtpDeviceInner::Wpd { wpd_device_id } => Arc::new(windows_wpd::WpdHandle::open(
            wpd_device_id,
            &info.friendly_name,
            storage_id,
        )?),
        #[cfg(unix)]
        MtpDeviceInner::Libmtp {
            bus_location,
            dev_num,
        } => {
            let _ = storage_id;
            Arc::new(libmtp::LibmtpHandle::open(*bus_location, *dev_num)?)
        }
    };
    Ok(MtpBackend::new(handle))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::{resolve_path_with_lookup, split_path_components};
    use anyhow::Result;
    use std::collections::HashMap;

    #[test]
    fn test_split_path_components_empty() {
        assert!(split_path_components("").is_empty());
        assert!(split_path_components("/").is_empty());
        assert!(split_path_components(".").is_empty());
    }

    #[test]
    fn test_split_path_components_single() {
        assert_eq!(split_path_components("Music"), vec!["Music"]);
        assert_eq!(split_path_components("/Music"), vec!["Music"]);
    }

    #[test]
    fn test_split_path_components_nested() {
        assert_eq!(
            split_path_components("Music/Artist/Album"),
            vec!["Music", "Artist", "Album"]
        );
        assert_eq!(
            split_path_components(".hifimule.json"),
            vec![".hifimule.json"]
        );
    }

    #[test]
    fn test_split_path_components_trailing_slash() {
        assert_eq!(split_path_components("Music/"), vec!["Music"]);
    }

    #[test]
    fn test_split_path_components_dotdot_filtered() {
        // Path traversal via ".." must be blocked.
        assert!(split_path_components("..").is_empty());
        assert_eq!(split_path_components("Music/../etc"), vec!["Music", "etc"]);
        assert_eq!(split_path_components("../secret"), vec!["secret"]);
    }

    struct MockPortableDeviceContent {
        children: HashMap<(String, String), String>,
    }

    impl MockPortableDeviceContent {
        fn new(edges: &[(&str, &str, &str)]) -> Self {
            let children = edges
                .iter()
                .map(|(parent, name, id)| {
                    (
                        (parent.to_ascii_lowercase(), name.to_ascii_lowercase()),
                        id.to_string(),
                    )
                })
                .collect();
            Self { children }
        }

        fn find_child_object_id(&self, parent_id: &str, name: &str) -> Result<Option<String>> {
            Ok(self
                .children
                .get(&(parent_id.to_ascii_lowercase(), name.to_ascii_lowercase()))
                .cloned())
        }
    }

    #[test]
    fn test_path_to_object_id_mock_empty_path_resolves_to_root() {
        let content = MockPortableDeviceContent::new(&[]);
        let parts = split_path_components("");
        let resolved = resolve_path_with_lookup("storage-1".to_string(), &parts, |parent, name| {
            content.find_child_object_id(parent, name)
        })
        .unwrap();
        assert_eq!(resolved, "storage-1");
    }

    #[test]
    fn test_path_to_object_id_mock_two_level_traversal() {
        let content = MockPortableDeviceContent::new(&[
            ("storage-1", "Music", "obj-music"),
            ("obj-music", "Artist", "obj-artist"),
        ]);
        let parts = split_path_components("Music/Artist");
        let resolved = resolve_path_with_lookup("storage-1".to_string(), &parts, |parent, name| {
            content.find_child_object_id(parent, name)
        })
        .unwrap();
        assert_eq!(resolved, "obj-artist");
    }

    #[test]
    fn test_path_to_object_id_mock_not_found_errors() {
        let content = MockPortableDeviceContent::new(&[("storage-1", "Music", "obj-music")]);
        let parts = split_path_components("Music/NonExistent/Track.mp3");
        let err = resolve_path_with_lookup("storage-1".to_string(), &parts, |parent, name| {
            content.find_child_object_id(parent, name)
        })
        .unwrap_err();
        assert!(
            err.to_string().contains("NonExistent"),
            "error should name the missing component: {}",
            err
        );
    }
}
