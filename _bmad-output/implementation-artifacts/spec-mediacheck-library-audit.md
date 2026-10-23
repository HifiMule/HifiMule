---
title: 'Ship mediacheck source library audit with HifiMule'
type: 'feature'
created: '2026-10-23'
status: 'done'
baseline_commit: 'cadc7fe40a29a05a02bc7d1111447bbeccecc0d3'
context: ['_bmad-output/implementation-artifacts/investigations/local-mp3-duration-dmv-investigation.md']
---

<frozen-after-approval reason="human-owned intent — user requested a packaged source-only audit CLI">

## Intent

**Problem:** Users cannot audit a local library against HifiMule’s actual source decoding and recovery rules before sync. Some apparent encoding errors originate in corrupt/incomplete sources.

**Approach:** Ship console tool `mediacheck FOLDER [--log PATH]`. Recursively inspect supported source containers, fully decode without target conversion, record per-file results and progress, then summarize checked, ignored, recoverable and bad files. Share source machinery and controlled runtime with sync so future policy/configuration changes apply automatically.

## Boundaries & Constraints

**Always:** Read sources without modification, use HifiMule’s linked controlled FFmpeg and the same source-open/decode/error-recovery/MP3-tail/duration policy. Preserve existing sync behavior and limits. Derive scanned extensions from enabled runtime demuxers, documenting that unsupported codecs inside recognized containers are bad. Bound memory through streaming and serial checks. Log one escaped `OK` line for healthy files and full diagnostic chains/recovery details for others. Show console progress and final counts; recoverable is a subset of checked and distinct from bad. Package the actual console executable and native dependency closure in every supported installer. Support Windows UNC network shares and mapped drive roots using native paths; never require changing into the library directory. Keep prior investigation artifacts.

**Ask First:** Changing sync policy, expanding codecs, modifying media, adding a GUI/settings/daemon dependency to the CLI, publishing installers or remote Git actions.

**Never:** Choose an arbitrary encode target, create temporary transcoded copies for auditing, clone recovery rules into a second implementation, require system FFmpeg, alter current damaged-file handling, follow directory-symlink loops, silently overwrite a log/media file, or claim unavailable platform-native installer tests passed.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Behavior | Failure Handling |
| --- | --- | --- | --- |
| Healthy library | Nested mixed supported formats | Full source decoding, one OK line/file, summary | No media writes |
| Ignored types | Images/text/unsupported container extensions | No decode; ignored count | Skip symlink traversal |
| Recoverable source | Tolerated invalid sections, complete duration | Recovery details, recoverable count | Same limits as sync |
| Bad source | Open/read/decode failure, sustained damage, trusted-duration shortfall | Full path and diagnostics, bad count; continue other files | No policy weakening |
| Target independence | Any valid source representation | Check source only | No output profile/encoder required |
| CLI misuse | Missing/invalid folder or arguments | Usage and nonzero exit | No misleading successful audit |
| Filesystem/log failure | Unreadable source or traversal/log error | Diagnose failure/incomplete audit | Nonzero exit; never silently omit inaccessible subtrees |
| Log safety | Existing explicit output or generated default under scan root | Refuse overwrite; exclude active log | Preserve media/data |
| Windows shared folder | UNC path or mapped network drive, spaces/Unicode/nested folders | Native full decode and accounting work directly on the share | Inaccessible folders are incomplete audits; no silent omissions |
| Installed invocation | macOS/Linux/Windows installer | Load packaged runtime, show console help/audit | No private developer paths |

</frozen-after-approval>

## Code Map

- `hifimule-daemon/src/transcoding/local.rs` — existing source opening, decode loop, recovery, metadata-tail and duration machinery to extract; encoder/resampling/output logic stays.
- New `hifimule-daemon/src/source_audio.rs`, `src/lib.rs` — target-independent shared decoder/report/policy used by daemon and CLI.
- New `hifimule-daemon/src/bin/mediacheck.rs`, `Cargo.toml` — second console binary; keep daemon default run target.
- `hifimule-daemon/build.rs` — restrict daemon macOS Info.plist args to daemon binary.
- `scripts/prepare-sidecar.mjs`, `bundle-macos-libs.mjs`, platform `*-audio-runtime.mjs` — stage both executables and verify/rewrite both native closures.
- `hifimule-ui/src-tauri/tauri.conf.json`, `.github/workflows/release.yml` — package CLI as sidecar; installed smoke/signature validation.
- Existing generated audio fixtures, daemon/native tests and `scripts/tests/*runtime*.test.mjs` — behavioral and packaging regression coverage.

## Tasks & Acceptance

**Execution:**
- [x] Shared source module and local encoder — extract reusable machinery without changing behavior; retain allocation observations and test gates.
- [x] CLI/Cargo/build metadata — implement recursive audit, safe log/progress/summary/exit codes and help/version; use shared decoder and runtime extension discovery.
- [x] Build/installer/runtime scripts — build, stage, package and verify mediacheck on all platforms; retain existing daemon checks and common native libraries.
- [x] Rust and packaging tests — cover healthy/recoverable/bad sources, truncation, metadata exceptions, policy parity, mixed folders/counts/log safety, shipped CLI/runtime resolution and Windows UNC/mapped-drive audits against an actual temporary SMB share in CI.
- [x] `docs/mediacheck.md` and development guide — document invocation/installed paths, report semantics, ignored files, exit codes and source-only scope; record verification here.

**Acceptance Criteria:**
- Given nested healthy, recoverable, damaged and unrelated files, when mediacheck completes, then all files are accounted for correctly, diagnostics identify failures, and source hashes stay unchanged.
- Given current sync sources/tests, when the source extraction is used, then sync acceptance, recovery, cancellation and cleanup behavior remain intact.
- Given a future source-policy/runtime change, when the package is rebuilt, then both tools consume the shared implementation/configuration without a separate CLI policy update.
- Given installer preparation, when outputs are inspected or tested on the available host, then mediacheck is included with a loadable controlled runtime; remaining platform-native checks are clearly identified.

## Spec Change Log

- 2026-10-23: User explicitly added Windows shared-folder support. Preserve native UNC/mapped paths end-to-end, diagnose permissions errors, and add Windows SMB execution coverage.

## Design Notes

Use a minimal library surface within the existing daemon package, not a duplicated daemon or new service. A SourceDecoder owns input/decoder and streams frames to either the encoder callback or an audit sink. SourceReport validates trusted source duration for audits and completed output duration for encoding. Retain independent pre-resampling sample counts. Healthy excludes tolerated invalid events; recoverable still requires complete source validation. Exit 0 means no bad sources, 1 bad sources found, 2 usage/operational/incomplete-audit failure. Default timestamped log goes in the working directory; optional --log uses create-new semantics. No automatic PATH system changes are required; document packaged locations.

## Verification

- Controlled-runtime daemon/source/CLI tests, then full `rtk npm run build:daemon -- test -p hifimule-daemon` with localhost mocks permitted.
- `rtk node --test scripts/tests/*.test.mjs` — build/runtime/installer contracts pass.
- Build actual CLI; audit a temporary mixed folder and supplied damaged files read-only. Check log lines, progress, summary, exit and unchanged hashes.
- Verify the available macOS packaged binary/closure and smoke help; Windows/Linux native installer execution remains CI validation where unavailable locally.

### Recorded results

- 2026-10-23: Full controlled-runtime Rust suite passed: shared source 10; daemon 1664 (14 ignored); CLI parser 1; existing integration 5; CLI process integration 5. Windows-only junction coverage awaits Windows execution.
- Actual CLI audit of temporary copies of supplied DMV and Naked City files plus a healthy WAV and ignored text: checked 3, ignored 1, recoverable 0, bad 1, operational errors 0, exit 1. DMV accepted; Naked City rejected at byte 4763771 as sync does. Original SHA-256 values unchanged. The previously supplied Beastie Boys FLAC is no longer present at its supplied path; generated truncation regressions remain covered.
- Packaging/runtime Node suite: 356 tests, 355 passed, 0 failed, 1 existing Linux-native test skipped on macOS. Windows real SMB tests are wired into build and extracted-MSI release jobs; execution awaits Windows CI. Native macOS app/DMG build is being verified locally.
- Native macOS app and DMG build passed. Installed-bundle architecture/dependency verification and CLI help passed with developer loader paths removed. Packaged CLI audited supplied MP3 copies in a Unicode/spaced directory: checked 2, ignored 1, bad 1, exit 1; source hashes unchanged. CLI codesign verification passed.
- Changed Rust files pass rustfmt; repository-wide cargo fmt --check reports extensive pre-existing formatting differences outside this feature. Whitespace checks pass with existing CRLF handling.

### Review triage

- Patch: ffmpeg-next unwraps non-UTF-8 filenames. Shared source opening now returns an explicit per-file error before that call; added a portable boundary unit test and Linux actual-filename integration regression.
- Rejected: source-open/read failures being BAD contradict an assumed incomplete-audit policy; the approved matrix explicitly treats source open/read failures as bad and directory/traversal failures as operational.
- Rejected: replacing container duration with stream duration changes existing sync policy, which this feature must preserve. The shared validator intentionally retains current completeness rules.
- Patch: broad Windows reparse-point filtering also skipped cloud placeholders. Removed the redundant attribute check: Rust FileType::is_symlink already identifies name-surrogate tags (including junctions), leaving non-redirecting cloud directories traversable. Confirmed implementation in installed Rust standard-library source; existing Windows junction integration remains the native regression.
- Post-review regressions passed: shared source 11, CLI process tests 5, native encoder tests 15. Linux non-UTF-8 actual-filename regression is platform-gated for CI.

- Final CLI integration regressions passed (5); read-only DMG mount confirmed the included mediacheck loads its bundled runtime and displays console help. Windows SMB/junction and Linux-specific execution remain CI checks.

## Suggested Review Order

**Source audit entry point**

- Run a recursive, read-only audit with safe reports and explicit exit status.
  [mediacheck.rs:320](../../hifimule-daemon/src/bin/mediacheck.rs#L320)

- Share source decoding and damage limits with sync.
  [source_audio.rs:71](../../hifimule-daemon/src/source_audio.rs#L71)

- Keep target encoding separate while consuming the shared source machinery.
  [local.rs:489](../../hifimule-daemon/src/transcoding/local.rs#L489)

**Installer and Windows support**

- Stage both executables with the controlled audio runtime.
  [prepare-sidecar.mjs:60](../../scripts/prepare-sidecar.mjs#L60)

- Verify the packaged console executable and native dependency closure.
  [macos-audio-runtime.mjs:143](../../scripts/macos-audio-runtime.mjs#L143)

- Validate both installed ELF executables and their runtime paths.
  [linux-audio-runtime.mjs:294](../../scripts/linux-audio-runtime.mjs#L294)

- Require the Windows console subsystem and colocated audio libraries.
  [windows-audio-runtime.mjs:371](../../scripts/windows-audio-runtime.mjs#L371)

- Exercise native decoding through UNC and mapped SMB paths.
  [check-mediacheck-windows-shares.ps1:19](../../scripts/smoke-tests/check-mediacheck-windows-shares.ps1#L19)

**Tests and usage**

- Check real CLI results, recovery, diagnostics, and unchanged media.
  [mediacheck_cli.rs:73](../../hifimule-daemon/tests/mediacheck_cli.rs#L73)

- Guard installer and Windows SMB workflow integration.
  [mediacheck-packaging.test.mjs:9](../../scripts/tests/mediacheck-packaging.test.mjs#L9)

- Document installed commands, Windows shares, reports, and limits.
  [mediacheck.md:1](../../docs/mediacheck.md#L1)
