// Platform-specific MTP handle implementations and device enumeration.
// Windows: Windows Portable Devices (WPD) via IPortableDeviceManager.
// Linux/macOS: libmtp FFI bindings.
// TODO: replace unix FFI with libmtp-rs when a stable crate is available.

use crate::device_io::{MtpBackend, MtpHandle};
use anyhow::Result;
use std::sync::Arc;

// ── Platform-independent types ────────────────────────────────────────────────

#[allow(dead_code)]
pub struct MtpDeviceInfo {
    /// Unique identifier: WPD device ID (Windows) or "bus_location:dev_num" (Unix).
    pub device_id: String,
    pub friendly_name: String,
    pub inner: MtpDeviceInner,
}

pub enum MtpDeviceInner {
    #[cfg(target_os = "windows")]
    Wpd { wpd_device_id: String },
    #[cfg(unix)]
    Libmtp { bus_location: u32, dev_num: u32 },
}

#[cfg_attr(not(unix), allow(dead_code))]
/// Splits a relative path into traversal components, filtering empty/dot segments.
/// Used by both WPD and libmtp handles to walk object hierarchies.
pub(super) fn split_path_components(path: &str) -> Vec<&str> {
    path.split('/').filter(|c| !c.is_empty() && *c != ".").collect()
}

// ── Windows WPD implementation ────────────────────────────────────────────────

#[cfg(target_os = "windows")]
pub mod windows_wpd {
    use super::{MtpDeviceInfo, MtpDeviceInner};
    use crate::device_io::{FileEntry, MtpHandle};
    use anyhow::Result;
    use windows::core::{HSTRING, PCWSTR, PWSTR};
    use windows::Win32::Devices::PortableDevices::*;
    use windows::Win32::System::Com::*;

    pub struct WpdHandle {
        #[allow(dead_code)]
        device: IPortableDevice,
    }

    // IPortableDevice COM interface lives in an MTA apartment — safe to move across threads.
    unsafe impl Send for WpdHandle {}
    unsafe impl Sync for WpdHandle {}

    impl WpdHandle {
        pub fn open(wpd_device_id: &str) -> Result<Self> {
            unsafe {
                // COM must be initialised on every thread that uses it.
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
                let device: IPortableDevice =
                    CoCreateInstance(&PortableDevice, None, CLSCTX_INPROC_SERVER)?;
                let client_info: IPortableDeviceValues =
                    CoCreateInstance(&PortableDeviceValues, None, CLSCTX_INPROC_SERVER)?;
                let id_hstr = HSTRING::from(wpd_device_id);
                device.Open(PCWSTR(id_hstr.as_ptr()), &client_info)?;
                Ok(Self { device })
            }
        }
    }

    impl MtpHandle for WpdHandle {
        fn read_file(&self, _path: &str) -> Result<Vec<u8>> {
            // Full WPD file I/O via IPortableDeviceResources requires PROPERTYKEY constants
            // (WPD_RESOURCE_DEFAULT etc.) which are not yet wired up. Stubbed for build.
            Err(anyhow::anyhow!("WPD read_file: not yet implemented"))
        }

        fn write_file(&self, _path: &str, _data: &[u8]) -> Result<()> {
            Err(anyhow::anyhow!("WPD write_file: not yet implemented"))
        }

        fn delete_file(&self, _path: &str) -> Result<()> {
            Err(anyhow::anyhow!("WPD delete_file: not yet implemented"))
        }

        fn list_files(&self, _path: &str) -> Result<Vec<FileEntry>> {
            Err(anyhow::anyhow!("WPD list_files: not yet implemented"))
        }

        fn free_space(&self) -> Result<u64> {
            Err(anyhow::anyhow!("WPD free_space: not yet implemented"))
        }
    }

    pub fn enumerate() -> Vec<MtpDeviceInfo> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let Ok(manager): std::result::Result<IPortableDeviceManager, _> =
                CoCreateInstance(&PortableDeviceManager, None, CLSCTX_INPROC_SERVER)
            else {
                return vec![];
            };

            let mut count = 0u32;
            if manager.GetDevices(std::ptr::null_mut(), &mut count).is_err() || count == 0 {
                return vec![];
            }

            let mut ids: Vec<PWSTR> = vec![PWSTR::null(); count as usize];
            if manager.GetDevices(ids.as_mut_ptr(), &mut count).is_err() {
                return vec![];
            }

            ids.into_iter().filter_map(|id| {
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
                    inner: MtpDeviceInner::Wpd { wpd_device_id: id_str.clone() },
                    device_id: id_str,
                })
            }).collect()
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
    use std::sync::{Arc, Mutex};
    use libc::{c_char, c_int};

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

    const LIBMTP_FILES_AND_FOLDERS_ROOT: u32 = 0xFFFF_FFFF;
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
    }

    pub struct LibmtpHandle {
        // libmtp is not thread-safe; the Mutex serialises all calls.
        device: Arc<Mutex<*mut LIBMTP_MtpDevice_t>>,
    }

    unsafe impl Send for LibmtpHandle {}
    unsafe impl Sync for LibmtpHandle {}

    impl Drop for LibmtpHandle {
        fn drop(&mut self) {
            let dev = *self.device.lock().unwrap();
            if !dev.is_null() {
                unsafe { LIBMTP_Release_Device(dev) };
            }
        }
    }

    impl LibmtpHandle {
        pub fn open(bus_location: u32, dev_num: u32) -> Result<Self> {
            unsafe {
                LIBMTP_Init();
                let mut raw_devs: *mut LIBMTP_RawDevice_t = std::ptr::null_mut();
                let mut numdevs: c_int = 0;
                let rc = LIBMTP_Detect_Raw_Devices(&mut raw_devs, &mut numdevs);
                if rc != LIBMTP_ERROR_NONE || numdevs == 0 {
                    return Err(anyhow::anyhow!("No MTP devices found (rc={})", rc));
                }
                let raws = std::slice::from_raw_parts_mut(raw_devs, numdevs as usize);
                let raw = raws.iter_mut()
                    .find(|r| r.bus_location == bus_location && r.devnum == dev_num as u8)
                    .ok_or_else(|| anyhow::anyhow!("MTP device {}:{} not found", bus_location, dev_num))?;
                let device = LIBMTP_Open_Raw_Device_Uncached(raw as *mut _);
                if device.is_null() {
                    return Err(anyhow::anyhow!("Failed to open MTP device {}:{}", bus_location, dev_num));
                }
                Ok(Self { device: Arc::new(Mutex::new(device)) })
            }
        }

        fn path_to_object_id(&self, path: &str) -> Result<u32> {
            let components = super::split_path_components(path);
            if components.is_empty() {
                return Ok(LIBMTP_FILES_AND_FOLDERS_ROOT);
            }
            let dev = *self.device.lock().unwrap();
            let mut parent_id = LIBMTP_FILES_AND_FOLDERS_ROOT;
            unsafe {
                for component in &components {
                    let files = LIBMTP_Get_Files_And_Folders(dev, 0, parent_id);
                    if files.is_null() {
                        return Err(anyhow::anyhow!("libmtp: path component '{}' not found", component));
                    }
                    let mut found = None;
                    let mut cur = files;
                    while !cur.is_null() {
                        let fname = std::ffi::CStr::from_ptr((*cur).filename)
                            .to_string_lossy();
                        if fname.eq_ignore_ascii_case(component) {
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
            }
            Ok(parent_id)
        }
    }

    impl MtpHandle for LibmtpHandle {
        fn read_file(&self, path: &str) -> Result<Vec<u8>> {
            let obj_id = self.path_to_object_id(path)?;
            let dev = *self.device.lock().unwrap();
            let tmp = temp_path();
            let tmp_cstr = std::ffi::CString::new(tmp.to_string_lossy().as_bytes())?;
            let rc = unsafe {
                LIBMTP_Get_File_To_File(
                    dev, obj_id, tmp_cstr.as_ptr(),
                    std::ptr::null(), std::ptr::null(),
                )
            };
            if rc != LIBMTP_ERROR_NONE {
                return Err(anyhow::anyhow!("libmtp read_file failed: rc={}", rc));
            }
            let data = std::fs::read(&tmp)?;
            let _ = std::fs::remove_file(&tmp);
            Ok(data)
        }

        fn write_file(&self, path: &str, data: &[u8]) -> Result<()> {
            let components = super::split_path_components(path);
            let filename = components.last()
                .ok_or_else(|| anyhow::anyhow!("Empty path"))?;
            let parent_path = components[..components.len()-1].join("/");
            let parent_id = self.path_to_object_id(&parent_path)?;
            let dev = *self.device.lock().unwrap();
            let tmp = temp_path();
            std::fs::write(&tmp, data)?;
            let tmp_cstr = std::ffi::CString::new(tmp.to_string_lossy().as_bytes())?;
            let fname_cstr = std::ffi::CString::new(*filename)?;
            let mut file_meta = LIBMTP_File_t {
                item_id: 0,
                parent_id,
                storage_id: 0,
                filename: fname_cstr.as_ptr() as *mut _,
                filesize: data.len() as u64,
                modificationdate: 0,
                filetype: 0,
                next: std::ptr::null_mut(),
            };
            let rc = unsafe {
                LIBMTP_Send_File_From_File(
                    dev, tmp_cstr.as_ptr(), &mut file_meta,
                    std::ptr::null(), std::ptr::null(),
                )
            };
            let _ = std::fs::remove_file(&tmp);
            if rc != LIBMTP_ERROR_NONE {
                return Err(anyhow::anyhow!("libmtp write_file failed: rc={}", rc));
            }
            Ok(())
        }

        fn delete_file(&self, path: &str) -> Result<()> {
            let obj_id = self.path_to_object_id(path)?;
            let dev = *self.device.lock().unwrap();
            let rc = unsafe { LIBMTP_Delete_Object(dev, obj_id) };
            if rc != LIBMTP_ERROR_NONE {
                return Err(anyhow::anyhow!("libmtp delete_file failed: rc={}", rc));
            }
            Ok(())
        }

        fn list_files(&self, path: &str) -> Result<Vec<FileEntry>> {
            let parent_id = self.path_to_object_id(path)?;
            let dev = *self.device.lock().unwrap();
            let mut entries = Vec::new();
            unsafe {
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
                    entries.push(FileEntry { path: entry_path, name, size });
                    let next = (*cur).next;
                    LIBMTP_destroy_file_t(cur);
                    cur = next;
                }
            }
            Ok(entries)
        }

        fn free_space(&self) -> Result<u64> {
            Err(anyhow::anyhow!("libmtp free_space: not yet implemented"))
        }
    }

    fn temp_path() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("jellyfinsync-mtp-{}.tmp", std::process::id()))
    }

    pub fn enumerate() -> Vec<MtpDeviceInfo> {
        unsafe {
            LIBMTP_Init();
            let mut raw_devs: *mut LIBMTP_RawDevice_t = std::ptr::null_mut();
            let mut numdevs: c_int = 0;
            let rc = LIBMTP_Detect_Raw_Devices(&mut raw_devs, &mut numdevs);
            if rc != LIBMTP_ERROR_NONE || numdevs == 0 {
                return vec![];
            }
            let raws = std::slice::from_raw_parts(raw_devs, numdevs as usize);
            raws.iter().map(|r| {
                let vendor = std::ffi::CStr::from_ptr(r.device_entry.vendor)
                    .to_string_lossy().into_owned();
                let product = std::ffi::CStr::from_ptr(r.device_entry.product)
                    .to_string_lossy().into_owned();
                let device_id = format!("{}:{}", r.bus_location, r.devnum);
                MtpDeviceInfo {
                    device_id: device_id.clone(),
                    friendly_name: format!("{} {}", vendor, product),
                    inner: MtpDeviceInner::Libmtp {
                        bus_location: r.bus_location,
                        dev_num: r.devnum as u32,
                    },
                }
            }).collect()
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
pub fn create_mtp_backend(info: &MtpDeviceInfo) -> Result<MtpBackend> {
    let handle: Arc<dyn MtpHandle> = match &info.inner {
        #[cfg(target_os = "windows")]
        MtpDeviceInner::Wpd { wpd_device_id } => {
            Arc::new(windows_wpd::WpdHandle::open(wpd_device_id)?)
        }
        #[cfg(unix)]
        MtpDeviceInner::Libmtp { bus_location, dev_num } => {
            Arc::new(libmtp::LibmtpHandle::open(*bus_location, *dev_num)?)
        }
    };
    Ok(MtpBackend { handle })
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::split_path_components;

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
            split_path_components(".jellyfinsync.json"),
            vec![".jellyfinsync.json"]
        );
    }

    #[test]
    fn test_split_path_components_trailing_slash() {
        assert_eq!(split_path_components("Music/"), vec!["Music"]);
    }
}
