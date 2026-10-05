---
title: 'Fix Ubuntu RPM extraction during release verification'
type: 'bugfix'
created: '2026-10-05'
status: 'done'
route: 'one-shot'
---

# Fix Ubuntu RPM extraction during release verification

## Intent

**Problem:** The updated release log confirms AppImage and DEB verification pass, then the RPM extraction pipeline exits 1 before native closure verification. Ubuntu 22.04's RPM 4.17.0 rpm2cpio compares the decompressed payload size against archive-size metadata that the rpm-rs 0.16.0 builder omits. Its consumer can extract the contents successfully while rpm2cpio reports failure, stopping the pipefail-enabled job.

**Approach:** Install libarchive-tools and extract RPM with bsdtar as a strict standalone command. Retain RPM metadata querying and mandatory private-runtime verification. Provision the extractor before packaging regressions in the Linux Build job, failing rather than skipping when it is absent in CI. Add a small generated gzip RPM fixture from the exact library version locked by Tauri CLI 2.12.1, and test actual extraction, invalid archives, extractor failure after writing files and runtime-verifier failure.

**Evidence:** Disposable Ubuntu 22.04 amd64 container: RPM 4.17.0 reproduces `rpm2cpio=1`, `cpio=0`; bsdtar extracts the expected content and rejects invalid archive data. Local combined suite: 79 passed, zero failed, one existing Linux-only ELF test skipped. After the final error-after-extraction scenario, all 16 release-contract tests pass. Whitespace validation passes. Actual 0.17.0 RPM ELF closure and Fedora installation still require the subsequent CI/native run.

**Review:** Independent review found no runtime regression. Fixed Linux CI extractor provisioning, enforced the fixture digest, and added the error-after-extraction case. Broader installation qualification, alternate compression/metadata contracts and speculative multiple-artifact cases do not change this extraction fix's scope. Effective Tauri RPM compression is gzip; no package-format change was needed.

**Source checks:** [RPM 4.17.0 rpm2cpio](https://github.com/rpm-software-management/rpm/blob/rpm-4.17.0-release/rpm2cpio.c), [Tauri CLI 2.12.1 dependency lock](https://github.com/tauri-apps/tauri/blob/tauri-cli-v2.12.1/Cargo.lock), and [Tauri RPM bundler](https://github.com/tauri-apps/tauri/blob/tauri-cli-v2.12.1/crates/tauri-bundler/src/bundle/linux/rpm.rs).

## Suggested Review Order

- Extract the RPM without the incompatible rpm2cpio size check; retain strict runtime verification.
  [release.yml:405](../../.github/workflows/release.yml#L405)

- Install the native extractor before Linux packaging regression tests.
  [build.yml:45](../../.github/workflows/build.yml#L45)

- Exercise real package extraction and ensure every failure blocks the verifier.
  [release-contract.test.mjs:201](../../scripts/tests/release-contract.test.mjs#L201)

- Document the generated fixture and reproduced Ubuntu incompatibility.
  [README.md:1](../../scripts/tests/fixtures/rpm/README.md#L1)
