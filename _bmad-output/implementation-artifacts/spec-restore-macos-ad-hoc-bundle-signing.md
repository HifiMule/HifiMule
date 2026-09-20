---
title: 'Restore valid macOS ad-hoc bundle signing'
type: 'bugfix'
created: '2026-09-20'
status: 'done'
baseline_commit: 56d5778d81e6da702b4afa72b2bd4e88036aa98d
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** The supplied HifiMule 0.15.0 ARM64 DMG contains a main app with an invalid bundle signature. Its daemon and all twelve bundled dylibs individually pass strict signature verification. The main app fails with `code has no resources but signature indicates they must be present`. The former packaging process explicitly configured ad-hoc signing and checked the bundle; current packaging sets the signing identity to null and performs signature verification only for credentialed builds.

**Approach:** Restore valid ad-hoc signing for macOS builds without Apple credentials, before DMG creation. Verify bundle integrity for every macOS release, while retaining optional Developer ID signing and notarization for credentialed releases. Add regression coverage and clarify the distinction between ad-hoc integrity and Developer ID trust.

## Boundaries & Constraints

**Always:** Keep credential-free builds independent of Apple certificates, keychains, and notarization credentials. Preserve partial-credential rejection and the complete credentialed build path. Sign after bundle contents are assembled and before the DMG is produced. Require strict signature verification and sealed resources for both macOS architectures. Preserve existing dylib relocation and signing.

**Ask First:** Publishing replacement release artifacts, changing release versions, or changing the supported macOS architectures or minimum versions.

**Never:** Disable Gatekeeper globally, present ad-hoc signing as notarization, mutate the supplied DMG, or repair only the staging app after its DMG has already been created.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|---------------|----------------------------|----------------|
| No Apple credentials | Candidate or tag build | Tauri produces an ad-hoc signed app with sealed resources before packaging | Invalid bundle signature fails verification |
| Complete Apple credentials | Candidate or tag build | Developer ID identity takes precedence over ad-hoc fallback; notarization remains enabled | Existing trust checks remain blocking |
| Partial Apple credentials | Any release build | Existing preflight rejects incomplete configuration | Identify missing credential names without values |
| Invalid main bundle | Linker-only or damaged signature | Common integrity check fails even without credentials | Preserve codesign diagnostics |
| Invalid nested code | Damaged sidecar or dylib | Verification rejects the package | Identify the failing code object |
| Local macOS build | No signing credentials | Same ad-hoc default applies | No certificate import is attempted |

</frozen-after-approval>

## Code Map

- `hifimule-ui/src-tauri/tauri.conf.json` — currently sets `bundle.macOS.signingIdentity` to null.
- `hifimule-ui/src-tauri/tauri.macos.conf.json` — macOS resource mapping and app/DMG targets.
- `.github/workflows/release.yml` — separate signed and unsigned candidate/tag branches; integrity verification currently resides in the Developer ID-only step.
- `scripts/bundle-macos-libs.mjs` — already ad-hoc signs relocated dylibs and the daemon.
- `scripts/tests/release-runtime-contract.test.mjs` — currently requires null identity and absence of the former ad-hoc verification step; preserves Apple environment isolation.
- `scripts/tests/tauri-platform-config.test.mjs` — validates merged platform configuration.
- `docs/release-guide.md` — optional signing policy and release checks.

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-ui/src-tauri/tauri.conf.json` — restore the ad-hoc fallback identity; confirm the installed Tauri implementation gives explicit Developer ID credentials precedence, adding an explicit signed-path override only if required.
- [x] `.github/workflows/release.yml` — run strict bundle-integrity and sealed-resource checks for every macOS build, leaving Developer ID authority, Gatekeeper assessment, and notarization checks conditional on credentials. Make credential-free diagnostics describe ad-hoc signing accurately.
- [x] `scripts/tests/release-runtime-contract.test.mjs` — replace the null-identity contract with the ad-hoc fallback, cover unconditional integrity checks and conditional trust checks, and preserve signed/unsigned environment-isolation assertions.
- [x] `scripts/tests/tauri-platform-config.test.mjs` — check that merged macOS configuration preserves the fallback identity.
- [x] `docs/release-guide.md` — document mandatory macOS signature integrity even when Developer ID credentials are absent.

**Acceptance Criteria:**
- Given absent Apple credentials, when either macOS release path packages the app, then it contains a valid ad-hoc bundle signature and sealed resources without attempting certificate import.
- Given complete Apple credentials, when either macOS release path packages the app, then Developer ID signing and notarization remain required and the fallback does not replace the configured identity.
- Given a malformed bundle signature, when release verification runs without credentials, then it fails instead of accepting the artifact.
- Given the original supplied DMG, when its signature is inspected, then the recorded defect is reproducible; when a disposable copy is ad-hoc signed, then strict deep verification succeeds.

## Spec Change Log

- Native packaging validation found that enabling Hardened Runtime for an ad-hoc daemon causes dyld to reject bundled libraries with a Team ID mismatch. Disable Hardened Runtime for the ad-hoc default and explicitly retain it on Developer ID build paths. Preserve credential isolation, bundle signing before DMG creation, and explicit library signature checks.

## Design Notes

Commit `38de51d` introduced `signingIdentity: "-"` and common signature verification. Restore that behavior while preserving the newer optional Developer ID workflow. A certificate-free build still needs valid local code signatures; distribution trust remains a separate property. Prefer Tauri's own signing phase so the DMG contains the final signed app.

## Verification

**Already observed:**
- Mounted `/Users/akartmann/Downloads/HifiMule_0.15.0_aarch64.dmg` read-only. All inspected Mach-O files are ARM64.
- Original app: `codesign --verify --deep --strict` fails with the missing-resource-seal diagnostic.
- Daemon and twelve dylibs: individual `codesign --verify --strict` checks pass.
- Disposable copy `/private/tmp/hifimule-signing-check.NhRROa/HifiMule.app`: signing the app with `codesign --force --sign -` makes deep strict verification pass. Signature inspection reports `Signature=adhoc` and `Sealed Resources version=2`.
- This proves signature repair, not application startup or playback. Those remain separate validation steps.

**Implementation checks:**
- Run focused release-runtime and platform-configuration Node tests, then the repository's packaging script tests.
- Run `rtk git diff --check`.
- Validate a rebuilt macOS app and the app inside its DMG with strict signature verification when build prerequisites are available. Record any inability to complete a full build; do not claim it ran.
- Retain Developer ID and notarization checks in CI; local certificate-free validation cannot prove Apple trust.


## Implementation Results

- Restored ad-hoc signing and disabled Hardened Runtime for certificate-free builds; both Developer ID release branches explicitly enable Hardened Runtime.
- All macOS release branches now verify the bundle seal, daemon, and each resource dylib. The latter is necessary: an experimental unsigned resource dylib passed the outer deep check after re-sealing, but failed individual verification.
- All 204 Node packaging-script tests passed. Workflow YAML parsed successfully; whitespace checks passed.
- Repackaged existing local ARM64 binaries using the installed Tauri CLI into an app and DMG. No fresh compilation was performed.
- Mounted the rebuilt DMG read-only: strict deep app verification, individual daemon/ten local dylib checks, and controlled-runtime verification all passed. The downloaded original had twelve dylibs; this local runtime closure has ten and passes the existing verifier.
- The packaged daemon reached Rust main with `--service` and returned the expected Windows-only-flag error, proving dynamic-library loading succeeds. Full UI startup, playback, x64 packaging, and Developer ID notarization were not exercised.
- Tauri CLI 2.9.6 and 2.10.1 source inspection confirmed `APPLE_SIGNING_IDENTITY` takes precedence over configured identity: `crates/tauri-cli/src/interface/rust.rs` at the corresponding release tags.
- Blind and edge-case reviews plus a separate acceptance audit completed. The actionable local Developer ID concern was addressed by documenting the explicit hardened-runtime override. No remaining introduced acceptance violations were found.

## Suggested Review Order

**Signing mode**

- Restore valid ad-hoc signatures with runtime settings compatible with bundled libraries.
  [tauri.conf.json:61](../../hifimule-ui/src-tauri/tauri.conf.json#L61)

**Credentialed release paths**

- Keep Hardened Runtime enabled for Developer ID builds.
  [release.yml:232](../../.github/workflows/release.yml#L232)

**Integrity checks**

- Verify bundle seals and resource dylib signatures for every macOS release.
  [release.yml:356](../../.github/workflows/release.yml#L356)

**Regression coverage**

- Guard unconditional integrity checks and conditional trust verification.
  [release-runtime-contract.test.mjs:129](../../scripts/tests/release-runtime-contract.test.mjs#L129)

**Local build guidance**

- Document the explicit runtime override required for local Developer ID builds.
  [release-guide.md:32](../../docs/release-guide.md#L32)
