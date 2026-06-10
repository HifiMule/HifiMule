# Changelog

## v0.11.0 - 2026-06-10

HifiMule now works with more than one media server. A new **Server Hub** lets you set up several servers — any mix of Jellyfin, Subsonic, and Navidrome — switch between them with a click, and add or remove servers without reconfiguring the app or losing what's in your basket. Your basket can even hold music from several servers at once: items are grouped by server, and a sync sends each one back to the server it came from. Items from a server that isn't currently active simply show as locked until you switch back to it.

You can also give each server a **name and icon of your choice**, so you can tell them apart at a glance in the hub, the switcher, and your basket — instead of squinting at server types and URLs.

Behind the scenes, servers now have a stable identity that travels with your devices. A player you synced on one computer is recognized on another, and removing then re-adding the same server no longer forces a full resync.

This release also fixes a bug where renaming a Jellyfin playlist could fail on newer servers.

## v0.10.0 - 2026-06-08

You can now edit playlists right from HifiMule. A new playlist curation view lets you create a playlist, add or remove tracks, rename or delete it, and drag tracks into the order you want — with a live count of how many tracks it holds, how long it plays, and how much space it takes. You can also turn whatever is in your basket into a new playlist (or add it to an existing one) in a couple of clicks. Playlist editing works whether you're connected to Jellyfin or to a Subsonic/Navidrome server.

Browsing your library got better too. A new **Tracks** view shows your songs in a scrollable list that loads more as you go, with an A–Z strip for jumping around big collections. The grid/table view switch now works everywhere — every browse mode and every level you drill into.

This release also adds a **German** translation (joining English, French, and Spanish), fixes Jellyfin so artists and their tracks show up correctly under the album artist, and includes a round of visual polish.

## v0.9.1 - 2026-06-05

You can now cancel a running sync — a **Cancel Sync** button appears while a sync is in progress. Cancellation stops cleanly after the current file transfer and keeps the device marked for resume, so the next time you connect it picks up where it left off.

The sync preview now tells you *why* each file is being added or removed. Instead of a raw file count, you'll see a breakdown: "3 files — new selection, 1 file — bitrate increased, 2 files — device file missing". This makes it easy to understand large syncs at a glance and spot unexpected changes.

Labels across the basket and repair flow have been rewritten in plain language. "Manifest Dirty" is now "Repair needed", "Open Manifest Repair" is "Open sync repair", and "Sync Proposed" is "Basket changed — ready to sync". Clearing the basket now asks for confirmation. The repair modal is also fully translated into French and Spanish.

A bug where Navidrome/Subsonic devices incorrectly showed "sync not needed" even when files were pending is fixed.

## v0.9.0 - 2026-05-30

Subsonic and Navidrome users get a much better sync experience: devices can now auto-sync when connected, and auto-fill works in both sync previews and automatic syncs. Sync is also safer and more resilient: empty baskets can clean old HifiMule-managed files from a device, concurrent syncs are blocked, large-library preparation is steadier, and temporary server errors are retried instead of immediately failing the whole sync.

This release also improves lossless-friendly DAP syncing by avoiding unnecessary transcoding when originals are already compatible, adds clearer sync speed/preparation logging, improves genre size/count estimates, and stores server credentials in a local machine-bound encrypted vault.

## v0.8.4 - 2026-05-29

Fixed RPC error when synching a large playlist or genre.

## v0.8.3 - 2026-05-28

Synced filenames from Navidrome, Subsonic, and Jellyfin (without transcoding) now carry the correct track number prefix — `03 - Title.flac` instead of the previous `00 - Title.flac` for every track. Albums, playlists, and other collections added to the basket now show their approximate track count and total file size; if that data isn't already loaded, HifiMule fetches it from the server the moment you add the item.

## v0.8.2 - 2026-05-26

HifiMule now tracks the bitrate of every file it writes to your device and automatically re-downloads tracks when a higher-quality version appears on your server. If you manually delete a synced file from the device, the next sync preview detects the gap and adds it back to the download queue. A new "Force Sync" option in the sync button dropdown lets you wipe and re-download everything in one click. M4A/AAC tracks from Jellyfin now have their bitrate recorded correctly, so they benefit from the quality-upgrade check too. On macOS, files with accented or non-Latin characters in their name are no longer incorrectly treated as missing during the sync preview. The A–Z letter navigation bar on the Albums tab no longer disappears after switching to another tab and coming back, and the "Load More" button now works correctly when browsing by letter — you can page through large filtered results without losing the active letter.

## v0.8.1 - 2026-05-24

Added support for multilanguage with french and spanish translation.

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
