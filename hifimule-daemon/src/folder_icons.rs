//! Mounted-folder adapters. Object transports never enter this module.
use anyhow::{Context, Result, bail, ensure};
use std::io::Write;
use std::path::{Component, Path, PathBuf};

pub(crate) fn safe_folder(root: &Path, relative: &Path) -> Result<PathBuf> {
    ensure!(
        !relative.as_os_str().is_empty(),
        "Root folder icons are not supported"
    );
    let mut full = root.to_path_buf();
    ensure!(
        !std::fs::symlink_metadata(&full)?.is_symlink(),
        "Device root is a symlink"
    );
    for component in relative.components() {
        let Component::Normal(name) = component else {
            bail!("Invalid icon folder")
        };
        ensure!(
            !name.to_string_lossy().contains([':', '\\']),
            "Invalid icon folder"
        );
        full.push(name);
        let metadata = std::fs::symlink_metadata(&full)?;
        ensure!(
            metadata.is_dir() && !metadata.is_symlink(),
            "Icon folder is not a real directory"
        );
    }
    ensure!(
        full.canonicalize()?.starts_with(root.canonicalize()?),
        "Icon folder escapes device"
    );
    Ok(full)
}

pub(crate) fn publish_new(path: &Path, data: &[u8]) -> Result<()> {
    let mut staged = tempfile::NamedTempFile::new_in(path.parent().context("Missing parent")?)?;
    staged.write_all(data)?;
    staged.as_file().sync_all()?;
    staged.persist_noclobber(path).map_err(|e| e.error)?;
    Ok(())
}

fn hash(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}
fn read_small(path: &Path) -> Result<Option<Vec<u8>>> {
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
        Ok(metadata) => {
            ensure!(
                metadata.is_file() && !metadata.is_symlink() && metadata.len() <= 8 * 1024 * 1024,
                "User icon file preserved"
            );
            Ok(Some(std::fs::read(path)?))
        }
    }
}

pub(crate) fn apply(
    root: &Path,
    relative: &str,
    kind: &str,
    jpeg: Option<&[u8]>,
    owned: Option<&str>,
) -> Result<Option<String>> {
    let folder = match safe_folder(root, Path::new(relative)) {
        Ok(folder) => folder,
        Err(error)
            if jpeg.is_none()
                && error
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
        {
            return Ok(None);
        }
        Err(error) => return Err(error),
    };
    match kind {
        "kde" => {
            let path = folder.join(".directory");
            let current = read_small(&path)?;
            match (jpeg, current) {
                (Some(_), Some(bytes)) => {
                    ensure!(
                        owned == Some(hash(&bytes).as_str()),
                        "Existing KDE customization preserved"
                    );
                    Ok(owned.map(str::to_owned))
                }
                (Some(_), None) => {
                    let bytes = b"[Desktop Entry]\nIcon=./folder.jpg\n";
                    publish_new(&path, bytes)?;
                    Ok(Some(hash(bytes)))
                }
                (None, Some(bytes)) => {
                    ensure!(
                        owned == Some(hash(&bytes).as_str()),
                        "Modified KDE customization preserved"
                    );
                    std::fs::remove_file(path)?;
                    Ok(None)
                }
                (None, None) => Ok(None),
            }
        }
        #[cfg(windows)]
        "windows" => windows(&folder, jpeg, owned),
        #[cfg(target_os = "macos")]
        "macos" => macos(&folder, jpeg, owned),
        #[cfg(target_os = "linux")]
        "gnome" => gnome(&folder, jpeg, owned),
        _ => bail!(
            "{kind} icons are pending: a compatible host and mass-storage connection are required"
        ),
    }
}

#[cfg(windows)]
fn windows(folder: &Path, jpeg: Option<&[u8]>, owned: Option<&str>) -> Result<Option<String>> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_READONLY,
        FILE_ATTRIBUTE_SYSTEM, GetFileAttributesW, INVALID_FILE_ATTRIBUTES, SetFileAttributesW,
    };
    #[derive(serde::Serialize, serde::Deserialize)]
    struct State {
        ini: String,
        ico: String,
        previous: u32,
    }
    fn attrs(path: &Path) -> Result<u32> {
        let name: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let value = unsafe { GetFileAttributesW(name.as_ptr()) };
        ensure!(
            value != INVALID_FILE_ATTRIBUTES,
            "Cannot inspect Windows icon attributes"
        );
        Ok(value)
    }
    fn set_attrs(path: &Path, value: u32) -> Result<()> {
        let name: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        ensure!(
            unsafe { SetFileAttributesW(name.as_ptr(), value) } != 0,
            "Cannot activate Windows folder icon"
        );
        unsafe {
            windows::Win32::UI::Shell::SHChangeNotify(
                windows::Win32::UI::Shell::SHCNE_UPDATEITEM,
                windows::Win32::UI::Shell::SHCNF_PATHW,
                Some(name.as_ptr().cast()),
                None,
            );
        }
        Ok(())
    }
    let ini = folder.join("desktop.ini");
    let ico = folder.join(".hifimule-folder.ico");
    let old: Option<State> = owned.map(serde_json::from_str).transpose()?;
    let ini_bytes = read_small(&ini)?;
    let ico_bytes = read_small(&ico)?;
    if let Some(old) = &old {
        ensure!(
            ini_bytes.as_ref().is_none_or(|v| hash(v) == old.ini)
                && ico_bytes.as_ref().is_none_or(|v| hash(v) == old.ico),
            "Modified Windows icon preserved"
        );
        if ini_bytes.is_some() && ico_bytes.is_some() {
            ensure!(
                attrs(folder)? & FILE_ATTRIBUTE_READONLY != 0,
                "Modified Windows folder attributes preserved"
            );
            if jpeg.is_some() {
                return Ok(owned.map(str::to_owned));
            }
        }
    } else {
        ensure!(
            ini_bytes.is_none() && ico_bytes.is_none(),
            "Existing Windows customization preserved"
        );
    }
    if jpeg.is_none() {
        if let Some(old) = old {
            if ini_bytes.is_some() {
                set_attrs(&ini, FILE_ATTRIBUTE_NORMAL)?;
                std::fs::remove_file(&ini)?;
            }
            if ico_bytes.is_some() {
                set_attrs(&ico, FILE_ATTRIBUTE_NORMAL)?;
                std::fs::remove_file(&ico)?;
            }
            let value = (attrs(folder)? & !FILE_ATTRIBUTE_READONLY)
                | (old.previous & FILE_ATTRIBUTE_READONLY);
            set_attrs(folder, value)?;
        }
        return Ok(None);
    }
    let previous = old.as_ref().map_or(attrs(folder)?, |v| v.previous);
    let mut encoded = std::io::Cursor::new(Vec::new());
    image::load_from_memory(jpeg.unwrap())?
        .thumbnail(256, 256)
        .write_to(&mut encoded, image::ImageFormat::Ico)?;
    let ini_data: Vec<u8> = std::iter::once(0xfeff)
        .chain(
            "[.ShellClassInfo]\r\nIconFile=.hifimule-folder.ico\r\nIconIndex=0\r\n".encode_utf16(),
        )
        .flat_map(u16::to_le_bytes)
        .collect();
    let created_ico = ico_bytes.is_none();
    let created_ini = ini_bytes.is_none();
    let result: Result<()> = (|| {
        if created_ico {
            publish_new(&ico, encoded.get_ref())?;
        }
        if created_ini {
            publish_new(&ini, &ini_data)?;
        }
        set_attrs(
            &ini,
            attrs(&ini)? | FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM,
        )?;
        set_attrs(&ico, attrs(&ico)? | FILE_ATTRIBUTE_HIDDEN)?;
        set_attrs(folder, attrs(folder)? | FILE_ATTRIBUTE_READONLY)?;
        Ok(())
    })();
    if let Err(error) = result {
        for (path, created, expected) in [
            (&ini, created_ini, hash(&ini_data)),
            (&ico, created_ico, hash(encoded.get_ref())),
        ] {
            if created
                && read_small(path)
                    .ok()
                    .flatten()
                    .is_some_and(|v| hash(&v) == expected)
            {
                let _ = set_attrs(path, FILE_ATTRIBUTE_NORMAL);
                let _ = std::fs::remove_file(path);
            }
        }
        if let Ok(current) = attrs(folder) {
            let _ = set_attrs(
                folder,
                (current & !FILE_ATTRIBUTE_READONLY) | (previous & FILE_ATTRIBUTE_READONLY),
            );
        }
        return Err(error);
    }
    Ok(Some(serde_json::to_string(&State {
        ini: hash(&ini_data),
        ico: hash(encoded.get_ref()),
        previous,
    })?))
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct GnomeOwnership {
    hosts: std::collections::BTreeMap<String, String>,
}
pub(crate) fn merge_gnome_ownership(first: &str, second: &str) -> Result<String> {
    let mut first: GnomeOwnership = serde_json::from_str(first)?;
    let second: GnomeOwnership = serde_json::from_str(second)?;
    for (scope, value) in second.hosts {
        first.hosts.entry(scope).or_insert(value);
    }
    Ok(serde_json::to_string(&first)?)
}
#[cfg(any(target_os = "linux", test))]
fn gnome_local_owner(token: Option<&str>, scope: &str) -> Result<Option<String>> {
    let ownership: GnomeOwnership = token
        .map(serde_json::from_str)
        .transpose()?
        .unwrap_or_default();
    Ok(ownership.hosts.get(scope).cloned())
}
#[cfg(any(target_os = "linux", test))]
fn gnome_record(token: Option<&str>, scope: &str, value: Option<&str>) -> Result<Option<String>> {
    let mut ownership: GnomeOwnership = token
        .map(serde_json::from_str)
        .transpose()?
        .unwrap_or_default();
    if let Some(value) = value {
        ownership.hosts.insert(scope.into(), value.into());
    } else {
        ownership.hosts.remove(scope);
    }
    if ownership.hosts.is_empty() {
        Ok(None)
    } else {
        Ok(Some(serde_json::to_string(&ownership)?))
    }
}
#[cfg(any(target_os = "linux", test))]
fn check_gnome_owner(
    token: Option<&str>,
    scope: &str,
    current: Option<&str>,
    themed: bool,
) -> Result<()> {
    ensure!(!themed, "Existing GNOME themed customization preserved");
    let owner = gnome_local_owner(token, scope)?;
    ensure!(
        current.is_none() || current == owner.as_deref(),
        "Existing GNOME customization preserved"
    );
    Ok(())
}
#[cfg(target_os = "linux")]
fn gnome_scope() -> Result<String> {
    let machine =
        machine_uid::get().map_err(|_| anyhow::anyhow!("GNOME ownership scope unavailable"))?;
    Ok(hash(
        format!("{machine}:{}", unsafe { libc::geteuid() }).as_bytes(),
    ))
}

#[cfg(target_os = "linux")]
fn gio_output(command: &mut std::process::Command) -> Result<std::process::Output> {
    use std::process::Stdio;
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .context("GNOME icon pending: gio is unavailable")?;
    let start = std::time::Instant::now();
    loop {
        if child.try_wait()?.is_some() {
            return Ok(child.wait_with_output()?);
        }
        if start.elapsed() > std::time::Duration::from_secs(3) {
            let _ = child.kill();
            let _ = child.wait();
            bail!("GNOME icon pending: GVfs timed out");
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

#[cfg(target_os = "linux")]
fn gnome(folder: &Path, jpeg: Option<&[u8]>, owned: Option<&str>) -> Result<Option<String>> {
    use std::process::Command;
    let scope = gnome_scope()?;
    let local = gnome_local_owner(owned, &scope)?;
    if jpeg.is_none() && local.is_none() {
        return Ok(owned.map(str::to_owned));
    }
    let result = gio_output(
        Command::new("gio")
            .env("LC_ALL", "C")
            .args([
                "info",
                "-a",
                "metadata::custom-icon,metadata::custom-icon-name",
                "--",
            ])
            .arg(folder),
    )?;
    ensure!(
        result.status.success(),
        "GNOME icon pending: GIO metadata is unavailable"
    );
    let text = String::from_utf8_lossy(&result.stdout);
    let themed = text
        .lines()
        .any(|line| line.trim().starts_with("metadata::custom-icon-name:"));
    let current = text.lines().find_map(|line| {
        line.trim()
            .strip_prefix("metadata::custom-icon:")
            .map(str::trim)
    });
    check_gnome_owner(owned, &scope, current, themed)?;
    if jpeg.is_some() && current == Some("folder.jpg") {
        return gnome_record(owned, &scope, Some("folder.jpg"));
    }
    if jpeg.is_none() && current.is_none() {
        return gnome_record(owned, &scope, None);
    }
    let mut command = Command::new("gio");
    command
        .env("LC_ALL", "C")
        .args([
            "set",
            "-t",
            if jpeg.is_some() { "string" } else { "unset" },
            "--",
        ])
        .arg(folder)
        .arg("metadata::custom-icon");
    if jpeg.is_some() {
        command.arg("folder.jpg");
    }
    ensure!(
        gio_output(&mut command)?.status.success(),
        "GNOME icon pending: GVfs metadata cannot be updated"
    );
    gnome_record(owned, &scope, jpeg.map(|_| "folder.jpg"))
}

#[cfg(target_os = "macos")]
fn macos(folder: &Path, jpeg: Option<&[u8]>, owned: Option<&str>) -> Result<Option<String>> {
    use objc2::AnyThread;
    use objc2_app_kit::{NSImage, NSWorkspace, NSWorkspaceIconCreationOptions};
    use objc2_foundation::{NSData, NSString};
    use std::os::unix::ffi::OsStrExt;
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    fn xattr(path: &Path, name: &std::ffi::CStr) -> Result<Vec<u8>> {
        let path = std::ffi::CString::new(path.as_os_str().as_bytes())?;
        let size = unsafe {
            libc::getxattr(
                path.as_ptr(),
                name.as_ptr(),
                std::ptr::null_mut(),
                0,
                0,
                libc::XATTR_NOFOLLOW,
            )
        };
        if size < 0 {
            let error = std::io::Error::last_os_error();
            if matches!(error.raw_os_error(), Some(libc::ENOATTR | libc::ENOENT)) {
                return Ok(Vec::new());
            }
            return Err(error.into());
        }
        ensure!(size <= 8 * 1024 * 1024, "Custom icon resource is too large");
        let mut bytes = vec![0; size as usize];
        let got = unsafe {
            libc::getxattr(
                path.as_ptr(),
                name.as_ptr(),
                bytes.as_mut_ptr().cast(),
                bytes.len(),
                0,
                libc::XATTR_NOFOLLOW,
            )
        };
        ensure!(got >= 0, "Cannot inspect Finder icon");
        bytes.truncate(got as usize);
        Ok(bytes)
    }
    fn current(folder: &Path) -> Result<Option<String>> {
        let info = xattr(folder, c"com.apple.FinderInfo")?;
        let has_icon = info.get(8).is_some_and(|byte| byte & 0x04 != 0);
        let icon = folder.join("Icon\r");
        let bytes = read_small(&icon)?;
        if !has_icon && bytes.is_none() {
            return Ok(None);
        }
        let mut evidence = vec![u8::from(has_icon)];
        evidence.extend(bytes.unwrap_or_default());
        evidence.extend(xattr(&icon, c"com.apple.ResourceFork")?);
        Ok(Some(hash(&evidence)))
    }
    let _guard = LOCK
        .lock()
        .map_err(|_| anyhow::anyhow!("Finder icon lock unavailable"))?;
    let before = current(folder)?;
    ensure!(
        before.is_none() || before.as_deref() == owned,
        "Existing Finder customization preserved"
    );
    if before.is_some() && jpeg.is_some() {
        return Ok(before);
    }
    if before.is_none() && jpeg.is_none() {
        return Ok(None);
    }
    objc2::rc::autoreleasepool(|_| {
        let image = jpeg
            .map(|bytes| {
                let data = NSData::with_bytes(bytes);
                NSImage::initWithData(NSImage::alloc(), &data).context("Cannot decode Finder image")
            })
            .transpose()?;
        let name = NSString::from_str(folder.to_str().context("Folder path is not UTF-8")?);
        ensure!(
            NSWorkspace::sharedWorkspace().setIcon_forFile_options(
                image.as_deref(),
                &name,
                NSWorkspaceIconCreationOptions::empty()
            ),
            "Finder rejected folder icon"
        );
        if jpeg.is_some() {
            current(folder)?
                .context("Finder icon could not be verified")
                .map(Some)
        } else {
            // Finder can leave its empty carrier after clearing the custom flag.
            let carrier = folder.join("Icon\r");
            if read_small(&carrier)?.is_some_and(|bytes| bytes.is_empty())
                && xattr(&carrier, c"com.apple.ResourceFork")?.is_empty()
            {
                std::fs::remove_file(carrier)?;
            }
            ensure!(
                current(folder)?.is_none(),
                "Finder icon cleanup could not be verified"
            );
            Ok(None)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn kde_ownership_and_modified_user_files() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("Music");
        std::fs::create_dir(&folder).unwrap();
        let owned = apply(root.path(), "Music", "kde", Some(b"image"), None)
            .unwrap()
            .unwrap();
        assert!(apply(root.path(), "Music", "kde", Some(b"image"), None).is_err());
        assert_eq!(
            apply(root.path(), "Music", "kde", Some(b"image"), Some(&owned)).unwrap(),
            Some(owned.clone())
        );
        std::fs::write(folder.join(".directory"), "user icon").unwrap();
        assert!(apply(root.path(), "Music", "kde", None, Some(&owned)).is_err());
        assert!(folder.join(".directory").exists());
        assert!(apply(root.path(), "../Music", "kde", Some(b"image"), None).is_err());
        assert!(apply(root.path(), "", "kde", Some(b"image"), None).is_err());
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn finder_apply_verify_and_retire() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("Artist")).unwrap();
        let mut image = std::io::Cursor::new(Vec::new());
        image::RgbImage::from_pixel(32, 32, image::Rgb([180, 70, 20]))
            .write_to(&mut image, image::ImageFormat::Jpeg)
            .unwrap();
        let token = apply(root.path(), "Artist", "macos", Some(image.get_ref()), None)
            .unwrap()
            .unwrap();
        assert!(apply(root.path(), "Artist", "macos", Some(image.get_ref()), None).is_err());
        assert_eq!(
            apply(
                root.path(),
                "Artist",
                "macos",
                Some(image.get_ref()),
                Some(&token)
            )
            .unwrap(),
            Some(token.clone())
        );
        assert!(
            apply(root.path(), "Artist", "macos", None, Some(&token))
                .unwrap()
                .is_none()
        );
        assert!(
            std::fs::read_dir(root.path().join("Artist"))
                .unwrap()
                .next()
                .is_none()
        );
    }
    #[test]
    fn gnome_ownership_is_scoped_to_host_and_user_and_preserves_foreign_evidence() {
        let a = gnome_record(None, "host-a-user-1", Some("folder.jpg"))
            .unwrap()
            .unwrap();
        assert!(check_gnome_owner(Some(&a), "host-b-user-1", Some("folder.jpg"), false).is_err());
        assert!(check_gnome_owner(Some(&a), "host-a-user-2", Some("folder.jpg"), false).is_err());
        assert!(check_gnome_owner(Some(&a), "host-a-user-1", Some("folder.jpg"), false).is_ok());
        let b = gnome_record(None, "host-b-user-1", Some("folder.jpg"))
            .unwrap()
            .unwrap();
        let merged = merge_gnome_ownership(&a, &b).unwrap();
        assert!(
            gnome_local_owner(Some(&merged), "host-a-user-1")
                .unwrap()
                .is_some()
        );
        assert!(
            gnome_local_owner(Some(&merged), "host-b-user-1")
                .unwrap()
                .is_some()
        );
        let both = gnome_record(Some(&a), "host-b-user-1", Some("folder.jpg"))
            .unwrap()
            .unwrap();
        let pending = gnome_record(Some(&both), "host-b-user-1", None)
            .unwrap()
            .unwrap();
        assert_eq!(
            gnome_local_owner(Some(&pending), "host-a-user-1")
                .unwrap()
                .as_deref(),
            Some("folder.jpg")
        );
        assert!(
            gnome_local_owner(Some(&pending), "host-b-user-1")
                .unwrap()
                .is_none()
        );
        assert!(
            gnome_record(Some(&pending), "host-a-user-1", None)
                .unwrap()
                .is_none()
        );
    }
    #[cfg(unix)]
    #[test]
    fn native_adapter_rejects_symlink_folders() {
        let root = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(other.path(), root.path().join("escape")).unwrap();
        assert!(apply(root.path(), "escape", "kde", Some(b"image"), None).is_err());
    }
}
