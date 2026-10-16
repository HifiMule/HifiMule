# Investigation: Device initialization reports inaccessible device

## Hand-off Brief

1. Initializing FLASHDISK fails with EPERM during HifiMule's recursive mass-storage liveness probe.
2. macOS grants removable-volume access but denies a Full Disk Access check at the failure timestamp; the drive root contains only macOS system directories.
3. Replace recursive enumeration for liveness with a shallow root accessibility check, and preserve useful permission-error context.

## Case Info

| Field | Value |
| --- | --- |
| Ticket | N/A |
| Date opened | 2026-10-16 |
| Status | Concluded — fix implemented and regression-tested; installed-device retry pending |
| System | User's local machine; macOS suggested by workspace paths, runtime details unverified |
| Evidence sources | User report; repository source |

## Problem Statement

User report: “On this machine, when I tried to configure a new device, we get an error: Initialization Failed / Failed to initialize device: Device no longer accessible — reconnect the device and try again”.

## Evidence Inventory

| Source | Status | Notes |
| --- | --- | --- |
| Source code | Available | Exact error in `hifimule-daemon/src/device/mod.rs:2374` and `:2387` |
| Runtime logs | Missing | Not inspected yet; underlying error is logged before the generic message |
| Device identity and transport | Missing | MTP versus mounted mass storage not established |
| Runtime version | Missing | Whether installed app matches checkout is unverified |
| Working tree | Available | Existing staged playback/main/RPC changes; left untouched |

## Investigation Backlog

| # | Path to Explore | Priority | Status | Notes |
| --- | --- | --- | --- | --- |
| 1 | Locate local daemon logs and initialization failure | High | Open | Capture path, device class, underlying error and timestamp |
| 2 | Trace failing backend operation | High | Open | Select backend from runtime evidence |
| 3 | Verify running version and device accessibility | High | Open | Separate stale handle from permissions or backend failure |

## Timeline of Events

Failure time not supplied; reported in this chat on 2026-10-16.

## Confirmed Findings

### Finding 1: A failed liveness probe produces this generic message

**Evidence:** `hifimule-daemon/src/device/mod.rs:2374`, `hifimule-daemon/src/device/mod.rs:2387`.

`initialize_reserved_pending` checks MTP with `storage_id()` and mass storage with `list_files("")`. Either error is logged under `[DeviceInit]` with device path, class, and error details, then replaced by the same user-facing message. This happens before creating the managed folder.

## Deduced Conclusions

The message alone does not establish that the device physically disconnected. The backend's underlying error is necessary to determine why the accessibility probe failed.

## Hypothesized Paths

### Hypothesis 1: Stale or inaccessible device handle

**Status:** Open.

**Theory:** Device state changed after discovery and before initialization.

**Supporting indicators:** The failing probe is intended to catch this situation.

**Would confirm:** Runtime logs and device state demonstrate removal or a stale backend handle.

**Would refute:** Logs instead establish a permission or backend error for a continuously connected device.

**Resolution:** Pending runtime evidence.

## Missing Evidence

| Gap | Impact | How to Obtain |
| --- | --- | --- |
| `[DeviceInit]` failure details | Determines actual backend failure | Read local daemon log |
| Running app version and device transport | Determines applicable implementation | Inspect local process/version and logged device class |

## Source Code Trace

| Element | Detail |
| --- | --- |
| Error origin | `hifimule-daemon/src/device/mod.rs:2374`, `:2387` |
| Trigger | Initializing a pending device |
| Condition | `storage_id()` fails for MTP, or `list_files("")` fails for mass storage |
| UI | `hifimule-ui/src/components/InitDeviceModal.ts:301` |
| RPC wrapper | `hifimule-daemon/src/rpc.rs:9818` |

## Conclusion

**Confidence:** High for source mapping; root cause undetermined.

## Recommended Next Steps

Inspect local runtime evidence, then trace the selected backend. No implementation change is justified yet.

## Reproduction Plan

After identifying the device and version, correlate the reported initialization attempt with its daemon log entry. Any new initialization attempt should account for its potential to write device metadata.

## Side Findings

None.

## Follow-up: 2026-10-16

### New Evidence

- Local daemon log located at `/Users/akartmann/Library/Application Support/HifiMule/daemon.log`.
- Lines 9, 10, and 15 show `[DeviceInit] Root liveness probe failed device_path=/Volumes/FLASHDISK class=Msc error=Operation not permitted (os error 1)` at log timestamps 21:56:10, 21:56:48, and 21:57:43. Log timezone not established.
- Log line 12 records a daemon restart at 21:57:19 with `release=true, service=false`; the failure recurs afterward.
- `ls -ldeO /Volumes/FLASHDISK` shows the directory exists and is owned by `akartmann`, mode `drwx------`.
- Local `runtime/daemon-on-demand.plist` names `/Applications/HifiMule.app/Contents/MacOS/hifimule-daemon` as its executable and `hifimule.github.io` as its associated bundle identifier. This is launch configuration evidence, not proof of the running process identity.
- Process inspection (`pgrep`) and DiskManagement inspection (`diskutil info`) were unavailable in the tool environment. These failures do not establish a problem with the device.

### Additional Findings

**Confirmed:** The reported failure is reproduced in local logs for a mass-storage device at `/Volumes/FLASHDISK`, and the underlying error is EPERM. Initialization's generic reconnect advice conceals this distinction.

**Deduced:** MTP-specific explanations do not apply to these attempts. A daemon restart alone did not remove the failure.

### Updated Hypotheses

- Hypothesis 1 (stale or inaccessible handle): remains Open; EPERM points toward an access-policy issue, but there is no complete removal/attachment timeline.
- Hypothesis 2 (macOS privacy/access policy denies the daemon removable-volume access): Open. EPERM despite matching directory ownership is consistent with this explanation. Confirm with macOS access-policy logs or permission state associated with the daemon/app; refute with evidence of another backend or filesystem cause.

### Backlog Changes

- Locate daemon logs: Done.
- Identify backend: Done — mass storage.
- Inspect volume mount state: Partial — directory exists; DiskManagement query unavailable.
- Inspect runtime version: Open; executable path identified from launch configuration only.
- Trace macOS access policy and daemon launch attribution: Open, highest priority.

### Updated Conclusion

High confidence that the immediate cause is denied filesystem access to `/Volumes/FLASHDISK`, rather than an MTP initialization failure. The specific policy denying access remains unconfirmed. No application code or device contents changed.

## Follow-up: 2026-10-16 #2

### New Evidence

- macOS unified logs were successfully read using an approved unsandboxed, read-only query after the sandboxed `log show` attempt failed.
- At 21:57:20.377, TCC request `417.5377` is for `kTCCServiceSystemPolicyRemovableVolumes`, accessing process `com.hifimule.daemon`, PID 29868, binary `/Applications/HifiMule.app/Contents/MacOS/hifimule-daemon`. The subject is `hifimule.github.io`. At 21:57:20.379 it returns `authValue=2, authReason=2` (allowed).
- At 21:57:43.260–264, TCC request `417.5379` is for `kTCCServiceSystemPolicyAllFiles`, same daemon and subject. It explicitly reports `Auth Right: Denied (Service Policy)`, `authValue=0`, and `result=false`. This coincides with daemon.log's 21:57:43 initialization failure.
- `hifimule-ui/src-tauri/src/lib.rs:352` launches the daemon using a launchd agent; line 398 associates it with `hifimule.github.io`. TCC's observed attribution matches that configuration.
- Installed application signing inspection reports an ad-hoc signature and no emitted sandbox entitlements. This alone is not evidence of the cause.
- `hifimule-daemon/src/device_io.rs:506–554` implements mass-storage `list_files` as recursive traversal. Line 528 propagates any directory read failure; line 536 queues every subdirectory without filtering macOS system directories.
- A shallow `ls -la /Volumes/FLASHDISK` succeeds in the inspection environment. Its only children are `.Spotlight-V100`, `.Trashes`, and `.fseventsd`. Success here is not proof that the daemon has identical permissions.
- A narrow sandboxd/kernel log query at 21:57:42–45 returned no matching denied-path entries. The precise protected child directory remains unknown.

### Updated Hypotheses and Refutation Pass

- Hypothesis 1 (stale device handle): remains unconfirmed; root existence, consistent EPERM, and correlated TCC denial make this less likely.
- Hypothesis 2 (missing removable-volume consent): Refuted for the observed post-restart attempt. TCC permits that service for HifiMule, and correctly attributes the daemon to the application.
- Hypothesis 3 (recursive liveness probe reaches protected macOS metadata): Strongly supported / Deduced. The probe recursively enters every directory, the drive contains only macOS system directories, and TCC denies Full Disk Access at the exact failure time. Confirm the precise path using path-bearing error context or a controlled daemon-context reproduction; refute if the failure occurs before recursive descent or on an unrelated path.

### Updated Conclusion

**Confidence: Medium** for the full causal chain; high for the recursive-probe design defect and observed permission decisions. The liveness probe tests all descendant directories instead of just the device root. A protected child therefore fails initialization of an otherwise accessible drive. Exact failing directory and successful post-fix reproduction remain outstanding.

### Fix Direction

Use a shallow, transport-appropriate accessibility check for mass-storage initialization. Avoid changing general recursive listing semantics merely to fix the probe. Preserve path and permission context in errors, and ensure missing roots still fail. Add a regression test proving initialization does not enumerate inaccessible descendants. Implement through `bmad-quick-dev`; no application source changes were made during this investigation.

### Verification Plan

Exercise a mounted root with an inaccessible child and a writable managed destination; root probing should succeed without traversing the child. Separately verify absent or unreadable roots fail. Retry initialization on FLASHDISK after the fix, verifying the manifest and managed directory can be written without broadening macOS privacy access.

## Implementation: 2026-10-16

User authorized implementation with “go”. Added `DeviceIO::probe_root` and an MSC override that validates and enumerates root entries without opening children. Initialization now uses that probe and reports root path plus the underlying cause on failure. MTP and intentional recursive listing remain unchanged.

Validation via the managed FFmpeg build wrapper passed: 16 initialization tests and 32 device I/O tests, including all 4 focused root-probe tests. Protected-child and unreadable-root tests reproduced actual permission denial on this machine. A removed-root regression verifies initialization does not recreate the root or register a recognized device. Temporary permissions restore through an RAII guard.

Three independent review perspectives completed; no actionable production defect remains. See [implementation spec](../spec-shallow-device-initialization-probe.md) for commands and review triage. No application installation, privacy-setting changes, or physical-device writes were performed. The updated app still needs to be installed and tested against FLASHDISK to verify the observed device-specific failure is resolved.
