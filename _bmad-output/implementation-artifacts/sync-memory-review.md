# Large-library sync implementation review

Three independent reviewers examined the patch without conversation context: a diff-only blind hunter, a source-aware edge-case hunter and an acceptance auditor with the approved spec. Original diffs are retained under `target/sync-review-code.diff` and `target/sync-review-full.diff`.

## Triage

These findings are implementation patches under the existing approved requirements. They do not require changing selection behavior, durability, or frozen intent.

| Finding | Classification | Required correction |
| --- | --- | --- |
| Forced blocked ID change can delete the retained file | patch | Remove paired old deletion and ID change; preserve old file and exact blocked counts |
| Equal native playlist IDs merge their tracks | patch | Use a unique plan record owner per playlist |
| Non-object prepare parameters panic | patch | Validate before any ownership mutation |
| Cancellation/shutdown during final preflight can publish late | patch | Recheck and coordinate publication with shutdown ownership clearing |
| Cancellation after publication retains the pending plan | patch | Clear ownership under the publication mutex after setting cancellation |
| Early playlist exits swallow warning-storage errors | patch | Propagate evidence-storage failures on every exit |
| Completed warning writers retain open handles | patch | Lazy creation and close writers after drain; preserve complete evidence with operation history |
| Invalid/failed preparation discards an existing valid plan | patch | Replace atomically only after successful preparation |
| Missing destructive confirmation consumes a token | patch | Keep it available for a confirmed retry |
| Late detail-page failure appears over a running sync | patch | Ignore detail results and errors after dialog closure |
| Sequential plan pages repeatedly scan preceding rows | patch | Use keyset cursors while retaining explicit offset detail paging |
| Superseded preparations retain expiry sleepers | patch | Bind abortable expiry ownership to the prepared plan |
| Unchanged playlists stage a full temporary M3U | patch | Check compact resolved identities before staging |

The proposed expiration of completed diagnostic evidence is rejected: existing operation history retains warning details until shutdown, and changing that retention would alter behavior. Closing inactive file handles addresses the new resource issue without discarding evidence. The existing lack of operation-history pruning is outside this change.

The original warning-pagination writer-lock claim was rejected because `reopen()` released the temporary mutex guard before scanning. Follow-up review found that the lazy-file patch introduced a guard spanning the scan; this was corrected by opening the file and capturing the committed count under the lock, then dropping the guard before reading. Random-access warning pages still scan preceding lines; this is bounded in memory and does not affect sequential transfer planning. A byte-offset index remains an optional performance improvement.

## Resolution

All required patches are complete. Thirteen focused Rust regressions reproduced the initial findings before passing with the fixes; a further cancellation-after-publication regression also failed before the final ownership correction and now passes. The deferred UI page-error regression passes.

Focused follow-up found no remaining correctness defects. The acceptance auditor confirmed cancellation/publication locking introduces no deadlock, and warning pages expose only committed records. Retaining warning files with operation history until shutdown preserves the approved evidence contract; adding completed-operation expiration would require a separate retention decision.

Final verification: 1,410 daemon unit tests and 5 contract tests passed, with 8 ignored; daemon check passed. UI TypeScript and production build passed, and 18 targeted tests passed. The broad JavaScript suite passed 366 tests, skipped 10, and retained only the two previously documented Windows harness failures. See `sync-memory-evidence.md` for logs and memory measurements. The approved frozen intent remains unchanged.
