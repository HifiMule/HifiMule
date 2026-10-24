---
version: 1
slug: "hifimule-ui-src-numericvalidation-ts"
primary_target: "hifimule-ui/src/numericValidation.ts"
related_targets:
  - "hifimule-ui/src/components/DeviceProfileManager.ts"
  - "hifimule-ui/src/components/AutoFillPanel.ts"
  - "hifimule-ui/src/components/PlaybackSelectionSettings.ts"
---

# Numeric configuration validation

Mode: Operate. Build path: code-led extension.

The approved intent is in `_bmad-output/implementation-artifacts/spec-numeric-configuration-validation.md`. Keep the existing Record Crate visual world, Shoelace/native controls, dialog layouts, and semantic tokens from `DESIGN.md` and `.impeccable/design.json`.

## Interaction contract

Keep a raw string draft until validation succeeds. Distinguish blank, zero, malformed values, fractions in whole-number fields, nonfinite numbers, and unsafe converted values. Do not silently correct units, clamp, round away invalid input, or replace it with a default. Preserve existing meanings of valid values, including fractional weights, fractional duration, wrapping dates, and partial shares.

Show a localized unit/range/example hint beside each active numeric field. After editing, blur where supported, or an attempted submission, replace the hint with the specific correction; correction restores the hint. Shared feedback sets custom validity, `aria-invalid`, and an `aria-describedby` relationship to a stable polite live message. Shoelace feedback occupies its help-text slot; native feedback follows the input. Feedback uses the existing small text and neutral/danger tokens with wrapping, without a new alert-card treatment.

Save and Auto-Fill Preview validate before serialization or requests. They remain actionable so an attempted submission can reveal the problem. Open the containing disclosure, scroll the first invalid field into view, and focus it. Raw drafts and errors survive Auto-Fill disclosure changes, rerenders, and context row additions/reordering; correction must not discard neighboring drafts. Ignore inactive or disabled settings according to their existing semantics. Retain server validation and useful general failure handling.

## Surface application

- **Device profiles:** five controls cover sample rate, bit depth, channels, output bitrate, and original bitrate. Preserve optional no-limit blanks, system locks, unsupported rules, and inactive conversion behavior. Explain Hz and codec floors rather than interpreting `48` as kHz. Structured daemon numeric errors belong beside their field. See [Device profiles](hifimule-ui-src-components-deviceprofilemanager-ts.md).
- **Auto-Fill:** validate budget/headroom/duration conversions as safe whole bytes/seconds; retain unchanged stored precision. Counts and weeks are nonnegative unsigned whole numbers, episode retention is 0–100, weights allow nonnegative fractions, hours are 0–23, months 1–12, and days 1–31. Validate every comma-separated month token. Hidden advanced content remains active; invalid drafts reveal the disclosure on Save/Preview. Preserve preview generation fencing and percentage-slider semantics.
- **Playback selection:** the native seed field is required, accepts whole numbers 0–4294967295, and treats zero as valid. Invalid Save retains and focuses the raw draft; the ordinary settings layout and action states remain intact.

## Responsive and localization evidence

Inline feedback must wrap with the field while preserving readable labels and reachable footer actions. The checked French profile and German Auto-Fill/seed error states at desktop and compact widths are listed in [the review evidence](../review/numeric-validation/README.md). They establish those captured compositions; source review establishes the interaction contract. They do not establish complete accessibility or every locale/state combination.

No shipping raster assets were added for this extension. The six PNGs are local renderer review evidence with synthetic fixture data, not generated design comps or production library screenshots. `DESIGN.md` and `.impeccable/design.json` remain the existing system authority.
