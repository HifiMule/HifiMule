---
title: 'Stream staged audio to devices with bounded memory'
type: 'bugfix'
created: '2026-09-26'
status: 'done'
baseline_commit: 'd91682d3212e0213759293cb01357399eb949c1b'
context:
  - '{project-root}/_bmad-output/planning-artifacts/project-context.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Syncing a large audiobook or podcast can grow daemon memory by roughly the file size. Provider sync stages downloads in chunks, then reads the entire staged file before writing it to the device; MTP copies those bytes again.

**Approach:** Transfer the existing staged file to each supported device backend in bounded chunks or through its native path-based transfer. Keep staging, one serial device writer, retry behavior, and manifest updates after a confirmed write.

## Boundaries & Constraints

**Always:** Keep provider downloads chunked and temp staging bounded. Preserve relative-path validation, MSC temporary-file atomic replacement, MTP object verification, cancellation cleanup, and the current one-writer pipeline. Large-file memory use must be independent of file size on MSC, Windows WPD, and Unix libmtp paths.

**Ask First:** A backend cannot support a path-backed transfer without weakening its existing write or verification guarantees.

**Never:** Hold a full audio file in a `Vec<u8>`; bypass `DeviceIO`; add parallel device writes; change playback buffering or the manifest format.

The 2 GiB staged-file limit remains a disk/backpressure limit for this change. Lifting it is separate from removing file-sized RAM allocations.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Large provider file | Staged audio exceeds normal RAM budget | Device receives identical bytes through a bounded-memory transfer | Existing 2 GiB staging limit still applies |
| MSC replacement | Target already exists | Copy into sibling `.tmp`, sync and rename after success | Remove partial `.tmp`; do not mark item synced |
| MTP transfer | Staged source path and target object | WPD reads chunks or uses Shell copy; libmtp sends from source path | Preserve native verification, retry and cleanup |
| Failed or cancelled write | Transfer ends before success | No new manifest entry; staged and partial files are cleaned | Surface file error or cancellation as today |
| Retried transfer | First device write fails, staged file remains | Reopen the same staged path and retry once | Report a file error if the second attempt fails |
| Small metadata write | Manifest or playlist bytes | Existing slice-based `DeviceIO` write still works | Preserve its current error reporting |

</frozen-after-approval>

## Code Map

- `hifimule-daemon/src/sync.rs` -- provider writer loads staged files wholly at `tokio::fs::read`; owns retry, cleanup, progress and manifest sequencing.
- `hifimule-daemon/src/device_io.rs` -- `DeviceIO` API and MSC/MTP adapters; current MTP slice write clones data for `spawn_blocking`.
- `hifimule-daemon/src/device/mtp.rs` -- Windows WPD stream/Shell copy and Unix `LIBMTP_Send_File_From_File` implementations.
- `hifimule-daemon/src/sync.rs` test doubles -- `BlockingFirstWriteDeviceIo` and failure wrappers currently observe slice writes and must observe path writes after the API change.

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-daemon/src/device_io.rs` -- add a path-backed verified-write operation with a compatible default for test doubles; stream MSC source into its sibling temporary file; forward MTP source path into its blocking handle without cloning file contents.
- [x] `hifimule-daemon/src/device/mtp.rs` -- implement native path-backed writes for WPD and libmtp, reusing existing object creation, overwrite, fallback, and verification rules.
- [x] `hifimule-daemon/src/sync.rs` -- send staged paths through the new operation, preserving retry, cancellation, staging cleanup, writer serialization and post-write bookkeeping.
- [x] `hifimule-daemon/src/device_io.rs` and `hifimule-daemon/src/sync.rs` -- update pipeline test doubles to intercept path writes; test byte integrity, replacement, partial-write cleanup, retries and manifest behavior using a staged file larger than the transfer chunk.

**Acceptance Criteria:**
- Given a provider file much larger than the transfer chunk, when it syncs to MSC or MTP, then peak file-transfer buffering stays bounded independently of the file size.
- Given a device-write error or cancellation, when the pipeline stops, then incomplete target data is not recorded as synced and staged files are cleaned.
- Given a successful write, when the sync finalizes, then the target bytes and manifest match the staged source.

## Spec Change Log

## Design Notes

The source is already on disk for overlap/backpressure. A path-backed `DeviceIO` method avoids the whole-file read in the writer and the extra MTP `to_vec()`. Keep the slice-based method for small manifests and playlist files.

For MSC, stream into the same sibling `.tmp` file used today, call `sync_all`, then rename. For libmtp, its existing native send operation already accepts a filesystem path; avoid creating a second temp copy. For WPD, read bounded chunks from that path into the COM stream and pass the same path to the Shell fallback. The staged source must survive both write attempts, then be removed by the current writer cleanup. Test doubles may use a small-file default, but every production backend must override it with a bounded path.

## Verification

**Commands:**
- `rtk cargo test -p hifimule-daemon device_io` -- device backend regressions pass.
- `rtk cargo test -p hifimule-daemon test_execute_provider_sync` -- provider sync regressions pass.
- `rtk cargo check -p hifimule-daemon` -- daemon compiles on the host platform.
- `rtk cargo test -p hifimule-daemon` -- full daemon regressions pass.

**Manual checks (if no CLI):**
- Inspect Windows WPD path for path-based chunk reads and Shell fallback; no full-file allocation remains.
- On a device, sync a large audio file while monitoring daemon RSS; memory must stay roughly flat as file size increases, and the device copy must match the source.

## Suggested Review Order

**Sync transfer**

- Start where the writer sends a staged path and retries without loading file bytes.
  [sync.rs:3279](../../hifimule-daemon/src/sync.rs#L3279)

- See the new device boundary while small metadata writes keep the slice API.
  [device_io.rs:25](../../hifimule-daemon/src/device_io.rs#L25)

**Device backends**

- Follow MSC's bounded copy, size check, atomic rename and partial-file cleanup.
  [device_io.rs:227](../../hifimule-daemon/src/device_io.rs#L227)

- Check how MTP preserves source and serialization after caller cancellation.
  [device_io.rs:453](../../hifimule-daemon/src/device_io.rs#L453)

- Review WPD's shared byte/path input and bounded COM stream.
  [mtp.rs:1030](../../hifimule-daemon/src/device/mtp.rs#L1030)

- Confirm libmtp sends the staged path directly through its native API.
  [mtp.rs:2312](../../hifimule-daemon/src/device/mtp.rs#L2312)

**Regression coverage**

- Verify partial MSC copies are removed after cancellation.
  [device_io.rs:644](../../hifimule-daemon/src/device_io.rs#L644)

- Verify a dropped MTP caller retains source bytes and the device lock.
  [device_io.rs:1058](../../hifimule-daemon/src/device_io.rs#L1058)

- Verify provider sync retries the same staged path and writes identical bytes.
  [sync.rs:5509](../../hifimule-daemon/src/sync.rs#L5509)
