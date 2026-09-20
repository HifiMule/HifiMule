---
title: 'Fix unsigned macOS and Linux release packaging'
type: 'bugfix'
created: '2026-09-20'
status: 'done'
baseline_commit: 'e31d21b9ac8234b25dd386cc130eb0941fd64677'
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Release CI still fails without certificates: macOS exports empty Apple variables that make Tauri attempt certificate import, while Linux rejects AppImage's valid installed `$ORIGIN/../lib` RUNPATH because the verifier expects the staging name `bundled-libs`.

**Approach:** Ensure unsigned macOS build processes receive no Apple signing variables at all, and validate Linux sidecar RUNPATHs by their resolved in-bundle topology instead of a literal directory substring.

## Boundaries & Constraints

**Always:** Preserve optional-signing classification and partial-secret rejection; pass all Apple variables to signed macOS builds; keep unsigned candidate and tag builds free of Apple signing variables; keep Linux ABI, SONAME, dependency-closure, and `ldd` checks strict; accept only RUNPATH entries that resolve inside the extracted bundle and reach the verified private-library directory; cover both AppImage and deb layouts.

**Ask First:** Any change to release artifact formats, signing policy, private-library destinations, or controlled FFmpeg contents.

**Never:** Disable macOS signing for credentialed builds; weaken Linux verification to substring matching or existence anywhere in the bundle; accept absolute/out-of-bundle RUNPATHs; remove runtime closure checks merely to make CI green.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|---------------|----------------------------|----------------|
| Unsigned candidate macOS | All Apple secrets absent | Direct Tauri process starts with all Apple signing variables unset | Any residual Apple signing variable fails the contract test |
| Unsigned tag macOS | All Apple secrets absent | Dedicated action invocation contains no Apple signing environment | Tauri must not attempt keychain certificate import |
| Signed macOS | Complete Apple secret set | Build receives all required Apple variables and strict verification follows | Import/sign/notarization failures remain blocking |
| AppImage layout | Sidecar under `usr/bin`, libraries under `usr/lib`, RUNPATH `$ORIGIN/../lib` | Verifier resolves the path and accepts the reachable private closure | Missing or mismatched libraries fail |
| Deb/resource layout | Sidecar RUNPATH resolves to its packaged `bundled-libs` directory | Verifier accepts the reachable private closure | Unreachable staging assumptions fail |
| Escaping Linux path | RUNPATH is absolute or resolves outside the bundle | Verifier rejects it | Report the invalid installed RUNPATH |

</frozen-after-approval>

## Code Map

- `.github/workflows/release.yml` -- dispatches direct candidate builds and tag-triggered Tauri action builds.
- `scripts/tests/release-runtime-contract.test.mjs` -- statically guards signed and unsigned workflow environment boundaries.
- `scripts/linux-audio-runtime.mjs` -- stages the Linux runtime and verifies extracted installed bundles.
- `scripts/tests/linux-audio-runtime.test.mjs` -- exercises Linux runtime validation and packaging assumptions.
- `hifimule-ui/src-tauri/tauri.linux.conf.json` -- maps private runtime resources into Linux packages.

## Tasks & Acceptance

**Execution:**
- [x] `.github/workflows/release.yml` -- separate signed and unsigned macOS build execution so absent credentials are absent from the Tauri process environment.
- [x] `scripts/tests/release-runtime-contract.test.mjs` -- assert unsigned candidate and tag paths contain no Apple signing environment while signed paths retain all required variables.
- [x] `scripts/linux-audio-runtime.mjs` -- resolve installed RUNPATH entries against the sidecar location and verified library directory, rejecting paths outside the bundle.
- [x] `scripts/tests/linux-audio-runtime.test.mjs` -- cover AppImage, deb/private-resource, unreachable, absolute, and escaping RUNPATH cases.

**Acceptance Criteria:**
- Given no Apple credentials, when either macOS release path builds, then Tauri receives no Apple signing variables and does not invoke certificate import.
- Given complete Apple credentials, when macOS builds, then signing inputs and strict trust verification are preserved.
- Given an extracted AppImage with `$ORIGIN/../lib`, when the private libraries are reachable there, then installed verification passes.
- Given a RUNPATH that cannot reach the verified in-bundle private libraries or escapes the bundle, when verification runs, then it fails clearly.

## Spec Change Log

## Design Notes

GitHub Actions cannot remove an action-level `env` entry before the action process starts. Use a dedicated unsigned macOS action step with no `APPLE_*` keys, while the direct shell build can unset variables immediately before invoking Tauri. For Linux, compare canonical resolved paths: textual names differ after linuxdeploy rewrites, but the security property is that the sidecar reaches the already verified private directory without leaving the extracted bundle.

## Verification

**Commands:**
- `rtk node --test scripts/tests/release-runtime-contract.test.mjs scripts/tests/linux-audio-runtime.test.mjs scripts/tests/tauri-platform-config.test.mjs` -- focused release and Linux layout contracts pass.
- `rtk node --test scripts/tests/*.test.mjs` -- full Node regression suite passes.
- `rtk git diff --check` -- no whitespace errors.

## Suggested Review Order

**macOS signing isolation**

- Separate credentialed and unsigned candidate processes at the release entry point.
  [`release.yml:232`](../../.github/workflows/release.yml#L232)

- Keep tag-triggered unsigned actions entirely free of Apple signing environment keys.
  [`release.yml:292`](../../.github/workflows/release.yml#L292)

**Linux installed-runtime validation**

- Resolve RUNPATH components canonically without permitting filesystem escape or shadowing.
  [`linux-audio-runtime.mjs:227`](../../scripts/linux-audio-runtime.mjs#L227)

- Require at least one safe entry to reach the verified private-library directory.
  [`linux-audio-runtime.mjs:246`](../../scripts/linux-audio-runtime.mjs#L246)

**Regression contracts**

- Exercise AppImage, deb, invalid, symlink, missing-component, and non-directory layouts.
  [`linux-audio-runtime.test.mjs:215`](../../scripts/tests/linux-audio-runtime.test.mjs#L215)

- Guard signed and unsigned workflow environment boundaries statically.
  [`release-runtime-contract.test.mjs:79`](../../scripts/tests/release-runtime-contract.test.mjs#L79)
