---
title: 'GNOME tray setup guidance and Fedora RPM recommendation'
type: 'feature'
ticket: ''
created: '2026-10-03'
status: 'built'
baseline_revision: '8b9003ebd9a6df3bfee9429e4fb612c846c3752f'
route: 'full'
route_source: 'auto'
risk: 'medium'
review: 'quick'
review_source: 'pinned'
lenses_ran: [quick]
review_loop_iteration: 0
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** HifiMule's tray menu exists, but Fedora GNOME users can install the app without the shell integration needed to display it. Current documentation alone does not guide users who discover this inside the app.

**Approach:** Recommend the AppIndicator extension in RPM metadata and show accessible, dismissible setup guidance when GNOME has no active tray host. Users install and enable the extension themselves; HifiMule checks support again when requested.

## Boundaries & Constraints

**Always:** Keep the extension a soft RPM recommendation. Show guidance only for a confirmed missing tray host in a GNOME session. Keep startup and onboarding usable, and keep all four supported UI languages complete. Preserve the existing Fedora packaging and working menus. Provide Fedora package commands on Fedora and generic extension-page instructions on other GNOME distributions.

**Never:** Automatically install software, enable extensions, log out, execute displayed shell commands, or modify desktop preferences. Treat a temporary D-Bus error as proof that support is missing. Show GNOME guidance on Windows, macOS, KDE or other non-GNOME desktops. Claim tray interaction qualification from mocked detection tests.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| RPM installation | Fedora RPM metadata | Recommends gnome-shell-extension-appindicator, retains existing required dependencies | Hard dependency is forbidden |
| Supported GNOME | Registered StatusNotifier host | No setup notice | No error expected |
| Missing GNOME support | Watcher absent or registered-host property false | Notice explains installation, logout/login and enabling; offers extension-page link, Check again and dismiss | Guidance does not block the app |
| Inconclusive check | Unavailable bus, malformed reply or timeout | No initial missing-support claim | Return unknown distinctly |
| Other desktop | Non-GNOME Linux, Windows or macOS | No guidance and no Linux-only calls on other platforms | No error expected |
| Recheck | Previously missing; host becomes available | Notice disappears | Unknown preserves existing notice with a neutral check result |
| Dismiss | User dismisses notice | Hidden for the current app session | Storage failure falls back to memory |
| Lifecycle race | Detection resolves after shutdown view replaces body | No notice inserted into shutdown view | Discard obsolete result |

</frozen-after-approval>

## Code Map

- `hifimule-ui/src-tauri/tauri.linux.conf.json` — RPM supports `recommends` in the installed CLI schema; required dependency/resource lists remain intact.
- `hifimule-ui/src-tauri/Cargo.toml` and `Cargo.lock` — GIO 0.18.4 and GLib 0.18.5 are already cached and locked through GTK. Add Linux-only direct dependencies for the native query.
- `hifimule-ui/src-tauri/src/lib.rs` — native Tauri command registration and async runtime; add bounded tray-support query off the main thread.
- `hifimule-ui/src/main.ts::init` — mount guidance after successful hydration; native startup acknowledgment must not depend on detection.
- `hifimule-ui/src/login.ts::initLoginView` — replaces `.app-container` content, so notice must live outside that root to survive onboarding.
- `hifimule-ui/src/main.ts::renderShutdownStatus` — shutdown replaces body content; guard stale detection responses against the original mount identity.
- `hifimule-ui/src/components/TraySetupGuidance.ts` — new component for presentation and recheck/dismiss handling.
- `hifimule-ui/src/styles.css` — reuse existing colors, spacing and accessible notice patterns.
- `hifimule-i18n/catalog.json` and `hifimule-ui/src/i18n.ts` — shared en/fr/es/de translations and `t()` helper.
- `hifimule-ui/tests/shutdownStatus.test.mjs` — transpiled TypeScript behavioral-test pattern to reuse in new `traySetupGuidance.test.mjs`.
- `scripts/tests/tauri-platform-config.test.mjs` — effective RPM metadata regressions.
- `docs/deployment-guide.md` and `docs/release-guide.md` — describe soft dependency, activation and verification limits.

## Tasks & Acceptance

**Execution:**
- [x] Linux bundle configuration and its existing test — recommend the Fedora AppIndicator package without promoting it to required dependencies.
- [x] Linux-only native dependencies, new `tray_support.rs` module and command registration — detect GNOME desktop tokens case-insensitively; read StatusNotifierWatcher host registration on session D-Bus with no service autostart and bounded timeout. Return available/missing/unknown/not-applicable states and Fedora identity.
- [x] Native unit tests — cover desktop-token parsing, registered-host true/false, missing-service classification and malformed/temporary error behavior using injected probe results.
- [x] New guidance component, main hydration hook and styles — nonblocking body-sibling notice; translated setup instructions, extension-page action, recheck and session dismissal; discard stale responses.
- [x] Shared translation catalog — add complete text for all four languages, using existing conventions.
- [x] New behavioral UI test — exercise every detection, dismissal/recheck, onboarding and shutdown-race scenario in the matrix.
- [x] Deployment/release guides — record optional RPM recommendation and the difference between package installation and extension activation.

**Acceptance Criteria:**
- Given a Fedora RPM build, when its metadata is queried, then the extension appears as a recommendation and existing required dependencies remain intact.
- Given GNOME without a tray host, when the hydrated UI opens, then setup guidance appears in onboarding and the normal app without delaying startup.
- Given an active host, another desktop, or an inconclusive probe, when the UI opens, then it does not display a missing-support notice.
- Given displayed guidance, when the user dismisses it or a successful recheck detects a host, then the notice disappears according to the matrix.
- Given a detection response arriving after shutdown replaces the UI, when that response completes, then the shutdown view remains unchanged.

## Implementation Notes

- Approved unchanged by the user (go). Implemented Linux-only native detection with a two-second cancellation deadline and four distinct states; guidance follows hydration and survives onboarding.
- Matrix audit: RPM metadata test; UI available/missing/unknown/not-applicable, recheck, session/storage dismissal and initial/recheck shutdown-race tests all ran and passed. Native tests cover desktop tokens, true/false host replies, absent services, transient errors and malformed replies.

## Plan Change Log

## Review Triage Log

- Quick reviewer: no findings. Other lenses skipped by configured quick review.
- Medium / defer: pre-existing concurrent sidecar staging uses one shared temporary filename; simultaneous test/package builds collided on rename. Sequential packaging passed; record for later build tooling correction.

## Verification

- Run focused native tray-support tests and UI behavioral tests covering all matrix states.
- Run existing packaging/config regressions and production TypeScript/Vite build.
- Build the Fedora RPM and query recommendation metadata with `rpm -qp --recommends`.
- Launch the extracted package in an isolated Fedora GNOME profile and verify hydration plus the missing-host guidance state available in this session.
- Real tray menu actions require an AppIndicator-enabled desktop and remain separately recorded if that environment is unavailable.

### Verification results

- 91 focused UI, shutdown, context-menu and packaging/audio regressions passed; three native tray-support tests passed. Production TypeScript/Vite and final RPM builds passed.
- RPM recommends `gnome-shell-extension-appindicator`; required dependencies remain intact. SHA256: `7104fbaf9d5afde1ee9259548ec61bc17812185d4033a773dd1d506afdae8080`.
- Extracted private runtime closure verified (25 libraries). Isolated native RPM UI reported hydrated on Fedora GNOME Wayland. Local WebKit inspection verified the actual native missing-host notice, Fedora command and controls (`/tmp/hifimule-tray-live-notice.json`). A separate real WebKitGTK 4.1 component check confirmed the notice fits the viewport.
- Active-host detection transitions pass injected tests; real enabled-host tray menu actions remain unqualified because this session has no AppIndicator host. Temporary profile cleanup requested through authenticated daemon.quit.
