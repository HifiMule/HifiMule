---
title: 'Render podcast descriptions as safe HTML'
type: 'bugfix'
created: '2026-09-25'
status: 'done'
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Podcast episode descriptions supplied by the server can contain HTML, which the Library currently displays as literal tags. This makes paragraphs, links, and donor lists hard to read.

**Approach:** Render common description formatting and links in podcast show and episode rows while keeping server content inert and limiting navigation to safe URLs.

## Boundaries & Constraints

**Always:** Preserve ordinary text and line breaks; keep unsafe markup, event handlers, and unsafe URL schemes from executing. Apply the same behavior to show and episode descriptions.

**Ask First:** None.

**Never:** Insert the server description directly with `innerHTML` or alter provider data.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Formatted description | Paragraphs, breaks, emphasis, lists, and HTTPS links | Readable formatting and clickable links | N/A |
| Plain text | Description with line breaks | Readable text with line breaks | N/A |
| Unsafe markup | Script, event attributes, or JavaScript URL | No script execution or unsafe navigation | Drop dangerous content and attributes |

</frozen-after-approval>

## Code Map

- `hifimule-ui/src/library.ts` -- podcast show and episode row rendering.
- `hifimule-ui/src/styles.css` -- podcast description presentation.
- `scripts/tests/browse-mode-ui.test.mjs` -- Library DOM harness and podcast behavior checks.

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-ui/src/library.ts` -- parse and rebuild description content with a small allowlist; use it for both podcast row types.
- [x] `hifimule-ui/src/styles.css` -- keep long description links and text readable.
- [x] `scripts/tests/browse-mode-ui.test.mjs` -- cover formatted, plain, and unsafe content.

**Acceptance Criteria:**
- Given a podcast episode with HTML markup, when its row appears, then formatting and safe links render as elements without visible tags.
- Given a podcast show with a description, when its row appears, then the same formatting rules apply.
- Given hostile markup, when either row appears, then executable content and unsafe links are absent.

## Spec Change Log

## Verification

**Commands:**
- `rtk node --test scripts/tests/browse-mode-ui.test.mjs` -- podcast Library checks pass.
- `rtk node node_modules/typescript/bin/tsc --noEmit` in `hifimule-ui` -- TypeScript check passes.

## Suggested Review Order

**Description safety**

- Parse server markup and copy only approved formatting and safe links.
  [library.ts:1712](../../hifimule-ui/src/library.ts#L1712)

**Library display**

- Use the same renderer for episode and show descriptions.
  [library.ts:1772](../../hifimule-ui/src/library.ts#L1772)

- Wrap long text and make links recognizable.
  [styles.css:2112](../../hifimule-ui/src/styles.css#L2112)

**Verification**

- Check formatting, unsafe markup, plain text, and both row types.
  [browse-mode-ui.test.mjs:83](../../scripts/tests/browse-mode-ui.test.mjs#L83)
