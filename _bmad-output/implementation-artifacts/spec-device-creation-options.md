---
title: 'Configure reconciliation and folder artwork when creating a device'
type: 'feature'
created: '2026-10-21'
status: 'done'
baseline_commit: 'c94c05950e20e95e882a99e277f3823a8cd0b4a5'
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Initialize Device omits the reconciliation and folder artwork preferences available in Device Settings, requiring users to reopen settings after creation.

**Approach:** Offer reconciliation, Add thumbnail, and independent Windows, macOS, KDE, and GNOME icon choices during initialization. Save them in the initial manifest.

## Boundaries & Constraints

**Always:** Default every option off. Reuse existing en/fr/es/de labels and helper text. Show native additions only while Add thumbnail is checked; preserve their independent selections when toggled. Validate RPC values before initialization side effects. Preserve pending-device identity/revision checks and the independent setup scan consent and cancellation flow.

**Ask First:** Changes to defaults, setup scan consent, artwork generation, or native host/transport capability rules.

**Never:** Enable reconciliation after setup scanning; generate artwork during creation; bypass native capability rules; add a second UI save request; redesign unrelated fields.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|---------------|----------------------------|----------------|
| Default creation | Options untouched or RPC fields omitted | Reconciliation and every artwork option persist as false | Existing creation errors remain visible |
| Explicit choices | Reconciliation true or false; mixed icon additions | Exact choices enter the first persisted manifest and device state | Failed persistence cannot report successful creation |
| Master toggling | Select native additions, disable and re-enable thumbnail | Additions hide/show without losing choices; disabled master still saves selections | Existing form submission flow applies |
| Invalid options | Non-boolean reconciliation, non-object artwork, non-boolean member, or unknown artwork key | Invalid-params response before initialization side effects | No device manifest or configured folders created |
| Setup scan | Either reconciliation value | Existing optional scan prompt and import behavior remain independent | Existing scan failure/cancellation behavior remains |
| Device disappears | Pending identity removed or stale revision | Existing initialization rejection; no settings applied to another device | Existing error UI remains |

</frozen-after-approval>

## Code Map

- `hifimule-ui/src/components/InitDeviceModal.ts` — form and initialization request; capture settings before replacing the form with progress.
- `hifimule-ui/src/components/BasketSidebar.ts` — existing edit controls to match.
- `hifimule-i18n/catalog.json` — existing reconciliation/artwork translations.
- `hifimule-daemon/src/rpc.rs` — initialization allowlist, validation, manager call, and RPC tests.
- `hifimule-daemon/src/device/mod.rs` — pending-device reservation, manifest construction, initial MSC/MTP persistence.
- `hifimule-daemon/src/device/tests.rs` — initialization callers and lifecycle tests.

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-ui/src/components/InitDeviceModal.ts` — add matching localized checkboxes after folder inputs, wire native-addition visibility, and capture explicit booleans before `renderSubmitting`; include them in the existing initialization request.
- [x] `hifimule-daemon/src/rpc.rs` — accept optional `reconcileBeforeSync` and `folderArtwork`; share existing strict artwork validation with editing. Creation uses defaults; editing preserves omitted members.
- [x] `hifimule-daemon/src/device/mod.rs`, `hifimule-daemon/src/device/tests.rs` — pass preferences into the initial manifest write, updating callers with defaults. Preserve reservation checks and publication order; verify an unselected pending target gets its own preferences.
- [x] `hifimule-ui/tests/initDeviceOptions.test.mjs` — cover defaults, master hide/show with retained additions, and explicit enabled/disabled request values using existing Node/TypeScript test patterns.
- [x] `hifimule-daemon/src/rpc.rs`, `hifimule-daemon/src/device/tests.rs` — cover matrix defaults, persistence/state, invalid inputs without side effects, and edit partial-update compatibility.

**Acceptance Criteria:**
- Given an authenticated user creating a device, when initialization options load, then the same reconciliation and folder artwork choices available during editing are present with all defaults off.
- Given selected preferences, when creation succeeds and Device Settings reopens or the device reconnects, then the saved values match the creation form.
- Given each supported language, when the creation form displays these options, then existing localized descriptions explain reconciliation cost and native icon requirements.
- Given existing device editing or initialization callers omitting these fields, when they run, then their previous behavior and defaults remain compatible.

## Spec Change Log

## Verification

- From `hifimule-ui`, `rtk proxy node --test tests/initDeviceOptions.test.mjs tests/initDeviceLoading.test.mjs tests/deviceMusicScan.test.mjs tests/deviceSettings.test.mjs` — new and existing regressions pass.
- From `hifimule-ui`, `rtk npm run build` — TypeScript, localization, production build pass.
- `rtk npm run build:daemon -- test -p hifimule-daemon device_initialize` plus focused manager/settings tests — persistence, validation, and destination lifecycle pass using the required FFmpeg wrapper.
- Inspect creation options off/on in normal and compact desktop windows; controls stay reachable and text wraps.
- `rtk git diff --check` — no whitespace errors.

### Execution Results — 2026-10-21

- Focused UI command above: **24 passed**, including all-off defaults, hide/show retention, explicit true/false creation payloads, existing device settings, credential loading, and independent setup scan consent/cancellation.
- UI `rtk npm run build`: **passed** (TypeScript and Vite production build).
- `rtk npm run build:daemon -- test -p hifimule-daemon device_initialize`: **7 passed**.
- `rtk npm run build:daemon -- test -p hifimule-daemon initialize`: **19 passed**, including MSC first-write preferences on an unselected pending target, reconnect, MTP authoritative-cache preferences with failed mirror, and initialization failure behavior.
- `rtk npm run build:daemon -- test -p hifimule-daemon device_update_manifest`: **2 passed**, including strict validation and preservation of omitted artwork members on editing.
- `rtk npm run build:daemon -- test -p hifimule-daemon device::tests::pending_`: **4 passed**, covering stale revision, duplicate submission, and removal during commit.
- Broader exploratory `pending_` daemon filter: **36 passed, 1 failed**. The unrelated existing `rpc::tests::reviewed_invalid_or_busy_preparation_preserves_pending_plan` panics in macOS `system-configuration` dynamic store with “Attempted to create a NULL object.” Its code is unchanged by this work.
- `rtk git diff --check`: **passed**.
- Rendered actual creation component with local Shoelace and application styles in an isolated Vite preview at 1280×720 and a 640×480 iframe viewport: defaults and expanded native options inspected; labels/helpers wrap, body scrolls, footer stays reachable. No physical device creation was performed.

## Review Outcome

Blind, edge-case, and acceptance reviews found no actionable defects.

## Suggested Review Order

**Creation form**

- Expose familiar default-off controls, preserving native selections when hidden.
  [InitDeviceModal.ts:220](../../hifimule-ui/src/components/InitDeviceModal.ts#L220)

- Capture all choices before submission replaces the form.
  [InitDeviceModal.ts:349](../../hifimule-ui/src/components/InitDeviceModal.ts#L349)

**Validation and persistence**

- Share strict artwork validation while preserving partial edit updates.
  [rpc.rs:9585](../../hifimule-daemon/src/rpc.rs#L9585)

- Validate new options before initializing the captured pending device.
  [rpc.rs:9703](../../hifimule-daemon/src/rpc.rs#L9703)

- Put preferences in the first persisted manifest, before publishing device state.
  [mod.rs:2478](../../hifimule-daemon/src/device/mod.rs#L2478)

**Regression checks**

- Verify first-write preferences, unselected-target isolation, and reconnect persistence.
  [tests.rs:5006](../../hifimule-daemon/src/device/tests.rs#L5006)

- Reject malformed options without creating folders or a manifest.
  [rpc.rs:15567](../../hifimule-daemon/src/rpc.rs#L15567)

- Cover defaults, localization, retained choices, and explicit request values.
  [initDeviceOptions.test.mjs:45](../../hifimule-ui/tests/initDeviceOptions.test.mjs#L45)

