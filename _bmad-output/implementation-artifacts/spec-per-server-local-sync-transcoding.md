---
title: 'Per-server local sync transcoding'
type: 'feature'
created: '2026-10-20'
status: 'done'
baseline_commit: 'a3cf28b6db7e2665a3104efe1a05ab5107c0a679'
context: ['_bmad-output/implementation-artifacts/local-sync-transcoding-design.md']
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Sync currently delegates conversion to the media server, even when the computer running HifiMule has substantially more encoding capacity.

**Approach:** Add a persisted server setting choosing server encoding or local daemon encoding. Local mode downloads original audio, converts it through the integrated FFmpeg libraries when the device profile requires conversion, and transfers the completed output through the existing sync writer.

## Boundaries & Constraints

**Always:** Default to server encoding. Apply settings per source server, including mixed baskets and automatic sync. Honor device profiles, direct-play rules, bitrate overrides and rate/channel/depth ceilings. Resolve custom container/codec targets generically. Preserve metadata and supported artwork. Bound memory, temporary disk usage and encoding concurrency; support cancellation.

**Ask First:** Changes to the selected device profile's meaning, mandatory external FFmpeg installations, or licensing changes beyond the current LGPL-compatible distribution.

**Never:** Silently fall back to server encoding, substitute codecs, transfer failed conversions or change playback. Location changes alone must not replace compatible device files.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Existing setup | Setting absent after upgrade | Server encoding remains selected | Migration preserves credentials and identity |
| Local conversion | Original violates selected profile | Download original; encode to profile target; validate before writer admission | Report conversion failure using existing sync policy |
| Compatible source | Passthrough or source meets direct-play rules and limits | Download unchanged; no encoder work | Existing download failures apply |
| Custom profile | Profile-defined audio container/codec and constraints | Resolve aliases and eligible audio targets; honor limits | Unsupported or invalid targets/conditions produce actionable preflight errors |
| Mixed servers | Local and server settings in one basket | Each source uses its own captured setting | Selection changes cannot redirect an active sync |
| Interrupted work | Cancel, invalid media, disk exhaustion or encoder error | Stop worker; clean both temporary files; preserve committed state | No partial output enters device manifest |

</frozen-after-approval>

## Code Map

Daemon paths below are relative to `hifimule-daemon/` unless prefixed otherwise. `src/providers/mod.rs::TranscodeProfile` normalizes targets; `src/sync.rs::spawn_provider!` stages audio before the serialized writer. `src/transcoding.rs` loads arbitrary profile JSON. The existing server edit dialog is in `hifimule-ui/src/components/ServerHub.ts`.

## Tasks & Acceptance

**Execution:**
- [x] `src/db.rs`, `src/server_manager.rs`, `src/rpc.rs` — add validated `transcodingLocation: server | local`, migration/default, atomic updates and list/state serialization; preserve on reconnect.
- [x] `hifimule-ui/src/components/ServerHub.ts`, `hifimule-ui/src/rpc.ts`, `hifimule-i18n/catalog.json` — add localized selector explaining CPU/bandwidth tradeoff; preserve save/error behavior.
- [x] `src/transcoding.rs`, new `src/transcoding/local.rs` — resolve targets and implement native decode/resample/encode/mux, flushing, metadata, cancellation and probing. Cover shipped MP3/AAC/FLAC and custom ALAC, PCM/WAV, Vorbis/OGG and Opus; reject unsupported combinations.
- [x] `audio-runtime.json`, repository `scripts/{macos,linux,windows}-audio-runtime.mjs`, `scripts/verify-audio-runtime.mjs` — provision required encoders/muxers with pinned dependencies, native closure, notices and capability verification.
- [x] `src/rpc.rs`, `src/main.rs`, `src/sync.rs`, `src/sync_performance.rs`, `src/api.rs` — snapshot `config.json` tuning; decouple download, shared encoder pool and writer. Add `sync_performance.local_encoding_workers` (0=auto, 1–64=limit), preserving login/logout and per-field validation. Prevent thread oversubscription and staging deadlocks; retain reuse, budgets, tagging and recovery; follow companion design.
- [x] Test modules in affected daemon files, new `hifimule-ui/tests/serverTranscoding.test.mjs`, repository `scripts/tests/*-audio-runtime.test.mjs` — cover matrix, routing, real output properties, concurrent encoding from one server, overlapping download/encode/write, pool limits and cancellation.
- [x] Repository `docs/api-contracts-hifimule-daemon.md`, `docs/development-guide.md`, `docs/sync-performance.md` — document server setting, local JSON tuning and resource tradeoffs.

**Acceptance Criteria:**
- Given two configured servers, when one location changes and the daemon restarts, then only that server retains the new location and both RPC views agree.
- Given local mode and a conversion-required fixture, when manual or automatic sync runs, then the provider receives no server-transcoding request and the device receives verified profile-compatible audio.
- Given a cancelled or failed conversion, when sync settles, then no partial output is recorded and temporary resources are released.

## Spec Change Log

## Design Notes

Map management IDs to portable sync IDs at admission. Validate original responses separately from encoded outputs. Existing FFmpeg builds omit MP3 encoding; runtime expansion is required.

## Verification

- `rtk npm run build:daemon -- test -p hifimule-daemon` — regression and real encoding tests pass.
- `rtk cargo test -p hifimule-i18n` — translations remain complete.
- `rtk npm run build --prefix hifimule-ui` — TypeScript and production UI build pass.
- `rtk node --test hifimule-ui/tests/serverTranscoding.test.mjs` and targeted runtime tests — settings and packaging contracts pass.
- Inspect dialog; verify packaged codec coverage and report unavailable platform checks.


Implementation and verification are complete; see the companion design’s
2026-10-20 evidence and isolated synthetic benchmark. Status remains in-progress
until the parent workflow completes its independent step-04 review. Native
Windows/Linux/macOS x86-64 execution remains platform release validation.

## Suggested Review Order

**Sync routing and queue**

- Follow conversion admission through the bounded producer and existing serialized writer.
  [sync.rs:3406](../../hifimule-daemon/src/sync.rs#L3406)

- Join cancellable blocking workers before releasing originals or their staging reservations.
  [sync.rs:3008](../../hifimule-daemon/src/sync.rs#L3008)

- Handle missing lossless properties without fabricated maxima or reservation-growth deadlocks.
  [sync.rs:4050](../../hifimule-daemon/src/sync.rs#L4050)

**Native encoding**

- Separate observed source preferences from actual device ceilings.
  [transcoding/local.rs:25](../../hifimule-daemon/src/transcoding/local.rs#L25)

- Decode, resample, encode, mux and reopen actual output before device admission.
  [transcoding/local.rs:455](../../hifimule-daemon/src/transcoding/local.rs#L455)

- Preserve Ogg and Opus covers as standard picture comments.
  [transcoding/local.rs:379](../../hifimule-daemon/src/transcoding/local.rs#L379)

**Server configuration**

- Persist the server choice atomically while preserving reconnect behavior.
  [db.rs:898](../../hifimule-daemon/src/db.rs#L898)

- Validate settings and expose consistent RPC state.
  [rpc.rs:4044](../../hifimule-daemon/src/rpc.rs#L4044)

**Controlled runtime**

- Build pinned static codec dependencies for the private FFmpeg package.
  [encoding-dependencies.mjs:11](../../scripts/encoding-dependencies.mjs#L11)

- Verify actual encoder and muxer availability alongside ABI checks.
  [verify-audio-runtime.mjs:44](../../scripts/verify-audio-runtime.mjs#L44)

**Settings and verification**

- Expose encoding location in the existing server settings dialog.
  [ServerHub.ts:164](../../hifimule-ui/src/components/ServerHub.ts#L164)

- Parse local worker tuning with independent fallback in config.json.
  [sync_performance.rs:69](../../hifimule-daemon/src/sync_performance.rs#L69)

- Exercise required formats using real encoding and decoded output.
  [transcoding/local.rs:949](../../hifimule-daemon/src/transcoding/local.rs#L949)

- Check manual and Auto-Fill policies for original-fetch failures.
  [sync.rs:7832](../../hifimule-daemon/src/sync.rs#L7832)

- Document configuration, supported targets and operational tradeoffs.
  [sync-performance.md:85](../../docs/sync-performance.md#L85)
