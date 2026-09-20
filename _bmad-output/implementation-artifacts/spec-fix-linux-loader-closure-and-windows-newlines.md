---
title: 'Fix Linux loader closure and Windows workflow parsing'
type: 'bugfix'
created: '2026-09-20'
status: 'done'
baseline_commit: '20f05e1e798413d5f1878b946560b4952f58ebc6'
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Linux AppImage verification rejects a valid `$ORIGIN/../lib` RUNPATH because it compares the loader-visible directory with an unrelated nested resource copy discovered first. Windows playback evidence fails earlier because its release-workflow test searches LF-only step delimiters in a CRLF checkout.

**Approach:** Make the installed sidecar RUNPATH select the authoritative Linux library directory before validating its controlled closure, and make workflow-step extraction newline-independent with explicit LF/CRLF/CR regression coverage.

## Boundaries & Constraints

**Always:** Derive the Linux validation directory from the canonical, safe, in-bundle RUNPATH; validate the exact controlled FFmpeg/libmtp/Pulse closure in that loader-reachable directory; remain independent of filesystem traversal order and unreachable duplicate resources; retain ABI, SONAME, dependency resolution, escape, symlink, collision, and `ldd` checks; parse workflow steps identically under LF, CRLF, and CR.

**Ask First:** Any change to packaged resource destinations, supported artifact formats, controlled runtime contents, or the evidence workflow's execution policy.

**Never:** Trust the first matching library found anywhere in an extracted bundle; accept a RUNPATH merely because another unreachable copy is complete; remove nested packaged resources to hide the ambiguity; make Windows CI depend on repository-wide line-ending configuration instead of robust parsing.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|---------------|----------------------------|----------------|
| Real AppImage topology | Sidecar in `usr/bin`, complete loader closure in `usr/lib`, duplicate codec under nested resources | `$ORIGIN/../lib` selects `usr/lib`; complete reachable closure passes | Ignore unreachable duplicate for loader validation |
| Traversal reversed | Nested resource codec enumerates before or after direct codec | Same authoritative loader directory and result | No first-match dependence |
| Unreachable-only closure | Nested copy is complete but RUNPATH directory lacks required runtime | Installed verification fails | Identify missing reachable library/closure |
| Ambiguous or unsafe RUNPATH | Existing entries resolve outside, collide, or select different directories | Verification fails | Report invalid installed RUNPATH |
| Windows checkout | Release workflow contains CRLF or CR | Named macOS steps are extracted and assertions run | No false missing-step failure |
| Unix checkout | Release workflow contains LF | Existing assertions remain unchanged | N/A |

</frozen-after-approval>

## Code Map

- `scripts/linux-audio-runtime.mjs` -- discovers extracted bundle files, resolves sidecar RUNPATH, and validates installed native closure.
- `scripts/tests/linux-audio-runtime.test.mjs` -- covers Linux package topology and runtime-validation boundaries.
- `scripts/tests/release-runtime-contract.test.mjs` -- extracts named release-workflow steps and guards signed/unsigned environment separation.
- `.github/workflows/release.yml` -- workflow text parsed by release contract tests across platforms.
- `.github/workflows/build.yml` -- runs Node contracts before generating playback evidence.

## Tasks & Acceptance

**Execution:**
- [x] `scripts/linux-audio-runtime.mjs` -- resolve a unique safe loader directory from RUNPATH first, then discover and verify required native libraries only in that directory.
- [x] `scripts/tests/linux-audio-runtime.test.mjs` -- model duplicate direct/nested AppImage libraries, traversal-order independence, reachable closure success, and unreachable-only closure failure.
- [x] `scripts/tests/release-runtime-contract.test.mjs` -- normalize newline styles during named-step extraction and exercise LF, CRLF, and CR inputs.

**Acceptance Criteria:**
- Given an AppImage with a complete runtime in `usr/lib` and duplicate nested resources, when verification runs, then `$ORIGIN/../lib` selects and validates `usr/lib` regardless of traversal order.
- Given only an unreachable nested runtime is complete, when verification runs, then it fails rather than borrowing that evidence.
- Given a Windows CRLF checkout, when release contract tests run, then all named workflow steps are found and the playback evidence job can proceed to generation.
- Given unsafe or ambiguous loader paths, when verification runs, then the existing strict failure behavior remains intact.

## Spec Change Log

## Design Notes

RUNPATH is the executable loader contract and must be authoritative. Resolve its entries canonically, require a single acceptable existing directory, and validate required libraries directly within that directory. Nested resource copies may remain packaged, but they cannot satisfy checks for an unreachable loader path. Normalize only the parser input so the workflow file itself remains unmodified.

## Verification

**Commands:**
- `rtk node --test scripts/tests/linux-audio-runtime.test.mjs scripts/tests/release-runtime-contract.test.mjs` -- Linux topology and all newline forms pass.
- `rtk node --test scripts/tests/*.test.mjs` -- full Node suite passes.
- `rtk python3 -m unittest discover -s scripts/tests -p 'test_*.py'` -- playback evidence and Python regressions pass.
- `rtk git diff --check` -- no whitespace errors.

## Suggested Review Order

**Loader-authoritative Linux validation**

- Resolve one safe existing RUNPATH directory before inspecting any packaged libraries.
  [`linux-audio-runtime.mjs:246`](../../scripts/linux-audio-runtime.mjs#L246)

- Validate the complete native closure only inside the loader-reachable directory.
  [`linux-audio-runtime.mjs:269`](../../scripts/linux-audio-runtime.mjs#L269)

**Regression coverage**

- Prove duplicate nested resources and traversal order cannot influence loader validation.
  [`linux-audio-runtime.test.mjs:300`](../../scripts/tests/linux-audio-runtime.test.mjs#L300)

- Cover safe SONAME symlinks without confusing backing filenames with declared SONAMEs.
  [`linux-audio-runtime.test.mjs:327`](../../scripts/tests/linux-audio-runtime.test.mjs#L327)

- Preserve exact SONAME validation for ordinary regular library entries.
  [`linux-audio-runtime.test.mjs:358`](../../scripts/tests/linux-audio-runtime.test.mjs#L358)

- Reject direct sidecar dependencies absent from the loader-authoritative closure.
  [`linux-audio-runtime.test.mjs:380`](../../scripts/tests/linux-audio-runtime.test.mjs#L380)

- Normalize workflow text locally before extracting named steps on every platform.
  [`release-runtime-contract.test.mjs:11`](../../scripts/tests/release-runtime-contract.test.mjs#L11)

- Exercise LF, CRLF, and CR extraction with identical expected output.
  [`release-runtime-contract.test.mjs:20`](../../scripts/tests/release-runtime-contract.test.mjs#L20)
