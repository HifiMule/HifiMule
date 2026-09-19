---
title: 'Make desktop release signing optional'
type: 'bugfix'
created: '2026-09-20'
status: 'done'
baseline_commit: '92d3a0eb50b902d4d55a6b16e33c1b220fefad66'
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Windows and macOS release jobs now abort when Authenticode or Apple Developer credentials are absent, regressing the project's previous ability to publish unsigned open-source builds.

**Approach:** Restore unsigned release builds when no platform credentials are configured, while retaining automatic signing and strict verification whenever a complete credential set is available.

## Boundaries & Constraints

**Always:** Treat a wholly absent credential set as an intentional unsigned build; keep signed builds and their verification unchanged when every required credential is present; reject partially configured credential sets; preserve controlled-runtime, packaging, checksum, upload, draft-release, and smoke-test gates; make release policy, automated contracts, and release-evidence validation agree.

**Ask First:** Any change that weakens non-signing release gates, changes artifact formats, publishes a non-draft release, or rewrites historical evidence records.

**Never:** Require certificates for ordinary open-source releases; silently ignore partial/broken credentials; represent unsigned artifacts as signed, notarized, or trusted by SmartScreen/Gatekeeper; rewrite historical Story 15.17 or release evidence as though unsigned acceptance had always been the policy.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|---------------|----------------------------|----------------|
| Unsigned Windows | Both Windows signing secrets absent | MSI and NSIS build through the existing no-op signing config; Authenticode verification is skipped with an explicit notice | Other package/runtime checks remain blocking |
| Signed Windows | Both Windows signing secrets present | Import certificate, sign artifacts, and verify every MSI/EXE strictly | Invalid certificate/signature fails the row |
| Partial Windows | Exactly one Windows signing secret present | No build proceeds under an ambiguous trust state | Fail with a targeted configuration error |
| Unsigned macOS | All Apple signing/notarization secrets absent | App and DMG build unsigned; Developer ID/notarization verification is skipped with an explicit notice | Other package/runtime checks remain blocking |
| Signed macOS | All Apple secrets present | Existing Developer ID signing, notarization, and verification run strictly | Invalid signing/notarization fails the row |
| Partial macOS | Some but not all Apple secrets present | No build proceeds under an ambiguous trust state | Fail and identify missing configuration |
| Evidence pass | Artifact explicitly records signed or unsigned distribution status | Both are valid policy states; signed status still requires an identity | Unknown/inconsistent states fail validation |

</frozen-after-approval>

## Code Map

- `.github/workflows/release.yml` -- configures platform signing, builds candidates/draft releases, and verifies trust.
- `scripts/tests/release-runtime-contract.test.mjs` -- guards release-workflow signing policy.
- `scripts/playback-installed-evidence.py` -- validates per-artifact release evidence, including distribution-signing state.
- `scripts/tests/test_playback_installed_evidence.py` -- exercises accepted and rejected release evidence.
- `docs/release-guide.md` -- normative release-manager instructions.
- `docs/playback-installed-test-checklist.md` -- active installed-release matrix and trust expectations.

## Tasks & Acceptance

**Execution:**
- [x] `.github/workflows/release.yml` -- classify complete, absent, and partial credential sets; configure signing conditionally; gate trust verification on the resulting platform state.
- [x] `scripts/tests/release-runtime-contract.test.mjs` -- replace the mandatory-signing contract with assertions for unsigned fallback, partial-secret rejection, and conditional strict verification.
- [x] `scripts/playback-installed-evidence.py`, `scripts/tests/test_playback_installed_evidence.py` -- accept an explicit unsigned/not-configured artifact state without identity while retaining strict signed-state validation.
- [x] `docs/release-guide.md`, `docs/playback-installed-test-checklist.md` -- document optional signing and the user-visible OS trust warnings without altering historical evidence.

**Acceptance Criteria:**
- Given no Windows or Apple signing secrets, when a release candidate or tag build runs, then all platform packaging and non-trust verification steps can complete without a credential error.
- Given complete signing credentials, when a release runs, then current signing, notarization, and signature verification remain mandatory.
- Given a partial credential set, when signing configuration runs, then the affected matrix row fails before packaging with a clear configuration error.
- Given unsigned artifacts, when release evidence is validated, then an explicit unsigned state is accepted but never described as trusted or signed.

## Spec Change Log

## Design Notes

Use step outputs such as `enabled=true|false` as the single source of truth for later verification conditions. Windows must still create an empty merge config when unsigned because the matrix build arguments always reference that file. Keep historical blocker JSON and Story 15.17 records unchanged; current documentation supersedes their old mandatory-signing policy.

## Verification

**Commands:**
- `rtk node --test scripts/tests/release-runtime-contract.test.mjs scripts/tests/release-contract.test.mjs scripts/tests/tauri-platform-config.test.mjs` -- focused release contracts pass.
- `rtk python -m unittest discover -s scripts/tests -p 'test_playback_installed_evidence.py'` -- signed, unsigned, and invalid evidence states pass their expectations.
- `rtk node --test scripts/tests/*.test.mjs` -- full Node regression suite passes.
- `rtk npx prettier --check .github/workflows/release.yml scripts/tests/release-runtime-contract.test.mjs docs/release-guide.md` -- changed workflow/code/docs formatting passes.
- `rtk git diff --check` -- no whitespace errors.

## Suggested Review Order

**Release-path behavior**

- Classifies Windows credentials and preserves both unsigned and strictly signed packaging paths.
  [`release.yml:152`](../../.github/workflows/release.yml#L152)

- Applies the same complete, absent, partial classification to Apple credentials.
  [`release.yml:180`](../../.github/workflows/release.yml#L180)

- Runs platform trust verification only when signing was deliberately enabled.
  [`release.yml:309`](../../.github/workflows/release.yml#L309)

**Evidence contract**

- Accepts explicit unsigned evidence while rejecting malformed or contradictory trust claims.
  [`playback-installed-evidence.py:235`](../../scripts/playback-installed-evidence.py#L235)

- Documents unsigned distribution warnings and the retained non-signing release gates.
  [`release-guide.md:30`](../../docs/release-guide.md#L30)

**Regression coverage**

- Guards workflow branches, step outputs, conditional verification, and whitespace handling.
  [`release-runtime-contract.test.mjs:43`](../../scripts/tests/release-runtime-contract.test.mjs#L43)

- Covers accepted unsigned records plus malformed and contradictory signing states.
  [`test_playback_installed_evidence.py:218`](../../scripts/tests/test_playback_installed_evidence.py#L218)
