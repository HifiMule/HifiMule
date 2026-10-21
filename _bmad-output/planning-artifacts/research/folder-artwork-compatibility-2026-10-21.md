# Folder artwork compatibility

Research date: 2026-10-21. Scope: device music folders viewed from Windows, macOS, and Linux. Documentation/source inspection only; physical-device and file-manager validation remains required.

## Findings

An artwork file inside a directory, a preview of that directory's contents, and an explicit custom directory icon are separate features. There is no universal folder-icon file understood by all supported desktops. Keep `folder.jpg` as the portable artwork and make native integrations explicit options.

| Environment | Mechanism | Portability and constraints |
| --- | --- | --- |
| Windows Explorer | Unicode `desktop.ini`, `[.ShellClassInfo]`, relative `IconFile=.hifimule-folder.ico`, `IconIndex=0`; folder read-only/system attribute and hidden/system INI | Needs an ICO generated from the same image. Activate attributes on a Windows-mounted filesystem. Relative references survive drive-letter changes. JPEG alone is not a dependable explicit-icon contract. |
| macOS Finder | `NSWorkspace.setIcon(_:forFile:options:)` on an existing writable folder | macOS-only native operation; do not assume an ICNS or JPEG sitting in the folder changes its icon. Filesystem metadata/resource-fork behavior requires FAT32/exFAT testing. |
| KDE Dolphin | `.directory` containing `[Desktop Entry]` and `Icon=./folder.jpg` | KDE source explicitly resolves `./` relative to the directory. These two regular files can travel with the device. Other file managers need not honor them. |
| GNOME Files / Nautilus | GIO `metadata::custom-icon` pointing at the image | Nautilus supports a relative image reference for a directory. GVfs metadata itself lives in the user's host database, so the customization does not travel to another computer. Requires a working user GVfs session. |
| Xfce Thunar | Its own `thunar-custom-icon-name` metadata setting | Current Thunar source has a different setting from Nautilus. Do not advertise generic “Linux icons” based on `.directory` or the GNOME key alone. |
| MTP devices | Copy ordinary `folder.jpg` via device transport | MTP folder objects are not mounted host filesystem directories. Native Finder/Explorer attribute APIs do not apply. Player and host browser display remain device-dependent. |

### Windows evidence

[Microsoft's Desktop.ini documentation](https://learn.microsoft.com/en-us/windows/win32/shell/how-to-customize-folders-with-desktop-ini) specifies folder activation attributes, Unicode encoding, hidden/system INI attributes, and relative icon references. It prefers ICO for `IconFile`. [SHGetSetFolderCustomSettings](https://learn.microsoft.com/en-us/windows/win32/api/shlobj_core/nf-shlobj_core-shgetsetfoldercustomsettings) is a native reader/writer for these settings, with its own compatibility caveat and path limit; direct INI handling should preserve unrelated settings.

[Microsoft announced folder-content previews in Windows 11 build 22557](https://blogs.windows.com/windows-insider/2022/02/16/announcing-windows-11-insider-preview-build-22557/). That announcement does not guarantee that `folder.jpg` will be selected, rendered full-size, or displayed in every Explorer view. Therefore use the documented explicit icon mechanism for the Windows option and validate on a shipping Windows 11 build. Do not infer permanent removal from reports about the original Windows 11 release.

### macOS evidence

[Apple's API documentation](https://developer.apple.com/documentation/appkit/nsworkspace/seticon(_:forfile:options:)?changes=l_2) accepts an arbitrary NSImage, returns success/failure, and requires an existing writable target. Calls must be serialized even though they may originate on different threads. Apple documents generated 512×512 icon representations. Thus the requested 500×500 cap should apply to exported/download-derived JPEG artwork; the native system can generate a 512-pixel representation. Surface this distinction in the spec approval.

[Apple's Finder instructions](https://support.apple.com/en-euro/guide/mac-help/mchlp2313/27/mac/27) confirm custom image icons. Use the native API rather than manually synthesizing `Icon\r`, FinderInfo, AppleDouble, or `.DS_Store`. Those implementation details, preservation of existing custom icons, and removable-volume behavior need native tests. No claim is made that macOS metadata will survive arbitrary non-Apple copying tools.

### Linux evidence

[KDE KIO source, iconFromDirectoryFile](https://raw.githubusercontent.com/KDE/kio/master/src/core/kfileitem.cpp) reads `.directory` with KDesktopFile and resolves icon paths starting with `./` against that folder. Its read policy also distinguishes slow/remote locations. [Dolphin's preview settings](https://docs.kde.org/trunk_kf6/en/dolphin/dolphin/configuring-dolphin.html) separately govern content previews; a preview is not a custom-icon guarantee.

[Nautilus 50 source](https://raw.githubusercontent.com/GNOME/nautilus/gnome-50/src/nautilus-file.c), `get_custom_icon_metadata_uri` / `get_custom_icon`, supports directory-relative icon metadata. [GNOME's GVfs architecture documentation](https://wiki.gnome.org/Projects/gvfs/doc#Metadata) locates metadata databases under `~/.local/share/gvfs-metadata` and explains device/mount association. A relative `folder.jpg` reference avoids mount-path coupling, but does not make that database portable. Missing GVfs support must be a nonfatal unsupported integration result.

[Current Thunar source](https://raw.githubusercontent.com/xfce-mirror/thunar/master/thunar/thunar-file.c), `thunar_file_set_custom_icon`, uses `thunar-custom-icon-name`; its icon getter accepts an absolute local path or a themed name. That path must be reconsidered after remount. This merits its own adapter and test, rather than silently treating GNOME behavior as universal. Nemo/Caja and other managers were not verified in this research.

## Proposed product behavior

- A per-device **Add thumbnail** switch, initially off. When enabled, export `folder.jpg`; show independent optional additions for **Windows icon**, **macOS Finder icon**, **KDE (.directory)**, and **GNOME Files icon**. State current-host/transport requirements beside unavailable integrations. Thunar remains outside the initial supported list.
- JPEG is the shared base for all selected additions. Native icons reuse the same bytes; they never trigger an additional server artwork request. Windows ICO can use representations up to 256×256; Finder generates its own representation as described above.
- Generate KDE sidecars on mounted mass-storage devices regardless of host OS. Apply Windows attributes on Windows MSC connections, Finder icons on macOS MSC connections, and GNOME metadata on Linux MSC connections. Store selections portably; an unavailable adapter stays pending until a supported host performs a sync. Ordinary sidecar preparation on another host must not be reported as native activation success.
- Fetch at most once per actual destination folder in one sync. Persist successful ownership/source records and reuse existing exported artwork across later syncs. Retry failed/missing artwork at most once per folder on a later sync. Backfill existing synced folders when the feature is enabled, including syncs with no new audio.
- Use actual sanitized destination paths. Album folders get album artwork, artist folders get artist artwork. Resolve artwork via provider identities, never display-name searches. Keep a stable prior winner when different sources collide into the same destination; otherwise select deterministically.
- Preserve user files/custom icons. A collision skips the affected output and produces a concise warning. Disabling the switch or an addition stops writes and leaves existing output. When the last managed track leaves a folder, retire only unchanged, explicitly owned outputs and owned native state before empty-directory pruning.

## HifiMule implementation constraints

The current provider boundary already exposes `MediaProvider::fetch_cover_art`, but Jellyfin authentication needs verification: the UI uses a different authenticated client path. Carry artwork and artist IDs through sync planning; they are currently discarded. Scope lookups to the captured source server and device.

`DeviceIO` covers MSC/MTP file publication, not custom folder metadata. Add native capabilities at that boundary with an unsupported default; implement mounted-path operations inside MSC, never by treating an MTP identity as a path. PNG-only `image` features currently require JPEG/ICO additions. Bound compressed response bytes and decoded allocation, preserve aspect ratio, and do not upscale.

Track artwork ownership separately from tracks, preserve records through compact manifest encoding and polling, include backfill in planning, and remove owned sidecars before pruning obsolete folders. MTP JPEG object classification also needs review. Detailed code locations belong in the feature spec after the compatibility research.

## Required compatibility checks

Windows 11 Explorer with thumbnails enabled/disabled; macOS Finder on APFS and FAT32/exFAT; KDE Dolphin and GNOME Files on removable mounts; reconnect with a different mount path; cross-host reconnection; existing user icons/INI/directory files; MTP transfer on supported backends. Test JPEG dimensions/encoding, one fetch for many tracks, zero fetch on unchanged repeat sync, no audio-delta backfill, missing/invalid artwork, cancellation, disk-full behavior, and owned cleanup. Documentation evidence does not replace those integration checks.
