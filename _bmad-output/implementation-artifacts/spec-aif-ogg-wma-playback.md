---
title: 'Add AIF, OGG, and WMA playback'
type: 'feature'
created: '2026-09-14'
status: 'done'
baseline_commit: 'c542e5b1b84b9d65c1fcbcb6ce12f799f7a5a81d'
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** HifiMule rejects AIF, OGG, and WMA representation labels before decoding. Its controlled FFmpeg build also omits AIFF/ASF demuxers and several required audio decoders, so changing the provider filter alone cannot deliver playback across packaged platforms.

**Approach:** Extend the existing native playback pipeline to accept these formats, enable their native FFmpeg components, and verify decoding through the production bounded reader. Treat AIF as AIFF PCM, OGG as Vorbis or Opus audio, and WMA as unencrypted Windows Media Audio in ASF.

## Boundaries & Constraints

**Always:** Preserve original-first representation selection, existing supported formats, provider authentication, typed playback failures, sanitized diagnostics, cancellation, and the 8 MiB compressed-memory bound. Keep FFmpeg on its worker and use the controlled runtime/build wrapper. Support `.aif`/`.aiff`, `.ogg`/`.oga`, and `.wma` plus their corresponding codec labels, case-insensitively. Enable native WMA v1/v2, Pro, and Lossless decoders; document which variants have fixture evidence.

**Ask First:** Changes to the pinned FFmpeg release or dependency versions, external codec libraries, or an architectural change to streaming/storage beyond repairing compatibility within the existing reader contract.

**Never:** Add sync/transcoding profiles, change provider endpoints, invoke an FFmpeg subprocess for production playback, add DRM decryption, or claim platform/variant validation that was not performed.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| AIFF | AIF/AIFF PCM, including big-endian 16/24/32-bit samples | Original selected and converted into non-empty finite PCM | Existing typed decode failure for malformed content |
| OGG | OGG/OGA Vorbis or Opus audio | Original selected and decoded completely | Unsupported/corrupt streams fail through existing typed path |
| WMA | Unencrypted ASF containing supported WMA audio | Original selected and decoded | DRM or malformed content fails without bypasses |
| Label variants | Uppercase suffixes and explicit codec labels | Same eligibility as lowercase counterparts | Unrelated unsupported codec remains rejected |
| Streaming/resume | New-format media larger than retained window; nonzero resume position | Complete decode with bounded compressed memory; resume produces expected remaining audio | Reader failures retain existing diagnostics |
| Diagnostics | New supported representation or arbitrary private label | Safe format label retained; unknown text redacted | No URLs or credentials exposed |

</frozen-after-approval>

## Code Map

- `hifimule-daemon/src/providers/mod.rs` -- representation eligibility, ranking, and selection regressions; both providers currently populate codec and container from song suffix.
- `hifimule-daemon/audio-runtime.json` -- authoritative controlled FFmpeg configuration; currently enables OGG demuxing and Opus, but omits AIFF, ASF, Vorbis, and WMA components.
- `hifimule-daemon/src/playback/decoder.rs` -- generic native decode pipeline and fixture helpers for frame counts, resume, and compressed high-water measurements.
- `hifimule-daemon/src/playback/audio.rs` -- decoder filename hints and privacy-preserving diagnostic format allowlist.
- `hifimule-daemon/tests/fixtures/` -- committed synthetic audio and source/provenance notes, following the existing generated FLAC example.
- `scripts/build-daemon.mjs` -- required test/build entry point; Windows/Linux runtime receipts include configure flags and therefore invalidate after configuration changes.
- `docs/playback-installed-test-checklist.md` -- installed-runtime playback validation guidance.

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-daemon/src/providers/mod.rs` -- add format/codec eligibility and table-driven regressions for aliases, uppercase labels, original preference, and continued unsupported rejection. Include big-endian PCM in lossless alternative ranking to preserve the existing quality policy.
- [x] `hifimule-daemon/audio-runtime.json` -- enable AIFF and ASF demuxers, big-endian PCM decoders required by AIFF, Vorbis, and WMA v1/v2/Pro/Lossless native decoders. Retain pinned versions and licensing constraints.
- [x] `hifimule-daemon/tests/fixtures/` -- add small deterministic synthetic fixtures and generation/provenance instructions for AIFF PCM, OGG Vorbis/Opus, and WMA v1/v2. Record any Pro/Lossless fixture gap explicitly rather than implying complete evidence.
- [x] `hifimule-daemon/src/playback/decoder.rs` -- extend production decoder coverage to the new fixtures, checking finite/nonempty PCM, complete duration, and resume. Exercise greater-than-window streaming cases using the existing helpers; repair format handling only if reproduction shows it necessary.
- [x] `hifimule-daemon/src/playback/audio.rs` -- extend diagnostic safe labels and test decoder hints for new aliases while preserving unknown-value redaction.
- [x] `docs/playback-installed-test-checklist.md` -- add the new format matrix and distinguish local automated evidence from pending packaged-platform and WMA variant verification.

**Acceptance Criteria:**
- Given a provider track in a newly supported format, when playback resolves its representations, then the original survives selection and reaches the native decoder.
- Given generated new-format fixtures, when decoded through the production reader, then their complete expected audio and resumed audio are produced within existing memory limits.
- Given the controlled build configuration, when Windows/Linux runtimes are rebuilt, then the requested demuxers/decoders are included without upgrading FFmpeg or adding external codec dependencies.
- Given existing playback tests, when the daemon suite runs after the change, then existing supported formats, selection priorities, and error/privacy behavior remain passing.

## Spec Change Log

## Design Notes

Eligibility and runtime capability must change together. The decoder already discovers the audio stream generically; avoid extension-specific decode branches unless fixture evidence requires them. macOS uses an installed FFmpeg with verified ABI, so local success alone does not prove the reduced Windows/Linux runtime supports the new formats. Preserve this distinction in verification reporting.

## Verification

**Commands:**
- `rtk node scripts/build-daemon.mjs test -p hifimule-daemon providers::tests` -- selection regressions pass.
- `rtk node scripts/build-daemon.mjs test -p hifimule-daemon playback::` -- decoder fixtures, resume, bounds, and diagnostics pass.
- `rtk node scripts/build-daemon.mjs test -p hifimule-daemon` -- full daemon suite passes; report environment-gated exclusions.
- `rtk proxy node --test scripts/tests/windows-audio-runtime.test.mjs scripts/tests/linux-audio-runtime.test.mjs` -- runtime build/cache contract tests pass.
- `rtk cargo fmt --all -- --check` -- Rust formatting passes.

Inspect available runtime codec capabilities and record exact local evidence. Packaged Windows/Linux rebuild and physical playback checks remain explicitly unverified if those environments are unavailable.


### Implementation evidence — 2026-09-14

- Extended provider eligibility for AIF/AIFF, OGG/OGA, Vorbis, WMA codec labels,
  and big-endian PCM, preserving original preference and lossless quality ranking.
- Enabled `aiff,asf` demuxers and `pcm_s16be,pcm_s24be,pcm_s32be,vorbis,wmav1,wmav2,wmapro,wmalossless`
  decoders in the authoritative runtime manifest. Pinned versions, network policy,
  and license configuration are unchanged.
- Added seven repository fixtures with synthetic-source/CC0 provenance and exact
  generation commands. AIFF 16/24/32, Vorbis, and Opus produce exactly 96,000 frames;
  WMA v1/v2 produce 94,208 frames from the two-second source (within one codec block).
  Tests verify finite PCM and exact subtraction of a 48,000-frame resume offset.
- The seven formats also decode completely and resume with temporary metadata
  larger than the 8 MiB retained window, while compressed high-water stays within
  the existing bound. No production reader or decoder change was necessary.
- Decoder filename hints normalize new suffixes; diagnostics preserve recognized
  format/codec labels and continue redacting arbitrary private labels.

Verification results:

- Full daemon suite via `rtk node scripts/build-daemon.mjs test -p hifimule-daemon -- --quiet`:
  **739 passed, 0 failed, 5 ignored**. The ignored tests are pre-existing external
  diagnostic-media tests. Initial sandbox run could not bind mock HTTP sockets;
  approved escalation resolved this environment failure.
- Provider suite: **27 passed**. Playback suite: **66 passed, 5 ignored**.
- Windows/Linux runtime script contracts: **33 passed**.
- `rtk cargo fmt --all -- --check` and `rtk git diff --check`: passed.
- Local FFmpeg 9.0.1 on macOS ARM64 reports `aiff`, `asf`, and `ogg` demuxers;
  native Opus, Vorbis, big-endian PCM 16/24/32, WMA v1/v2/Pro/Lossless decoders.
  Build wrapper verified ABI versions: avcodec 63.1.101, avformat 63.1.101,
  avutil 61.1.101, swresample 7.1.101.

Remaining external verification: WMA Pro/Lossless fixture playback, controlled
Windows/Linux runtime rebuilds, and installed physical playback on every target.
The checklist explicitly records these as pending; no cross-platform success is
inferred from local Homebrew FFmpeg. No dependency versions or frozen intent changed.

## Review Results

Three independent reviewers completed blind adversarial, edge-case, and acceptance reviews. Two deduplicated patch findings were fixed: accept ASF suffix labels consistently with the enabled demuxer, and classify WMA Lossless alternatives as lossless. Regression tests cover lowercase/uppercase original selection and lossless quality ranking; both passed after the patch. No intent or spec changes were needed.

The documented WMA Pro/Lossless fixture and installed-platform gaps remain verification limits. The metadata-expansion tests establish bounded-reader metadata eviction, not exhaustive long-track or channel-layout certification.

## Suggested Review Order

**Playback selection**

- Accept provider format labels and preserve original-first and lossless quality selection.
  [mod.rs:85](../../hifimule-daemon/src/providers/mod.rs#L85)

**Decoder verification**

- Verify complete decoding, resume, finite PCM, and bounded-reader metadata eviction.
  [decoder.rs:455](../../hifimule-daemon/src/playback/decoder.rs#L455)

**Safe diagnostics**

- Retain new format labels while preserving unknown-value redaction.
  [audio.rs:327](../../hifimule-daemon/src/playback/audio.rs#L327)

**Runtime and evidence**

- Enable native demuxers and decoders without changing the pinned FFmpeg release.
  [audio-runtime.json:13](../../hifimule-daemon/audio-runtime.json#L13)

- Reproduce synthetic fixtures and see variant-specific evidence limits.
  [generated-audio.source.md:1](../../hifimule-daemon/tests/fixtures/generated-audio.source.md#L1)

- Keep local evidence separate from installed-platform verification.
  [playback-installed-test-checklist.md:147](../../docs/playback-installed-test-checklist.md#L147)

