---
title: 'Profile defaults for folder.jpg thumbnails'
type: 'feature'
created: '2026-10-24'
status: 'done'
route: 'one-shot'
---

# Profile defaults for folder.jpg thumbnails

## Intent

**Problem:** Device profiles cannot supply the initial folder.jpg thumbnail preference when creating a device. Rockbox profiles should enable this supported feature by default.

**Approach:** Persist `defaultFolderArtwork` in the existing profile JSON store, expose a localized checkbox under Default folders, and initialize the device creation checkbox from the selected profile until the user edits it. Cloning preserves the default. All three bundled Rockbox profiles enable it; absent values remain off. Existing-device settings keep their saved preference. Legacy stock migration accepts rows predating this field while preserving explicit custom choices. Artwork defaults do not change audio fingerprints.

Validation passed: Rust profile-store tests and manifest migration regressions, combined script/UI JavaScript suites (one existing skip), TypeScript checking, and UI production build. Real-renderer desktop/compact checks verified locked system defaults and editable clone persistence. Independent correctness and finish reviews found no material defects; no patches or deferrals were required.

## Suggested Review Order

- Persist and migrate creation defaults; verify all Rockbox profiles and custom cloning.
  [transcoding.rs:27](../../hifimule-daemon/src/transcoding.rs#L27), [regression tests:467](../../hifimule-daemon/src/transcoding.rs#L467), [bundled profiles:11](../../hifimule-daemon/assets/device-profiles.json#L11).

- Carry the default through list RPC responses, profile drafts, and the locked/custom editor.
  [rpc.rs:10336](../../hifimule-daemon/src/rpc.rs#L10336), [deviceProfiles.ts:95](../../hifimule-ui/src/deviceProfiles.ts#L95), [DeviceProfileManager.ts:128](../../hifimule-ui/src/components/DeviceProfileManager.ts#L128).

- Seed new-device creation while retaining manual overrides; verify existing devices keep their preference.
  [InitDeviceModal.ts:242](../../hifimule-ui/src/components/InitDeviceModal.ts#L242), [creation tests:113](../../hifimule-ui/tests/initDeviceOptions.test.mjs#L113), [existing-device test:121](../../hifimule-ui/tests/deviceSettings.test.mjs#L121).

- Inspect the rendered checkbox and creation-only help at desktop and compact widths.
  [Visual evidence](../../.impeccable/review/device-profile-manager/artwork-default-review.md).
