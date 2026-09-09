# HifiMule 0.14.0

Release date: 2026-09-09

## Highlights

- **Jellyfin 12 compatibility**: HifiMule can connect to Jellyfin 12 installations with legacy authentication disabled, without requiring a server-side compatibility setting.
- **Reliable authenticated operations**: Browsing, Auto-Fill, playlists, playback reporting, and other Jellyfin requests now use the supported authentication scheme after login or stored-token reconnect.
- **Compatible downloads**: Direct-play and transcoded download URLs now use Jellyfin's supported `ApiKey` parameter while preserving existing query parameters and URL fragments.

---

## Changed

- Jellyfin API requests now use the supported `Authorization: MediaBrowser` token scheme instead of the deprecated `X-Emby-Token` header.
- Jellyfin download URLs now normalize legacy `api_key` authentication to the supported `ApiKey` spelling without duplicating credentials.
- The same authentication protocol is used across the supported Jellyfin 10.8–12.0 range, with no server-version branching.

---

## Fixed

- Automatic server detection no longer reports `Unknown server type` after a successful password login to Jellyfin 12 when legacy authentication is disabled.
- Reconnecting with a stored Jellyfin token now uses the supported authentication header.
- Direct-play downloads now receive supported URL authentication when Jellyfin does not include a token in the source URL.
- Transcoding URLs retain unrelated query parameters and fragments while empty or legacy authentication values are replaced safely.

---

## Internal

- Jellyfin token-header construction is centralized with validation, encoding, sensitive-value handling, and errors that do not expose credentials.
- Stream diagnostics no longer print playback responses or authenticated transcoding URLs.
- Regression coverage now exercises fresh login, stored-token reconnect, authenticated reads and writes, Auto-Fill, scrobbling, direct and transcoded downloads, credential redaction, and representative Jellyfin 10.8–12.0 contracts.

