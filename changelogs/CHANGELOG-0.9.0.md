# HifiMule 0.9.0 - 2026-05-30

## Highlights

- Subsonic and Navidrome devices can now auto-sync when connected, matching the Jellyfin auto-sync workflow.
- Auto-fill sync now works for Subsonic/Navidrome, including manual sync previews and connect-time auto-sync.
- Sync is safer around empty baskets, concurrent runs, large libraries, transient server failures, and stale device files.
- Server credentials are now stored in a local machine-bound encrypted vault instead of the OS keyring integration.
- Lossless-friendly DAP sync is faster because HifiMule avoids unnecessary transcoding when the device can play the original file.

## Added

- Added provider-based auto-sync for Subsonic/Navidrome servers. When a configured device is connected, HifiMule now detects the active non-Jellyfin provider, loads the stored server secret, resolves basket items through the provider API, calculates the delta, and runs the provider sync path.
- Added provider-based auto-fill for Subsonic/Navidrome. HifiMule can fill remaining device capacity from favorites, frequently played tracks, and recently played tracks when those server capabilities are available.
- Added support for Subsonic/Navidrome auto-fill in interactive sync preview. The UI path no longer fails with "Auto-fill sync is not available for Subsonic servers yet."
- Added genre track count and size calculation, so genres added to the basket can show more useful sync estimates.
- Added sync speed logging for download and write phases, including fractional timing so very fast operations do not show misleading `0.0MB/s` values on Windows.
- Added richer preparation and sync logging to make long or large syncs easier to understand from daemon logs.
- Added a machine-bound encrypted credential vault using `machine-uid`, `blake3`, and `chacha20poly1305`.

## Changed

- Replaced OS keyring-backed credential storage with HifiMule's encrypted `secrets.enc` vault in the app data directory.
- Improved sync preparation for large libraries and large device volumes.
- Improved manifest repair behavior and presentation, including a clearer repair modal flow.
- Improved the lossless-friendly DAP profile path so compatible originals are passed through instead of being transcoded unnecessarily.
- Improved sync progress and speed measurement logs across both legacy Jellyfin and provider-based sync paths.
- Updated planning and implementation artifacts for the credential-vault change.

## Fixed

- Fixed empty baskets blocking cleanup syncs. If a device still has HifiMule-managed synced files, the sync button stays available so those files can be removed safely.
- Fixed connect-time auto-sync skipping cleanup when the basket had been emptied.
- Fixed a safety edge case where failed basket resolution could otherwise be confused with an intentional empty basket cleanup.
- Fixed auto-fill sync aborting immediately on transient Jellyfin server failures such as HTTP 5xx responses or network timeouts by retrying page fetches before failing.
- Fixed concurrent sync attempts by adding guards so a second sync cannot start while another sync is already running.
- Fixed misleading Windows speed logs for sub-millisecond transfer phases.
- Fixed stale manifest handling for devices needing repair.

## Security

- Credentials are now encrypted in a hardware-bound local vault file instead of relying on platform keyring services.
- Vault writes are atomic, use random nonces, restrict Unix file permissions, validate payload size, and avoid overwriting existing secrets when the vault cannot be read.
- Clearing credentials now reports file deletion errors instead of silently ignoring them.

## Notes

- The new credential vault is bound to the current machine identity. Moving the app data directory to another machine, reinstalling an OS, or changing virtual-machine hardware identity may make existing stored credentials unreadable; logging in again recreates the vault for the new machine.
