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
    /// Unique identifier: WPD device ID (Windows) or "bus_location:devnum" (Unix).
    pub device_id: String,
    pub friendly_name: String,
    pub inner: MtpDeviceInner,
}

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

// ── Windows WPD implementation ────────────────────────────────────────────────

#[cfg(target_os = "windows")]
pub mod windows_wpd {
    use super::{MtpDeviceInfo, MtpDeviceInner};
    use crate::device_io::{FileEntry, MtpHandle};
    use anyhow::Result;
    use windows::core::{HSTRING, PCWSTR, PWSTR};
    use windows::Win32::Devices::PortableDevices::*;
    use windows::Win32::System::Com::*;
    use windows::Win32::UI::Shell::PropertiesSystem::PROPERTYKEY;

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
    }

    impl Drop for CoInitGuard {
        fn drop(&mut self) {
            unsafe { CoUninitialize() };
        }
    }

    pub struct WpdHandle {
        device: IPortableDevice,
        // Keeps COM initialized for the lifetime of this handle.
        _com_guard: CoInitGuard,
    }

    // Safety: IPortableDevice is a COM MTA interface. _com_guard guarantees COM is initialized
    // on every thread that uses this handle (because spawn_blocking creates fresh threads).
    unsafe impl Send for WpdHandle {}
    unsafe impl Sync for WpdHandle {}

    impl WpdHandle {
        pub fn open(wpd_device_id: &str) -> Result<Self> {
            let com_guard = CoInitGuard::init()?;
            unsafe {
                let device: IPortableDevice =
                    CoCreateInstance(&PortableDevice, None, CLSCTX_INPROC_SERVER)?;
                let client_info: IPortableDeviceValues =
                    CoCreateInstance(&PortableDeviceValues, None, CLSCTX_INPROC_SERVER)?;
                let id_hstr = HSTRING::from(wpd_device_id);
                device.Open(PCWSTR(id_hstr.as_ptr()), &client_info)?;
                Ok(Self { device, _com_guard: com_guard })
            }
        }

        // Resolves a slash-delimited path to a WPD object ID string.
        // Traversal starts at the first storage child of the root "DEVICE" object.
        fn path_to_object_id(&self, path: &str) -> Result<HSTRING> {
            let components = super::split_path_components(path);
            unsafe {
                let content = self.device.Content()?;
                let props = content.Properties()?;

                // Find storage root: first child enumerated under "DEVICE".
                let device_hstr = HSTRING::from("DEVICE");
                let root_enum = content.EnumObjects(0, PCWSTR(device_hstr.as_ptr()), None)?;
                let mut storage_buf = [PWSTR::null()];
                let mut fetched = 0u32;
                let _ = root_enum.Next(&mut storage_buf, &mut fetched);
                if fetched == 0 || storage_buf[0].is_null() {
                    return Err(anyhow::anyhow!("WPD: no storage objects found under DEVICE"));
                }
                let storage_id = storage_buf[0].to_string()?;
                CoTaskMemFree(Some(storage_buf[0].0 as *const _));

                if components.is_empty() {
                    return Ok(HSTRING::from(&storage_id));
                }

                let mut current_id = storage_id;

                for component in &components {
                    let parent_hstr = HSTRING::from(&current_id);
                    let child_enum =
                        content.EnumObjects(0, PCWSTR(parent_hstr.as_ptr()), None)?;
                    let mut found: Option<String> = None;

                    // Fetch children in batches, matching by WPD_OBJECT_ORIGINAL_FILE_NAME.
                    let mut batch: Vec<PWSTR> = vec![PWSTR::null(); 64];
                    'outer: loop {
                        let mut fetched = 0u32;
                        let _ = child_enum.Next(
                            batch.as_mut_slice(),
                            &mut fetched,
                        );
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
                            if let Ok(values) =
                                props.GetValues(PCWSTR(obj_hstr.as_ptr()), None)
                            {
                                if let Ok(name_pwstr) =
                                    values.GetStringValue(&WPD_OBJECT_ORIGINAL_FILE_NAME)
                                {
                                    if !name_pwstr.is_null() {
                                        let name =
                                            name_pwstr.to_string().unwrap_or_default();
                                        CoTaskMemFree(Some(name_pwstr.0 as *const _));
                                        if name.eq_ignore_ascii_case(component) {
                                            found = Some(obj_id);
                                            break 'outer;
                                        }
                                    }
                                }
                            }
                        }
                    }

                    current_id = found.ok_or_else(|| {
                        anyhow::anyhow!("WPD: path component '{}' not found", component)
                    })?;
                }

                Ok(HSTRING::from(&current_id))
            }
        }
    }

    impl MtpHandle for WpdHandle {
        fn read_file(&self, path: &str) -> Result<Vec<u8>> {
            let obj_id = self.path_to_object_id(path)?;
            unsafe {
                let content = self.device.Content()?;
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
            if manager.GetDevices(std::ptr::null_mut(), &mut count).is_err() || count == 0 {
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
                        String::from_utf16_lossy(
                            &name_buf[..name_len.saturating_sub(1) as usize],
                        )
                    } else {
                        id_str.clone()
                    };

                    Some(MtpDeviceInfo {
                        friendly_name: friendly,
                        inner: MtpDeviceInner::Wpd { wpd_device_id: id_str.clone() },
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
        std::env::temp_dir()
            .join(format!("jellyfinsync-mtp-{}-{}.tmp", std::process::id(), n))
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
        // libmtp is not thread-safe. The Mutex must be held for the full duration of every
        // FFI call — do NOT copy the raw pointer out and drop the guard before calling FFI.
        device: Arc<Mutex<*mut LIBMTP_MtpDevice_t>>,
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
                Ok(Self { device: Arc::new(Mutex::new(device)) })
            }
        }

        // Resolves a path to a libmtp object ID. Caller must already hold the device Mutex
        // and pass the raw device pointer — this avoids re-entrant locking.
        unsafe fn path_to_object_id_raw(
            dev: *mut LIBMTP_MtpDevice_t,
            path: &str,
        ) -> Result<u32> {
            let components = super::split_path_components(path);
            if components.is_empty() {
                return Ok(LIBMTP_FILES_AND_FOLDERS_ROOT);
            }
            let mut parent_id = LIBMTP_FILES_AND_FOLDERS_ROOT;
            for component in &components {
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
                    let fname =
                        std::ffi::CStr::from_ptr((*cur).filename).to_string_lossy();
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
    }

    impl MtpHandle for LibmtpHandle {
        fn read_file(&self, path: &str) -> Result<Vec<u8>> {
            let guard = self.device.lock().unwrap();
            let dev = *guard;
            let obj_id = unsafe { Self::path_to_object_id_raw(dev, path)? };
            let tmp = temp_path();
            let tmp_cstr =
                std::ffi::CString::new(tmp.to_string_lossy().as_bytes())?;
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
            let parent_id =
                unsafe { Self::path_to_object_id_raw(dev, &parent_path)? };
            let tmp = temp_path();
            std::fs::write(&tmp, data)?;
            let tmp_cstr =
                std::ffi::CString::new(tmp.to_string_lossy().as_bytes())?;
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
                    dev,
                    tmp_cstr.as_ptr(),
                    &mut file_meta,
                    std::ptr::null(),
                    std::ptr::null(),
                )
            };
            drop(guard);
            let _ = std::fs::remove_file(&tmp);
            if rc != LIBMTP_ERROR_NONE {
                return Err(anyhow::anyhow!("libmtp write_file failed: rc={}", rc));
            }
            Ok(())
        }

        fn delete_file(&self, path: &str) -> Result<()> {
            let guard = self.device.lock().unwrap();
            let dev = *guard;
            let obj_id = unsafe { Self::path_to_object_id_raw(dev, path)? };
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
                    let vendor = std::ffi::CStr::from_ptr(r.device_entry.vendor)
                        .to_string_lossy()
                        .into_owned();
                    let product = std::ffi::CStr::from_ptr(r.device_entry.product)
                        .to_string_lossy()
                        .into_owned();
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

    #[test]
    fn test_split_path_components_dotdot_filtered() {
        // Path traversal via ".." must be blocked.
        assert!(split_path_components("..").is_empty());
        assert_eq!(split_path_components("Music/../etc"), vec!["Music", "etc"]);
        assert_eq!(split_path_components("../secret"), vec!["secret"]);
    }
}
