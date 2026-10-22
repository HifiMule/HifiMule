---
title: 'Automatic folder artwork during track synchronization'
type: 'bugfix'
created: '2026-10-22'
status: 'done'
baseline_commit: '6ead820fab7310b25f1ea37d921eb94293798999'
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Host-specific artwork preferences pollute portable manifests. Final image copying leaves sync reporting “nearly finished” for minutes.

**Approach:** Keep one device artwork switch; prepare artwork/native integrations during music transfer before reporting track completion. Unchanged folders wait for an update or full sync.

## Boundaries & Constraints

**Always:** When enabled, prepare `folder.jpg`, `.directory` and Windows ICO/`desktop.ini` on mounted devices on every OS. Apply Windows attributes on Windows, Finder on macOS, GNOME on compatible Linux sessions only. Skip unavailable integration without compatibility warnings. Persist only `folderArtwork.enabled` as configuration; retain ownership/source evidence. Load legacy desktop booleans without losing enabled or ownership. Keep image limits, authentication, cancellation, free-space checks and unmanaged files.

**Ask First:** Changes to the master-switch default, artwork size limits, or unmanaged-file preservation.

**Never:** Bulk final image creation, unchanged-library backfill, new ETA machinery, embedded-artwork changes or mounted APIs for MTP. Explorer activation still depends on Windows attributes/filesystem behavior.

## I/O & Edge-Case Matrix

| State | Behavior | Failures |
| --- | --- | --- |
| New/updated music | Artist/album artwork before track completion | Nonfatal to audio |
| Shared physical folder | One attempt per sync; reuse JPEG/source | Retry on later touched sync |
| Legacy flags | Preserve enabled; omit obsolete flags on save | Validate input |
| Unsupported native adapter | Portable files only | No compatibility warning |
| User files/icons | Preserve | Keep conflict diagnostics |
| Unchanged older folder | No artwork request/generation | Update/full sync triggers it |
| Last descendant removed | Retire owned outputs before pruning | Keep modified/foreign evidence |
| MTP | Safe JPEG publication | Skip mounted-only icons |

</frozen-after-approval>

## Code Map

- `hifimule-daemon/src/device/{mod.rs,manifest_codec.rs}` — configuration and ownership persistence.
- `hifimule-daemon/src/{rpc.rs,main.rs}` — validation, providers and auto-sync admission.
- `hifimule-daemon/src/{sync.rs,sync/artwork.rs}` — writer, checkpoints, physical identity and artwork lifecycle.
- `hifimule-daemon/src/{folder_icons.rs,device_io.rs}` — portable files, native APIs and transports.
- `hifimule-ui/src/components/{BasketSidebar.ts,InitDeviceModal.ts}` — both settings surfaces.

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-daemon/src/device/{mod.rs,manifest_codec.rs}`, `hifimule-daemon/src/rpc.rs` — enabled-only settings; tolerate legacy manifest flags without reserializing them; retain strict validation.
- [x] `hifimule-daemon/src/{folder_icons.rs,device_io.rs}` — separate portable Windows preparation from activation; gate native work by compatibility; preserve old ownership tokens and safe retirement.
- [x] `hifimule-daemon/src/sync/artwork.rs` — split incremental creation from final cleanup. Initialize ownership/identity state once; process touched artist/album folders, deduplicate aliases, reuse stable winners/JPEGs and checkpoint publication immediately. Avoid full-manifest clones or plan/library scans per track.
- [x] `hifimule-daemon/src/sync.rs` — run artwork after verified music publication and successful audio checkpoint, before advancing bytes/files. Preserve checkpoint-failure accounting/cancellation. Final artwork work only retires obsolete outputs.
- [x] `hifimule-daemon/src/{main.rs,rpc.rs}` — remove enabled-only no-op admission and retained-library provider acquisition; capture providers for touched folders and prior winners. Keep pending cleanup retry possible.
- [x] `hifimule-ui/src/components/{BasketSidebar.ts,InitDeviceModal.ts}`, `hifimule-i18n/catalog.json` — remove desktop controls/types/listeners, send enabled only, and explain automatic integration during track updates in all four languages.
- [x] `hifimule-daemon/src/{sync/artwork.rs,sync.rs,folder_icons.rs,device/tests.rs,rpc.rs}`, `hifimule-ui/tests/{deviceSettings.test.mjs,initDeviceOptions.test.mjs}` — test the matrix; replace obsolete backfill/option expectations. Cover artwork before the next audio write, pending final-track progress, full sync, aliases, cancellation and cleanup preservation.

**Acceptance Criteria:**
- Given artwork enabled, when copying music to a mounted device, then portable files and compatible native integrations are prepared automatically.
- Given slow artwork, when its track syncs, then completion counters remain pending until artwork finishes, without bulk creation afterward.
- Given unchanged older folders, when syncing, then missing artwork is untouched; given a full sync, when tracks are recopied, then their folders become eligible.
- Given legacy configuration, when reconnecting and saving, then enabled survives and desktop preferences disappear from saved configuration and UI.

## Spec Change Log

## Design Notes

ETA uses wall-clock throughput. Delaying completion until artwork finishes includes its cost naturally. Final retirement still protects directory pruning, including old-root descendants, aliases and foreign-host ownership.

## Verification

- `rtk npm run build:daemon -- test -p hifimule-daemon` — daemon regressions pass with repository FFmpeg wrapper.
- `rtk test node --test hifimule-ui/tests/deviceSettings.test.mjs hifimule-ui/tests/initDeviceOptions.test.mjs` — UI checks pass.
- From `hifimule-ui`: `rtk npm run build` — production build passes.
- `rtk git diff --check` — passes; state unavailable native/file-manager validation.

### Execution Results — 2026-10-22

- Daemon regression suite passed: 1,655 unit tests and 5 contract tests; 14 existing tests ignored. Used the repository FFmpeg wrapper with loopback access for mock HTTP servers.
- Both targeted UI test files passed (9 tests), and the UI production build passed.
- `rtk git diff --check` passed. Original mixed line endings retained outside edited regions.
- Regression coverage includes slow inline artwork before the next audio write and before final progress, full-sync repair, physical aliases/root boundaries, cancellation after publication, disk-space failures, legacy manifest validation/ownership, portable Windows sidecar reuse/conflicts, and foreign-host cleanup evidence.
- Existing ICO encoding produced RGB PNG content rejected by the ICO decoder; output now uses RGBA and is decoded by the portable sidecar regression.
- Windows/Linux native activation and visual Explorer/Finder/GNOME behavior were not manually exercised on this macOS host.

### Review Patches — 2026-10-22

- Portable Windows ownership without saved attributes now removes only its sidecars; folder READONLY restoration requires an owned previous value. Added a Windows-only regression for preserving an independently applied READONLY attribute.
- Partial sidecar repair and Windows activation retain hashes of actual reused legacy RGB ICO/INI bytes. A portable regression repairs a missing INI beside a legacy RGB ICO, reuses the result, and retires it safely.
- Current track metadata replaces its indexed same-server/track candidate before source election. The production context filters scheduled deletion, replacement, and rekey candidates once at initialization; saved ownership winners remain intact. Added changed-album-identity and actual-writer deleted-competing-source regressions.
- Focused post-review verification passed: 14 `folder_icons::tests` and 26 artwork-filtered tests. `rtk git diff --check` passed. The Windows-only attribute regression is compiled/run only on Windows and was not exercised on this macOS host. Earlier complete-suite and UI results remain above.

## Suggested Review Order

**Track loop and progress**

- Create artwork after audio checkpoint, before reporting track completion.
  [sync.rs:4721](../../hifimule-daemon/src/sync.rs#L4721)

- Initialize once and deduplicate touched physical folders.
  [artwork.rs:400](../../hifimule-daemon/src/sync/artwork.rs#L400)

- Keep final work limited to retiring obsolete owned outputs.
  [artwork.rs:604](../../hifimule-daemon/src/sync/artwork.rs#L604)

**Portable files and host integration**

- Prepare Windows sidecars everywhere; preserve native activation ownership across hosts.
  [folder_icons.rs:190](../../hifimule-daemon/src/folder_icons.rs#L190)

- Attempt GNOME metadata only in a compatible session.
  [folder_icons.rs:421](../../hifimule-daemon/src/folder_icons.rs#L421)

**Settings and supporting checks**

- Persist only the master switch while reading legacy desktop preferences.
  [mod.rs:138](../../hifimule-daemon/src/device/mod.rs#L138)

- Expose one artwork switch in device settings.
  [BasketSidebar.ts:774](../../hifimule-ui/src/components/BasketSidebar.ts#L774)

- Verify slow artwork ordering, final progress, and full-sync repair.
  [sync.rs:8496](../../hifimule-daemon/src/sync.rs#L8496)

- Preserve legacy ICO ownership while repairing missing sidecars.
  [folder_icons.rs:668](../../hifimule-daemon/src/folder_icons.rs#L668)

- Protect Windows attributes never activated by HifiMule.
  [folder_icons.rs:723](../../hifimule-daemon/src/folder_icons.rs#L723)
