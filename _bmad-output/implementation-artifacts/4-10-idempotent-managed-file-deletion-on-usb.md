# Story 4.10: Idempotent Managed File Deletion on USB

Status: review

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a user syncing a USB drive,
I want cleanup to tolerate files that are already missing,
so that stale manifest entries do not turn into noisy sync failures.

## Acceptance Criteria

1. **Given** a managed manifest entry points to a file that is already absent on an MSC device, **When** sync cleanup deletes it, **Then** the delete is treated as successful. **And** the manifest entry is removed.

2. **Given** deletion fails because of permission, read-only media, or another real IO error, **When** sync cleanup deletes it, **Then** sync reports the error. **And** the manifest entry is not silently dropped.

3. **Given** an MTP backend reports an item missing during delete, **When** the backend can distinguish missing-object errors from real IO errors, **Then** the sync layer treats the missing-object case equivalently to MSC not-found deletion.

## Tasks / Subtasks

- [x] Task 1: Make missing managed-file deletion idempotent for MSC (AC: 1, 2)
  - [x] Update MSC delete behavior or sync cleanup handling so `NotFound`/OS error 2 is treated as already-deleted success.
  - [x] Preserve errors for permission, read-only, disconnect, and other real IO failures.
  - [x] Keep path validation in place before deletion.

- [x] Task 2: Keep manifest cleanup consistent (AC: 1, 2)
  - [x] Ensure stale manifest entries are removed when the corresponding managed file is already missing.
  - [x] Ensure manifest entries are not removed when deletion fails for a real IO reason.
  - [x] Confirm cleanup does not touch unmanaged files.

- [x] Task 3: Handle MTP missing-object cases where distinguishable (AC: 3)
  - [x] Review MTP delete error mapping from WPD/libmtp backends.
  - [x] Where a missing object can be identified, map it to the same idempotent delete behavior.
  - [x] Do not hide generic MTP failures that cannot be confidently classified as missing-object.

- [x] Task 4: Verification (AC: 1-3)
  - [x] Add an MSC test for deleting a missing managed file.
  - [x] Add a test proving a real deletion error remains visible.
  - [x] Add a sync cleanup test proving stale manifest entries are removed without failing the operation.
  - [x] Add or update an MTP mock test if missing-object classification is implemented.
  - [x] Run `rtk cargo test -p hifimule-daemon`.

## Dev Notes

### Current Code Context

- `hifimule-daemon/src/device_io.rs` has `MscBackend::delete_file()` calling `tokio::fs::remove_file(&full).await?`, which propagates missing-file errors.
- The user-visible failure was: `Failed to delete file: Le fichier specifie est introuvable. (os error 2)`.
- `hifimule-daemon/src/sync.rs` calls `device_io.delete_file()` in multiple cleanup/remove paths. Review the deletion call sites before deciding whether the idempotency belongs in `MscBackend`, the sync layer, or both.
- Device IO paths must remain relative and must pass through `DeviceIO`; do not add direct `std::fs` operations against device paths outside the MSC backend.

### Safety Rule

Missing managed files are expected after manual deletion or prior partial cleanup. Treat them as already gone. Genuine delete failures must remain visible.

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Story-4.10-Idempotent-Managed-File-Deletion-on-USB]
- [Source: _bmad-output/planning-artifacts/sprint-change-proposal-2026-05-23-navidrome-subsonic-parity.md]
- [Source: hifimule-daemon/src/device_io.rs] (`MscBackend::delete_file`, `MtpBackend::delete_file`)
- [Source: hifimule-daemon/src/sync.rs] (delete call sites and manifest cleanup)
- [Source: _bmad-output/planning-artifacts/architecture.md#Device-IO-Abstraction]

## Dev Agent Record

### Agent Model Used

GPT-5 Codex

### Debug Log References

- `rtk cargo test -p hifimule-daemon msc_delete_missing_file_is_idempotent` - passed
- `rtk cargo test -p hifimule-daemon mtp_delete_missing_object_is_idempotent_when_distinguishable` - passed
- `rtk cargo test -p hifimule-daemon test_execute_sync_removes_manifest_entry_when_managed_file_missing` - passed
- `rtk cargo test -p hifimule-daemon msc_delete_directory_reports_real_io_error` - passed
- `rtk cargo test -p hifimule-daemon mtp_delete_generic_failure_remains_visible` - passed
- `rtk cargo test -p hifimule-daemon test_delete_validation_rejects_unmanaged_relative_path` - passed
- `rtk cargo test -p hifimule-daemon` - passed, 334 tests

### Completion Notes List

- Implemented idempotent MSC deletion by treating `std::io::ErrorKind::NotFound` from `MscBackend::delete_file` as success while preserving all other IO failures.
- Updated sync cleanup validation so missing managed files can still pass the managed-zone check without requiring the leaf file to exist, while rejecting absolute paths, parent traversal, and unmanaged relative paths.
- Added shared missing-delete classification in sync cleanup for OS error 2/localized not-found messages and distinguishable MTP missing-object errors.
- Updated MTP delete handling to treat explicit WPD/libmtp path-component missing errors as already deleted, while preserving generic MTP delete failures.
- Added tests for MSC idempotent delete, real delete errors, stale manifest cleanup, unmanaged path rejection, and MTP missing/generic delete classification.

### File List

- hifimule-daemon/src/device_io.rs
- hifimule-daemon/src/sync.rs
- _bmad-output/implementation-artifacts/4-10-idempotent-managed-file-deletion-on-usb.md
- _bmad-output/implementation-artifacts/sprint-status.yaml

## Change Log

- 2026-05-23: Created story from approved Correct Course proposal for USB deletion hardening.
- 2026-05-23: Implemented idempotent managed-file deletion for MSC/MTP cleanup paths and added regression coverage.
