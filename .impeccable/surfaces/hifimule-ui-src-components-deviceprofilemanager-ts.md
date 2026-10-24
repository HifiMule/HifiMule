---
version: 1
slug: "hifimule-ui-src-components-deviceprofilemanager-ts"
primary_target: "hifimule-ui/src/components/DeviceProfileManager.ts"
related_targets: []
---

# Device profiles
Mode: Operate. Build path: code-led.
User approved option A: Custom/System profile list left and capability editor right; narrow windows replace the list with a top Profile picker. Inherit DESIGN.md dark palette, spacing, typography, and Shoelace controls. System values are readable and locked, with Clone to customize. New and custom editing show identity, formats, conversion, collapsed limits and folder defaults. Native menu works without a device; device dropdown shortcuts preserve mounted parent drafts and return focus. Keep raw capability rules intact; visibly explain unsupported rules and server validation errors. Save identifies shared-device usage and future sync rewrites; switching/closing protects unsaved changes. No deletion, import/export, raw JSON UI, dependencies, or retroactive folder edits.

## Direction contract

THESIS: Selected layout A keeps profile selection beside capability editing, so users can inspect or clone a system definition and keep their place.

OWN-WORLD: Inherit the existing dark surfaces, Inter typography, restrained cyan state treatment, spacing tokens, and Shoelace controls from DESIGN.md.

STORY: Open from the native menu or selected device profile. Select a custom/system entry, read its capabilities, clone if needed, edit, validate, and save; return to the mounted device form with its draft intact.

FIRST VIEWPORT: A grouped Custom/System list occupies the left column; the right starts with profile name and ownership, identity fields, supported formats, and conversion. New is above; Clone, Close, and Save remain in the dialog footer. Narrow windows use a top Profile selector. Advanced limits and default folders start collapsed.

FORM: User-pinned Profile manager (option A), code-led, selected through the interactive overview; no randomized seed or raster comp applies. Native Shoelace focus and dialog transitions carry state; the selected list row and generated conversion summary are the defining interactions.

FINISH: unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, DESIGN.md, and every shipping raster carrying its provenance
