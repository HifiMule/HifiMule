# Changelog

## v0.8.0 - 2026-05-24

Device setup is much more flexible now. You can edit an already-managed device, change its name, icon, transcoding profile, music folder, or playlist folder, and HifiMule will clearly flag when the next sync needs cleanup/resync work. Playlists can now be written to their own folder, so Rockbox-style devices can use `Music` for tracks and `Playlists` for `.m3u` files.

Device profiles also got smarter. Rockbox, Garmin, generic MP3 players, modern DAPs, Sony Walkman players, car USB sticks, and audiobook/podcast devices now have better built-in presets with recommended folder defaults. Folder changes are safer too: HifiMule only cleans up files it owns, previews large cleanup work, and asks before removing many managed files.

## v0.7.0 - 2026-05-24

Favorites now browse naturally as Artists -> Albums -> Tracks, so you can sync favorite artists, favorite albums, or only the favorite tracks inside an album without accidentally pulling in more music than intended. Navidrome/OpenSubsonic users get better library parity too: supported servers can now show Recently Added, Frequently Played, and Recently Played, with more consistent album quick navigation.

Sync is safer and less noisy in this release. Single-track sync from Subsonic/Navidrome libraries now resolves the selected song correctly, device-format rules are enforced before files are written, incompatible or unconfirmed transcoding is skipped with warnings instead of producing unplayable files, and USB cleanup no longer fails just because a managed file was already deleted.

## v0.6.0 — 2026-05-22

Eight browse modes are now available in the Library Browser: Artists, Albums, Playlists, Genres, Recently Added, Frequently Played, Recently Played, and Favorites. A compact tab bar at the top switches between them — only modes your server supports are shown. Genres can now be added to the sync basket as a single item; HifiMule resolves the full track list at sync time and removes duplicates automatically. Track cards in Frequently Played show the server play count, and Recently Played cards show the last-listened date. 

## v0.5.6 - 2026-05-20

Rockbox scrobbling works again with the current `.rockbox/playback.log` format. HifiMule now reads the newer Rockbox log location, keeps compatibility with older `.scrobbler.log` files, matches plays against the files HifiMule synced to the device, and reports completed listens to Jellyfin using playback-session events. This should mean fewer missed scrobbles, fewer incorrect track matches, and more reliable Jellyfin play counts after listening on a Rockbox device.

## v0.5.5 — 2026-05-15

Deletes now work correctly when syncing to Garmin smartwatches and other MTP devices — tracks and playlists removed from the basket are actually removed from the device. Android phones connected in charge-only (USB charging) mode no longer show a broken Initialize button; the app waits silently until the user switches to file-transfer mode. Connecting to devices with large music libraries (smartphones) is faster because the folder scan is now triggered on demand instead of upfront.

## v0.5.4 — 2026-05-13

HifiMule now syncs music to Garmin smartwatches (Forerunner, Fenix, Venu, Vivoactive). A bundled device profile selects the right audio format automatically — MP3 and AAC pass through directly, everything else is transcoded to MP3 320 kbps. A crash that could occur when connecting unrecognised MTP devices is fixed, and macOS notification delivery is now reliable. Release builds are code-signed, which removes the Gatekeeper prompt on macOS 13+.

## v0.5.1 — 2026-05-11

HifiMule now runs on macOS. The daemon starts automatically at login, MTP devices (phones, DAPs) connect and sync reliably via libmtp, and the app no longer shows a Dock icon. Read-only volumes such as mounted disk images are silently skipped and will no longer trigger the "unrecognized device" prompt.

## v0.4.2 — 2026-05-10

MTP device connection is more reliable on Windows. WPD `device.Open()` now retries on transient errors, and the init liveness probe for MTP no longer triggers a full recursive storage walk.

## v0.4.1 — 2026-05-10

Auto-sync now fires reliably when a device is connected, even if the app was never opened after pairing. Jellyfin playlists are correctly written to the device as `.m3u` files in both manual and auto-sync modes. The login screen shows the detected server type (Jellyfin / Subsonic) before you submit, and a logout button is now accessible directly from the main view.
