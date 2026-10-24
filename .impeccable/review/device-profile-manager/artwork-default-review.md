# Folder artwork profile default review

This ordinary extension adds a profile creation default inside the existing Default folders disclosure. It preserves the Record Crate visual world in `DESIGN.md` and `.impeccable/design.json`; no system change was approved or recorded.

## Artifact and provenance

The evidence uses the production `DeviceProfileManager.ts` renderer, Shoelace dark theme, and application styles through Vite at `hifimule-ui/tests/device-profiles.html`. That development fixture loads the bundled profile catalog and substitutes an in-memory Tauri RPC adapter for profile reads and saves. Its custom identities and usage data are synthetic. Both captures use English and scroll the dialog body to the expanded Default folders disclosure; the profile heading and picker are above the captured scroll position.

These files are review screenshots, not application assets or shipping imagery. No generated, stock, or externally sourced imagery was added.

| Evidence | Viewport | Captured state |
| --- | --- | --- |
| [Desktop](artwork-default-desktop.jpg) | 1280 × 900 | Bundled Rockbox system profile; artwork default checked and disabled; Clone to customize and Close remain visible. |
| [Compact](artwork-default-compact.jpg) | 520 × 900 | Saved and reopened custom clone; artwork default unchecked and editable; Clone, Close, and Save profile remain visible. |

The documenter visually inspected both supplied captures and checked their dimensions. The evidence files use JPEG/JFIF encoding; their extensions were normalized to `.jpg` without changing pixels.

## Comparison with the incumbent system

The new control uses the same Shoelace checkbox treatment as the existing profile format controls. It follows the folder inputs within the disclosure's existing three-quarter-rem grid gap. Its help text uses the inherited neutral control text; the checked system value uses the inherited disabled state. No new color, type role, border, radius, shadow, or motion rule appears.

At desktop width, the selected list/editor composition remains in place. At compact width, the existing single-column editor holds the label and wrapped two-line help text within the disclosure, and the footer actions remain reachable. Both captures retain the neutral folder help, optional-folder placeholders, collapsed advanced limits, and conversion summary. The artwork label and help fit without visible clipping in these captured states. These screenshots do not establish coverage for every locale, focus state, or viewport.

## Interaction and implementation evidence

The implementation session reports the live sequence: clone a Rockbox profile with the default enabled, verify that the clone inherits checked, uncheck it, save, reopen, and verify that false persists. The compact capture records the resulting unchecked editable state. The documenter inspected the supplied screenshots and source; it did not independently repeat that browser sequence.

Source evidence checked:

- `hifimule-ui/src/components/DeviceProfileManager.ts`: disclosure placement, localized label/help, system lock, clone copy, draft capture, and save path.
- `hifimule-ui/src/deviceProfiles.ts`: absent values map to false; drafts serialize an explicit boolean while retaining capability rules.
- `hifimule-ui/src/components/InitDeviceModal.ts`: initial/profile-selected defaults use strict true; a manual checkbox change protects the user's choice on later profile changes.
- `hifimule-daemon/assets/device-profiles.json`: true on `rockbox-mp3-320`, `rockbox-mp3-192`, and `rockbox-audiobooks-m4b`.
- `hifimule-i18n/catalog.json`: label and creation-only help exist in English, French, Spanish, and German.
- `hifimule-ui/tests/deviceProfiles.test.mjs`, `tests/initDeviceOptions.test.mjs`, and `tests/deviceSettings.test.mjs`: regression assertions cover draft/clone persistence, creation overrides, and preservation of existing-device artwork settings. Test execution belongs to the implementation validation record.
- `hifimule-ui/src/styles.css`, `PRODUCT.md`, `DESIGN.md`, and `.impeccable/design.json`: incumbent surface vocabulary, typography, spacing, responsive rules, and product constraints.

The correctness reviewer reported no defects. The finish reviewer returned **ship**, with all five sections passing at the scope of this narrow extension and no material fixes required. These verdicts were supplied to the documenter by the implementation session.

## Not canonized

No defect or visual exception is promoted into the design system. The review found no material visual defect in the extension; existing system drift is outside its scope. `DESIGN.md` and `.impeccable/design.json` remain unchanged.
