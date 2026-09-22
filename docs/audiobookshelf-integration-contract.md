# Audiobookshelf integration contract

## Record 1 — v2.36.1 (2026-09-21)

This is the mandatory discovery gate for Epic 17, not an enabled integration. Its controlled deployment used local authentication and a disposable test account. The server advertised local authentication, successful login returned an access token and refresh token, and authenticated library discovery returned distinct `book` and `podcast` roles. The pinned upstream source revision for tag `v2.36.1` is `e4569a4fdb85233d6c6c7bb4aa744f3cff9c7f56`.

No real server origin, account identifier, raw endpoint identifier, token, cookie, request-header value, authenticated URL, provider error body, title, or filesystem path is retained here or in fixtures.

### Evidence and vocabulary

Fixture schema version 1 is at `hifimule-daemon/tests/fixtures/audiobookshelf/2.36.1/manifest.json`. Fixtures are synthetic, redacted at authoring time, and use only local aliases. Future records are additive: never replace an observed record with a pass.

- **verified** — observed against the controlled v2.36.1 deployment.
- **unsupported** — observed unavailable; later stories must capability-gate it.
- **ambiguous** — observation or semantics insufficient for implementation.
- **blocked** — required controlled evidence cannot presently be collected.
- **untested** — not probed; never a pass.

### Verified transport and DTO observations

| Contract area | Observation | Safe later consequence |
| --- | --- | --- |
| Authentication | `POST /login` accepts local credentials; v2.26+ JWT access/refresh behavior was observed with `x-return-tokens`. Login and refresh responses carry `accessToken` and `refreshToken` inside the top-level `user` object. With controlled five-second access expiry, the protected request returned 401 and `POST /auth/refresh` restored protected access. Logout invalidated refresh but not an already-issued access token. Requests use an `Authorization: Bearer` header. | Daemon-only auth. Refresh once after an expiry 401, never in a loop. Never place a token in a URL, RPC, UI state, fixture, or log. |
| Libraries | `GET /api/libraries` returns immutable library IDs and `mediaType`; both `book` and `podcast` roles were observed. A restricted account returned only one `book` library and received 403 for the withheld Podcast library. | A selected library must retain its upstream ID and role. Books and Podcasts are independent future servers; inaccessible libraries are a non-retryable permission result. |
| Catalog pagination | `GET /api/libraries/{library}/items?page=0&limit=1` returned `page`, `limit`, `offset`, `total`, `results`; a far-final page returned an empty result list. This was observed for both roles. Two items with the same display title had distinct library-item identities. | Paginate by server response, stop on an empty final page, and keep role-specific mapping. Title is never identity. |
| Search | `GET /api/libraries/{library}/search?q=...` returned role-specific empty and nonempty result shapes: Books exposed `book`, `authors`, `narrators`, `series`, `genres`, `tags`; Podcasts exposed `podcast`, `episodes`, `genres`, `tags`. On the controlled v2.36.1 server, a query matching at least two items in each role returned the same first result for `page=0`, `page=1`, and `page=999999` at `limit=1`; no paging fields appeared. Raising `limit` from 1 to 2 returned two results in each role. | The `page` parameter is ignored while `limit` caps each result category. Do not loop page values or claim complete search results when a category reaches the limit. Books and Podcasts need separate result mapping; a later story must choose and validate a bounded search strategy before enabling search. |
| Book metadata | Item detail exposes library-item `id`, media `id`, `audioFiles`, `chapters`, `metadata.authors`, `metadata.narrators`, and `coverPath`. The controlled corpus now includes a multi-part book with 10 audio files in indices 1–10, 24 chapters, multiple authors, and multiple narrators; it also retains single-file/missing-credit examples. | Later Books mapping uses stable library-item and media identity, author as primary artist-equivalent, narrator as secondary, and the returned file/chapter ordering. |
| Podcast metadata | Detail exposes library-item `id`, media `id`, `episodes`, podcast metadata, and `coverPath`. | Podcast shows and episodes remain a distinct future mapper and playback contract. |

The stable identity candidate is the tuple `(library ID, library-item ID, media ID)` plus podcast episode ID where relevant. A title, path, ordinal/index, artwork URL, or transient playback URL is never identity.

### Endpoint and failure register

| Case | Status | Contract |
| --- | --- | --- |
| Invalid bearer and revoked refresh token | verified | Both returned 401. Logout revokes refresh, while an existing access token remains usable until expiry. Treat 401 as an authentication failure; refresh once only after an access-token expiry and never retain the response body. |
| Missing item/library | verified | Reads of removed items and a fixture-only nonexistent library returned 404. Treat either as stale/missing identity; do not retry blindly. |
| Changed/removed item with read-only account | verified | `PATCH /api/items/{libraryItem}/media` and `DELETE /api/items/{libraryItem}` both returned 403 for the supplied disposable target. Treat this as non-retryable permission denial; no server state changed. |
| Inaccessible library | verified | The restricted account received 403 for the withheld Podcast library. Treat it as non-retryable permission denial. |
| Changed/removed remote item with mutation permission | verified | A disposable Book accepted a metadata PATCH (200, observable change), followed by a non-hard DELETE (200); its subsequent item read returned 404. Later consumers must classify this as stale remote identity and must never assume delete permission from read access. |
| Cover delivery | verified route shape only | `GET /api/items/{libraryItem}/cover`; fetch only daemon-side and never retain authenticated artwork URLs. |
| Direct stream, range, content type | verified for a disposable book | A `forceDirectPlay: true` session returned `playMethod: 0`, one server-relative audio track, and an authenticated `Range: bytes=0-0` read returned `206`, `Accept-Ranges: bytes`, and `audio/mp4`. The session was closed with `POST /api/session/{session}`. |
| Transcode | verified for playback only | The controlled `forceTranscode: true` session returned `playMethod: 2` and one server-relative playback track. This is session playback, not an item-download conversion capability. The pinned v2.36.1 source revision for this record is stated above. |
| Mismatched client capability | verified | A session supplied only an unsupported MIME type. It returned one server-relative track with `playMethod: 2`, selecting transcode fallback rather than an unusable direct track; the session was closed. |
| Unavailable or incompatible delivery | untested | No controlled failure response was observed. Do not infer a status, classification, or retry policy from the fixture. Later playback work must probe this case before enabling a corresponding capability or error mapping. |
| Book progress read/write | verified for a disposable book | `PATCH` and `GET /api/me/progress/{libraryItem}` accepted/read `currentTime`, `duration`, and `isFinished`; deletion requires the returned media-progress ID, not the library-item ID. Two identical writes each returned 200. An empty PATCH also returned 200 (accepted/defaulting), and every disposable record was deleted; subsequent reads returned 404. Progress remains player-owned. |
| Podcast episode progress and completion | verified for a disposable episode | Episode playback returned one audio track. The episode-scoped progress route accepted a write/read round trip and an `isFinished: true` completion at the duration offset; its disposable record was deleted by returned media-progress ID. |
| 429 | verified | With a 15-second window and maximum two attempts, invalid local logins returned 401 then 429. Do not retry until the server-advertised/rate-limit window elapses. |
| 409 progress conflict | unsupported | A pinned v2.36.1 source scan found 409 only in share creation for duplicate shares/slugs, not catalog, playback, or progress controllers. Do not invent a progress-conflict retry path. |
| 5xx | verified through controlled fault injection | A temporary Nginx rule returned a bare authenticated catalogue 500 for one fixture-shaped request. The rule must be removed after the probe. This establishes only classification, not a provider retry policy. |

### Controlled 5xx completion protocol

Do not corrupt a library, media file, database, or server configuration to obtain a 5xx. Instead, place a temporary authenticated reverse-proxy rule in front of the controlled deployment that returns a bare `500` for one fixture-alias catalogue request, such as `GET /api/libraries/{libraryAlias}/items?page=0&limit=1`. Record only the status, request shape, retry decision, and probe date; do not retain the origin, library ID, token, headers, or generated error body. Remove the rule immediately after one request. This record establishes classification only; later HifiMule stories must still define retry policy deliberately.

### Fixture mapping and offline checks

`audiobookshelf_contract.rs` is entirely offline. It validates fixture schema and status, redaction policy, selected pagination/order/identity invariants, and an HTTP request’s method, path, paging query, and bearer-header shape through `mockito`. It uses only `[fixture-token]`, never a secret. These tests validate the recorded fixture contract; they do not exercise a provider, response DTO parsing, or runtime error classification, because Story 17.1 ships no provider. Those checks remain requirements for later provider implementation.

### Binding rules for Stories 17.2–17.9

All server traffic remains behind `MediaProvider`; authentication and authenticated URLs stay daemon-side. Books and Podcasts retain distinct roles. For future book mapping author is primary and narrator secondary. Progress is player-owned only, never ordinary device sync, and may use only proven stable remote identity plus a whole-item offset. This record grants no permission to add `AudiobookshelfProvider`, `ServerType::Audiobookshelf`, factory detection, vault/config, UI, RPCs, catalog mapping, direct playback, progress write-back, sync, Autofill, or collections.

## Record 2 — controlled playback follow-up (2026-09-22)

The user supplied access to a controlled endpoint with a disposable test account. Requests below were observed live; only redacted shapes and statuses are retained. A follow-up playback response reported `serverVersion: 2.36.1`.

| Probe | Observation | Implementation consequence |
| --- | --- | --- |
| `POST /api/items/{item}/play` with `forceDirectPlay: true` | 200; `playMethod: 0`; one server-relative `audioTracks` entry with `mimeType: audio/mp4` and `codec: aac`. `POST /api/session/{session}/close` returned 200. | Direct AAC/MP4 is a candidate only after authenticated media range and decoder checks. |
| `POST /api/items/{item}/play` with `forceTranscode: true` | 200; `playMethod: 2`; one server-relative track with `mimeType: application/vnd.apple.mpegurl` and no codec. Close returned 200. | The response is HLS, which the current byte-stream decoder does not accept as audio. Do not pass this representation to it or claim transcode playback support. |
| `POST /api/items/{item}/play` with `forceDirectPlay: true` and an unsupported MIME in `supportedMimeTypes` | 200; `playMethod: 0`, AAC/MP4. Close returned 200. | The server did not honor the MIME list as an incompatibility trigger when direct play was forced. This does not establish an incompatible delivery failure mapping. |
| `POST /api/items/{missingItem}/play` | 404. | A missing item is observed; the wider unavailable/incompatible delivery case remains untested. |
| First catalog page with limit 50 | No multipart item in the returned page. | A later explicit item probe was needed for multipart evidence. |
| User-identified ten-part book, `GET /api/items/{item}` then direct play | Detail contained ten indexed audio files. `forceDirectPlay: true` returned `playMethod: 0` and ten tracks. The first track was matched by `ino`; it reported `audio/mpeg` and `mp3`. Authenticated `Range: bytes=0-0` returned 206, `audio/mpeg`, and `Accept-Ranges: bytes`; session close returned 200. | Multipart direct response shape and first-part mapping are observed. This file was still readable during the probe, so it does not establish an unavailable delivery response. |
| Same book, forced transcode | 200, `playMethod: 2`, one HLS track with no matching first-file `ino`; close returned 200. | Do not claim per-part transcode admission from this response. |
| Indexed first part removed after detail and direct session were captured | After refreshing the disposable access token, authenticated `GET` of the previously returned first-track `contentUrl` with `Range: bytes=0-0` returned 404 and no range header. The session close returned 200. An earlier attempt without refresh returned 401 for both media and close; that was authentication expiry, not delivery evidence. | A file that vanishes after admission is a media-read 404. Keep the book available for explicit retry and never skip to another part. Refresh at most once after 401, including cleanup. |
| Follow-up of the ten-part direct response | The session reported `serverVersion: 2.36.1`, `playMethod: 0`, and an item-scoped, server-relative `contentUrl` without a query string. Close returned 200. | Gate the provider adapter on the observed server version and reject URLs outside the selected item path. |

The unavailable-after-admission race is verified. The incompatible-format failure remains `untested`; the forced-direct unsupported-MIME request did not trigger it. No failure body, session identifier, token, item identifier, title, or authenticated URL was retained.
