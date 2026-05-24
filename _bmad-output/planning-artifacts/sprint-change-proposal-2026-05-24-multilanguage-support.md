# Sprint Change Proposal: Shared Multilanguage Support

**Date:** 2026-05-24  
**Project:** HifiMule  
**Requested by:** Alexis

## 1. Issue Summary

HifiMule needs multilanguage support across both the daemon and UI, with French included at minimum. The trigger is a product direction change: user-facing strings should not remain duplicated or hardcoded separately in Rust and TypeScript.

## 2. Impact Analysis

**Epic Impact:** Minor addition to the foundation and UX surface. The change supports Epic 1's detachable daemon/UI architecture by introducing a shared localization catalog instead of separate text ownership.

**Story Impact:** Existing UI and daemon stories continue unchanged. Future stories that add user-visible strings should add keys to the shared catalog rather than embedding strings directly.

**Artifact Conflicts:** PRD and epics do not currently require localization. This proposal adds a maintainability constraint without changing MVP behavior or user journeys.

**Technical Impact:** Adds a small workspace crate, `hifimule-i18n`, with an embedded JSON catalog consumed by the daemon. The UI imports the same JSON catalog through a Vite/TypeScript alias.

## 3. Recommended Approach

**Direct Adjustment.** Implement shared localization infrastructure immediately and translate a first slice of high-visibility strings:

- daemon tray menu and tooltip labels
- daemon sync notification/error messages
- UI shell library heading/subtitle
- splash status messages
- status bar state/device/RPC labels

French is included as the first non-English language. Language selection uses `HIFIMULE_LANG`/`LANG` on the daemon and `localStorage('hifimule.language')` or browser language in the UI.

## 4. Detailed Change Proposals

**Architecture**

OLD:
- Daemon and UI own user-facing strings independently.

NEW:
- Shared `hifimule-i18n/catalog.json` contains English and French keys.
- Rust code uses `hifimule-i18n::t()` / `tf()`.
- UI code uses `src/i18n.ts` against the same catalog.

**Future Story Guidance**

OLD:
- New UI/daemon text may be added inline.

NEW:
- New user-visible UI and daemon text should add keys to `hifimule-i18n/catalog.json`.
- English remains the fallback language.

## 5. Implementation Handoff

**Scope:** Minor  
**Route:** Developer direct implementation  
**Success Criteria:**

- Shared catalog exists and is used by both daemon and UI.
- French translations are present for the initial shared key set.
- Rust formatting/checks pass.
- UI TypeScript and Vite builds pass.
