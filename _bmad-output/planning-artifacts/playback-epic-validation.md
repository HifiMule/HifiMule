# Desktop Playback — Coverage and Readiness Review

All 29 story proposals have user approval. Review scope is the playback amendment, not reapproval of completed Epics 1–14.

## Coverage

Story numbers below are within Epic 15. Each mapping identifies concrete acceptance coverage; later release checks do not substitute for feature implementation.

| Requirement | Stories |
|---|---|
| FR55 | 12, 13, 15 |
| FR56 | 1, 4, 6 |
| FR57 | 6, 15–20 |
| FR58 | 4–8, 14 |
| FR59 | 12, 14, 20 |
| FR60 | 2, 3, 4, 11, 16, 17 |
| FR61 | 8–10 |
| FR62 | 11, 14 |
| FR63 | 11 |
| FR64 | 4–6 |
| FR65 | 15–20 |
| FR66 | 17, 18 |
| FR67 | 16, 17 |
| FR68 | 13, 16 |
| FR69 | 16, 18 |
| FR70 | 15, 18 |
| FR71 | 4, 26 |
| FR72 | 26, 29 |
| FR73 | 8, 9, 16 |
| FR74 | 10, 19 |
| FR75 | 4, 7, 9, 26–28 |
| FR76 | 21 |
| FR77 | 22 |
| FR78 | 23 |
| FR79 | 23, 24 |
| FR80 | 25 |
| FR81 | 21, 24, 25 |

| Quality / design requirement | Stories |
|---|---|
| P-NFR1 | 9, 10, 19, 27–29 |
| P-NFR2 | 3, 4, 9, 13, 15–19, 21, 23, 24, 26, 28 |
| P-NFR3 | Platform-specific acceptance in all applicable stories; installed matrix in 29 |
| P-NFR4 | 1–29, through lifecycle, state, generation and operation-specific criteria |
| P-NFR5 | 1, 3, 4, 15, 17, 18, 21–25 |
| P-NFR6 | 4–8, 11–15, 20, 22–25, 29 |
| P-AR1–2 | 1, 2, 6 |
| P-AR3–4 | 3 and incremental state extensions in 11–18, 21–25 |
| P-AR5–7 | 4, 9, 10, 19, 29 |
| P-AR8 | 15–18 |
| P-AR9 | 4, 7, 17, 18, 21, 22, 24, 26 |
| P-AR10 | 3, 6–8, 11, 13, 14, 16, 20 |
| P-AR11 | 23–25 |
| P-AR12 | 4, 21, 26, 28 |
| P-AR13 | Ordered groups: 1–3; 4–7; 8–11; 12–14; 15–20; 21–25; 26–29 |
| P-AR14 | 5, 6, 9, 27–29 |
| P-UX-DR1 | 12 |
| P-UX-DR2 | 14, 20 |
| P-UX-DR3 | 11, 14 |
| P-UX-DR4 | 6, 20 |
| P-UX-DR5 | 13, 16 |
| P-UX-DR6 | 12, 15 |
| P-UX-DR7 | 5, 7, 14, 16, 17, 20, 26 |
| P-UX-DR8 | 22, 24 |
| P-UX-DR9 | 25 |
| P-UX-DR10–12 | 12–14, with browse preservation in 4, 8, 11 |
| P-UX-DR13–14 | Applicable UI/source criteria throughout; integrated check in 29 |
| P-UX-DR15 | 12, 23–25 |

## Structural and dependency checks

- Exactly 29 unique sequential story headings, 15.1–15.29; each contains the required user-story and Given/When/Then format.
- Explicit dependencies point only backward. Later native seek/Next, Radio entry points and export features are omitted until usable; intermediate stories do not claim their completion.
- Foundation state is introduced incrementally. No starter generation or upfront schema for all playback features is requested; this is a brownfield extension.
- One user-approved epic avoids artificial component-level epic boundaries. Existing Epics 1–14 are retained.
- Story 15.3 has a command-driven production state contract before audio exists; its tests do not depend on a future decoder. Stories 15.15–16 explicitly limit interim behavior until artist progression and entry-point integration arrive.
- Mid-track quality replacement is conditionally excluded from initial delivery. Stories 26 and 29 require it to remain disabled without separate validation; this is consistent with FR72.

## Readiness limits requiring attention before implementation

Coverage is complete at planning level. Unrestricted development readiness is NOT established.

1. Stories 15.4, 15.21 and 15.24 explicitly allow provider-specific splitting. Their single-session size cannot be certified until provider capability inspection; the implementing story author must split oversized integrations before execution, preserving these requirements and approval provenance.
2. Story 15.29 spans a shipping matrix whose exact architectures are not enumerated here. Resolve against release configuration and split execution work by platform if needed. It remains a release gate, not evidence that validation has happened.
3. Each implementation gate must be closed before coding the affected behavior: ownership/access/shutdown contracts; versioned schemas; runtime packaging; metadata conventions; provider eligibility/reconciliation; measured buffer and performance thresholds. Owner: implementing story author and reviewer, before that story executes.
4. Basket representation/source constraints in Story 15.25 require inspection. If the existing basket cannot represent an approved mixed-source selection, flag the product gap and resolve it explicitly; do not silently narrow FR80 through unsupported-content handling.
5. Explicit main-session Return restores previous intent as approved in Story 15.11. Preview-only snapshot treatment and preview restart policy remain contract decisions; they must preserve the main session and paused restoration.

No production tests or playback implementation were performed by this documentation review. Mechanical document checks passed; runtime claims still depend on implementation evidence.
