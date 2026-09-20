---
title: 'Restore draft-release access for smoke tests'
type: 'bugfix'
created: '2026-09-20'
status: 'done'
route: 'one-shot'
---

# Restore draft-release access for smoke tests

## Intent

**Problem:** Smoke tests list only published releases and cannot download the existing v0.15.0 draft. Commit 92d3a0e reduced the reusable smoke workflow to contents:read; both callers also grant only read access. GitHub only lists drafts for callers with push access. The previous release workflow used contents:write for this purpose.

**Approach:** Restore contents:write on the smoke workflow and both reusable-workflow callers, retaining actions:read. Both callers must permit the callee's requested permission, including candidate runs sharing the workflow. Keep releases in draft, preserve authenticated asset downloads, and add regression contracts for caller/callee compatibility across LF, CRLF and CR line endings. Document why this permission is needed.

Evidence: Git history confirms the previous permission and regression. [GitHub release-list documentation](https://docs.github.com/en/rest/releases/releases#list-releases) states draft visibility requires push access. All 16 focused release tests pass. No workflow was rerun, release modified, or remote change published.

The separately reported Linux playback session failure is not attributed to this permission defect. All 89 session tests pass locally on macOS; authenticated Linux panic output is required for diagnosis.

## Suggested Review Order

- Restore draft visibility in the reusable workflow.
  [smoke-test.yml:25](../../.github/workflows/smoke-test.yml#L25)
- Allow the permission at both caller boundaries.
  [release.yml:450](../../.github/workflows/release.yml#L450)
- Guard caller/callee compatibility and draft preservation.
  [release-contract.test.mjs:88](../../scripts/tests/release-contract.test.mjs#L88)

Independent review found no actionable introduced defects. Both workflows parse as valid YAML.
