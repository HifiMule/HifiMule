---
title: 'Fix: Provider sync track number prefix always "00 - "'
type: 'bugfix'
created: '2026-05-28'
status: 'done'
baseline_commit: '67c8077'
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** When syncing tracks via the provider path (Navidrome / Subsonic, or Jellyfin without transcoding) every output filename is prefixed `00 - <title>` regardless of the actual track number, because `construct_desired_file_path` hardcodes `"00"`. The Jellyfin API path is unaffected because it uses a separate function (`construct_file_path_with_extension`) that reads `JellyfinItem.index_number` directly.

**Approach:** Propagate the track number from the `Song` domain model through `DesiredItem` → `SyncAddItem` and use it in `construct_desired_file_path`; fall back to `"00"` only when the track number is genuinely absent.

## Boundaries & Constraints

**Always:**
- Adding new fields to `DesiredItem` and `SyncAddItem` must use `#[serde(default)]` to remain backward-compatible with serialized manifests / delta JSON already on disk.
- When track number is `None`, keep the existing `"00"` fallback so the filename format is unchanged for those cases.
- Only the provider sync path (`execute_provider_sync`) and its supporting structs are in scope — do not touch `construct_file_path_with_extension` or `execute_sync`.

**Ask First:** None anticipated.

**Never:**
- Do not store `track_number` in `SyncedItem` or the on-disk manifest as part of this fix.
- Do not change the filename format itself (e.g. `"01 - Title.flac"` must remain the pattern).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Normal Navidrome track, track_number = 3 | `Song { track_number: Some(3), title: "Foo", … }` | filename `03 - Foo.flac` | N/A |
| Track with no track number | `Song { track_number: None, … }` | filename `00 - Title.ext` (unchanged fallback) | N/A |
| Track 10+ | `Song { track_number: Some(12), … }` | filename `12 - Title.ext` | N/A |
| Force-add relocation (SyncedItem → SyncAddItem, no track data) | manifest item re-sync due to folder move | `00 - Title.ext` (no track data available; acceptable) | N/A |

</frozen-after-approval>

## Code Map

- `hifimule-daemon/src/sync.rs:52` — `DesiredItem` struct — add `track_number: Option<u32>`
- `hifimule-daemon/src/sync.rs:69` — `SyncAddItem` struct — add `track_number: Option<u32>`
- `hifimule-daemon/src/sync.rs:440` — `construct_desired_file_path` — fix hardcoded `"00"` on line 460
- `hifimule-daemon/src/sync.rs:2444` — `SyncAddItem` from `desired` (missing-file recovery) — propagate `track_number`
- `hifimule-daemon/src/sync.rs:2748` — `SyncAddItem` from `desired` (initial adds) — propagate `track_number`
- `hifimule-daemon/src/sync.rs:3282` — test helper `make_desired_item` — add `track_number: None`
- `hifimule-daemon/src/sync.rs:4315` — inline `DesiredItem` in test — add `track_number: None`
- `hifimule-daemon/src/sync.rs:4375` — inline `DesiredItem` in test — add `track_number: None`
- `hifimule-daemon/src/sync.rs:3405` — test helper `add_item_with_provider_format` — add `track_number: None`
- `hifimule-daemon/src/rpc.rs:1481` — `provider_song_to_desired_item` — add `track_number: song.track_number` (**core fix**)
- `hifimule-daemon/src/rpc.rs:1813` — `jellyfin_item_to_desired_item` — add `track_number: item.index_number`
- `hifimule-daemon/src/rpc.rs:2473` — inline `JellyfinItem → DesiredItem` — add `track_number: item.index_number`
- `hifimule-daemon/src/rpc.rs:2765` — auto-fill `DesiredItem` construction — add `track_number: None`
- `hifimule-daemon/src/rpc.rs:2967` — force-add `SyncAddItem` from `SyncedItem` — add `track_number: None`
- `hifimule-daemon/src/main.rs:583` — auto-fill `DesiredItem` in daemon auto-sync — add `track_number: None`
- `hifimule-daemon/src/main.rs:932` — `to_desired_item` from `JellyfinItem` — add `track_number: item.index_number`

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-daemon/src/sync.rs` — Add `pub track_number: Option<u32>` to `DesiredItem` (no serde — not serialized) and `#[serde(default)] pub track_number: Option<u32>` to `SyncAddItem`; fix `construct_desired_file_path` to use `item.track_number.map(|n| format!("{:02}", n)).unwrap_or_else(|| "00".to_string())`; propagate `track_number` at the two `SyncAddItem` construction sites; add `track_number: None` to all inline test struct literals
- [x] `hifimule-daemon/src/rpc.rs` — Populate `track_number` at every `DesiredItem` and `SyncAddItem` construction site: `provider_song_to_desired_item` → `song.track_number`; `jellyfin_item_to_desired_item` → `item.index_number`; inline JellyfinItem→DesiredItem → `item.index_number`; auto-fill DesiredItem → `None`; force-add SyncAddItem from SyncedItem → `None`
- [x] `hifimule-daemon/src/main.rs` — Add `track_number: None` to auto-fill `DesiredItem`; add `track_number: item.index_number` to `to_desired_item`

**Acceptance Criteria:**
- Given a Navidrome album sync without transcoding, when the sync completes, then each track file on the device is named `<track_number_zero_padded> - <title>.<ext>` matching the track's index on the server.
- Given a track with no track number metadata, when synced via the provider path, then the filename falls back to `00 - <title>.<ext>`.
- Given any existing Jellyfin API sync path, when synced, then behaviour is unchanged (no regression).
- Given `cargo build`, when compiled, then zero new warnings or errors.

## Spec Change Log

## Verification

**Commands:**
- `cargo build --manifest-path hifimule-daemon/Cargo.toml` -- expected: compiles without errors
- `cargo test --manifest-path hifimule-daemon/Cargo.toml` -- expected: all tests pass

## Suggested Review Order

**Core fix**

- Entry point: replaces hardcoded `"00"` with zero-padded track number; `None` falls back to `"00"`.
  [`sync.rs:460`](../../hifimule-daemon/src/sync.rs#L460)

**Data model**

- New field on `DesiredItem` (non-serialized internal struct, no serde attr needed).
  [`sync.rs:65`](../../hifimule-daemon/src/sync.rs#L65)

- New field on `SyncAddItem` (serialized — `#[serde(default)]` ensures backward compat with old manifests).
  [`sync.rs:84`](../../hifimule-daemon/src/sync.rs#L84)

**Provider data wiring (Navidrome/Subsonic path — the reported bug)**

- `provider_song_to_desired_item`: sources `song.track_number` from the domain model — this is the root fix.
  [`rpc.rs:1490`](../../hifimule-daemon/src/rpc.rs#L1490)

- `calculate_delta`: propagates `track_number` when building initial `SyncAddItem` adds.
  [`sync.rs:2764`](../../hifimule-daemon/src/sync.rs#L2764)

- `augment_delta_with_existence_check`: propagates `track_number` for missing-file recovery adds.
  [`sync.rs:2459`](../../hifimule-daemon/src/sync.rs#L2459)

**Peripherals (Jellyfin + autofill sites, `None` where data unavailable)**

- `jellyfin_item_to_desired_item`: wires `item.index_number` for completeness (Jellyfin uses a separate sync path).
  [`rpc.rs:1852`](../../hifimule-daemon/src/rpc.rs#L1852)

- `to_desired_item` (daemon auto-sync, Jellyfin items): same completeness wiring.
  [`main.rs:941`](../../hifimule-daemon/src/main.rs#L941)

- Force-add and autofill sites: `None` — no track data available from these sources.
  [`rpc.rs:2483`](../../hifimule-daemon/src/rpc.rs#L2483)
