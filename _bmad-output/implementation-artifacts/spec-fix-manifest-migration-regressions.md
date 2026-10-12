---
title: 'Fix profile and discovery regressions exposed by manifest migration'
type: 'bugfix'
created: '2026-10-11'
status: 'done'
baseline_commit: '8d04e27'
context: []
---

<frozen-after-approval reason="Correction within the previously approved lossless migration and unchanged sync behavior">

## Intent

**Problem:** After migrating manifests, an unchanged passthrough profile can mark every track for replacement, and an empty optical drive can produce a misleading device read failure.

**Approach:** Compare profiles by their effective transcoding behavior, recover the provably false passthrough dirty state already persisted, and exclude optical drives from manifest discovery while retaining real device read failures.

## Boundaries & Constraints

**Always:** Treat `null` and `"passthrough"` as the same no-transcoding selection. Preserve genuine profile changes, missing historical profile evidence, recovery dirty/pending state, track records and compression round-trip fidelity. Keep supported managed mass-storage destinations and corrupt-manifest errors. Repair persisted false profile state through normal application persistence.

**Ask First:** Destructive repairs or changing the storage format, migration authority or checkpoint policy.

**Never:** Manually edit user device files, suppress corruption or permission errors on actual destinations, clear genuine pending profile changes, add media transfers solely for storage migration.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
| --- | --- | --- | --- |
| Equivalent passthrough | Current `"passthrough"`, saved selection `null`, historical `"passthrough"` | No profile replacement; save stays clean | Standard persistence failures |
| Existing false flag | Current `null`, historical `"passthrough"`, profile dirty true | Preview keeps unchanged tracks; normal persistence repairs flag | Preserve unrelated dirty/pending state |
| Genuine change | Historical transcoding profile, current passthrough or another profile | Matching tracks are replaced | Existing sync errors |
| Unknown history | Historical profile absent, profile dirty true | Retain conservative replacement behavior | Existing sync errors |
| Empty optical drive | Windows CD/DVD logical drive without media | No manifest discovery/read issue | Excluded before file probe |
| Actual invalid manifest | Present corrupt compressed manifest on supported destination | Retryable read issue | Never fall back to legacy |

</frozen-after-approval>

The frozen matrix records the original proposed recovery. The accepted amendment
in the change log and final acceptance criteria below replace inferred repair of
populated manifests with explicit confirmation: older automatic syncs make the
original profile-history assumption insufficient. Its preservation constraints
remain binding.

## Code Map

- `hifimule-daemon/src/device/mod.rs` — semantic profile comparisons, conservative recovery, fallible connection-bound confirmation and Windows optical exclusion.
- `hifimule-daemon/src/device/tests.rs` — persistence, stale upgrade state, marker enumeration and confirmation validation.
- `hifimule-daemon/src/device_io.rs` — strict scoped recovery scans for MSC, conservative transport defaults and backend boundary tests; ordinary listing remains unchanged.
- `hifimule-daemon/src/rpc.rs` — profile setters, strict shared loader, guarded confirmation RPC and admission coverage.
- `hifimule-daemon/src/sync.rs` — profile replacement and playlist decisions.
- `hifimule-daemon/src/main.rs` — strict automatic-sync preflight and completion using the operation's captured profile.
- `hifimule-daemon/src/tests.rs` — durable automatic completion, concurrent profile selection, failed/cancelled recovery and invalid-profile rejection.
- `hifimule-ui/src/components/BasketSidebar.ts`, `hifimule-i18n/catalog.json` — Device Settings confirmation action and translated copy.
- `docs/api-contracts-hifimule-daemon.md`, `docs/development-guide.md` — confirmation contract and recovery instructions.

## Tasks & Acceptance

**Execution:**

- [x] Reproduce settings equivalence, stale profile state, genuine transitions and optical discovery behavior with failing tests.
- [x] Centralize semantic profile comparisons; retain uncertain true dirty flags on populated manifests and permit only safe empty-library alias repair.
- [x] Exclude optical drives before probing; retain supported destination errors and useful daemon diagnostics.
- [x] Preserve physical/unknown recovery evidence; record the automatic operation's actual profile snapshot and reject invalid selected profiles before admission.
- [x] Add a connection-bound, fallible confirmation mutation with durable persistence and rejection tests; expose guarded explicit confirmation through RPC and Device Settings.
- [x] Require complete scoped MSC recovery scans, retain unknown MTP recovery and bind confirmation to the same configured/recorded roots under the commit lock.
- [x] Run targeted red/green daemon checks and record verification; complete full-suite validation and independent review triage.

**Acceptance Criteria:**

- Saving a semantically equivalent null/passthrough selection on an already clean manifest does not introduce a profile rewrite.
- Existing true profile dirty on populated manifests remains conservative because historical passthrough may be stale. Explicit confirmation that current files match selected passthrough clears the flag durably without transferring media.
- Confirmation rejects absent consent, non-passthrough selection, active sync, dirty/pending recovery, physical interruption markers, incomplete recovery scans, changed connections and changed media roots; profile/recovery/scope checks are repeated under the mutation commit lock. The application action is available for MSC; current MTP enumerators cannot establish a clean scan.
- Genuine profile changes and uncertain older-auto state retain replacement behavior. Successful automatic completion records the captured operation profile, keeping profile dirty if the current effective selection differs; failed/cancelled completion retains recovery.
- An empty Windows optical drive is excluded while other logical-drive candidates remain admitted. Corrupt authoritative compressed manifests remain visible read failures with no legacy fallback.

## Spec Change Log

- 2026-10-11: User reported both symptoms following the approved migration. Read-only inspection confirmed current null/historical passthrough/profile dirty true on G: and an unready optical drive D:. Correction stays within the already approved preservation requirements.
- 2026-10-12: Independent review established that older automatic syncs can leave the same historical passthrough state after real conversion. Populated manifests therefore require explicit confirmation rather than inferred repair. The user confirmed their player files were copied without transcoding. Add a guarded Device Settings recovery action and preserve all uncertain or interrupted states.

## Design Notes

Storage decoding remains lossless. Null and explicit passthrough are behaviorally equivalent for new settings changes, but no existing manifest field reliably distinguishes unchanged source copies from older automatic conversion that left stale historical passthrough. Timestamps, track containers and format version are insufficient. Existing true profile dirty remains true through settings updates, including equivalent saves and real-profile transitions on empty manifests: settings cannot prove recovery complete. Only an empty library with explicit passthrough history, no interrupted evidence and a complete scoped recovery scan during admission permits automatic alias repair. Successful operation completion or guarded explicit confirmation establishes a trustworthy baseline.

The reported player's populated state cannot be automatically repaired with confidence. The user's confirmation that its files were copied without transcoding authorizes an explicit application recovery action. The action establishes the current passthrough baseline through normal verified manifest persistence; it does not copy or delete audio. Frozen intent is preserved, with the accepted amendment recorded in the change log.

Recovery-marker scanning runs once before alias decisions. Physical markers establish general dirty state; incomplete enumeration is unknown recovery and skips alias repair. Preview always respects retained profile dirty. MSC scanning strictly enumerates root names plus configured media/playlist roots and parents of recorded audio/playlist paths. It propagates root, entry and type errors, rejects managed symlinks/non-directories and unsafe paths, and prunes duplicate descendants. Unrelated subtrees such as System Volume Information are not traversed; a missing managed folder is clean only after complete root enumeration. Ordinary best-effort `list_files` is unchanged.

WPD ignores some object/property errors and LIBMTP cannot prove complete recursive enumeration, so current MTP transports return unsupported completeness when no marker is observed. Known positive markers still establish recovery. The confirmation RPC holds pipeline admission, scans the exact connection's captured roots, then repeats profile/recovery/roots validation under the normal commit lock. Changed scopes reject confirmation rather than trusting a stale scan. Rejected fallible mutations restore in-memory state without encoding or writing.

Automatic profile loading shares the strict manual loader and fails before provider resolution/admission, publishing terminal Error. Successful finalization records the operation's captured profile instead of the mutable current selection. Effective comparison against the current selection retains pending profile work when settings changed during execution. Failed/cancelled operations still skip the completion mutation.

Windows enumeration tests inject an optical predicate for a logical-drive mask (D excluded, C/G/N/R retained); they do not inspect actual drive classifications. Track/state fixtures exercise legacy JSON and compressed storage, null/explicit passthrough, dirty-only/pending-only recovery, stale older-auto upgrades, failed directory enumeration and durable explicit confirmation. Playlist preservation uses a matching audio path with distinct valid formatting.

## Verification

- Targeted command: `rtk node scripts/build-daemon.mjs test -p hifimule-daemon manifest_migration_regression -- --test-threads=1`.
- Final behavioral red: **11 passed, 8 failed**, exit 1, exposing conservative-history, unknown-root, profile-snapshot and confirmation violations. Log: `target/manifest-migration-regression-conservative-red.log`. The preceding concurrent RPC harness typo was corrected before this behavioral run.
- Prior conservative targeted green: **19 passed, 0 failed**, exit 0. Log: `target/manifest-migration-regression-conservative-green.log`.
- Strict recovery scan behavioral red: **18 passed, 6 failed**, exit 1. Log: `target/manifest-migration-regression-strict-red.log`. It reproduced incomplete/unsafe MSC negatives, unrelated subtree traversal, missing recorded roots, unsupported MTP clean claims and settings clearing unknown recovery.
- Scan/commit roots-binding red: filtered `manifest_migration_regression_explicit_confirmation_rejects_changed_state` — **0 passed, 1 failed**, exit 1, when roots changed after capture. Log: `target/manifest-migration-regression-roots-red.log`.
- Final targeted green: **24 passed, 0 failed**, exit 0. Log: `target/manifest-migration-regression-strict-final-green.log`. Includes real MSC missing/non-directory roots, scoped nested/root markers, unsafe roots, known MTP positives, conservative MTP negatives, unknown-scan settings cycles and changed-scope confirmation rejection. The Unix managed-symlink test is present but was not run on this Windows host.
- Earlier behavioral evidence is retained in `target/manifest-migration-regression-red.log` (0 passed/6 failed), `target/manifest-migration-regression-partial-red.log` (0/2), `target/manifest-migration-regression-review-red.log` (8/3), and `target/manifest-migration-regression-auto-entry-red.log` (0/1). Intermediate green logs record 8 and 12 passing tests before final conservative acceptance refinement.
- Prior controlled check: `rtk node scripts/build-daemon.mjs check -p hifimule-daemon` — **passed**, exit 0, finished in **10.68s**. Log: `target/manifest-migration-regression-conservative-check.log`. There are 31 dead-code warnings: the prior 30 plus the now-unused permissive `find_device_profile` helper after strict loading is shared. Fresh strict-scan check is recorded below.
- Inspector unchanged: root verified `rtk proxy node --test scripts/tests/inspect-manifest.test.mjs` — **10 passed**, exit 0. Codec/schema are unchanged.
- Root verified UI TypeScript, Vite production build and **7 i18n tests** pass.
- New Rust blocks were formatted individually. `rtk proxy git -c safe.directory=C:/Workspaces/HifiMule -c core.whitespace=cr-at-eol diff --check` validates whitespace without whole-file formatting churn.
- Before final review patches, root's serial daemon suite passed **1,449 unit + 5 contract tests**, 10 ignored (`target/manifest-migration-full-serial.log`). Final serial results are recorded below.
- Root's read-only player inspection restored all **44,767 tracks**. No user device files were manually edited or written during implementation, and no Git mutation was performed by the implementation agent.

## Review Resolution

Three independent review perspectives checked behavior, boundary cases and amended acceptance. Findings about stale historical profiles, incomplete transport enumeration, captured operation profiles and changed scan roots were resolved with conservative state handling, explicit confirmation and durable regression coverage. No findings were deferred.

Final edge review identified Windows drive-relative roots such as `C:Music`. The unsafe-scope regression reproduced the failure (**0 passed, 1 failed**, exit 1; `target/manifest-migration-regression-drive-relative-red.log`). Shared path validation now rejects Windows prefix/root components and initial drive-relative spellings. Review then found a later drive component (`Music/C:`) can be reinterpreted by per-component traversal; recovery scanning also rejects colon-bearing components before pushing them. Final validation follows below.

The nested-drive regression also failed as expected (**0 passed, 1 failed**, exit 1; `target/manifest-migration-regression-nested-drive-red.log`) before the traversal guard was added.

## Final Validation

- After all review fixes, `rtk proxy node scripts/build-daemon.mjs test -p hifimule-daemon -- --test-threads=1` passed **1,465 unit tests and 5 contract tests**, with 10 ignored and no failures; exit 0. The unit suite finished in **70.58s**. Log: `target/manifest-migration-final-full-serial.log`.
- `rtk proxy node scripts/build-daemon.mjs check -p hifimule-daemon` passed, exit 0, in **11.40s**. Log: `target/manifest-migration-regression-strict-final-check.log`. The existing 31 dead-code warnings remain.
- UI TypeScript and Vite production build passed; **7 i18n tests** and **10 inspector tests** passed. No UI, catalog or codec changes followed those checks.
- Final blind, edge and acceptance reviews reported no remaining actionable findings. Whitespace validation passed.

## Suggested Review Order

**Profile decisions and explicit recovery**

- Compare effective selections while retaining uncertain history for existing tracks.
  [mod.rs:238](../../hifimule-daemon/src/device/mod.rs#L238)

- Require explicit confirmation, idle sync, readable recovery state and the same device connection.
  [rpc.rs:10334](../../hifimule-daemon/src/rpc.rs#L10334)

- Validate current profile and recovery under the normal manifest commit lock.
  [mod.rs:1903](../../hifimule-daemon/src/device/mod.rs#L1903)

- Require verified recovery scanning; unverified transports cannot report a clean state.
  [device_io.rs:60](../../hifimule-daemon/src/device_io.rs#L60)

- Reject Windows drive-relative prefixes before recovery traversal can leave the device.
  [device_io.rs:115](../../hifimule-daemon/src/device_io.rs#L115)

- Offer confirmed recovery for copied audio when a passthrough profile change is pending.
  [BasketSidebar.ts:584](../../hifimule-ui/src/components/BasketSidebar.ts#L584)

**Automatic sync and discovery**

- Record the profile actually applied; keep selections changed during sync pending.
  [main.rs:1811](../../hifimule-daemon/src/main.rs#L1811)

- Reject invalid selected profiles before admitting an automatic operation.
  [main.rs:1857](../../hifimule-daemon/src/main.rs#L1857)

- Preserve physical recovery evidence and treat failed enumeration as unknown.
  [mod.rs:1161](../../hifimule-daemon/src/device/mod.rs#L1161)

- Exclude optical drives before probing without dropping supported storage destinations.
  [mod.rs:3790](../../hifimule-daemon/src/device/mod.rs#L3790)

**Verification and documentation**

- Exercise legacy and compressed manifests across recovery and history boundaries.
  [tests.rs:4931](../../hifimule-daemon/src/device/tests.rs#L4931)

- Verify durable recovery and refusal during interrupted, active or changed-profile states.
  [rpc.rs:11831](../../hifimule-daemon/src/rpc.rs#L11831)

- Verify strict traversal covers recorded roots and detects nested interruption markers.
  [device_io.rs:759](../../hifimule-daemon/src/device_io.rs#L759)

- Verify automatic completion records the applied profile despite concurrent selection changes.
  [tests.rs:5](../../hifimule-daemon/src/tests.rs#L5)

- Follow the recovery action without manually editing device files.
  [development-guide.md:259](../../docs/development-guide.md#L259)
