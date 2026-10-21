# Investigation: Folder thumbnails not created

## Hand-off Brief

1. **What happened.** The user confirmed no `folder.jpg` appeared after enabling thumbnails and syncing. The local daemon log confirms 1,665 artwork export errors: `Operation not supported (os error 45)`.
2. **Where the case stands.** Confirmed root cause: FLASHDISK is exFAT through macOS FSKit and rejects the publisher's exclusive-rename operation with error 45.
3. **What's needed next.** Relaunch the rebuilt application and sync again. The corrected publisher passed automated checks, independent review and exact-code verification on FLASHDISK; the running app remains untouched.

## Case Info

| Field | Value |
| --- | --- |
| Date opened | 2026-10-21 |
| Status | Concluded — correction verified; full library resync pending |
| System | macOS, mounted FLASHDISK |
| Evidence sources | User clarification; local daemon log; commit `98b632c`; current publisher and locked dependency source |

## Problem Statement

The user ran a sync with thumbnails enabled and confirmed that no `folder.jpg` files were created.

## Evidence Inventory

| Source | Status | Notes |
| --- | --- | --- |
| `~/Library/Application Support/HifiMule/daemon.log` | Available | Lines 612–2276 contain 1,665 identical artwork error kinds, 21:54:09–21:54:13 on 2026-10-21 |
| `/Volumes/FLASHDISK` | Available | Mounted target, inspected without changing media |
| `hifimule-daemon/src/folder_icons.rs:38` | Available | Shared no-clobber publisher stages, syncs, then calls `persist_noclobber` |
| Locked `tempfile`/`rustix` source | Available | Investigating native exclusive rename and fallback behavior |

## Investigation Backlog

| # | Path to Explore | Priority | Status | Notes |
| --- | --- | --- | --- | --- |
| 1 | Identify target filesystem and exact unsupported operation | High | Done | Exclusive rename and hard links reproduce error 45; normal rename and exclusive-create/write/sync/readback pass |
| 2 | Preserve no-overwrite and failed-write cleanup when correcting publisher | High | Done | Unsupported-operation-only fallback uses exclusive creation and descriptor-identity cleanup |
| 3 | Verify against real device filesystem and automated regression tests | High | Done | Exact shared publisher, JPEG preservation, KDE and Finder apply/verify/cleanup passed in an owned temporary FLASHDISK directory |

## Confirmed Findings

### Finding 1: Every logged artwork export failed with the same filesystem error

**Evidence:** Local daemon log, lines 612–2276, timestamps 2026-10-21 21:54:09–21:54:13.

**Detail:** All 1,665 `[Artwork]` entries report `Operation not supported (os error 45); retry next sync`. This is a publication-path failure hypothesis anchored in runtime evidence, rather than a missing-image assumption.

## Hypothesized Paths

### Hypothesis 1: Thumbnail activation did not persist or sync skipped existing tracks

**Status:** Refuted.

**Theory:** An inactive preference or zero-addition shortcut prevented artwork execution.

**Resolution:** The runtime emitted 1,665 artwork export errors during the reported sync, proving the artwork path ran.

### Hypothesis 2: Native no-clobber publication is unsupported by the target filesystem

**Status:** Confirmed.

**Theory:** `persist_noclobber` uses a filesystem operation that the removable volume does not support.

**Would confirm:** A small same-directory staged-file probe reproduces error 45 on exclusive rename, while ordinary writes/sync succeed.

**Would refute:** The publisher succeeds under the same volume and target conditions, or a different stage produces the error.

**Resolution:** An isolated temporary directory on FLASHDISK reproduced `renameatx_np(RENAME_EXCL)` returning errno 45. Creating/writing/syncing the staged file succeeded; ordinary rename and exclusive destination creation/write/sync/readback succeeded. Hard-link creation also returned errno 45. Only probe-owned files were created and removed; no media files or actual sync were changed.

### Finding 2: The dependency propagates the unsupported exclusive rename

**Evidence:** `hifimule-daemon/src/folder_icons.rs:38`; locked tempfile 3.24.0 `src/file/imp/unix.rs:123`; rustix 1.1.3 Apple `renameat_with` maps NOREPLACE to `renameatx_np(RENAME_EXCL)`.

**Detail:** `persist_noclobber` first uses exclusive rename on macOS. It only tries hard-link fallback on ENOSYS/EINVAL, not errno 45; hard links are also unavailable on this target. Native icons reuse the same publisher, so the correction belongs in the shared publication helper.

### Finding 3: FSKit changes provisional file identifiers after the first sync

**Evidence:** Isolated Rust publisher and descriptor/path metadata probes on FLASHDISK.

**Detail:** The empty file initially has a provisional inode number; after writing and syncing, fresh descriptor and path metadata agree on a different identifier. The fallback compares fresh descriptor metadata against `symlink_metadata` when verifying or cleaning up, rather than retaining the initial identifier. Keeping the descriptor open distinguishes an observed user replacement from the file this attempt created.

## Correction and Verification

- Keep staged no-clobber publication where supported. Only Unix ENOTSUP/EOPNOTSUPP/ENOSYS permits fallback to `create_new`, which cannot overwrite an existing file, directory or symlink. Permission and collision errors still propagate.
- Write and sync the exclusively created destination; ordinary failures remove it only if its current identity still matches the open descriptor. A changed destination is preserved. Error messages retain both operation context and the underlying errno.
- Fallback limitation: the destination is visible during writing; abrupt process/device interruption can leave incomplete unowned artwork. Subsequent syncs preserve that existing file, so a confirmed incomplete file must be removed before retrying. POSIX also has no portable conditional unlink, so an adversarial replacement between identity inspection and unlink cannot be ruled out. Existing staged atomic publication remains preferred.
- Focused helper tests: 11 passed. MSC publication integration: 1 passed. Artwork lifecycle tests: 9 passed. Final combined `artwork` suite: 22 passed. Controlled-runtime daemon check and debug build passed. Publisher formatting and `git diff --check` passed.
- Actual exFAT verification: shared publication/readback, preserving an existing JPEG, KDE `.directory` apply/verify/cleanup and macOS Finder icon apply/verify/cleanup all passed. Only an isolated probe-owned temporary directory was written and removed; existing media and the running app were untouched.
- Root independently repeated the exact-code volume harness successfully. The temporary harness source is `/private/tmp/hifimule-artwork-exfat-check-20261021/src/main.rs`; its binary creates a unique owned directory and removes it after validation.
- Three focused independent reviews found no ordinary-path regression. Review raised interruption recovery and the identity-check/unlink interval; these remain documented platform limits. The approved invalid/failed-image handling still validates complete image bytes before publication and cleans up ordinary write failures. No filesystem-specific atomic/crash-recovery guarantee is claimed.

## Remaining Validation

| Gap | Impact | How to Obtain |
| --- | --- | --- |
| User's full sync after relaunch | Confirms rebuilt application behavior across the actual music library | Relaunch the rebuilt version, then sync again; no audio retransfers are required for artwork backfill |

## Conclusion

**Confidence:** High. The actual target reproduces the exact unsupported exclusive-rename syscall and error observed during sync. The corrected publisher and native adapters succeed on that same filesystem. A full library sync remains a user follow-up after relaunching the rebuilt application.
