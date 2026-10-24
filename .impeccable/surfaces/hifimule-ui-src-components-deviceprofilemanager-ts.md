---
version: 1
slug: "hifimule-ui-src-components-deviceprofilemanager-ts"
primary_target: "hifimule-ui/src/components/DeviceProfileManager.ts"
related_targets:
  - "hifimule-ui/src/numericValidation.ts"
  - "hifimule-ui/src/deviceProfiles.ts"
---

# Device profiles
Mode: Operate. Build path: code-led.
User approved option A: Custom/System profile list left and capability editor right; narrow windows replace the list with a top Profile picker. Inherit DESIGN.md dark palette, spacing, typography, and Shoelace controls. System values are readable and locked, with Clone to customize. New and custom editing show identity, formats, conversion, collapsed limits and folder defaults. Native menu works without a device; device dropdown shortcuts preserve mounted parent drafts and return focus. Keep raw capability rules intact; visibly explain unsupported rules and server validation errors. Save identifies shared-device usage and future sync rewrites; switching/closing protects unsaved changes. No deletion, import/export, raw JSON UI, dependencies, or retroactive folder edits.

Default folders cover music, playlists, audiobooks, and podcasts. Audiobook and podcast defaults are optional; blank values inherit the music folder. These profile defaults seed device initialization and apply to untouched fields when choosing a profile in device settings, preserving each field the user has edited. Keep the four fields within the existing collapsed Default folders section and reuse its labels, help text, controls, and responsive layout.

## Folder artwork default extension

The Default folders disclosure also contains the Shoelace checkbox “Create folder.jpg thumbnails by default,” after the four folder fields. Its help text states that it applies only when creating a device and preserves existing device thumbnail settings. Reuse the existing disclosure spacing, neutral help text, checkbox states, and compact wrapping; this adds no global token or primitive.

The optional profile boolean `defaultFolderArtwork` is enabled for all three bundled Rockbox profiles and is false when absent. System profiles display its checked or unchecked value as locked; cloning retains the value and makes it editable. Custom profiles save an explicit boolean, including false, and reopening retains it. During new-device creation, the selected profile seeds the existing folder.jpg option until the user edits that checkbox; a manual choice survives later profile switches and profile-editor saves. Selecting or saving a profile in existing-device settings never reapplies this artwork default.

The desktop locked Rockbox state and compact saved custom state are recorded in [the artwork default review](../review/device-profile-manager/artwork-default-review.md). These are scrolled views of the production renderer with a synthetic RPC fixture, not shipping imagery.

## Numeric configuration extension

The five editable numeric controls retain raw text drafts and show localized units, examples, and actionable inline errors. Sample rate uses Hz with `48000 Hz = 48 kHz` as the unit example; an AAC ceiling of `48` remains visible and explains the codec's minimum. Blank optional limits still mean no limit. Bitrates use kbps with up to three decimal places. Bounds come from the approved numeric configuration contract and additive daemon constraint metadata; older daemons retain ordinary save validation.

Errors use the incumbent Shoelace danger token and help-text position; hints use its neutral token. Save remains an available action that validates before sending, opens the advanced-limits disclosure when necessary, scrolls to the first invalid field, and focuses it. Correction clears the field error. Structured daemon rejections map back to the numeric field while retaining the draft. System locks, unsupported capability rules, inactive conversion controls, and existing discard protection remain in force.

The shared interaction contract is in [Numeric configuration validation](hifimule-ui-src-numericvalidation-ts.md). French desktop and compact error captures and their fixture provenance are recorded in [the numeric validation review](../review/numeric-validation/README.md). This is an extension of the existing visual world; no global token or primitive change is approved.

## Direction contract

THESIS: Selected layout A keeps profile selection beside capability editing, so users can inspect or clone a system definition and keep their place.

OWN-WORLD: Inherit the existing dark surfaces, Inter typography, restrained cyan state treatment, spacing tokens, and Shoelace controls from DESIGN.md.

STORY: Open from the native menu or selected device profile. Select a custom/system entry, read its capabilities, clone if needed, edit, validate, and save; return to the mounted device form with its draft intact.

FIRST VIEWPORT: A grouped Custom/System list occupies the left column; the right starts with profile name and ownership, identity fields, supported formats, and conversion. New is above; Clone, Close, and Save remain in the dialog footer. Narrow windows use a top Profile selector. Advanced limits and default folders start collapsed.

FORM: User-pinned Profile manager (option A), code-led, selected through the interactive overview; no randomized seed or raster comp applies. Native Shoelace focus and dialog transitions carry state; the selected list row and generated conversion summary are the defining interactions.

FINISH: unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, DESIGN.md, and every shipping raster carrying its provenance
