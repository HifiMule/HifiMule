---
title: 'Fedora 44 x86_64 desktop and RPM support'
type: 'feature'
ticket: ''
created: '2026-10-03'
status: 'built'
baseline_revision: 'af078f5e7f923e2fb6ee2184dcd8f4bfd7a81313'
route: 'full'
route_source: 'auto'
risk: 'medium'
review: 'quick'
review_source: 'pinned'
lenses_ran: ['quick']
review_loop_iteration: 0
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Fedora 44 users need an installable x86_64 RPM and a functioning desktop UI with its context menu. Current Linux distribution targets are DEB and AppImage; Fedora desktop behavior has not been established.

**Approach:** Extend the existing Linux packaging and private audio-runtime verification to RPM. Diagnose the reported Fedora desktop behavior and make a focused compatibility change backed by regression checks and available native verification.

**Decision (user said “go”):** Proceed with both in-window right-click and daemon tray menus. Investigate actual Fedora launch behavior rather than inventing a failure symptom; fix demonstrated incompatibilities and document desktop integration requirements.

## Boundaries & Constraints

**Always:** Preserve existing DEB, AppImage, Windows and macOS packaging. Include the daemon sidecar, private native library closure, audio manifest and notices. Use Fedora runtime package names for RPM dependencies. Preserve the existing initiative document and the latest Fedora Clang fix. Report native checks that cannot be completed separately from automated results.

**Never:** Publish a release, install packages globally, change user media settings, or claim installed Fedora qualification from configuration checks alone. Rewrite historical versioned release evidence.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| RPM packaging | Linux x86_64 release build | RPM includes desktop launcher, UI, sidecar and private audio resources | Missing prerequisites produce actionable diagnostics |
| Installed runtime | Extracted RPM contents | Daemon resolves its private native closure | Verification fails on missing or host-only audio dependencies |
| Desktop menus | Fedora desktop with relevant menu opened | Menu appears and its action works | Platform requirements and unverified sessions are stated explicitly |
| Existing bundles | Existing supported targets | Prior packaging remains functional | Contract regressions fail verification |

</frozen-after-approval>

## Code Map

- `hifimule-ui/src-tauri/tauri.linux.conf.json` — Linux bundle targets and private resource mapping; currently DEB/AppImage.
- `hifimule-ui/src-tauri/tauri.conf.json` — shared window configuration, sidecar and Debian dependency list; RPM must use Fedora names.
- `.github/workflows/release.yml` — Ubuntu Linux builds; generic bundle upload already covers added formats. Existing DEB/AppImage extraction checks provide the RPM verification pattern.
- `scripts/linux-audio-runtime.mjs` — controlled runtime and installed closure verification; preserve latest Fedora include-path change.
- `scripts/run-tauri.mjs` — AppImage prerequisites and pinned runtime setup; explicit RPM builds must avoid AppImage-only downloads/probes.
- `hifimule-ui/src/components/MediaCard.ts:377` — DOM right-click menu shared by cards, list rows and track browse; dismisses on click, scroll, resize and keyboard input.
- `hifimule-ui/src/styles.css:2342` — menu visibility, animation and positioning.
- `hifimule-daemon/src/main.rs` — Tao event loop and daemon tray menu, separate from the DOM menu.
- `scripts/tests/release-contract.test.mjs`, `tauri-platform-config.test.mjs`, `run-tauri.test.mjs`, `linux-audio-runtime.test.mjs` — existing packaging and runtime regressions.
- `docs/development-guide.md`, `docs/release-guide.md`, `docs/deployment-guide.md` — prerequisites, distribution and verification guidance.

## Tasks & Acceptance

**Execution:**
- [x] Linux Tauri configuration — add RPM target and runtime dependencies, retaining resource layout.
- [x] Release workflow — provision extraction tools and verify the extracted RPM's native closure before upload.
- [x] Linux build wrappers — check Fedora prerequisites and ensure explicit RPM builds do not require AppImage tooling.
- [x] UI or daemon menu files identified by reproduction — investigation found no application menu incompatibility; preserve working shared behavior and document GNOME's missing AppIndicator integration.
- [x] Existing packaging tests and focused menu regression — exercise RPM configuration, explicit bundle selection and diagnosed menu behavior.
- [x] Development, deployment and release guides — document Fedora 44 x86_64 build/install steps, desktop requirements and verification limits.

**Acceptance Criteria:**
- Given an x86_64 Linux release build, when RPM is selected, then an RPM containing the UI, daemon and required resources is produced.
- Given the RPM extraction, when the native runtime verifier runs, then required private audio libraries resolve without dependence on host FFmpeg.
- Given Fedora 44 and the reported menu scenario, when the app opens and the menu is activated, then the expected menu action is available and works.
- Given existing supported bundle formats, when targeted packaging regressions run, then their contracts continue to pass.

## Implementation Notes

- User authorized proceeding after the draft; scope covers both menu surfaces. Current HEAD includes the user’s subsequent CI test fix; preserve it. Risk is medium because packaging touches runtime dependency resolution and menu behavior spans desktop integrations.
- RPM dependency names were checked against the Fedora 44 installed packages. No changes to the latest Linux Clang include-path fix or historical release contract were needed.
- Desktop investigation found GNOME's only enabled extension was `background-logo@fedorahosted.org`; there was no AppIndicator host, so tray visibility/actions cannot be qualified in this session. The guides now state this shell requirement separately from runtime library dependencies.
- The shared DOM menu worked in actual WebKitGTK 4.1 with the existing source and CSS. No menu compatibility code change was justified. Initial missing readiness markers came from invalid diagnostic smoke IDs, which must be UUIDs. Temporary stage tracing confirmed frontend init, monitor fitting, event registration, native readiness, daemon state, routing and main-window display all succeeded. A valid UUID produced the hydrated acknowledgment. Temporary instrumentation was removed; no speculative blanket Fedora rendering override was added.

## Plan Change Log

## Review Triage Log

Quick review: 2 findings; 2 medium, 0 high, 0 low, 0 false, 0 maybe-false.

| Finding | Verdict | Route | Evidence / action |
|---------|---------|-------|-------------------|
| Native menu qualification is incomplete | medium | defer | Original engine fixture replaced the playlist-dialog entry point, and this GNOME session has no AppIndicator host. A stronger native WebKitGTK test now uses actual MediaCard and Shoelace dialog: dialog opens and selecting the fixture playlist calls `playlist.addItems` with expected item IDs. Provider persistence and installed tray actions remain unverified; no menu application defect was demonstrated. Record the remaining desktop qualification separately. |
| Fedora audio preflight gives Debian installation commands | medium | patch | Confirmed every audio/toolchain failure printed apt-get after desktop preflight. Added injectable Fedora detection and dnf development-package guidance, preserving Ubuntu and Clang discovery. Missing tool and all four audio-module cases now pass; final combined suite passes 70/70. |

## Verification

**Commands:**
- `rtk node --test scripts/tests/tauri-platform-config.test.mjs scripts/tests/release-contract.test.mjs scripts/tests/run-tauri.test.mjs scripts/tests/linux-audio-runtime.test.mjs` — packaging and runtime regressions pass.
- `rtk npm run build --prefix hifimule-ui` — TypeScript and frontend build pass.
- `rtk npm run tauri --prefix hifimule-ui -- build --bundles rpm` — native RPM build succeeds when toolchain and dependencies are available.

**Native checks:** Launch and menu reproduction on Fedora 44; extract RPM, inspect metadata and run the existing installed native closure verifier. Record environmental blockers explicitly.

**2026-10-03 implementation evidence:**
- Final packaging/runtime/menu suite: 68/68 pass. The standard command needs normal child-process access on this host; sandbox-only execution reports `EPERM` for Node/cc/pkg-config child processes.
- `rtk node --test --test-isolation=none scripts/tests/context-menu-ui.test.mjs`: 3/3 pass for viewport/reveal/action, keyboard behavior, dismissal and replacement.
- Frontend production build passes (existing Vite chunk warnings).
- Native RPM build passes on Fedora 44 x86_64: `target/release/bundle/rpm/HifiMule-0.16.1-1.x86_64.rpm`, SHA-256 `f44707a7925f31bea2bff16ea26f47bf34a08be84c474ed895103a0279a07022`.
- `rpm -qp --requires` confirms Fedora dependencies. `rpm -qpl` confirms UI/daemon, desktop launcher, icons, private libraries, manifest and notices. Extraction to `/tmp/hifimule-fedora-rpm-root` and the installed closure verifier pass with 25 libraries at `usr/lib/HifiMule/bundled-libs`.
- A temporary GTK3/Python GI WebKitGTK 4.1 window loaded the transpiled actual MediaCard source and project stylesheet, opened the menu, observed `visibility=visible` and viewport bounds, activated Add to playlist with the expected item, and confirmed menu removal. This engine fixture mocks the playlist-dialog entry point and does not certify installed Tauri RPC or tray actions.
- Fedora ships the installed `/usr/bin/WebKitWebDriver` from WebKitGTK 6.0; it cannot create a session with the installed 4.1 MiniBrowser. Official Fedora repo queries exposed only 6.0 driver providers. Direct GI verification avoided installing packages globally.
- Initial default/software-rendering launches used invalid readable smoke IDs; the native acknowledgment correctly rejected them. With valid UUID `596a018b-d029-441a-ae28-b0f75f8b1b44`, the Fedora UI published `state=hydrated` after startup. Actual installed clean launch and both menu surfaces in the installed app remain separate qualification; real tray actions remain unverified because this GNOME session lacks AppIndicator integration.

- Final uninstrumented RPM extraction to `/tmp/hifimule-fedora-rpm-final` passes the 25-library closure verifier. Launching its extracted `usr/bin/hifimule-ui` on Fedora with UUID `5e2e7870-487a-46d0-a70e-fcc1de5e662d` and isolated `/tmp/hifimule-fedora-final-extracted` profile publishes `state=hydrated`. This verifies extracted payload startup without globally installing the package. Normal user profile and enabled extensions were unchanged.

**Final review verification:**
- Root independently ran the combined packaging, audio preflight and DOM menu regressions with `--test-isolation=none`: 70/70 passed, zero skipped.
- Root independently verified the extracted final RPM's 25-library closure and parsed its valid-UUID `state=hydrated` acknowledgment. The extracted UI launched on Fedora 44 GNOME Wayland using an isolated temporary profile.
- Root strengthened native menu verification: actual MediaCard and bundled Shoelace components opened the real playlist dialog; selecting fixture playlist `playlist-1` invoked `playlist.addItems` with `track-1`. RPC responses were fixture data; no user's media profile or provider content was touched.
- Production TypeScript/Vite build passed; the existing chunk-size and mixed static/dynamic import warnings remain.

- Final prerequisite check also includes `tar` in Fedora guidance; targeted Fedora/Ubuntu diagnostic tests passed 3/3 after this correction.

- Final post-review RPM rebuild passed with exit 0. SHA-256 `f8ecf13376f87a61faedfe6de37c5c7f7695aca6e4b38d9175eb4048ec2bc40f`. Fresh extraction passed the 25-library private closure check. UI and daemon executables are byte-for-byte identical to the prior extracted build that passed Fedora GNOME Wayland hydration.
