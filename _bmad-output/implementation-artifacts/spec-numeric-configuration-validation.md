---
title: 'Inline validation for numerical configuration'
type: 'feature'
created: '2026-10-24'
status: 'done'
baseline_commit: '2e6afe52c40fd11f56d4ecfeda271864e6a8283f'
context: []
---

# Inline validation for numerical configuration

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Numeric configuration controls accept confusing or invalid values and report generic errors later. A sample-rate ceiling of 48, entered as kHz in a Hz field, failed with an unexplained encoder error. Auto-fill also silently rounds, clamps or drops invalid entries.

**Approach:** Validate every editable numerical configuration field in the profile editor, auto-fill dialog and playback selection settings. Show localized units, examples and actionable field errors; preserve invalid drafts; prevent invalid Save/Preview requests. Reuse Shoelace/native validity APIs and existing styling.

## Boundaries & Constraints

**Always:** Match domain constraints; distinguish blank/zero/invalid; require finite numbers and safe converted integers. Preserve system locks, unsupported rules, preview fencing, unchanged byte/second precision and server validation. Localize all four languages.

**Ask First:** Changes to stored schemas, encoding behavior, or existing meanings of valid settings.

**Never:** Add dependencies, silently correct units, invent business limits or reject valid wrapping/partial shares. Playback seeking is outside configuration scope.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected behavior |
|---|---|---|
| Unit confusion | AAC sample-rate ceiling `48` | Inline codec-aware error; explain Hz, show `48000 Hz = 48 kHz`, retain input. |
| Valid sample ceiling | `48000`, or blank | Save accepted; blank retains no-limit semantics. |
| Malformed number | Blank required field, fraction in count, nonfinite or overflow | Explain required integer/range/precision beside field; no save/preview. |
| Optional values | Blank or valid zero | Preserve documented default/disable/zero semantics; do not conflate invalid input with either. |
| Dynamic form | Invalid context/rarity field, then toggle disclosure or add/reorder a row | Draft/error survives; correction clears it; first invalid field is revealed/focused on submit. |
| Legacy/system | Read-only or unsupported capability rules | Do not block cloning/metadata edits on disabled numeric fields. |
| Backend failure | Numeric profile constraint rejected by daemon | Associate structured error with field; keep draft and useful general failure handling. |

</frozen-after-approval>

## Code Map

- `hifimule-ui/src/components/DeviceProfileManager.ts`, `src/deviceProfiles.ts`: five numeric profile controls; generic validation today.
- `hifimule-daemon/src/transcoding.rs`, `src/transcoding/local.rs`, `src/rpc.rs`: authoritative profile bounds and controlled encoder capabilities.
- `hifimule-ui/src/components/AutoFillPanel.ts`, `src/state/autoFill.ts`: numeric drafts, conversions, dynamic rows and shared Save/Preview serialization.
- `hifimule-ui/src/components/PlaybackSelectionSettings.ts`: required unsigned 32-bit seed; currently blank becomes zero.
- `hifimule-i18n/catalog.json`, `hifimule-ui/src/styles.css`: localized copy and existing control/error presentation.

## Tasks & Acceptance

**Execution:**

- [x] `hifimule-ui/src/numericValidation.ts`: shared numeric validation and field feedback, using native/Shoelace validity APIs.
- [x] `hifimule-daemon/src/transcoding/local.rs`, `src/rpc.rs`: expose numeric bounds and codec sample-rate/bit-depth floors via `device_profiles.constraints`; provide structured field errors on save. If metadata is unavailable, retain ordinary backend validation.
- [x] `hifimule-ui/src/deviceProfiles.ts`, `src/components/DeviceProfileManager.ts`: validate all five numbers; add units/examples and reveal errors. Bounds: channels 1–32, bit depth 1–64, sample rate 1–768000 Hz plus codec floor, bitrate 0.001–100000 kbps (three decimals). Ignore inactive/read-only controls.
- [x] `hifimule-ui/src/components/AutoFillPanel.ts`: validate episodes 0–100, nonnegative u32 counts/weeks, fractional weights, hours 0–23, months 1–12/days 1–31, and month-list tokens. Validate converted budget/headroom/duration as safe integers; retain arbitrary positive duration fractions and existing date semantics.
- [x] `hifimule-ui/src/components/AutoFillPanel.ts`: preserve raw strings through rerenders; guard Save/Preview before lossy conversion. Preserve unchanged byte/second values and percentage-slider semantics.
- [x] `hifimule-ui/src/components/PlaybackSelectionSettings.ts`: seed integer 0–4294967295, required; inline feedback and Save guard.
- [x] `hifimule-i18n/catalog.json`, `hifimule-ui/src/styles.css`: localized hints/errors and accessible feedback after editing/blur or submission; clear on correction.
- [x] `hifimule-ui/tests/{numericValidation,deviceProfiles,autoFillAccessibility,radioDefaults}.test.mjs` and daemon profile/RPC tests: boundaries, no invalid writes, correction, draft preservation and legacy behavior.

**Acceptance Criteria:**

- Given any editable numeric configuration field, when its active value is invalid, then a specific localized correction appears beside it and Save/Preview cannot submit it.
- Given corrected values, when saving or previewing, then validation clears and existing behavior proceeds without losing other drafts.
- Given French or German labels and compact windows, when errors wrap, then controls and actions remain readable and reachable by keyboard.

## Verification

- `rtk test node --test tests/numericValidation.test.mjs tests/deviceProfiles.test.mjs tests/autoFillAccessibility.test.mjs tests/radioDefaults.test.mjs` in `hifimule-ui`.
- `rtk npx tsc --noEmit` and `rtk npm run build` in `hifimule-ui`: pass.
- `rtk npm run build:daemon -- test -p hifimule-daemon`: target profile/constraint/RPC tests through the controlled runtime wrapper.
- Inspect desktop/compact errors, focus, disclosure reveal, correction and translations; independent correctness/visual reviews; surface documentation.

## Implementation Checks

- Shared raw-number validation, native/Shoelace custom validity, localized inline feedback and invalid-field reveal are implemented across all three surfaces.
- Runtime constraints are additive RPC metadata; editor maxima are checked only on profile save, retaining existing runtime/legacy capability semantics.
- Targeted numeric/profile/AutoFill/seed tests and the full UI Node test suite pass. TypeScript and the UI production build pass.
- Controlled-runtime daemon tests pass: numeric configuration metadata/bounds (1), profile RPC structured rejection/no write (1), and profile storage/legacy tests (8).
- A broader daemon `profile` filter ran: 22 passed, 3 failed. Both migration failures reproduce on isolated baseline `2e6afe5` and are recorded in deferred-work.md. The SystemConfiguration case passes outside the macOS sandbox. The local encoding suite passes: 19 passed, 1 ignored.
- Actual-renderer review fixture: `hifimule-ui/tests/numeric-validation.html` with `open=profile|autofill|podcast|radio`, `lang=fr|de`, and optional profile `noMetadata` / `serverError` flags. French/German desktop and compact captures, correction/save interactions, and fresh independent visual review are complete; disposition: ship.

## Review Outcome

- Applied fixes for Shoelace render timing/focus, optional whitespace, cached codec errors, numeric underflow, restored context values, podcast checkbox drafts, and disclosure focus restoration. Regression checks pass.
- Blind, edge-case, and acceptance reviews completed; patched tree has no remaining concrete findings. Two baseline migration failures are deferred; no intent gaps or spec loopbacks.
- Full UI tests, TypeScript, production build, targeted profile storage/RPC/metadata tests, and local encoding checks pass. Detector returned no findings; surface documentation and six real-renderer screenshots are complete.

## Suggested Review Order

- Reject invalid raw numbers before conversion, then present accessible feedback.
  [numericValidation.ts:8](../../hifimule-ui/src/numericValidation.ts#L8)

- Use controlled encoder capabilities and structured numeric errors.
  [local.rs:65](../../hifimule-daemon/src/transcoding/local.rs#L65)

- Attach profile errors to fields, preserving drafts and codec-dependent guidance.
  [DeviceProfileManager.ts:159](../../hifimule-ui/src/components/DeviceProfileManager.ts#L159)

- Validate active and collapsed auto-fill settings before Save or Preview.
  [AutoFillPanel.ts:1232](../../hifimule-ui/src/components/AutoFillPanel.ts#L1232)

- Keep required seed validation separate from source-selection errors.
  [PlaybackSelectionSettings.ts:297](../../hifimule-ui/src/components/PlaybackSelectionSettings.ts#L297)

- Exercise lifecycle timing, underflow, correction, and domain boundaries.
  [numericValidation.test.mjs:5](../../hifimule-ui/tests/numericValidation.test.mjs#L5)

- Inspect localized real-renderer captures and interaction evidence.
  [README.md:1](../../.impeccable/review/numeric-validation/README.md#L1)

