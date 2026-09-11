# Playback PRD Reconciliation — Architecture

Reviewed 2026-09-11 against architecture.md Playback Extension (approved context through implementation handoff) and initial FR55–81/P-NFR1–6 draft. This is product preservation review; technical mechanisms can remain in the linked architecture. Findings reflect this review snapshot and require closure after correction.

## Preservation map

| Approved section | PRD coverage |
|---|---|
| Context, ownership, typed destination and cross-server identities | Purpose, FR55–60, FR69–70, FR78, P-NFR4 |
| Audio pipeline, output continuity, adaptation, recovery | FR61, FR64, FR71–75, P-NFR1–3 |
| Provider integration, reporting, genuine feedback, snapshots | FR70, FR76–81, P-NFR5 |
| Radio selection, artist fallback, bounded replenishment | FR57, FR65–70, P-NFR2 |
| UI and session control, restoration and concurrency | FR56–60, FR68, FR75, P-NFR4–6 |
| Implementation contracts, previews, snapshot occurrences | FR63, FR67–69, FR78–81 |
| Validation refinements and evidence limits | FR20/FR33 amendments, FR61, FR74–75, P-NFR1–5, release evidence |
| Deployment sequence and critical gates | Purpose scope and linked architecture; owning-story open decisions |

## Amendment gap

- **Medium — Exhaustion behavior weakened (FR67).** Architecture Implementation Contracts says “begin another listening cycle” when unheard eligible tracks are exhausted, retaining session exclusions. “Another listening cycle may begin” permits stopping despite eligible heard tracks remaining. Make the approved transition mandatory; waiting is for no eligible tracks.

## Deliberate technical retention, not loss

Native FFmpeg/CPAL selection, controlled runtime and exact validation versions; configuration-file versus SQLite ownership; typed identities; command-ID retention; callback restrictions; event/reconnect sequencing; single-instance mechanism; and module structure remain authoritative in architecture. They need applicable story acceptance checks before coding, not duplicate PRD implementation prose. The PRD correctly avoids claiming unrestricted implementation readiness or x64/physical gapless certification.

Brainstorming-specific idle-entry and Preview issues are recorded in reconcile-playback-brainstorming.md and also matter to preservation of approved architecture's low-friction controls.

## Resolution — 2026-09-11

FR59 now includes the idle translucent bar, Play something and device-open failure feedback. FR62 distinguishes Preview from Play and preserves explicit return. FR67 requires eligible cycle renewal. PRD metadata now identifies the brownfield extension and reconciliation inputs. Historical FR42–44 numbering gaps remain unchanged; no playback requirement depends on renumbering them.
