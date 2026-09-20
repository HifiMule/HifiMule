---
title: 'Stage the complete Linux daemon dependency graph'
type: 'bugfix'
created: '2026-09-20'
status: 'done'
route: 'one-shot'
---

# Stage the complete Linux daemon dependency graph

## Intent

**Problem:** The prior AppImage custom-file mapping correctly places staged libraries in the loader directory, but staging itself only starts from FFmpeg, libmtp, and PulseAudio. The daemon also links GTK through its tray stack. The installed verifier walks all daemon dependencies, finding libgdk supplied by linuxdeploy but missing its excluded libfontconfig dependency. Staging and verification therefore traverse different dependency graphs.

**Approach:** Seed recursive staging from every non-baseline DT_NEEDED entry of the actual daemon as well as the existing explicit roots. Preserve SONAME collision detection, architecture inspection, recursive traversal, private RUNPATH rewriting, and strict installed verification. Match the platform loader exemption by exact filename, consistent with verification. Keep the previous AppImage placement fix.

Regression coverage includes direct daemon-only GTK dependencies, transitive fontconfig/freetype dependencies, cycles, duplicate roots, host-baseline exclusion, and missing-library failure. A native Linux test compiles a daemon -> GDK -> fontconfig ELF chain and exercises production readelf/ldd-based discovery. All 36 Linux runtime tests pass in an isolated Node 24 Debian container with read-only project mounting and no network. This validates native dependency collection, not a complete Ubuntu AppImage release build.

## Suggested Review Order

- Seed recursive copying from the same daemon dependency graph that installed verification follows.
  [linux-audio-runtime.mjs:181](../../scripts/linux-audio-runtime.mjs#L181)
- Retain explicit controlled roots and existing RUNPATH rewriting in the packaging entry point.
  [linux-audio-runtime.mjs:220](../../scripts/linux-audio-runtime.mjs#L220)

- Exercise graph traversal, baseline handling, and native ELF discovery.
  [linux-audio-runtime.test.mjs:683](../../scripts/tests/linux-audio-runtime.test.mjs#L683)

Validation: the full macOS-hosted suite passes 209 tests with the Linux-only native test skipped; that native test passes in the Linux container (36/36 Linux runtime tests). Independent review found no actionable introduced defects. Whitespace checks pass.
