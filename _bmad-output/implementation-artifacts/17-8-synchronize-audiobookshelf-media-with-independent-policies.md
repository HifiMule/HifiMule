# Story 17.8: Synchronize Audiobookshelf media with independent policies

---
baseline_commit: da404dcbc881f6cf9b2d67dfc484ea8595073032
---

Status: done

## Story

As a portable-listening user,
I want audiobooks and podcasts to sync under appropriate rules and into configurable device folders,
so that durable books and changing episode feeds fit my device.

## Acceptance Criteria

1. **Independent audiobook sync.** Each Audiobookshelf Books server participates in existing per-server basket selection, budget, compatibility preview, transfer, manifest ownership, and remote-removal reconciliation. Ordered book parts remain associated with their book and stable remote identity. A changed or missing remote item follows normal managed-sync cleanup; unrelated device files remain untouched.
2. **Podcast Autofill.** Each Podcasts server is an independent existing Autofill source with a configurable, capacity-managed recent/unplayed retention policy and its own budget. Selection is deterministic and bounded by available capacity; missing dates or play-state information have explicit fallback behavior. Never claim that playback in another app can be observed or protected. Podcasts remain typed shows/episodes, not fake books or songs in the public catalog.
3. **Pre-transfer compatibility.** Resolve a verifiable direct or transcoded representation within the target profile before any media write. Preview and sync explain why an unsupported book part or episode is blocked; failures do not leave a falsely owned or partially published file. Preserve existing provider authentication and private URL handling.
4. **Device destination folders.** Initialize Device and Device Settings offer device-relative **Music**, **Audiobooks**, **Podcasts**, and existing **Playlists** folder controls. Audiobook and podcast folder settings are optional; each defaults to the effective Music root when unset. Music files use Music, audiobook parts use Audiobooks, podcast episodes use Podcasts. A Garmin-like configuration can use `/Music`, `/Audiobooks`, `/Podcasts` independently. The existing playlist folder setting, default, relative references, and generation behavior remain intact.
5. **Safe configuration and reconciliation.** Persist the two new folder choices in the device manifest and expose them consistently through device list, initialize, and update RPCs. Older manifests and unchanged settings continue to sync to Music. Validate/normalize paths with the same device-relative safety rules as Music and Playlists. Preview the cleanup/resync impact of changed folders before writes; delta calculation, deletion protection, transfer, and repeat sync agree on each media type's effective root. A rerun with unchanged configuration is stable.
6. **Regression evidence.** Offline tests cover mixed music/book/podcast servers, independent budgets, recent/unplayed retention, incompatible items, legacy-manifest defaults, separate and shared folder roots, folder changes, managed-only deletion, playlist references, and preview/execution parity. Existing Jellyfin/Subsonic music sync and direct playback remain functional.

## Tasks / Subtasks

### Review Findings

- [x] [Review][Patch] [P1] Preserve direct-media validation on staged retry [hifimule-daemon/src/sync.rs:3013] — The retry now disables redirects for direct media and rechecks the expected content type before staging.
- [x] [Review][Patch] [P2] Give zero-duration podcast episodes a positive capacity estimate [hifimule-daemon/src/rpc.rs:5981] — `None` and `Some(0)` now use the same one-hour fallback in Autofill, desired items, and UI basket estimates.
- [x] [Review][Patch] [P2] Keep the audiobook size fallback scoped to books [hifimule-ui/src/library.ts:381] — Ordinary music tracks with missing size metadata retain their prior zero-byte UI estimate.

- [x] Admit Audiobookshelf media to device selection and sync (AC: 1, 3)
  - [x] Carry explicit media role and stable provider identity from Books/Podcasts catalog through desired items, preview, add plan, and synced manifest entries; do not infer a podcast from a song-shaped ID.
  - [x] Reuse server budgets, remote-removal reconciliation, compatibility checks, atomic transfer, and managed-path ownership. Ensure partial book/episode failures are reported precisely.
- [x] Add podcast Autofill retention (AC: 2)
  - [x] Extend the existing per-server pipeline and settings contract for recent/unplayed episodes, with a documented ordering/tie-break, fallback for unavailable progress/date, byte estimate, and capacity limit.
  - [x] Keep background sync entirely separate from player-owned Audiobookshelf progress read/write.
- [x] Add device folder configuration end to end (AC: 4, 5)
  - [x] Add optional audiobook/podcast paths to manifest with backward-compatible defaults and serialization aliases; initialize directories as needed.
  - [x] Extend device initialize/update/list RPCs, UI types, Initialize Device, Device Settings, and localized labels/help. Preserve Playlists as its own control.
  - [x] Route each media role to its effective root in preview, transfer, delta/reconciliation, and safe delete validation. Handle equal roots, changed roots, MTP folder-cache invalidation, and playlist relative paths.
- [x] Verify integrations and regressions (AC: 1–6)
  - [x] Add focused daemon provider/RPC/sync tests and UI settings tests; run existing relevant suites and record actual evidence and limitations.

## Dev Notes

### Architecture and current code

- `hifimule-daemon/src/device/mod.rs` owns `DeviceManifest`: `managed_paths[0]` is the Music root, `playlist_path` is optional, and `resolved_playlist_path()` inherits Music. Extend this shape without changing the meaning of existing fields. Initialization currently creates Music/playlist directories. Existing manifests must deserialize without migration failure.
- `hifimule-daemon/src/rpc.rs` implements `device.list`, `device.initialize`, and `device.update_manifest`; the latter validates Music/Playlists paths, handles relocation, and invalidates MTP folder IDs. Extend the same paths for Audiobooks/Podcasts. Also audit all basket/Autofill producers: `DesiredItem` is built in several RPC branches, and role must survive every route.
- `hifimule-daemon/src/sync.rs` currently selects `managed_paths.first()` as the single media root in the transfer producer and `calculate_delta`. `DesiredItem`, `SyncAddItem`, and `SyncedItem` carry server ID but no media role. A root change only in the producer will make delta repeatedly relocate new media: update both planning and reconciliation. Playlist generation already uses `resolved_playlist_path()` and relative media paths; preserve it.
- `hifimule-daemon/src/providers/mod.rs` exposes `MediaProvider::library_role()` and `ProviderLibraryRole::{Audiobook,Podcast}`. Preserve the `MediaProvider` boundary and immutable selected library role. The provider owns Audiobookshelf API calls and authenticated URLs. Do not call its endpoints from generic sync or UI code.
- `hifimule-ui/src/components/BasketSidebar.ts` and `InitDeviceModal.ts` show/save Music and Playlists fields. Extend both and `hifimule-ui/src/rpc.ts`; use existing localization and accessible control patterns. Changes to folder roots must appear in the next sync preview.
- Story 17.7 intentionally kept podcast episodes out of generic song admission, basket, and sync. Its typed show/episode model and opaque identity are the starting point. Add explicit sync admission while retaining that boundary. Keep audiobook whole-item progress tied to the player; synchronization does not read/write playback progress.

### Policy decisions for implementation

- Treat Audiobooks and Podcasts folder fields as **device-wide destinations**, never Audiobookshelf source filtering. All eligible media of that role use the selected device folder, regardless of server. Empty/unset inherits current Music root dynamically, so changing Music updates inherited destinations; an explicit equal path remains explicit.
- Reuse current device-relative path validation. Reject traversal, absolute host paths, reserved manifest paths, and unsafe overlap as applicable; allow intentional equality among media roots. Manifest-owned prior paths may be cleaned only through existing managed-sync safeguards. Device folder changes are relocation candidates, not evidence that external files are managed.
- Define the retention policy and fallback in code and UI before implementation. “Unplayed” can only use progress data actually available from Audiobookshelf; stale/unknown status must not be silently described as unplayed. Never delete externally played episodes on an unverifiable assumption.
- Avoid new dependencies unless the current provider/sync abstractions cannot support the contract. Use repository-pinned toolchain and Audiobookshelf fixture/controlled-server contract; this story requires no library upgrade.

### Previous story and test intelligence

- Story 17.7 delivered role-scoped podcast browse, stable episode identity, typed playback, and synthetic fixtures. Review fixes explicitly rejected podcast IDs from generic song lookup and kept episodes out of generic session admission. Use a separate typed sync adapter rather than undoing those guards.
- Relevant existing tests are in `hifimule-daemon/src/sync.rs`, `hifimule-daemon/src/rpc.rs`, provider Audiobookshelf tests/fixtures, and `hifimule-ui/scripts/destination-ui.test.mjs`. Add test cases that compare preview with actual manifest paths and verify a second sync produces no needless move/delete.
- Git history inspection was unavailable in this workspace because Git reported dubious ownership. Do not assume recent commit contents from the working tree.

### References

- [Source: `_bmad-output/planning-artifacts/epics.md` § Epic 17, Story 17.8]
- [Source: `_bmad-output/planning-artifacts/prd.md` § Audiobookshelf Extension, FR82–FR87]
- [Source: `_bmad-output/planning-artifacts/architecture.md` § Epic 17 — Audiobookshelf Architecture Amendment]
- [Source: `_bmad-output/planning-artifacts/ux-design-specification.md` §§ 5.4, 5.6, 8]
- [Source: `_bmad-output/implementation-artifacts/17-7-add-audiobookshelf-podcast-servers-and-direct-playback.md`]

## Dev Agent Record

### Debug Log

- 2026-09-24: Began story from baseline `da404dcbc881f6cf9b2d67dfc484ea8595073032`. Traced provider selection, delta, transfer, and manifest paths. Audiobookshelf deliberately rejects generic `download_url`; direct media requires an authenticated session-scoped request, and the verified transcode response is HLS rather than a downloadable audio file.
- Wrote a red test for audiobook role propagation and made it pass using the repository daemon build wrapper (`node scripts/build-daemon.mjs test -p hifimule-daemon audiobook_role_survives_delta_planning`). Prototype edits then encountered repeated implementation failures while extending the transfer and policy seam. The prototype did not meet the story's acceptance criteria, so all production and test edits were restored to the baseline. No task is claimed complete.
- Resumed implementation and replaced the prototype with typed provider media resolution, authenticated direct transfer with one refresh on 401, preview compatibility blocks, role-aware delta and manifest ownership, per-server podcast retention, and device folder settings. Incomplete podcast catalog pages abort retention reconciliation to protect managed episodes.
- 2026-09-25: Follow-up found that book and book-part basket controls were hidden and podcast rows had playback only. Added grid/list selection for books and parts, podcast show/episode selection, typed basket labels, and matching UI/daemon audiobook size estimates when source byte metadata is unavailable.

### Implementation Plan

- Implemented typed provider-owned sync representation with request headers and session cleanup through staging, preview compatibility admission, role-aware delta and transfer, podcast retention, and the device folder contract and UI.

### Agent Model Used

GPT-6

### Completion Notes List

- Ultimate context engine analysis completed - comprehensive developer guide created.
- 2026-09-24: Resumed after the earlier halted attempt. Audiobookshelf Books and Podcasts use explicit media roles, typed catalog identities, verified direct representations, and preview blocked reasons. The UI offers independent folder paths and podcast retention settings.
- Podcast retention sorts parsed UTC publication times newest first with episode ID as a stable tie-break; missing or malformed dates sort last. It estimates 128 kbps using a one-hour duration fallback and fills within the server's byte budget. Because typed catalog playback state is unavailable, `unplayedOnly` uses the recent fallback and tells the user so; it does not read or write player-owned progress.
- Verification: daemon suite `1131 passed, 0 failed, 6 ignored`; Audiobookshelf contract suite `5 passed`; UI source tests `19 passed` plus retention round-trip tests `2 passed`; UI TypeScript check passed; Rust formatting and Git whitespace checks passed. No live device or remote Audiobookshelf was used.
- 2026-09-25 follow-up verification: daemon suite `1132 passed, 0 failed, 6 ignored`; Audiobookshelf contract suite `5 passed`; UI tests `24 passed`; UI TypeScript check passed. Book, book-part, podcast show, and episode basket entries now reach the existing sync item ID and server ID route. No live UI/device run was performed.
- 2026-09-25 code review: Three findings patched. Focused Rust duration test and four UI selection tests pass; Rust formatting and Git whitespace checks pass. UI TypeScript check was unavailable in this environment because npm and local TypeScript dependencies are absent.

### File List

- `_bmad-output/implementation-artifacts/17-8-synchronize-audiobookshelf-media-with-independent-policies.md`
- `_bmad-output/implementation-artifacts/sprint-status.yaml`
- `hifimule-daemon/src/auto_fill/pipeline.rs`
- `hifimule-daemon/src/device/mod.rs`
- `hifimule-daemon/src/device/tests.rs`
- `hifimule-daemon/src/main.rs`
- `hifimule-daemon/src/providers/audiobookshelf.rs`
- `hifimule-daemon/src/providers/mod.rs`
- `hifimule-daemon/src/rpc.rs`
- `hifimule-daemon/src/scrobbler.rs`
- `hifimule-daemon/src/sync.rs`
- `hifimule-i18n/catalog.json`
- `hifimule-ui/src/components/AutoFillPanel.ts`
- `hifimule-ui/src/components/BasketSidebar.ts`
- `hifimule-ui/src/components/InitDeviceModal.ts`
- `hifimule-ui/src/library.ts`
- `hifimule-ui/src/state/autoFill.ts`
- `hifimule-ui/src/state/mediaSyncSelection.ts`
- `hifimule-ui/tests/audiobookshelfBrowse.test.mjs`
- `hifimule-ui/tests/story17-8Policy.test.mjs`
- `hifimule-ui/tests/story17-8Selection.test.mjs`

### Change Log

- 2026-09-24: Captured baseline, marked story in progress, and recorded the halted implementation attempt; no tasks completed.
- 2026-09-24: Implemented Story 17.8 and moved to review after daemon, UI, contract, formatting, and whitespace verification.
- 2026-09-25: Completed missing audiobook and podcast UI selection and corrected audiobook size estimates; reran daemon and UI regression suites.
