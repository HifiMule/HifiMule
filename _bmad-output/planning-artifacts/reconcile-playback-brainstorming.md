# Playback PRD Reconciliation — Brainstorming

Reviewed 2026-09-11 against brainstorming-session-2026-09-11-090745.md and the initial FR55–81/P-NFR1–6 draft. Later approved architecture resolves early open questions and supersedes preliminary feasibility claims. Findings reflect this review snapshot; subsequent PRD corrections require a closure check.

## Preservation map

| Source ideas | PRD coverage |
|---|---|
| #1–6 everyday listening, albums, preview, continuous Radio | Purpose, FR56, FR61–65, UJ-P1–3 |
| #7–12 artist journey, preferences, exclusions | FR66–70, FR76–77 |
| #13–15 shared output, loss pause, UI independence | FR56, FR58, FR64 |
| #16–22 editable local queue, immutable saves, lookahead, cross-server recording identity | FR65, FR68, FR70, FR78–79 |
| #23–29 sustainable quality, adaptation, recovery, loudness, sync coexistence | FR71–75, P-NFR1–3 |
| #30–32 device Add/Replace, paused restoration, logical exclusions | FR60, FR69, FR80 |
| #33–38 blank-page remedy, app menu, fresh vs resume, independent first destination | Purpose, FR55, FR57 |
| #39–42 physical arrival/setup, floating and idle controls | FR55, FR59 — omissions below |
| #43–46 distinct full-track Preview, restoration and reporting | FR62–63, FR76 — semantic correction below |

## Amendment gaps

- **High — Preview is conflated with Play (FR62).** “A browse play affordance ... starts ... audition” commits an unapproved interaction and drops the explicit Preview distinction. Use an explicitly labeled Preview action, distinct from ordinary Play; preserve a return-to-session affordance (#5). Exact button/context-menu placement remains open (#43).
- **Medium — Idle listening entry omitted (FR59).** #42 explicitly keeps the floating player visible while idle with Play something. FR57 only ensures the desktop menu action; add idle floating-bar behavior. Preserve #41 translucent under-browser treatment as the agreed direction without inventing final control placement.
- **Medium — Detected-device failure feedback omitted (FR59).** #39 accepts visible feedback when a detected device cannot be opened. Include this alongside arrival selection and blank-device setup.

No other loss of accepted product behavior identified. Provisional ASIO/external-metadata research and early unresolved questions were correctly not elevated into commitments. Platform feasibility is appropriately qualified.

## Resolution — 2026-09-11

FR59 now includes the idle translucent bar, Play something and device-open failure feedback. FR62 distinguishes Preview from Play and preserves explicit return. FR67 requires eligible cycle renewal. PRD metadata now identifies the brownfield extension and reconciliation inputs. Historical FR42–44 numbering gaps remain unchanged; no playback requirement depends on renumbering them.
