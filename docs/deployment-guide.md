# HifiMule — Build and Deployment Guide

**Last Updated:** 2026-09-27 | **Workspace version:** 0.15.0

HifiMule is installed as a desktop application with a bundled daemon. It does not deploy to a hosted server. The Tauri native shell and daemon use `hifimule-lifecycle` to locate the user's private app-data profile and coordinate ownership after installation.

## Build pipeline

| Workflow | Trigger | Purpose |
| --- | --- | --- |
| `.github/workflows/build.yml` | Pull requests and pushes to `main` | Cross-platform playback evidence and build/runtime regression checks |
| `.github/workflows/release.yml` | `v*` tag or manual candidate dispatch | Four platform rows, packaged artifacts and checksums; tag path prepares a draft release |
| `.github/workflows/smoke-test.yml` | Called by release or manual dispatch | Install/package lifecycle smoke tests |

The release matrix covers Windows x64, Linux x64, macOS x64, and macOS ARM64. The current release guide lists MSI/NSIS, deb/AppImage, and architecture-specific DMG output, signing behavior, evidence requirements, and the `0.14.0` same-host upgrade baseline for `0.15.0`.

## Native runtime and package sequence

The daemon uses FFmpeg, CPAL, and platform audio libraries. Run the root `build:daemon` wrapper to provision the controlled audio runtime before Cargo builds dependencies. Build the UI, stage the daemon sidecar with `scripts/prepare-sidecar.mjs`, then package with Tauri. CI verifies the platform-specific runtime and bundle. Platform prerequisites and local commands are in the [Development Guide](./development-guide.md).

## Release decision

Candidate dispatch builds an immutable ref without creating a tag or published release. A tag run prepares a draft and attaches package artifacts. Publishing is a separate release-manager action after all required package rows pass. Automated smoke establishes installation, launch, authenticated health, UI attachment, and basic lifecycle only. It does not establish audible physical playback, hardware media keys, real-device MTP/MSC sync, or provider-specific listening continuity. See the [Release Guide](./release-guide.md) for the exact gates and evidence contract.

## Fedora 44 x86_64 RPM

Install the built artifact with `sudo dnf install ./HifiMule-0.16.1-1.x86_64.rpm`
(use the actual filename under `target/release/bundle/rpm/`). DNF resolves Fedora
runtime dependencies: GTK3, WebKitGTK 4.1, libsoup3, libappindicator-gtk3, libmtp,
openssl-libs and libxdo. The package includes a desktop launcher, UI, daemon,
private audio libraries, manifest and notices; host FFmpeg is not required.

On Fedora GNOME, install `gnome-shell-extension-appindicator`, then enable its
AppIndicator extension using the Extensions application (a session restart may
be needed after installing). The shell must display StatusNotifier/AppIndicator
icons for the daemon tray menu to appear. Installing the library alone does not
enable GNOME tray support. Desktop environments with a built-in indicator host
can provide the tray directly. This requirement is separate from the DOM
right-click menu inside the UI, which offers Add to playlist.

For desktop qualification, launch from both the desktop entry and terminal,
open a media card, list row and track context menu, choose Add to playlist, and
check Escape, outside-click and scroll dismissal. Open the daemon tray and test
Open UI, playback settings, audio output and Quit in an isolated test profile.
Record Fedora version, GNOME version, Wayland/X11, enabled extensions, artifact
checksum and source revision. Use `HIFIMULE_APP_DATA_DIR` with an absolute
temporary directory for diagnosis without changing the normal media profile.

RPM extraction and automated DOM tests do not certify installed desktop menus.
The DEB lifecycle smoke remains a separate result; Fedora clean installation,
upgrade, rendering and tray actions require their own evidence.

## Operational state

The app stores server metadata, credentials, playback data, and lifecycle runtime records in the user's app-data profile. The daemon binds an available loopback port and publishes it privately; installed clients must attach through the lifecycle descriptor and authenticated health check. Device manifests live on devices and remain portable across machines through deterministic server IDs. Backup and migration checks should treat machine-local vault state and device manifests separately.
