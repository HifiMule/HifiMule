# Numeric validation review evidence

Reviewed 2026-10-24 against `_bmad-output/implementation-artifacts/spec-numeric-configuration-validation.md`, `PRODUCT.md`, `DESIGN.md`, `.impeccable/design.json`, the profile surface brief, and the numeric validation implementation. This extension preserves the existing dark Shoelace/native styling and tokens; it introduces field feedback and validation behavior within the incumbent surfaces.

## Capture provenance

All six PNGs are screenshots from the real local renderer: a CUA-controlled ChatGPT in-app browser running the Vite-served `hifimule-ui/tests/numeric-validation.html` fixture. The fixture imports the production styles, Shoelace controls, `DeviceProfileManager`, `AutoFillPanel`, and `PlaybackSelectionSettings`. Its profile, library, Auto-Fill pipeline, and invoke/RPC responses are synthetic test data. The fixture buttons behind the dialogs are test harness controls.

These are review evidence, not shipping raster assets, generated raster comps, or captures of a user's live collection. No new shipping imagery was required or added. Desktop captures use a 1280 × 900 viewport; compact captures use 520 × 900.

| Capture | Locale / fixture route | Visible state |
| --- | --- | --- |
| [profile-desktop.png](profile-desktop.png) | French; `?open=profile&lang=fr` | AAC sample-rate draft `48`, inline codec-floor error with the Hz/kHz example; advanced limits revealed and field focused. |
| [profile-compact.png](profile-compact.png) | French; `?open=profile&lang=fr` | Same error at compact width; labels and error wrap, editor scrolls, footer actions remain visible. |
| [autofill-desktop.png](autofill-desktop.png) | German; `?open=autofill&lang=de` | Budget draft `-1`, localized minimum error and focused field; Preview, Cancel, and Save remain visible. |
| [autofill-compact.png](autofill-compact.png) | German; `?open=autofill&lang=de` | Same error at compact width; dialog title wraps and footer actions fit. |
| [seed-desktop.png](seed-desktop.png) | German; `?open=radio&lang=de` | Seed draft `1.5`, whole-number error beneath the focused native input. |
| [seed-compact.png](seed-compact.png) | German; `?open=radio&lang=de` | Same error at compact width; description wraps and actions form two columns. |

## Evidence checked

The documenting agent visually inspected all six images. Their dark surfaces, compact controls, inherited focus styling, small neutral hints, and semantic danger feedback fit the existing design authority. French/German wrapping remains readable in the captured states. The profile captures show a scrolled editor after invalid-field reveal, rather than the initial profile view.

The finishing agent reported these separate live browser checks: profile Save retained `48`, showed the AAC error, and prevented submission; correction to `48000` cleared the error and saved. Auto-Fill Save retained `-1`, showed its error, and focused the budget; correction to `.5` reached Preview loading. Seed `1.5` showed the whole-number error; correction to `0` saved. These behavior findings are reported interaction evidence, not conclusions inferred from the pixels.

Source inspection confirms shared custom validity, `aria-invalid`, described polite inline feedback, first-invalid disclosure reveal/focus, and Save/Preview guards. The finishing agent also reported regression coverage for no invalid callbacks/RPCs, raw numeric/context drafts, and podcast retention. The screenshots alone do not prove those behaviors, full keyboard coverage, contrast compliance, every numeric boundary, or all four locale combinations.

## Disposition

The independent finish reviewer reported **ship**, with no material fixes. Documentation review found no material incumbent visual-system drift requiring a repair or global design update. `DESIGN.md` and `.impeccable/design.json` remain unchanged. The extension contract is recorded in [Numeric configuration validation](../../surfaces/hifimule-ui-src-numericvalidation-ts.md) and the updated [Device profiles brief](../../surfaces/hifimule-ui-src-components-deviceprofilemanager-ts.md).
