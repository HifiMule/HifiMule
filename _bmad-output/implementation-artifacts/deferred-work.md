# Deferred Work

All previously deferred items have been incorporated into Epic 7 stories (7.1–7.4) in `_bmad-output/planning-artifacts/epics.md`.

## Deferred from: code review of 8-5-subsonic-url-credential-sanitization (2026-05-09)

- **`sync.rs` `SyncFileError` audit evidence absent** — sync is Jellyfin-specific so no Subsonic URLs flow through it today; revisit if sync becomes provider-neutral.
- **`main.rs` daemon logging audit evidence absent** — no known Subsonic URL exposure in current logging paths; revisit when tracing is added to daemon startup.
- **`rpc.rs` image proxy path not patched** — pre-existing path unchanged by this diff; if `cover_art_url()` result ever appears in an error propagated through the image proxy, it would leak credentials.
- **`rpc.rs` sync spawning path not patched** — same concern as image proxy; no current evidence of URL exposure.
- **`sanitize_subsonic_url` does not strip URL authority credentials (`user:pass@host`)** — Subsonic REST does not use authority-embedded credentials today; handle if a future client variant does.
- **Percent-encoded credentials not matched by `sanitize_subsonic_message`** — scanner requires literal `key=` bytes; double-encoded forms (e.g. `password%3Dxxx`) are missed. Narrow edge case with no current path producing such strings.
- **`sanitize_subsonic_message` applied to `Deserialization` errors** — deserialization errors unlikely to contain credentials but are now silently mangled; consider whitelisting error types that go through sanitization.

## Deferred from: code review of 8-4-runtime-server-type-detection-factory (2026-05-09)

- **Auto mode discards Subsonic and Jellyfin error details** — on all-fail, caller gets only "Unknown server type at this URL" with no indication of whether failures were network, auth, or protocol errors. Pre-existing design choice per AC 1; consider richer diagnostics in Story 8.6+. [`providers/mod.rs:163`]
- **`check_server_connection_cached` falls back to Jellyfin-only credentials check when no provider loaded** — ignores Subsonic servers for connectivity status when the provider is None. Pre-existing behavior not introduced by this story.
- **Three separate `RwLock`s for `provider`/`server_type`/`server_version` allow inconsistent intermediate reads** — a reader can observe `server_type = "jellyfin"` while `provider` is `None`. Pre-existing architectural pattern; fix requires a composite lock or deriving server_type from the provider directly. [`rpc.rs:60`]
- **`restore_provider_from_config` sets `state.server_type` from raw DB string instead of provider's `server_type()`** — harmless today since DB strings are written by `server_type_slug`, but could drift if the DB was written by an older schema version. [`rpc.rs:174`]

## Deferred from: code review of 8-3-subsonicprovider-adapter (2026-05-09)

- **`t=` and `s=` auth params not sanitized in error messages** — `sanitize_message` only strips `password=` and `p=`; the derived token `t=` and salt `s=` also appear in Subsonic URLs embedded in error strings. Story 8.5 owns comprehensive credential sanitization.
- **`ProviderError::NotFound` always reports `item_type="item", id="unknown"`** — loses the actual endpoint and item ID that triggered the not-found; pre-existing design constraint shared with JellyfinProvider.
- **Passwords stored as plaintext `String` with no `zeroize`-on-drop** — `SubsonicClient.password` is a plain `String` with no secure memory clearing. Pre-existing pattern across the entire daemon crate.
- **`reqwest::Client` instantiated per `SubsonicClient` with no shared connection pool** — each provider instance allocates its own TLS stack and DNS resolver. Pre-existing pattern.

## Deferred from: code review of 8-1-mediaprovider-trait-and-domain-models (2026-05-09)

- **`changes_since` token is untyped `Option<&str>`** — no newtype or enum prevents arbitrary strings; a dedicated `ChangeCursor` newtype would enforce a single contract. Story 8.4 owns connection/token semantics.
- **`ChangeEvent.version` name may mislead implementors** — "version" reads as a content version, not a sync cursor/position marker; consider renaming to `sync_cursor` or `change_token` in story 8.2 when change event semantics are fully defined.
- **`search` lacks pagination/limit parameters** — trait-level `search(query: &str)` forces every implementation to silently truncate or return everything; add `limit`/`offset` or a `SearchOptions` struct in a future story.
- **`#[non_exhaustive]` missing on public enums** — `ItemType`, `ChangeType`, `ServerType`, `ScrobbleSubmission` are all public enums; adding a variant later is a breaking change for any downstream match arms. Add `#[non_exhaustive]` when the domain module stabilizes.
- **`ProviderError::Http.status` is raw `u16` with no range validation** — an implementor can set `status: Some(99)` without compile-time rejection; consider `http::StatusCode` or a validated newtype when the `http` crate is added to the workspace.

## Deferred from: code review of 8-2-jellyfinprovider-adapter (2026-05-09)

- **`download_url` without profile returns unauthenticated URL** — `JellyfinProvider::download_url(None)` constructs `/Items/{id}/Download` with no token. Jellyfin requires auth on this endpoint. Deferred because sync.rs still uses JellyfinClient directly; Story 8.4 owns provider integration and must resolve the auth header contract.
- **Token stored as plain `String` without `CredentialKind` wrapper** — `JellyfinProvider` stores the auth token as a raw `String` field rather than using the `CredentialKind::Token` type from Story 8.1. No Debug impl exists so no actual leak, but diverges from the established security pattern. Story 8.4 owns the constructor interface and full provider lifecycle.
- **`user_id` not url-encoded in `get_items_changed_since`** — Consistent with the rest of `JellyfinClient` which also inserts `userId` raw. Jellyfin UUIDs (hex + hyphens) do not require URL encoding in practice. Pre-existing pattern.

## Deferred from: code review of 7-4-packaging-and-cicd-hardening (2026-05-08)

- **`copy_brew_dylib` basename collision** — two dylibs from different Homebrew prefix paths with identical basenames overwrite each other in `LIB_DIR`; install_name_tool rewrites may miss the dropped copy. Unlikely for libmtp's typical transitive deps but not impossible.
- **AppImage `files` mapping hardcodes x86_64 source path** — `/usr/lib/x86_64-linux-gnu/libmtp.so.9` will fail silently if CI runner ever changes to arm64. Should use a `find`-based path resolution at build time.
- **macOS DMG smoke test MOUNT_POINT conflict** — `/Volumes/HifiMule` is hardcoded; a different volume mounted at that path before the test would be silently detached. Pre-existing issue.
- **`-displayfd` polling timeout** — 50 × 0.1s = 5 seconds max wait for Xvfb to write the display number; may not be sufficient on very slow or heavily loaded CI runners.
- **`is_boot_volume_device` fail-safe skip on metadata error** — `std::fs::metadata` failure causes the candidate volume to be silently skipped rather than retried. Documented design decision; a momentary metadata error could cause a connected device to be missed until the next observer cycle.

## Deferred from: code review of 7-2-devicemanager-concurrency-refactor (2026-05-08)

- **TOCTOU in `handle_device_detected`** — read-lock `contains_key` check followed by separate write-lock insert; two concurrent callers can both pass the guard and both insert for the same path. Pre-existing pattern unchanged by this story.
- **MTP tight retry loop on read failure** — `emit_mtp_probe_event` returning `false` leaves the device retryable but the 2-second observer loop has no backoff or retry counter. Intentional per AC4 but needs a broader cooldown design.
- **`list_root_folders` TOCTOU** — selected path can be removed between snapshot lock release and `read_dir`; error propagates via `?`. Pre-existing.
- **`run_observer` silent dropped `Removed` events** — `tx.try_send` for eviction and removal events can silently fail if channel is full, leaving ghost entries in `connected_devices`. Pre-existing mechanism.
- **`get_mounts` accidental volume-disappearance skip** — volumes that disappear between `read_dir` and `is_mount_point` return `false` from `is_mount_point` (not a hard error), so they are not included in `current_mounts`. AC9 is met behaviourally but without explicit handling.
