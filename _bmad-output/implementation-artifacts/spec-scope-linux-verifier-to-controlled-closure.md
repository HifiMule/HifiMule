---
title: 'Scope Linux verification to the controlled runtime closure'
type: 'bugfix'
created: '2026-09-20'
status: 'done'
baseline_commit: 'a453375776ae5559840e4f453d08af7d83ed623a'
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Linux AppImage verification now reaches the correct loader directory but rejects `usr/lib/im-am-et.so`, an unrelated GTK input-method symlink intentionally created by Tauri's linuxdeploy GTK plugin. Treating every library and plugin in the mixed AppImage `usr/lib` directory as HifiMule-controlled causes sequential false failures.

**Approach:** Validate only the exact native dependency closure reachable from the daemon plus HifiMule's explicitly packaged FFmpeg, libmtp, and PulseAudio roots, while ignoring unrelated linuxdeploy-owned GTK/GStreamer libraries and plugins.

## Boundaries & Constraints

**Always:** Keep the sidecar RUNPATH-selected directory authoritative; seed validation with all non-system daemon dependencies plus exact FFmpeg ABI SONAMEs, `libmtp.so.9`, and `libpulse.so.0`; resolve each queued SONAME only from the authoritative directory; recursively validate architecture, exact SONAME, `$ORIGIN` RUNPATH, safe in-directory symlinks, transitive dependencies, and `ldd`; reject missing, path-containing, broken, escaping, or conflicting reachable dependencies.

**Ask First:** Any change to packaged resources, controlled-runtime roots, supported Linux formats, or linuxdeploy/Tauri configuration.

**Never:** Validate every unrelated `.so` in AppImage `usr/lib`; whitelist `im-am-et.so` or individual GTK filenames; allow an unrelated nested/resource copy to satisfy the controlled closure; weaken checks for any library actually reachable by the daemon or explicit runtime roots.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|---------------|----------------------------|----------------|
| GTK input module | `im-am-et.so` points into nested GTK immodules and is not in controlled closure | Ignore it entirely | No false package failure |
| Unrelated regular library | GTK/GStreamer library has foreign SONAME or non-`$ORIGIN` RUNPATH and is unreachable | Ignore it entirely | No false package failure |
| Required regular library | Exact queued SONAME exists directly in loader directory | Validate ELF, SONAME, RUNPATH, dependencies, and resolution | Any mismatch fails |
| Required SONAME symlink | Queued name links to regular target directly in loader directory | Validate target against queued SONAME | Broken, nested, or escaping target fails |
| Missing transitive dependency | Controlled library needs a non-system SONAME absent from loader directory | Fail closure validation | Name the requiring library and missing SONAME |
| Path dependency | `DT_NEEDED` contains a slash or escape attempt | Reject before filesystem resolution | Report invalid dependency name |

</frozen-after-approval>

## Code Map

- `scripts/linux-audio-runtime.mjs` -- resolves installed RUNPATHs and validates extracted Linux native libraries.
- `scripts/tests/linux-audio-runtime.test.mjs` -- models AppImage/deb layouts and closure boundary cases.
- `hifimule-ui/src-tauri/tauri.linux.conf.json` -- declares HifiMule-controlled bundled resources.
- `.github/workflows/release.yml` -- extracts and invokes AppImage verification after packaging.

## Tasks & Acceptance

**Execution:**
- [x] `scripts/linux-audio-runtime.mjs` -- replace directory-wide validation with queue-based traversal of daemon dependencies and explicit controlled roots.
- [x] `scripts/tests/linux-audio-runtime.test.mjs` -- reproduce the real nested GTK input-module symlink and unrelated-library cases, plus reachable symlink/RUNPATH/transitive-dependency failures.

**Acceptance Criteria:**
- Given the AppImage layout from the attached CI log, when verification encounters unrelated `im-am-et.so`, then it ignores that plugin and continues validating the controlled closure.
- Given unrelated linuxdeploy libraries with different policies, when they are unreachable from controlled roots, then they cannot pass or fail HifiMule closure validation.
- Given any daemon/controlled-root dependency is missing, unsafe, incorrectly signed by SONAME, wrong-architecture, or unresolved, when verification runs, then the release remains blocked.
- Given duplicate nested resource copies, when the loader-visible closure is incomplete, then unreachable copies cannot satisfy it.

## Spec Change Log

## Design Notes

The AppImage `usr/lib` directory is shared infrastructure, not an ownership boundary. Ownership is defined by reachability: start from the daemon's non-system `DT_NEEDED` entries and the deliberately packaged runtime roots, then walk exact SONAME edges within the authoritative loader directory. This matches dynamic-loader behavior while preserving strict verification for everything HifiMule actually ships and uses.

## Verification

**Commands:**
- `rtk node --test scripts/tests/linux-audio-runtime.test.mjs` -- mixed AppImage content and controlled-closure edge cases pass.
- `rtk node --test scripts/tests/*.test.mjs` -- full Node suite passes.
- `rtk python3 -m unittest discover -s scripts/tests -p 'test_*.py'` -- Python evidence regressions pass.
- `rtk git diff --check` -- no whitespace errors.

## Suggested Review Order

**Controlled closure**

- Start here: the verifier now walks only loader-visible, HifiMule-controlled dependencies.
  [`linux-audio-runtime.mjs:270`](../../scripts/linux-audio-runtime.mjs#L270)

- The sidecar RUNPATH remains the authoritative library directory boundary.
  [`linux-audio-runtime.mjs:247`](../../scripts/linux-audio-runtime.mjs#L247)

- Exact platform loaders prevent prefix-based dependency bypasses.
  [`linux-audio-runtime.mjs:19`](../../scripts/linux-audio-runtime.mjs#L19)

**Boundary evidence**

- Reproduces and ignores linuxdeploy's real nested GTK input-module symlink.
  [`linux-audio-runtime.test.mjs:314`](../../scripts/tests/linux-audio-runtime.test.mjs#L314)

- Confirms malformed reachable dependency names fail before filesystem resolution.
  [`linux-audio-runtime.test.mjs:367`](../../scripts/tests/linux-audio-runtime.test.mjs#L367)

- Confirms bundled baseline libraries cannot evade validation by masquerading as system libraries.
  [`linux-audio-runtime.test.mjs:446`](../../scripts/tests/linux-audio-runtime.test.mjs#L446)
