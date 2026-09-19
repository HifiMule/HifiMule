# Story 15.17: Ship verified playback builds for Windows, macOS and Linux

Status: ready-for-dev

## Story

As a HifiMule user,
I want the installed application to provide the tested playback behavior on my supported platform,
so that listening works without a development environment or manually installed decoder libraries.

## Acceptance Criteria

1. **Controlled runtime in every shipping artifact.** Given the four shipping targets enumerated below, when release artifacts are built, each artifact contains the controlled playback runtime and records the exact FFmpeg libraries, Rust audio bindings, backend, source/configuration hash, licenses and installed load paths. A development-machine, Homebrew, distro or other system FFmpeg must not silently satisfy this criterion.
2. **Clean installed playback.** Given a clean supported environment without development tools or a manually installed FFmpeg, when the matching package is installed and supported Jellyfin and OpenSubsonic/Subsonic media are played, streaming, decoding and shared output work without elevation. Package permissions, signing and documented platform limitations match the existing distribution model.
3. **Safe upgrade and migration.** Given an existing HifiMule installation with server credentials, device profiles/manifests, basket/sync settings and a persisted playback session, when it is upgraded to the playback build, supported configuration and session migrations preserve those values. An interrupted or failed migration leaves recoverable prior data and never reports erased device state as success. Downgrade limitations are documented without claiming automatic rollback safety.
4. **Installed lifecycle and native integration.** On every shipping target, UI close/reopen, simultaneous launch, native transport, output loss/reconnection, sleep/wake, paused restoration and Quit during active sync verify the production daemon lifetime, one-player ownership, output safety and managed-device cancellation contract. Native API delivery and physical media-key routing are separate evidence entries.
5. **Integrated manual playback and provider routing.** On configured representative providers and metadata, installed tests cover selected-track and ordered-album playback, continuity/gain, manual queue edits, Preview/Return, Back/Next/seek, output selection, the always-available Playback destination, floating bar, compact browse navigation and existing physical-device sync. Source identity remains independent of the browsed server; unsupported capabilities remain unavailable. Manual idle browsing/Resume works and no unimplemented Radio action is advertised.
6. **Continuity, bounded resources, recovery and real-sync coexistence.** Release evidence links deterministic decoder continuity, physical-output continuity, configured buffer high-water values, process memory, worker cleanup, recovery and ordinary playback-plus-real-sync measurements to an exact artifact, source revision, runtime and environment. A changed packaged dependency invalidates affected historical evidence. No monotonic resource growth, obsolete audio publication or false successful sync completion is accepted.
7. **Accessible installed UI.** Across supported themes, widths, divider extremes, text scaling and keyboard use, the floating bar including Back, compact browse-mode bar, Playback queue, Preview and error/recovery states retain accessible names, visible focus, adequate contrast and reachable final rows. Background updates do not steal focus or announce elapsed-time ticks. Screen-reader/native observations are distinguished from DOM-only checks.
8. **Truthful failures and certification boundaries.** Any untested or failed target, architecture, provider or hardware interaction remains an explicit release blocker or an accurately scoped unsupported capability with owner and rationale. ARM64/VM evidence never certifies x64; native API injection never certifies physical keys; callback/sample counters never certify audible physical continuity. Existing Linux teardown warnings are resolved or receive a reproducible, reviewed disposition.
9. **Auditable per-row release decision.** Each shipping row links artifact filename and SHA-256, source revision, OS/architecture, install/upgrade environment, loaded native versions/paths, automated and manual outcomes, limitations and final decision. The aggregate decision fails if any required row fails or is missing; no global pass may hide row failures. The workflow may create a draft release, but publishing remains outside this story.
10. **Inherited release gates are reconciled.** Story 15.12 R16 and Story 15.14 R8/R9 are fixed with regression tests before acceptance. All applicable installed/full-application checks and recorded defects from Stories 15.1–15.16 are inventoried; blockers are resolved or explicitly dispositioned with evidence. Historical unverified rows remain unverified, and stale status prose is corrected without rewriting old results as passes.

## Tasks / Subtasks

- [ ] Freeze the release contract before changing packages or evidence (AC: 1, 2, 8, 9).
  - [ ] Record the actual matrix: Windows x64, Linux x64, macOS x64 and macOS ARM64. Do not add Windows/Linux ARM claims. Explicitly configure Windows to produce MSI and NSIS, Linux to produce only deb and AppImage (not Tauri's broader `all`, which also includes RPM), and macOS to produce separate x64/ARM64 app payloads and DMGs.
  - [ ] Use release candidate `0.15.0` (the first unused minor after shipped `v0.14.0`) consistently across Cargo, Tauri and artifact metadata unless Alexis approves a different version before implementation. Treat `v0.14.0` as the mandatory in-place upgrade baseline on the same host; older direct upgrades are unsupported unless separately tested, and machine-bound credentials are not cross-host migration evidence.
  - [ ] Define supported OS floors: Windows 10 and 11 x64 desktop; Ubuntu 22.04 x64; macOS 10.15+ x64 and macOS 11+ ARM64. Run install/launch/playback at the declared floor and a current environment, or raise/narrow the advertised floor in configuration and documentation with evidence. A mutable `*-latest`/Server runner is build evidence, not a desktop-floor test.
  - [ ] Define clean-install and upgrade fixtures, supported provider/version/capability combinations, physical versus virtual output expectations, evidence owner and blocker/disposition vocabulary.
  - [ ] Add a non-publishing candidate path (`workflow_dispatch` or equivalent) that builds from an explicit commit/ref and version, uploads immutable candidate artifacts, and hands them to smoke/evidence jobs without a pushed `v*` tag. Keep release creation draft-only; do not tag, push or publish as part of this story.
  - [ ] Reconcile `docs/release-guide.md` with the current four-job workflow; remove the stale three-job/universal-macOS description and accurately document the draft/smoke/manual gates.
- [ ] Close the controlled-runtime, licensing and bundle-verification gates (AC: 1, 2, 6, 8, 9).
  - [ ] Decide the FFmpeg release deliberately. The repository pins 9.0.1, while official FFmpeg 9.0.2 was released 2026-09-18. Either retain 9.0.1 with an evidence-backed security/compatibility decision or update the source URL/signature/hash/ABI/receipts/build constant/notices together and rerun every affected package/runtime/playback check. Never accept an incidental package-manager upgrade.
  - [ ] Reconcile `audio-runtime.json` ABI declarations against the actual source-built and loaded libraries. Remove the non-shipping Linux ARM verification row and replace pending labels only with real evidence.
  - [ ] Fix `THIRD_PARTY_AUDIO_NOTICES.md`: it currently says CPAL 0.16.0 while code pins the local CPAL 0.18.2 patch. Verify FFmpeg LGPL configuration/source offer, Rust crate notices and packaged notice presence.
  - [ ] Make macOS use and prove a controlled runtime rather than accepting an arbitrary Homebrew FFmpeg. Preserve per-architecture builds, rewrite private dylib load paths, sign the full closure and verify no Homebrew/developer path remains.
  - [ ] Add equivalent post-bundle audio-runtime/architecture/load-path checks for Windows and macOS to the existing Linux bundle checks. Fail on missing libraries, wrong architecture/ABI, system paths, incomplete transitive closure or mismatched manifest/receipt.
  - [ ] Treat distribution trust as a shipping gate: Windows MSI/NSIS require valid Authenticode signing; macOS app/DMG require Developer ID signing and notarization for direct distribution. Missing credentials/signatures/notarization block the row rather than becoming an optional limitation. Linux packages require recorded SHA-256 and an accurate package-origin/install policy.
  - [ ] Pin CI Rust to workspace MSRV/toolchain 1.93.0 unless a reviewed reproducibility decision changes both together. Preserve Tauri sidecar naming and platform-specific resource placement.
- [ ] Resolve inherited release-blocking defects, then retain the fixes in the release matrix (AC: 5, 7, 10).
  - [ ] **15.12 R16:** add one shared physical-target admission API for user mutations (`add`, `remove`, `toggle`, `clear`) and apply it through `library.ts`, `MediaCard.ts`, `TracksBrowseView.ts`, `BasketSidebar.ts` and `state/basket.ts`. Do not block trusted reconciliation paths such as daemon hydration, device cleanup or server-removal cleanup. Preserve browsing, Play and Preview with no physical device and preserve cross-server read-only behavior.
  - [ ] **15.14 R8:** when a seek conflicts or its observed identity/revision is stale, discard queued seeks and refresh authoritative state before allowing a new user action; never replay queued work from the stale snapshot.
  - [ ] **15.14 R9:** scope or clear command errors when session/occurrence/generation identity changes while preserving actionable same-identity failures.
  - [ ] Add controlled store/DOM/promise regressions for all three defects and update `deferred-work.md` only after the fixes and tests exist.
- [ ] Extend evidence tooling from feature probes to a release-decision schema (AC: 3–10).
  - [ ] Version `scripts/playback-installed-evidence.py` rather than weakening older evidence rules. Add artifact/signing/license/permission facts, clean-install and upgrade/migration outcomes, daemon lifecycle/safe-Quit scenarios, Preview/queue/Back/compact-browse/UI accessibility, real-sync coexistence and explicit blocker/limitation/disposition fields.
  - [ ] Use a stable evidence identity containing target OS/architecture + package format + artifact SHA-256 + provider/version/capability set + install/upgrade environment. Require clean-install/upgrade for every end-user installer (MSI, NSIS, deb, each DMG) and clean-launch for AppImage; extracted app/bundle inspection is supplemental and cannot hide a failed installer row.
  - [ ] Sanitize all output: never persist owner tokens, credentials, request headers, authenticated URLs, raw endpoint identifiers, provider error bodies or home/profile paths.
  - [ ] Make validation fail for a missing row, failed required scenario, absent artifact/runtime identity, stale/incompatible evidence, unapproved limitation or aggregate/pass disagreement.
  - [ ] Store the release manifest and target records under the existing `docs/playback-evidence/` convention. Do not fabricate result files; unavailable machines/hardware produce explicit blocker records, not passes.
  - [ ] Retain physical continuity captures and other material binary evidence in immutable release/CI artifact storage with SHA-256, URI, capture metadata and retention policy. A disappearing machine-local `rawCapturePath` is not auditable evidence.
- [ ] Build and inspect every configured artifact without publishing it (AC: 1, 2, 8, 9).
  - [ ] Verify artifact names, hashes, target architecture, sidecar placement, runtime manifest/notices and private library closure.
  - [ ] Linux: verify both AppImage and deb extraction, ELF architecture/RUNPATH and private FFmpeg/Pulse/libmtp closure.
  - [ ] Windows: verify MSI/NSIS outputs actually configured for shipping, DLL/import-runtime architecture and installed load paths under the application root.
  - [ ] macOS: verify each architecture separately, minimum-system-version policy, dylib closure/load commands, sealed resources and current signing/notarization limitation. Ad-hoc signing must not be described as notarization.
  - [ ] Preserve draft-only release behavior and make partial/missing artifacts visible rather than allowing whichever matrix job finishes last to imply completeness.
- [ ] Run the clean-install and upgrade matrix on real target environments (AC: 2–5, 8, 9).
  - [ ] Install without build tools/system FFmpeg; capture actual loaded modules and `daemon.health.audioRuntime`, then exercise every supported format/provider combination including AIF/AIFF, OGG/OGA and WMA coverage already marked pending.
  - [ ] Upgrade a fixture with real server/device configuration and a paused session; verify credentials, profiles, manifests, baskets and playback state. Inject or rehearse interrupted migration and confirm prior state remains recoverable.
  - [ ] Exercise UI close/reopen, duplicate launches, sleep/wake, selected-output unplug/replug, explicit replacement, native controls with UI open/closed, physical media keys, paused restore and Quit while playback plus a real managed-device sync are active.
  - [ ] Confirm Quit stops audio, checkpoints playback and requests orderly sync cancellation without marking incomplete writes successful; relaunch stays paused.
- [ ] Run the integrated manual-playback regression workload (AC: 4–6, 8–10).
  - [ ] Test track/album playback, unavailable-source retry, ordered multi-disc queues, prepared boundaries, common album gain/peak protection, Preview replacement/natural return/Stop/failed return, queue append/reorder/remove, Back at 0/3000/3001 ms, Next, seek conflicts and output-loss inhibition.
  - [ ] Verify provider/source routing while browsing another server; manual idle/Resume remains useful and no Radio/reporting/export/adaptation UI or claim leaks into this release.
  - [ ] Run ordinary playback plus a real sync. Record underruns/timeouts, sync outcome, high-water counters, process RSS/native heap and worker cleanup. Conditional sync backoff and adaptive quality are out of scope and must not be claimed.
  - [ ] Enforce current configured bounds per active source and in aggregate: 8 MiB compressed/16 MiB aggregate, 1 MiB network chunk, 500 ms PCM target, 1 MiB PCM/2 MiB aggregate, two source slots, 100 ms startup fill and 60 s preparation deadline.
  - [ ] Measure a 30-minute representative playback-plus-sync workload after a 5-minute warm-up, sampling process RSS/native heap and normalized process CPU at least every 5 seconds. Per target, active RSS p95 must be no more than idle p95 + 128 MiB and retained RSS growth over the final 20 minutes no more than 16 MiB. For the same fixture, p95 active RSS delta and normalized p95 CPU delta between OS rows must each remain within the inherited 15% cross-OS requirement; otherwise block or formally revise NFR10 with evidence. Keep the idle <10 MB goal separate from active playback.
  - [ ] Run deterministic sample fixtures and separate physical-output capture/listening checks. Preserve recorded silence; do not replace physical evidence with counters.
- [ ] Verify the installed UI and Story 15.16 carry-forward contract (AC: 5, 7, 8, 10).
  - [ ] Exercise Library/Playing/device views, current/upcoming/history, nested and virtualized rows, real provider switching and a physical basket while main and Preview playback are active.
  - [ ] Cover supported themes, 599/800/1000/1280 px probes plus actual 900×640 minimum-window behavior, divider extremes, 200% OS text scaling/text zoom, EN/FR/ES/DE labels, keyboard operation and screen-reader announcements.
  - [ ] Preserve 15.16 stable browse-mode nodes/listeners/focus, provider order, capability filtering, loading/selected state, Grid/List rules, no-refetch navigation and zero playback/source/basket side effects.
  - [ ] Verify bottom-row focus/actions remain reachable above the two-row playback bar; background time updates do not steal focus or flood live regions.
- [ ] Reconcile evidence and make the release decision (AC: 6, 8–10).
  - [ ] Add Story 15.15, 15.16 and 15.17 sections to `docs/playback-installed-test-checklist.md`; retain historical observations and explicit unverified rows.
  - [ ] Inventory all open playback items in `deferred-work.md`. Resolve release blockers or record an evidence-backed, narrowly scoped unsupported capability with owner; do not silently waive a failed acceptance criterion.
  - [ ] Produce one per-row decision and one mechanically derived aggregate decision. A required missing/failed row blocks acceptance.
  - [ ] Record material limitations and future Epic 16 ownership without claiming Radio, reporting/preferences, snapshots/exports, adaptive quality, conditional backoff or full Radio soak.

## Dev Notes

### Scope, value and precedence

- This is the final Epic 15 manual-playback release gate. It turns working source-tree behavior into reproducible installed packages and truthful, linked evidence. Epic 16 is not a prerequisite.
- Story 15.17 may fix defects required to satisfy inherited Epic 15 acceptance, but it must not implement deferred Radio/Recommendations work. Story 16.14 reuses this packaging process and supplies fresh evidence for later features.
- The current repository, `epics.md`, playback PRD/architecture amendments and this story override stale historical prose. `project-context.md` remains useful for provider abstraction, managed-sync ownership and credential boundaries, but its “greenfield” phase is obsolete.
- A validator or workflow improvement is not itself platform evidence. If target machines, real outputs, media keys or devices are unavailable, leave blockers explicit and do not mark the story done.

### Shipping matrix and artifact truth

| Release row | Current CI runner/target | Configured package expectation | Certification boundary |
| --- | --- | --- | --- |
| Windows x64 | `windows-latest`, host `x86_64-pc-windows-msvc` | Tauri Windows installers; inventory MSI and NSIS and declare which ship | No Windows ARM claim; API-injected controls are not physical keys |
| Linux x64 | `ubuntu-22.04`, host x86_64 GNU | deb and AppImage only; restrict Tauri `all` so RPM is not emitted unverified | Xvfb/lifecycle smoke is not physical Pulse/output/media-key evidence |
| macOS x64 | `macos-15-intel`, `x86_64-apple-darwin` | x64 app/DMG | Separate from ARM64; ad-hoc signing is not notarization |
| macOS ARM64 | `macos-15`, `aarch64-apple-darwin` | ARM64 app/DMG | Does not certify macOS x64 or another OS architecture |

The evidence tool already names these four targets. Align the release guide, release workflow, smoke workflow and evidence manifest to one matrix source or add a parity test so they cannot drift. Evidence aggregates by artifact, not merely by target: no package format may inherit another format's clean-install result.

Supported floors are Windows 10/11 x64 desktop, Ubuntu 22.04 x64, macOS 10.15+ x64 and macOS 11+ ARM64. CI runners may build newer systems, but floor compatibility needs a real/virtual installed environment with the correct architecture and desktop/audio stack. If a floor cannot be tested or a dependency no longer supports it, update the advertised support/configuration before acceptance.

### Current runtime and version contract

- Workspace: Rust 1.93.0, edition 2024; Tokio ~1.49; locked Tauri CLI/API 2.10.1; locked TypeScript 5.6.3; locked Vite 6.4.1; locked Shoelace 2.20.1. Record lockfile hashes/resolved versions in candidate evidence; package ranges alone are not a release identity.
- Audio bindings: local patched CPAL 0.18.2, `ffmpeg-next`/`ffmpeg-sys-next` 9.0.0, Souvlaki 0.8.3, Linux `libpulse-binding` 2.30.1. Do not upgrade as incidental release work.
- `audio-runtime.json` currently pins FFmpeg 9.0.1 from signed official source. Official FFmpeg now lists 9.0.2 as the latest 9.0 point release (2026-09-18). Treat this as a release decision, not an automatic upgrade. The FFmpeg project describes point releases as distributor/integrator bug-fix releases; check release/security changes against the enabled reduced decoder/demuxer set.
- `THIRD_PARTY_AUDIO_NOTICES.md` is stale about CPAL and must agree with the lockfile/runtime. Verify actual LGPL-compatible FFmpeg flags and include the manifest/notices in every package.
- Tauri `externalBin` uses target-triple-specific sidecars. Preserve that convention and verify every target rather than renaming/copying opportunistically.

### Existing files: current state, required change and preservation

| File / area | Current state | Required Story 15.17 work and preservation |
| --- | --- | --- |
| `.github/workflows/release.yml` | Four build targets triggered only by pushed `v*`; draft release; Linux private-runtime checks; macOS libmtp/path/ad-hoc-sign checks; Rust `stable`. | Add non-publishing candidate dispatch/artifact handoff, deterministic toolchain and complete audio-runtime/artifact/evidence checks. Preserve draft-only publication boundary and fail-fast-per-row visibility. |
| `.github/workflows/build.yml` | Four-target source-build/session evidence, not installed packages. | Keep source evidence labeled as such; run updated schema/unit regressions and avoid promoting it to installed acceptance. |
| `.github/workflows/smoke-test.yml`, `scripts/smoke-tests/*` | Install/start/auth/lifecycle smoke; limited manual-device facts. | Extend or complement with release evidence orchestration; do not pretend health checks play audio or test upgrade/output/media keys. |
| `hifimule-daemon/audio-runtime.json`, `build.rs`, `THIRD_PARTY_AUDIO_NOTICES.md` | Pinned runtime/buffer policy; pending verification labels; stale CPAL notice. | Freeze one coherent release/runtime/license contract and verify loaded values. Preserve reduced-codec and no-system-fallback intent. |
| `scripts/verify-audio-runtime.mjs`, platform runtime/bundle scripts, `prepare-sidecar.mjs` | Windows/Linux controlled source builds; macOS Homebrew-derived closure; Linux installed-closure verifier. | Close macOS control gap, add Windows/macOS installed bundle verification, keep architecture/ABI/hash/receipt validation and add regression tests. |
| `hifimule-ui/src-tauri/tauri*.conf.json` | Sidecar plus runtime manifest/notices; `targets: all`; platform bundled-libs destinations; macOS 10.15/ad-hoc policy. | Restrict platform artifacts to the declared matrix, preserve sidecar ownership, and require real Windows signing plus macOS Developer ID/notarization for shipping candidates. |
| `scripts/playback-installed-evidence.py` and tests | Sanitized four-target validator for earlier playback stories; one record per target and incomplete release-level schema. | Version/extend for target+provider/scenario evidence, upgrades, UI/lifecycle/sync and release aggregation. Preserve privacy rejection and strict non-inference. |
| `docs/playback-installed-test-checklist.md`, `docs/playback-evidence/` | Extensive historical evidence and many explicit open rows; no 15.15–15.17 release closeout. | Append auditable release matrix and decisions; never rewrite old unverified evidence as passes. |
| `docs/release-guide.md` | Stale three-job/universal-macOS description and health-only smoke path. | Match current four-target/separate-mac workflow and manual playback acceptance gates. |
| `hifimule-ui/src/state/basket.ts`, `components/TracksBrowseView.ts` and all basket entry points | Server/CSS checks can allow mutation without a physical target (R16). | Centralize physical-target authorization while preserving library/Play/Preview and server ownership rules. |
| `hifimule-ui/src/components/PlaybackControls.ts` | Queued seek may drain after conflict; error may survive identity change (R8/R9). | Cancel stale queued work and scope errors to the observed playback identity. Preserve valid same-identity retry/error UX. |

### Regression and safety guardrails

- Daemon owns playback, queue and generation state. UI/native controls use the same serialized commands. Never add a second player or infer success from UI state alone.
- Credentials and authenticated stream URLs stay daemon-side and must not enter logs/evidence. Route every track through portable server ID plus provider track identity, independent of selected browsing server.
- Output loss pauses and inhibits resume; reconnect/default changes never silently reroute or start audio. Shared output remains the default; no ASIO/exclusive-mode work.
- Preserve safe device sync cancellation, manifests and atomic writes. Playback-plus-sync evidence cannot relax managed-device integrity.
- Preserve bounded compressed/PCM queues and two active source slots; callbacks perform no blocking IO, allocation, persistence or sync-lock acquisition.
- Preserve recorded silence and album-common gain. Do not claim adaptive quality, mid-track replacement, conditional backoff or reporting.
- Keep Back semantics: main position >3000 ms restarts current; <=3000 ms selects previous or restarts current without wrap; Preview restarts audition only. Preserve paused intent, occurrence identity and durable outcomes.
- Keep the compact mode bar's current implementation (quiet controls, cyan treatment and current icon mapping) rather than reverting to the earlier planning prototype. Installed checks validate production code.

### Testing requirements

Run the narrow regressions first, then the complete controlled suites. Record environmental blocks honestly.

```sh
rtk node --test scripts/tests/playback-ui.test.mjs scripts/tests/destination-ui.test.mjs scripts/tests/browse-mode-ui.test.mjs
rtk node --test scripts/tests/*.test.mjs
rtk proxy python3 -m unittest discover -s scripts/tests -p 'test*playback*evidence*.py'
rtk npm --prefix hifimule-ui run build
rtk cargo test -p hifimule-daemon playback::
rtk proxy env HIFIMULE_FFMPEG_RUNTIME_VERIFIED=ffmpeg-9-runtime-verified-v2 cargo test --workspace
rtk cargo fmt --all -- --check
rtk git diff --check
```

Also run platform runtime-script tests, platform package builds, extracted-bundle verifiers, installed evidence validation and the real/manual matrix described above. Do not call Clippy clean if blocked by known unrelated debt; record the exact result.

### Previous story and git intelligence

- Story 15.16 is done. It established stable keyed browse controls, focus-preserving capability reconciliation, Grid/List lifecycle cleanup, scoped compact/wrapping CSS and disposable preview-file ownership. Full Node tests reached 165 and the focused browse-mode file contains 9 tests; those are source/browser evidence, not installed-platform certification.
- 15.17 must still check native minimum-height/divider extremes, real provider switching, playback/Preview with a physical basket, OS text scaling, screen readers and changed packages on all targets.
- Recent commits: `1f1b7fa` review 15.16, `5a9edb3` close 15.16 findings, `c84ea89` develop 15.16, `2a88f9b` prepare 15.16, `53da1a4` review 15.15. Preserve the review fixes for focus, listener teardown, preview cleanup, Back persistence/failure handling and gain/version stability.

### Latest technical information checked 2026-09-19

- FFmpeg's official download page lists 9.0.2 as the latest 9.0 point release, released 2026-09-18. The repository's 9.0.1 pin remains valid only after an explicit reviewed release decision; any update changes hashes/ABI evidence and invalidates affected runtime/package results.
- CPAL 0.18.2 is the current crate documentation line and includes breaking behavior/API changes from 0.17; the project already pins and patches 0.18.2, so release work validates the patched implementation rather than upgrading again.
- Tauri v2 requires target-triple-specific `externalBin` sidecars and supports platform-specific installers. Existing package/resource conventions should be verified, not replaced.

### References

- [Source: `_bmad-output/planning-artifacts/epics.md` — Playback requirements inventory; Epic 15; Story 15.17]
- [Source: `_bmad-output/planning-artifacts/prd.md` — Playback quality/release evidence and non-goals]
- [Source: `_bmad-output/planning-artifacts/architecture.md` — Playback deployment, contracts, project structure and validation gates]
- [Source: `_bmad-output/planning-artifacts/ux-design-specification.md` — §§5–7 navigation, responsive/accessibility and release refinement]
- [Source: `_bmad-output/planning-artifacts/project-context.md` — provider abstraction, managed sync and credential boundaries]
- [Source: `_bmad-output/planning-artifacts/playback-epic-validation.md` — Story 15.17 coverage]
- [Source: `_bmad-output/implementation-artifacts/15-16-compact-the-library-browse-mode-bar-with-icons-and-smaller-labels.md` — previous-story handoff]
- [Source: `_bmad-output/implementation-artifacts/deferred-work.md` — Story 15.12 R16 and Story 15.14 R8/R9]
- [Source: `docs/playback-installed-test-checklist.md`; `scripts/playback-installed-evidence.py` — existing evidence contract and gaps]
- [Source: `.github/workflows/release.yml`; `.github/workflows/build.yml`; `.github/workflows/smoke-test.yml`; `docs/release-guide.md` — current release paths]
- [Source: `Cargo.toml`; `hifimule-daemon/audio-runtime.json`; `hifimule-daemon/THIRD_PARTY_AUDIO_NOTICES.md`; `hifimule-ui/package.json` — pinned versions and policy]
- [Source: `https://ffmpeg.org/download.html` — official FFmpeg 9.0.2 release]
- [Source: `https://docs.rs/crate/cpal/latest/source/UPGRADING.md` — CPAL 0.18.2 upgrade contract]
- [Source: `https://v2.tauri.app/reference/config/#bundleconfig` — Tauri v2 bundling and `externalBin`]

## Dev Agent Record

### Agent Model Used

Story preparation: Codex. Implementation agent: record during dev-story.

### Debug Log References

### Completion Notes List

- Ultimate context engine analysis completed - comprehensive developer guide created.
- Story prepared against repository state after `1f1b7fa` (Review 15.16).
- No release artifact, installed-platform pass or defect fix is claimed by story preparation.

### File List

- `_bmad-output/implementation-artifacts/15-17-ship-verified-playback-builds-for-windows-macos-and-linux.md`
