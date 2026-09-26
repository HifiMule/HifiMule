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

## Operational state

The app stores server metadata, credentials, playback data, and lifecycle runtime records in the user's app-data profile. The daemon binds an available loopback port and publishes it privately; installed clients must attach through the lifecycle descriptor and authenticated health check. Device manifests live on devices and remain portable across machines through deterministic server IDs. Backup and migration checks should treat machine-local vault state and device manifests separately.
