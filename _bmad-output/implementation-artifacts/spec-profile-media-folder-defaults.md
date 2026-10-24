---
title: 'Audiobook and podcast profile folder defaults'
type: 'bugfix'
created: '2026-10-24'
status: 'done'
route: 'one-shot'
---

# Audiobook and podcast profile folder defaults

## Intent

**Problem:** Device profiles and their editor exposed only music and playlist folder defaults, preventing users from defining audiobook and podcast destinations in a profile.

**Approach:** Add optional `defaultAudiobookFolder` and `defaultPodcastFolder` values to the existing JSON profile store and editor. Preserve them through saves and clones, validate relative paths, and apply them to individually untouched device setup/settings fields. Missing defaults inherit music during initialization; existing device settings retain their configured paths when a selected profile omits defaults. Folder changes preserve audio capabilities and their fingerprint.

## Suggested Review Order

1. [Profile storage and validation](../../hifimule-daemon/src/transcoding.rs) and [RPC summaries](../../hifimule-daemon/src/rpc.rs): both optional fields persist, clone, clear and validate with existing folder rules.
2. [Draft mapping](../../hifimule-ui/src/deviceProfiles.ts) and [profile editor](../../hifimule-ui/src/components/DeviceProfileManager.ts): four default folders, inherited music hints, and locked system values.
3. [Device initialization](../../hifimule-ui/src/components/InitDeviceModal.ts) and [device settings](../../hifimule-ui/src/components/BasketSidebar.ts): apply defaults independently while preserving manual edits.
4. [Profile tests](../../hifimule-ui/tests/deviceProfiles.test.mjs), [initialization tests](../../hifimule-ui/tests/initDeviceOptions.test.mjs) and [settings tests](../../hifimule-ui/tests/deviceSettings.test.mjs): 37 focused UI tests and 8 Rust profile-store tests pass; TypeScript and production UI build pass. Fresh correctness review found no introduced defects.
5. [Desktop evidence](../../.impeccable/review/device-profile-manager/folder-defaults-desktop.png) and [compact evidence](../../.impeccable/review/device-profile-manager/folder-defaults-compact.png): saved/reopened audiobook and podcast defaults alongside music and playlists. Fresh visual review: ship, no material fixes. [Surface documentation](../../.impeccable/surfaces/hifimule-ui-src-components-deviceprofilemanager-ts.md) updated; established design system preserved.
